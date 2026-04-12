mod router;

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_stream::StreamExt as _;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

use std::collections::HashMap;

use atn_core::agent::{AgentConfig, AgentId, AgentState};
use atn_core::config::load_project_config;
use atn_core::event::{InputEvent, OutputSignal, PushEvent};
use atn_core::router::EventLogEntry;
use atn_pty::manager::SessionManager;
use atn_trail::reader::{SagaConfig, StepConfig, Trajectory};
use atn_wiki::storage::FileWikiStorage;
use wiki_common::async_storage::AsyncWikiStorage;
use wiki_common::etag::content_etag;
use wiki_common::model::WikiPage;
use wiki_common::parser::render_wiki_content;
use wiki_common::patch::{PatchRequest, apply_ops};

#[derive(Clone)]
struct SharedState {
    manager: Arc<Mutex<SessionManager>>,
    wiki: Arc<FileWikiStorage>,
    event_log: router::EventLog,
    /// Resolved repo_path for each agent (for saga lookups).
    agent_repo_paths: Arc<Mutex<HashMap<String, PathBuf>>>,
    /// Agent configs for restart support.
    agent_configs: Arc<Mutex<HashMap<String, AgentConfig>>>,
    /// Base directory for resolving relative paths.
    base_dir: PathBuf,
    /// Path to agents.toml for saving config.
    config_path: PathBuf,
}

type AppState = SharedState;

static INDEX_HTML: &str = include_str!("../static/index.html");

const DEFAULT_CONFIG_PATH: &str = "agents.toml";

#[derive(Deserialize)]
struct InputPayload {
    #[serde(default)]
    text: String,
    #[serde(default)]
    raw_bytes: Vec<u8>,
}

#[derive(Serialize)]
struct AgentInfo {
    id: String,
    name: String,
    role: String,
    state: AgentState,
}

#[derive(Serialize)]
struct WikiPageResponse {
    title: String,
    content: String,
    html: String,
    created_at: u64,
    updated_at: u64,
}

#[derive(Deserialize)]
struct WikiPutBody {
    content: String,
}

#[derive(Serialize)]
struct WikiConflictResponse {
    error: String,
    current_etag: String,
    page: WikiPageResponse,
}

#[derive(Deserialize)]
struct EventLogQuery {
    since: Option<usize>,
}

#[derive(Deserialize)]
struct SubmitEventBody {
    #[serde(flatten)]
    event: PushEvent,
}

/// Full saga overview returned from the saga endpoint.
#[derive(Serialize)]
struct SagaResponse {
    saga: Option<SagaConfig>,
    steps: Vec<StepConfig>,
    trajectories: Vec<Trajectory>,
}

#[derive(Deserialize)]
struct DistillBody {
    task_type: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("atn=info".parse().unwrap())
                .add_directive("atn_server=info".parse().unwrap())
                .add_directive("atn_pty=info".parse().unwrap()),
        )
        .with_target(true)
        .with_thread_ids(false)
        .init();

    // Install a panic hook that logs panics via tracing before aborting.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("PANIC: {info}");
        default_hook(info);
    }));

    tracing::info!("All Together Now — PGM server starting");

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());
    let config_path = PathBuf::from(&config_path);

    let base_dir = config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();

    let project_config = load_project_config(&config_path).unwrap_or_else(|e| {
        tracing::error!("Failed to load {}: {e}", config_path.display());
        tracing::info!("Starting with no agents — create agents.toml to configure");
        atn_core::config::ProjectConfig {
            project: Default::default(),
            agents: vec![],
        }
    });

    let log_dir = project_config
        .project
        .log_dir
        .as_ref()
        .map(|d| base_dir.join(d));

    let mut manager = SessionManager::new(log_dir);

    // Build agent configs and spawn sessions.
    let mut agent_configs_map: HashMap<String, AgentConfig> = HashMap::new();
    for entry in &project_config.agents {
        let config = entry.to_agent_config(&base_dir);
        agent_configs_map.insert(entry.id.clone(), config.clone());
        match manager.spawn_agent(config) {
            Ok(id) => tracing::info!("Spawned agent: {id} ({})", entry.name),
            Err(e) => tracing::error!("Failed to spawn agent '{}': {e}", entry.id),
        }
    }

    let agent_ids: Vec<String> = manager.agent_ids().iter().map(|id| id.0.clone()).collect();

    // Build repo_path map for saga lookups.
    let agent_repo_paths: HashMap<String, PathBuf> = agent_configs_map
        .iter()
        .map(|(id, config)| (id.clone(), config.repo_path.clone()))
        .collect();

    tracing::info!("{} agent(s) running", manager.len());

    // Initialize wiki storage and seed coordination pages.
    let wiki_dir = base_dir.join(".atn").join("wiki");
    let wiki = Arc::new(FileWikiStorage::new(&wiki_dir));
    let now = wiki_common::time::now();
    atn_wiki::coordination::seed_coordination_pages(wiki.as_ref(), now).await;
    tracing::info!("Wiki storage initialized at {}", wiki_dir.display());

    // Initialize event log and message router.
    let event_log: router::EventLog = Arc::new(Mutex::new(Vec::new()));
    router::ensure_outbox_dirs(&base_dir, &agent_ids).await;
    let manager = Arc::new(Mutex::new(manager));

    let _router_handle = router::spawn_message_router(
        base_dir.clone(),
        manager.clone(),
        wiki.clone(),
        event_log.clone(),
        std::time::Duration::from_secs(2),
    );
    tracing::info!("Message router started (polling every 2s)");

    let state = SharedState {
        manager: manager.clone(),
        wiki,
        event_log,
        agent_repo_paths: Arc::new(Mutex::new(agent_repo_paths)),
        agent_configs: Arc::new(Mutex::new(agent_configs_map)),
        base_dir: base_dir.clone(),
        config_path: config_path.clone(),
    };

    // Spawn config hot-reload watcher.
    spawn_config_watcher(config_path, base_dir, state.clone());

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/agents", get(list_agents).post(create_agent))
        .route("/api/agents/graph", get(agent_dependency_graph))
        .route("/api/agents/save", post(save_config))
        .route(
            "/api/agents/{id}",
            axum::routing::put(update_agent).delete(delete_agent),
        )
        .route("/api/agents/{id}/sse", get(agent_sse))
        .route("/api/agents/{id}/input", post(agent_input))
        .route("/api/agents/{id}/ctrl-c", post(agent_ctrl_c))
        .route("/api/agents/{id}/resize", post(agent_resize))
        .route("/api/agents/{id}/state", get(agent_state))
        .route("/api/agents/{id}/restart", post(agent_restart))
        .route("/api/agents/{id}/stop", post(stop_agent))
        .route("/api/saga", get(get_project_saga))
        .route("/api/saga/distill", post(saga_distill))
        .route("/api/agents/{id}/saga", get(get_agent_saga))
        .route("/api/events", get(list_events).post(submit_event))
        .route("/api/wiki", get(wiki_list_pages))
        .route(
            "/api/wiki/{*title}",
            get(wiki_get_page)
                .put(wiki_put_page)
                .patch(wiki_patch_page)
                .delete(wiki_delete_page),
        )
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let addr = "0.0.0.0:7500";
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(
                "Cannot bind to {addr}: {e}. Another atn-server is probably running. \
                 Kill it first with: pkill -f atn-server"
            );
            std::process::exit(1);
        }
    };
    tracing::info!("Listening on http://{addr}");

    let manager_for_shutdown = state.manager.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    // Server has stopped accepting — now clean up agent sessions.
    tracing::info!("Shutting down all agent sessions...");
    let sessions = {
        let mut mgr = manager_for_shutdown.lock().await;
        mgr.drain_all()
    };
    for mut session in sessions {
        let _ = session.shutdown().await;
    }
    tracing::info!("All agents shut down. Goodbye.");
}

/// Wait for SIGINT (Ctrl-C) or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("Received Ctrl-C, starting graceful shutdown"),
        () = terminate => tracing::info!("Received SIGTERM, starting graceful shutdown"),
    }
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// Returns full agent info with state for all agents.
async fn list_agents(State(state): State<AppState>) -> Json<Vec<AgentInfo>> {
    let pending: Vec<_> = {
        let mgr = state.manager.lock().await;
        mgr.agent_ids()
            .iter()
            .filter_map(|id| {
                mgr.get_session(id).ok().map(|session| {
                    (
                        id.0.clone(),
                        session.name().to_string(),
                        session.role().to_string(),
                        session.state(),
                    )
                })
            })
            .collect()
    };
    let mut agents = Vec::with_capacity(pending.len());
    for (id, name, role, state_lock) in pending {
        let s = state_lock.read().await;
        agents.push(AgentInfo {
            id,
            name,
            role,
            state: s.clone(),
        });
    }
    Json(agents)
}

/// Returns current state for a single agent.
async fn agent_state(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<AgentInfo>, StatusCode> {
    let agent_id = AgentId(id);
    let (name, role, state_lock) = {
        let mgr = state.manager.lock().await;
        let session = mgr
            .get_session(&agent_id)
            .map_err(|_| StatusCode::NOT_FOUND)?;
        (
            session.name().to_string(),
            session.role().to_string(),
            session.state(),
        )
    };
    let s = state_lock.read().await;
    Ok(Json(AgentInfo {
        id: agent_id.0,
        name,
        role,
        state: s.clone(),
    }))
}

async fn agent_sse(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let agent_id = AgentId(id);
    let mgr = state.manager.lock().await;
    let session = mgr
        .get_session(&agent_id)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let rx = session.output_receiver();
    drop(mgr);

    let stream =
        tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(|result| match result {
            Ok(OutputSignal::Bytes(bytes)) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Some(Ok(Event::default().data(b64)))
            }
            _ => None,
        });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn agent_input(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<InputPayload>,
) -> Result<StatusCode, StatusCode> {
    let agent_id = AgentId(id);
    let tx = {
        let mgr = state.manager.lock().await;
        let session = mgr
            .get_session(&agent_id)
            .map_err(|_| StatusCode::NOT_FOUND)?;
        session.input_sender()
    };
    let event = if !payload.raw_bytes.is_empty() {
        InputEvent::RawBytes {
            bytes: payload.raw_bytes,
        }
    } else {
        InputEvent::HumanText {
            text: payload.text,
        }
    };
    tx.send(event)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn agent_ctrl_c(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let agent_id = AgentId(id);
    let tx = {
        let mgr = state.manager.lock().await;
        let session = mgr
            .get_session(&agent_id)
            .map_err(|_| StatusCode::NOT_FOUND)?;
        session.input_sender()
    };
    tx.send(InputEvent::RawBytes { bytes: vec![0x03] })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct ResizeBody {
    cols: u16,
    rows: u16,
}

async fn agent_resize(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(body): Json<ResizeBody>,
) -> Result<StatusCode, StatusCode> {
    let agent_id = AgentId(id);
    let mgr = state.manager.lock().await;
    let session = mgr
        .get_session(&agent_id)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    session
        .resize(body.cols, body.rows)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

// ── Restart / graph / hot-reload ──────────────────────────────────────

/// Restart an agent session: shut down if running, then re-spawn from stored config.
async fn agent_restart(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let agent_id = AgentId(id.clone());

    // Get the config for this agent.
    let config = {
        let configs = state.agent_configs.lock().await;
        configs.get(&id).cloned().ok_or(StatusCode::NOT_FOUND)?
    };

    // Remove the existing session (if any) without holding the lock across await.
    let old_session = {
        let mut mgr = state.manager.lock().await;
        mgr.remove_agent(&agent_id).ok()
    };
    if let Some(mut session) = old_session {
        let _ = session.shutdown().await;
    }

    // Re-spawn from stored config.
    {
        let mut mgr = state.manager.lock().await;
        mgr.spawn_agent(config).map_err(|e| {
            tracing::error!("Failed to restart agent '{id}': {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    tracing::info!("Restarted agent: {id}");
    Ok(StatusCode::OK)
}

// ── Agent CRUD ───────────────────────────────────────────────────────

/// Request body for creating a new agent.
#[derive(Deserialize)]
struct CreateAgentBody {
    id: String,
    name: String,
    repo_path: String,
    #[serde(default = "default_role_str")]
    role: String,
    #[serde(default)]
    setup_commands: Vec<String>,
    #[serde(default)]
    launch_command: String,
}

fn default_role_str() -> String {
    "developer".to_string()
}

/// Request body for updating an agent's config.
#[derive(Deserialize)]
struct UpdateAgentBody {
    name: Option<String>,
    repo_path: Option<String>,
    role: Option<String>,
    setup_commands: Option<Vec<String>>,
    launch_command: Option<String>,
}

fn parse_role(s: &str) -> atn_core::agent::AgentRole {
    match s.to_lowercase().as_str() {
        "qa" => atn_core::agent::AgentRole::QA,
        "pm" => atn_core::agent::AgentRole::PM,
        "coordinator" => atn_core::agent::AgentRole::Coordinator,
        _ => atn_core::agent::AgentRole::Developer,
    }
}

/// Create a new agent session.
async fn create_agent(
    State(state): State<AppState>,
    Json(body): Json<CreateAgentBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let id = body.id.clone();

    // Check if agent already exists.
    {
        let configs = state.agent_configs.lock().await;
        if configs.contains_key(&id) {
            return Err((StatusCode::CONFLICT, format!("Agent '{id}' already exists")));
        }
    }

    let role = parse_role(&body.role);
    let repo_path = if std::path::Path::new(&body.repo_path).is_absolute() {
        PathBuf::from(&body.repo_path)
    } else {
        state.base_dir.join(&body.repo_path)
    };

    let config = AgentConfig {
        id: AgentId(id.clone()),
        name: body.name,
        repo_path: repo_path.clone(),
        role,
        setup_commands: body.setup_commands,
        launch_command: body.launch_command,
    };

    // Spawn the agent session.
    {
        let mut mgr = state.manager.lock().await;
        mgr.spawn_agent(config.clone()).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to spawn agent: {e}"),
            )
        })?;
    }

    // Store config and repo path.
    {
        let mut configs = state.agent_configs.lock().await;
        configs.insert(id.clone(), config);
    }
    {
        let mut repo_paths = state.agent_repo_paths.lock().await;
        repo_paths.insert(id.clone(), repo_path);
    }

    tracing::info!("Created agent: {id}");
    Ok(StatusCode::CREATED)
}

/// Delete (destroy) an agent session.
async fn delete_agent(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let agent_id = AgentId(id.clone());

    // Remove session.
    let old_session = {
        let mut mgr = state.manager.lock().await;
        mgr.remove_agent(&agent_id)
            .map_err(|_| StatusCode::NOT_FOUND)?
    };

    // Shutdown gracefully.
    let mut session = old_session;
    let _ = session.shutdown().await;

    // Remove from config and repo paths.
    {
        let mut configs = state.agent_configs.lock().await;
        configs.remove(&id);
    }
    {
        let mut repo_paths = state.agent_repo_paths.lock().await;
        repo_paths.remove(&id);
    }

    tracing::info!("Deleted agent: {id}");
    Ok(StatusCode::NO_CONTENT)
}

/// Update an agent's configuration. Restarts the agent with new config.
async fn update_agent(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(body): Json<UpdateAgentBody>,
) -> Result<StatusCode, StatusCode> {
    let agent_id = AgentId(id.clone());

    // Get current config.
    let mut config = {
        let configs = state.agent_configs.lock().await;
        configs.get(&id).cloned().ok_or(StatusCode::NOT_FOUND)?
    };

    // Apply updates.
    if let Some(name) = body.name {
        config.name = name;
    }
    if let Some(repo_path) = body.repo_path {
        config.repo_path = if std::path::Path::new(&repo_path).is_absolute() {
            PathBuf::from(&repo_path)
        } else {
            state.base_dir.join(&repo_path)
        };
    }
    if let Some(role) = body.role {
        config.role = parse_role(&role);
    }
    if let Some(setup_commands) = body.setup_commands {
        config.setup_commands = setup_commands;
    }
    if let Some(launch_command) = body.launch_command {
        config.launch_command = launch_command;
    }

    // Shutdown existing session.
    let old_session = {
        let mut mgr = state.manager.lock().await;
        mgr.remove_agent(&agent_id).ok()
    };
    if let Some(mut session) = old_session {
        let _ = session.shutdown().await;
    }

    // Re-spawn with updated config.
    {
        let mut mgr = state.manager.lock().await;
        mgr.spawn_agent(config.clone()).map_err(|e| {
            tracing::error!("Failed to respawn agent '{id}' with new config: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    // Update stored config and repo path.
    {
        let mut configs = state.agent_configs.lock().await;
        configs.insert(id.clone(), config.clone());
    }
    {
        let mut repo_paths = state.agent_repo_paths.lock().await;
        repo_paths.insert(id.clone(), config.repo_path);
    }

    tracing::info!("Updated agent: {id}");
    Ok(StatusCode::OK)
}

/// Stop an agent without removing its config (can be restarted later).
async fn stop_agent(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let agent_id = AgentId(id.clone());

    let old_session = {
        let mut mgr = state.manager.lock().await;
        mgr.remove_agent(&agent_id)
            .map_err(|_| StatusCode::NOT_FOUND)?
    };

    let mut session = old_session;
    let _ = session.shutdown().await;

    tracing::info!("Stopped agent: {id}");
    Ok(StatusCode::OK)
}

/// Save current agent configs back to agents.toml.
async fn save_config(State(state): State<AppState>) -> Result<StatusCode, (StatusCode, String)> {
    let configs = state.agent_configs.lock().await;

    let agents: Vec<atn_core::config::AgentEntry> = configs
        .values()
        .map(|c| {
            // Convert absolute path back to relative if under base_dir.
            let repo_path = c
                .repo_path
                .strip_prefix(&state.base_dir)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| c.repo_path.to_string_lossy().to_string());
            let repo_path = if repo_path.is_empty() {
                ".".to_string()
            } else {
                repo_path
            };
            atn_core::config::AgentEntry {
                id: c.id.0.clone(),
                name: c.name.clone(),
                repo_path,
                role: c.role.clone(),
                setup_commands: c.setup_commands.clone(),
                launch_command: c.launch_command.clone(),
            }
        })
        .collect();

    // Read existing config to preserve project metadata.
    let project = atn_core::config::load_project_config(&state.config_path)
        .map(|c| c.project)
        .unwrap_or_default();

    let config = atn_core::config::ProjectConfig { project, agents };

    let toml_str = toml::to_string_pretty(&config).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize config: {e}"),
        )
    })?;

    tokio::fs::write(&state.config_path, toml_str)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to write config: {e}"),
            )
        })?;

    tracing::info!("Saved config to {}", state.config_path.display());
    Ok(StatusCode::OK)
}

/// Dependency graph: which agents are blocked and by what.
#[derive(Serialize)]
struct DepGraphNode {
    id: String,
    name: String,
    state: String,
    blocked_on: Vec<String>,
}

async fn agent_dependency_graph(State(state): State<AppState>) -> Json<Vec<DepGraphNode>> {
    let pending: Vec<_> = {
        let mgr = state.manager.lock().await;
        mgr.agent_ids()
            .iter()
            .filter_map(|id| {
                mgr.get_session(id)
                    .ok()
                    .map(|session| (id.0.clone(), session.name().to_string(), session.state()))
            })
            .collect()
    };

    let mut nodes = Vec::with_capacity(pending.len());
    for (id, name, state_lock) in pending {
        let s = state_lock.read().await;
        let blocked_on = match &*s {
            AgentState::Blocked { on } => on.clone(),
            _ => vec![],
        };
        let state_key = serde_json::to_value(&*s)
            .ok()
            .and_then(|v| v.get("state").and_then(|s| s.as_str()).map(String::from))
            .unwrap_or_else(|| "unknown".to_string());
        nodes.push(DepGraphNode {
            id,
            name,
            state: state_key,
            blocked_on,
        });
    }

    Json(nodes)
}

/// Watch agents.toml for changes and hot-reload agent configuration.
fn spawn_config_watcher(config_path: PathBuf, base_dir: PathBuf, state: SharedState) {
    use notify::{EventKind, RecursiveMode, Watcher};

    let (tx, mut rx) = tokio::sync::mpsc::channel(16);

    // Spawn the blocking file watcher in a dedicated thread.
    std::thread::spawn(move || {
        let tx = tx;
        let mut watcher =
            match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res
                    && matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
                {
                    let _ = tx.blocking_send(());
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!("Failed to create config file watcher: {e}");
                    return;
                }
            };
        if let Err(e) = watcher.watch(&config_path, RecursiveMode::NonRecursive) {
            tracing::error!("Failed to watch {}: {e}", config_path.display());
            return;
        }
        tracing::info!("Watching {} for changes", config_path.display());
        // Keep watcher alive.
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    });

    // Process reload signals on the async runtime.
    // We use LocalSet-free approach: collect decisions without holding manager lock,
    // then apply them.
    let manager = state.manager.clone();
    let agent_configs = state.agent_configs.clone();
    let agent_repo_paths = state.agent_repo_paths.clone();

    tokio::spawn(async move {
        // Debounce: wait 500ms after last event before reloading.
        while rx.recv().await.is_some() {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            while rx.try_recv().is_ok() {}

            let config_path_for_load = base_dir.join(DEFAULT_CONFIG_PATH);
            tracing::info!("Config file changed, reloading...");

            let new_config = match load_project_config(&config_path_for_load) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to reload config: {e}");
                    continue;
                }
            };

            let new_ids: std::collections::HashSet<String> =
                new_config.agents.iter().map(|a| a.id.clone()).collect();
            let current_ids: std::collections::HashSet<String> = {
                let mgr = manager.lock().await;
                mgr.agent_ids().iter().map(|id| id.0.clone()).collect()
            };

            // Remove agents no longer in config.
            let to_remove: Vec<String> = current_ids.difference(&new_ids).cloned().collect();
            for id in &to_remove {
                let agent_id = AgentId(id.clone());
                let old_session = {
                    let mut mgr = manager.lock().await;
                    mgr.remove_agent(&agent_id).ok()
                };
                if let Some(mut session) = old_session {
                    if let Err(e) = session.shutdown().await {
                        tracing::warn!("Hot-reload: error shutting down agent '{id}': {e}");
                    }
                    tracing::info!("Hot-reload: removed agent '{id}'");
                }
            }

            // Add new agents or update configs.
            let to_add: Vec<_> = new_config
                .agents
                .iter()
                .filter(|entry| !current_ids.contains(&entry.id))
                .cloned()
                .collect();

            // Update all configs and repo paths.
            {
                let mut configs = agent_configs.lock().await;
                let mut repo_paths = agent_repo_paths.lock().await;
                for entry in &new_config.agents {
                    let config = entry.to_agent_config(&base_dir);
                    repo_paths.insert(entry.id.clone(), config.repo_path.clone());
                    configs.insert(entry.id.clone(), config);
                }
            }

            // Spawn newly added agents.
            // Each spawn_agent call is done in a scope that drops the lock before the next.
            for entry in &to_add {
                let config = entry.to_agent_config(&base_dir);
                let id = entry.id.clone();
                let result = manager.lock().await.spawn_agent(config);
                match result {
                    Ok(_) => tracing::info!("Hot-reload: added agent '{id}'"),
                    Err(e) => {
                        tracing::error!("Hot-reload: failed to spawn agent '{id}': {e}");
                    }
                }
            }
        }
    });
}

// ── Saga / Agentrail handlers ─────────────────────────────────────────

/// Build a SagaResponse from a repo path (blocking I/O in spawn_blocking).
async fn saga_for_path(repo_path: PathBuf) -> SagaResponse {
    tokio::task::spawn_blocking(move || {
        let saga = atn_trail::reader::load_saga(&repo_path).ok().flatten();
        let steps = atn_trail::reader::list_steps(&repo_path).unwrap_or_default();
        let task_type = steps
            .iter()
            .find(|s| s.status == "in-progress")
            .and_then(|s| s.task_type.as_deref())
            .unwrap_or("");
        let trajectories = if task_type.is_empty() {
            vec![]
        } else {
            atn_trail::reader::load_trajectories(&repo_path, task_type).unwrap_or_default()
        };
        SagaResponse {
            saga,
            steps,
            trajectories,
        }
    })
    .await
    .unwrap_or_else(|_| SagaResponse {
        saga: None,
        steps: vec![],
        trajectories: vec![],
    })
}

/// Get saga info for the project (uses cwd).
async fn get_project_saga() -> Json<SagaResponse> {
    let cwd = std::env::current_dir().unwrap_or_default();
    Json(saga_for_path(cwd).await)
}

/// Get saga info for a specific agent's repo.
async fn get_agent_saga(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<SagaResponse>, StatusCode> {
    let repo_paths = state.agent_repo_paths.lock().await;
    let repo_path = repo_paths.get(&id).ok_or(StatusCode::NOT_FOUND)?.clone();
    drop(repo_paths);
    Ok(Json(saga_for_path(repo_path).await))
}

/// Trigger skill distillation for a task type.
async fn saga_distill(
    Json(body): Json<DistillBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let saga_path = cwd.join(".agentrail");
    let (output, code) = atn_trail::cli::agentrail_distill(&saga_path, &body.task_type)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "exit_code": code,
        "output": output,
    })))
}

// ── Wiki handlers ──────────────────────────────────────────────────────

/// List all wiki page titles.
async fn wiki_list_pages(State(state): State<AppState>) -> Json<Vec<String>> {
    Json(state.wiki.list_pages().await)
}

/// Get a wiki page by title. Returns JSON with ETag header.
async fn wiki_get_page(
    Path(title): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, StatusCode> {
    let page = state
        .wiki
        .get_page(&title)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    let etag = content_etag(&page.content);
    let html = render_wiki_content(&page.content);
    let body = WikiPageResponse {
        title: page.title,
        content: page.content,
        html,
        created_at: page.created_at,
        updated_at: page.updated_at,
    };

    let mut response = Json(body).into_response();
    response.headers_mut().insert("ETag", etag.parse().unwrap());
    Ok(response)
}

/// Create or update a wiki page. Requires If-Match header for existing pages.
async fn wiki_put_page(
    Path(title): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<WikiPutBody>,
) -> Result<Response, Response> {
    let existing = state.wiki.get_page(&title).await;
    let now = wiki_common::time::now();
    let is_update = existing.is_some();
    let prev_created_at = existing.as_ref().map(|p| p.created_at);

    if let Some(ref page) = existing {
        let current_etag = content_etag(&page.content);
        let if_match = headers
            .get("If-Match")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        match if_match {
            None => {
                return Err(conflict_response(page, &current_etag));
            }
            Some(provided) if provided != current_etag => {
                return Err(conflict_response(page, &current_etag));
            }
            _ => {}
        }
    }

    let created_at = prev_created_at.unwrap_or(now);
    let page = WikiPage {
        title: title.clone(),
        content: body.content.clone(),
        created_at,
        updated_at: now,
    };
    state.wiki.save_page(page).await;

    let etag = content_etag(&body.content);
    let html = render_wiki_content(&body.content);
    let resp = WikiPageResponse {
        title,
        content: body.content,
        html,
        created_at,
        updated_at: now,
    };

    let mut response = Json(resp).into_response();
    response.headers_mut().insert("ETag", etag.parse().unwrap());
    *response.status_mut() = if is_update {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok(response)
}

/// Patch a wiki page using structured operations. Requires If-Match or etag in body.
async fn wiki_patch_page(
    Path(title): Path<String>,
    State(state): State<AppState>,
    Json(patch): Json<PatchRequest>,
) -> Result<Response, Response> {
    let page = state
        .wiki
        .get_page(&title)
        .await
        .ok_or_else(|| StatusCode::NOT_FOUND.into_response())?;

    let current_etag = content_etag(&page.content);
    if patch.etag != current_etag {
        return Err(conflict_response(&page, &current_etag));
    }

    let new_content = apply_ops(&page.content, &patch.ops).map_err(|e| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": "patch failed", "op_index": e.op_index, "match_str": e.match_str })),
        )
            .into_response()
    })?;

    let now = wiki_common::time::now();
    let updated = WikiPage {
        title: title.clone(),
        content: new_content.clone(),
        created_at: page.created_at,
        updated_at: now,
    };
    state.wiki.save_page(updated).await;

    let etag = content_etag(&new_content);
    let html = render_wiki_content(&new_content);
    let resp = WikiPageResponse {
        title,
        content: new_content,
        html,
        created_at: page.created_at,
        updated_at: now,
    };

    let mut response = Json(resp).into_response();
    response.headers_mut().insert("ETag", etag.parse().unwrap());
    Ok(response)
}

/// Delete a wiki page. Requires If-Match header.
async fn wiki_delete_page(
    Path(title): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<StatusCode, Response> {
    let page = state
        .wiki
        .get_page(&title)
        .await
        .ok_or(StatusCode::NOT_FOUND.into_response())?;

    let current_etag = content_etag(&page.content);
    let if_match = headers
        .get("If-Match")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    match if_match {
        None => return Err(conflict_response(&page, &current_etag)),
        Some(provided) if provided != current_etag => {
            return Err(conflict_response(&page, &current_etag));
        }
        _ => {}
    }

    state.wiki.delete_page(&title).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Build a 409 Conflict response with current page state.
fn conflict_response(page: &WikiPage, current_etag: &str) -> Response {
    let body = WikiConflictResponse {
        error: "ETag mismatch — page was modified".to_string(),
        current_etag: current_etag.to_string(),
        page: WikiPageResponse {
            title: page.title.clone(),
            content: page.content.clone(),
            html: render_wiki_content(&page.content),
            created_at: page.created_at,
            updated_at: page.updated_at,
        },
    };
    let mut resp = Json(body).into_response();
    *resp.status_mut() = StatusCode::CONFLICT;
    resp.headers_mut()
        .insert("ETag", current_etag.parse().unwrap());
    resp
}

// ── Event log handlers ─────────────────────────────────────────────────

/// List event log entries. Optional `?since=N` returns entries after index N.
async fn list_events(
    Query(query): Query<EventLogQuery>,
    State(state): State<AppState>,
) -> Json<Vec<EventLogEntry>> {
    let log = state.event_log.lock().await;
    let since = query.since.unwrap_or(0);
    Json(log.iter().skip(since).cloned().collect())
}

/// Submit a push event for routing (e.g., from the UI or an external tool).
async fn submit_event(
    State(state): State<AppState>,
    Json(body): Json<SubmitEventBody>,
) -> StatusCode {
    let event = body.event;

    // Write the event to the source agent's outbox as a JSON file.
    let outbox_dir = state
        .base_dir
        .join(".atn")
        .join("outboxes")
        .join(&event.source_agent);
    let _ = tokio::fs::create_dir_all(&outbox_dir).await;
    let file_path = outbox_dir.join(format!("{}.json", event.id));
    if let Ok(json) = serde_json::to_string_pretty(&event) {
        let _ = tokio::fs::write(&file_path, json).await;
    }

    // The background router will pick it up on next poll.
    StatusCode::ACCEPTED
}
