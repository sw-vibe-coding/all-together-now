mod heat;
mod prs;
mod prs_stream;
mod router;
mod watchdog_actor;

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
use atn_core::spawn_spec::SpawnSpec;
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
    /// Original structured spawn spec for agents created via the New Agent dialog.
    /// Absent for agents loaded from agents.toml (which only has the flat shape).
    agent_specs: Arc<Mutex<HashMap<String, SpawnSpec>>>,
    /// Per-agent activity heat for the scale-UI treemap.
    heat: heat::HeatMap,
    /// Base directory for resolving relative paths.
    base_dir: PathBuf,
    /// Path to agents.toml for saving config.
    config_path: PathBuf,
    /// `/api/prs` registry: where PR records live + central repo
    /// for `git merge`. Populated from `--prs-dir` / `--central-repo`.
    prs: prs::PrsState,
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
    /// Watchdog flag — `true` when the agent is `running` but has been
    /// quiet longer than the configured stall threshold.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    stalled: bool,
    /// Seconds since the stall was first flagged. `None` if not stalled.
    #[serde(skip_serializing_if = "Option::is_none")]
    stalled_for_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spec: Option<SpawnSpec>,
    /// The actual shell command the PTY runs (derived from spec for new-dialog
    /// agents, raw `launch_command` for agents loaded from agents.toml).
    #[serde(skip_serializing_if = "Option::is_none")]
    launch_command: Option<String>,
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

    let parsed = parse_server_args();
    let config_path = parsed.config_path;
    let base_dir = config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let prs_dir = parsed
        .prs_dir
        .unwrap_or_else(|| base_dir.join(".atn").join("prs"));
    if let Err(e) = std::fs::create_dir_all(&prs_dir) {
        tracing::warn!("create --prs-dir {}: {e}", prs_dir.display());
    }
    // Canonicalize so the filesystem watcher sees the same path
    // notify uses internally (kqueue doesn't always honor relative
    // paths) and so saga lookups don't depend on process cwd.
    let prs_dir = prs_dir.canonicalize().unwrap_or(prs_dir);
    let central_repo = parsed.central_repo.unwrap_or_else(|| {
        prs_dir
            .parent()
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .unwrap_or_else(|| base_dir.clone())
    });
    tracing::info!(
        "/api/prs: prs_dir={} central_repo={}",
        prs_dir.display(),
        central_repo.display()
    );

    // Filesystem watcher fans out registry deltas to /api/prs/stream
    // subscribers. Spawned once at boot; lives for the process.
    let prs_broadcast = prs_stream::PrsBroadcast::new();
    prs_stream::spawn_watcher(prs_dir.clone(), prs_broadcast.clone());

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
    let mut agent_specs_map: HashMap<String, SpawnSpec> = HashMap::new();
    for entry in &project_config.agents {
        let config = entry.to_agent_config(&base_dir);
        agent_configs_map.insert(entry.id.clone(), config.clone());
        if let Some(spec) = &entry.spec {
            agent_specs_map.insert(entry.id.clone(), spec.clone());
        }
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

    let heat_map = heat::new_heat_map();
    // Spawn a heat tracker for every agent that made it up through startup.
    {
        let mgr = manager.lock().await;
        for id in mgr.agent_ids() {
            if let Ok(session) = mgr.get_session(id) {
                heat::spawn_heat_tracker(session.output_receiver(), heat_map.clone(), id.0.clone());
            }
        }
    }

    let state = SharedState {
        manager: manager.clone(),
        wiki,
        event_log,
        agent_repo_paths: Arc::new(Mutex::new(agent_repo_paths)),
        agent_configs: Arc::new(Mutex::new(agent_configs_map)),
        agent_specs: Arc::new(Mutex::new(agent_specs_map)),
        heat: heat_map,
        base_dir: base_dir.clone(),
        config_path: config_path.clone(),
        prs: prs::PrsState::new(prs_dir, central_repo, prs_broadcast),
    };

    // Spawn config hot-reload watcher.
    spawn_config_watcher(config_path, base_dir.clone(), state.clone());

    // Spawn the watchdog action loop — turns per-agent stall signals
    // into Ctrl-C + blocked_notice escalation.
    let coordinator_hint: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let _watchdog_handle = watchdog_actor::spawn_watchdog_actor(
        state.manager.clone(),
        base_dir,
        coordinator_hint,
    );
    tracing::info!("Watchdog actor started (polling every 1s)");

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/agents", get(list_agents).post(create_agent))
        .route("/api/agents/graph", get(agent_dependency_graph))
        .route("/api/agents/heat", get(list_agents_heat))
        .route("/api/agents/save", post(save_config))
        .route(
            "/api/agents/{id}",
            axum::routing::put(update_agent).delete(delete_agent),
        )
        .route("/api/agents/{id}/sse", get(agent_sse))
        .route("/api/agents/{id}/screenshot", get(agent_screenshot))
        .route("/api/agents/{id}/input", post(agent_input))
        .route("/api/agents/{id}/ctrl-c", post(agent_ctrl_c))
        .route("/api/agents/{id}/resize", post(agent_resize))
        .route("/api/agents/{id}/state", get(agent_state))
        .route("/api/agents/{id}/restart", post(agent_restart))
        .route("/api/agents/{id}/reconnect", post(agent_reconnect))
        .route("/api/agents/{id}/stop", post(stop_agent))
        .route("/api/saga", get(get_project_saga))
        .route("/api/saga/distill", post(saga_distill))
        .route("/api/agents/{id}/saga", get(get_agent_saga))
        .route("/api/events", get(list_events).post(submit_event))
        .route("/api/prs", get(prs::list_prs))
        .route("/api/prs/stream", get(prs_stream::pr_stream))
        .route("/api/prs/{id}", get(prs::get_pr))
        .route("/api/prs/{id}/merge", post(prs::merge_pr))
        .route("/api/prs/{id}/reject", post(prs::reject_pr))
        .route("/api/wiki", get(wiki_list_pages))
        .route(
            "/api/wiki/{*title}",
            get(wiki_get_page)
                .put(wiki_put_page)
                .patch(wiki_patch_page)
                .delete(wiki_delete_page),
        )
        .route("/wiki", get(wiki_html_index))
        .route("/wiki/{*title}", get(wiki_html_page))
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    // Port: ATN_PORT overrides the default 7500. Passing ATN_PORT=0 lets the
    // OS pick a free port; we then print the resolved port so integration
    // tests (and humans) can discover it.
    let port = std::env::var("ATN_PORT").unwrap_or_else(|_| "7500".to_string());
    let addr = format!("0.0.0.0:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(
                "Cannot bind to {addr}: {e}. Another atn-server is probably running. \
                 Kill it first with: pkill -f atn-server"
            );
            std::process::exit(1);
        }
    };
    let bound = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| addr.clone());
    tracing::info!("Listening on http://{bound}");
    // Machine-readable marker so test harnesses can regex the port back out.
    println!("atn-server ready on {bound}");

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
                        session.watchdog(),
                    )
                })
            })
            .collect()
    };
    let specs = state.agent_specs.lock().await.clone();
    let configs = state.agent_configs.lock().await.clone();
    let mut agents = Vec::with_capacity(pending.len());
    for (id, name, role, state_lock, watchdog_lock) in pending {
        let s = state_lock.read().await;
        let spec = specs.get(&id).cloned();
        let launch_command = configs
            .get(&id)
            .map(|c| c.launch_command.clone())
            .filter(|s| !s.is_empty());
        let (stalled, stalled_for_secs) = {
            let w = watchdog_lock.read().await;
            let now = std::time::Instant::now();
            (w.stalled, w.stalled_for_secs(now))
        };
        agents.push(AgentInfo {
            id,
            name,
            role,
            state: s.clone(),
            stalled,
            stalled_for_secs,
            spec,
            launch_command,
        });
    }
    Json(agents)
}

/// Returns one `HeatInfo` per live agent, driving the scale-UI treemap.
async fn list_agents_heat(State(state): State<AppState>) -> Json<Vec<heat::HeatInfo>> {
    // Snapshot current agent states without holding the manager lock across
    // the state RwLock await calls.
    let agent_state_handles: Vec<(String, Arc<tokio::sync::RwLock<AgentState>>)> = {
        let mgr = state.manager.lock().await;
        mgr.agent_ids()
            .iter()
            .filter_map(|id| {
                mgr.get_session(id)
                    .ok()
                    .map(|s| (id.0.clone(), s.state()))
            })
            .collect()
    };

    // Hold the write lock across the read so we can decay every entry to
    // the current instant — the EWMA is update-driven, so a quiet agent's
    // rate only moves when someone rolls the clock forward.
    let mut heat_guard = state.heat.lock().await;
    let now = std::time::Instant::now();
    for h in heat_guard.values_mut() {
        h.update(0, now);
    }

    let mut out = Vec::with_capacity(agent_state_handles.len());
    for (id, state_arc) in agent_state_handles {
        let agent_state = state_arc.read().await.clone();
        let boost = heat::state_boost(&agent_state);
        let (score, bytes_per_sec) = match heat_guard.get(&id) {
            Some(h) => (heat::compute_score(h, &agent_state), h.bytes_per_sec),
            None => {
                // Heat tracker hasn't spawned yet or it exited early. Report
                // a zero-activity entry so the UI can still place the tile.
                let empty = heat::HeatState::new(now);
                (heat::compute_score(&empty, &agent_state), 0.0)
            }
        };
        out.push(heat::HeatInfo {
            id,
            heat: score,
            bytes_per_sec,
            state_boost: boost,
        });
    }

    Json(out)
}

/// Returns current state for a single agent.
async fn agent_state(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<AgentInfo>, StatusCode> {
    let agent_id = AgentId(id);
    let (name, role, state_lock, watchdog_lock) = {
        let mgr = state.manager.lock().await;
        let session = mgr
            .get_session(&agent_id)
            .map_err(|_| StatusCode::NOT_FOUND)?;
        (
            session.name().to_string(),
            session.role().to_string(),
            session.state(),
            session.watchdog(),
        )
    };
    let s = state_lock.read().await;
    let spec = state.agent_specs.lock().await.get(&agent_id.0).cloned();
    let launch_command = state
        .agent_configs
        .lock()
        .await
        .get(&agent_id.0)
        .map(|c| c.launch_command.clone())
        .filter(|s| !s.is_empty());
    let (stalled, stalled_for_secs) = {
        let w = watchdog_lock.read().await;
        let now = std::time::Instant::now();
        (w.stalled, w.stalled_for_secs(now))
    };
    Ok(Json(AgentInfo {
        id: agent_id.0,
        name,
        role,
        state: s.clone(),
        stalled,
        stalled_for_secs,
        spec,
        launch_command,
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

#[derive(Deserialize)]
struct ScreenshotParams {
    #[serde(default = "default_screenshot_format")]
    format: String,
    #[serde(default = "default_screenshot_rows")]
    rows: usize,
    #[serde(default = "default_screenshot_cols")]
    cols: usize,
}

fn default_screenshot_format() -> String {
    "text".to_string()
}
fn default_screenshot_rows() -> usize {
    40
}
fn default_screenshot_cols() -> usize {
    120
}

/// GET /api/agents/{id}/screenshot — return a rendered terminal
/// snapshot built from the tail of the agent's transcript. Delegates
/// parsing to `atn_pty::snapshot`; this handler just plumbs the
/// query-string format + size into the right Content-Type and body.
async fn agent_screenshot(
    Path(id): Path<String>,
    Query(params): Query<ScreenshotParams>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, String)> {
    // Validate format before touching the filesystem — this path is
    // trivially cachable on a bad-request retry.
    let format = params.format.to_ascii_lowercase();
    match format.as_str() {
        "text" | "ansi" | "html" => {}
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unknown format '{other}'; expected one of text|ansi|html"),
            ));
        }
    };

    // Clamp geometry. vt100 panics on zero; huge values would read a
    // pathological chunk of the transcript.
    let rows = params.rows.clamp(1, 500);
    let cols = params.cols.clamp(1, 500);

    // Confirm the agent exists (404 before we go to disk) and capture
    // the log_dir the manager was started with.
    let transcript_path = {
        let mgr = state.manager.lock().await;
        let _session = mgr
            .get_session(&AgentId(id.clone()))
            .map_err(|_| (StatusCode::NOT_FOUND, format!("agent '{id}' not found")))?;
        let log_dir = mgr.log_dir().map(std::path::Path::to_path_buf);
        log_dir.map(|dir| dir.join(&id).join("transcript.log"))
    };

    // Read the tail of the transcript (or an empty buffer if logging
    // was disabled for this session / the file hasn't flushed yet).
    let bytes = match transcript_path {
        Some(path) => read_transcript_tail(&path, rows, cols).await,
        None => Vec::new(),
    };

    let snap = atn_pty::snapshot::snapshot_from_bytes(&bytes, rows, cols);
    let (content_type, body) = match format.as_str() {
        "text" => ("text/plain; charset=utf-8", snap.render_text()),
        // ANSI is also `text/plain`; it contains raw SGR escape codes
        // and is meant to be piped into another terminal.
        "ansi" => ("text/plain; charset=utf-8", snap.render_ansi()),
        "html" => ("text/html; charset=utf-8", snap.render_html()),
        _ => unreachable!("validated above"),
    };

    Ok(([(axum::http::header::CONTENT_TYPE, content_type)], body).into_response())
}

async fn read_transcript_tail(path: &std::path::Path, rows: usize, cols: usize) -> Vec<u8> {
    // Keep enough bytes to fully reconstruct a rows×cols screen even
    // with modest ANSI-escape overhead. Empirically ~8× the cell count
    // is comfortable; cap the read at 256 KiB to bound memory.
    let want = (rows * cols).saturating_mul(8).min(256 * 1024);
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return Vec::new();
    };
    let len = metadata.len();
    let start = len.saturating_sub(want as u64);

    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return Vec::new();
    };
    use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
    if file.seek(SeekFrom::Start(start)).await.is_err() {
        return Vec::new();
    }
    let mut buf = Vec::with_capacity((len - start) as usize);
    let _ = file.read_to_end(&mut buf).await;
    buf
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
        InputEvent::HumanText { text: payload.text }
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

    rearm_heat_tracker(&state, &id).await;

    tracing::info!("Restarted agent: {id}");
    Ok(StatusCode::OK)
}

/// (Re)attach a heat tracker to an agent's current output stream. Called
/// whenever we spawn or respawn a session. If a tracker was already running
/// on the previous session, its channel has since closed, so it will drain
/// and exit on its own.
async fn rearm_heat_tracker(state: &AppState, id: &str) {
    let mgr = state.manager.lock().await;
    if let Ok(session) = mgr.get_session(&AgentId(id.to_string())) {
        heat::spawn_heat_tracker(
            session.output_receiver(),
            state.heat.clone(),
            id.to_string(),
        );
    }
}

/// Reconnect an agent by re-running its launch command without sending
/// Ctrl-C to whatever was inside the old PTY. For mosh/ssh+tmux agents the
/// composed command (`mosh USER@HOST -- tmux new-session -A -s atn-NAME ...`)
/// naturally re-attaches to the still-running remote tmux session.
async fn agent_reconnect(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let agent_id = AgentId(id.clone());

    let config = {
        let configs = state.agent_configs.lock().await;
        configs.get(&id).cloned().ok_or(StatusCode::NOT_FOUND)?
    };

    // Hard-kill any existing session (no Ctrl-C). The remote tmux lives on.
    let old_session = {
        let mut mgr = state.manager.lock().await;
        mgr.remove_agent(&agent_id).ok()
    };
    if let Some(mut session) = old_session {
        let _ = session.hard_kill().await;
    }

    {
        let mut mgr = state.manager.lock().await;
        mgr.spawn_agent(config).map_err(|e| {
            tracing::error!("Failed to reconnect agent '{id}': {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    rearm_heat_tracker(&state, &id).await;

    tracing::info!("Reconnected agent: {id}");
    Ok(StatusCode::OK)
}

// ── Agent CRUD ───────────────────────────────────────────────────────

/// Request body for updating an agent's config.
#[derive(Deserialize)]
struct UpdateAgentBody {
    name: Option<String>,
    repo_path: Option<String>,
    role: Option<String>,
    setup_commands: Option<Vec<String>>,
    launch_command: Option<String>,
    /// Structured spawn spec. When present, takes precedence over any
    /// flat fields above: the server re-composes launch_command from
    /// the spec and stores the spec in agent_specs so subsequent Save
    /// / Config flows see the structured shape.
    spec: Option<SpawnSpec>,
}

fn parse_role(s: &str) -> atn_core::agent::AgentRole {
    match s.to_lowercase().as_str() {
        "qa" => atn_core::agent::AgentRole::QA,
        "pm" => atn_core::agent::AgentRole::PM,
        "coordinator" => atn_core::agent::AgentRole::Coordinator,
        _ => atn_core::agent::AgentRole::Developer,
    }
}

#[derive(Serialize)]
struct FieldError {
    error: &'static str,
    missing: Vec<&'static str>,
}

/// Create a new agent session from a structured SpawnSpec.
///
/// Validates required fields per transport, composes the shell command,
/// spawns the PTY, and stores the spec alongside the runtime config.
async fn create_agent(
    State(state): State<AppState>,
    Json(spec): Json<SpawnSpec>,
) -> Result<(StatusCode, Json<AgentInfo>), (StatusCode, Json<FieldError>)> {
    if let Err(missing) = spec.validate() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(FieldError {
                error: "invalid or missing fields",
                missing,
            }),
        ));
    }

    let id = spec.name.trim().to_string();

    // Check if agent already exists.
    {
        let configs = state.agent_configs.lock().await;
        if configs.contains_key(&id) {
            return Err((
                StatusCode::CONFLICT,
                Json(FieldError {
                    error: "agent already exists",
                    missing: vec!["name"],
                }),
            ));
        }
    }

    let role = parse_role(&spec.role);
    let launch_command = spec.compose_command();

    // For local transport, working_dir is the real repo_path. For remote, we
    // use base_dir as a placeholder (the real dir lives on the target host).
    let repo_path = if spec.transport == atn_core::spawn_spec::Transport::Local {
        let wd = std::path::Path::new(&spec.working_dir);
        if wd.is_absolute() {
            wd.to_path_buf()
        } else {
            state.base_dir.join(wd)
        }
    } else {
        state.base_dir.clone()
    };

    let display_name = format!("{} ({})", spec.name, spec.project_label());

    let config = AgentConfig {
        id: AgentId(id.clone()),
        name: display_name.clone(),
        repo_path: repo_path.clone(),
        role,
        setup_commands: Vec::new(),
        launch_command: launch_command.clone(),
        watchdog: spec.watchdog,
    };

    {
        let mut mgr = state.manager.lock().await;
        mgr.spawn_agent(config.clone()).map_err(|e| {
            tracing::error!("spawn failed for {id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(FieldError {
                    error: "failed to spawn agent",
                    missing: vec![],
                }),
            )
        })?;
    }

    rearm_heat_tracker(&state, &id).await;

    {
        let mut configs = state.agent_configs.lock().await;
        configs.insert(id.clone(), config.clone());
    }
    {
        let mut repo_paths = state.agent_repo_paths.lock().await;
        repo_paths.insert(id.clone(), repo_path);
    }
    {
        let mut specs = state.agent_specs.lock().await;
        specs.insert(id.clone(), spec.clone());
    }

    tracing::info!("Created agent: {id} (transport={:?})", spec.transport);

    let state_lock = {
        let mgr = state.manager.lock().await;
        mgr.get_session(&AgentId(id.clone()))
            .map(|sess| sess.state())
            .ok()
    };
    let current_state = if let Some(lock) = state_lock {
        lock.read().await.clone()
    } else {
        AgentState::Starting
    };

    Ok((
        StatusCode::CREATED,
        Json(AgentInfo {
            id,
            name: display_name,
            role: format!("{:?}", config.role).to_lowercase(),
            state: current_state,
            // Freshly created agent: watchdog hasn't seen any output
            // yet, so it's definitionally not stalled.
            stalled: false,
            stalled_for_secs: None,
            spec: Some(spec),
            launch_command: Some(launch_command),
        }),
    ))
}

/// Delete (destroy) an agent session.
async fn delete_agent(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
    let agent_id = AgentId(id.clone());

    // For remote (mosh/ssh+tmux) agents, send `C-b :kill-session <Enter>` over
    // the PTY before tearing down so the tmux session doesn't survive on the
    // remote host. Local agents skip this step.
    let is_remote = state
        .agent_specs
        .lock()
        .await
        .get(&id)
        .map(|s| s.transport != atn_core::spawn_spec::Transport::Local)
        .unwrap_or(false);
    if is_remote {
        let tx = {
            let mgr = state.manager.lock().await;
            mgr.get_session(&agent_id).ok().map(|s| s.input_sender())
        };
        if let Some(tx) = tx {
            let _ = tx
                .send(InputEvent::RawBytes {
                    bytes: b"\x02:kill-session\r".to_vec(),
                })
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    // Remove session.
    let old_session = {
        let mut mgr = state.manager.lock().await;
        mgr.remove_agent(&agent_id)
            .map_err(|_| StatusCode::NOT_FOUND)?
    };

    // Shutdown gracefully.
    let mut session = old_session;
    let _ = session.shutdown().await;

    // Remove from config, repo paths, specs, and heat map.
    {
        let mut configs = state.agent_configs.lock().await;
        configs.remove(&id);
    }
    {
        let mut repo_paths = state.agent_repo_paths.lock().await;
        repo_paths.remove(&id);
    }
    {
        let mut specs = state.agent_specs.lock().await;
        specs.remove(&id);
    }
    {
        let mut heat = state.heat.lock().await;
        heat.remove(&id);
    }

    tracing::info!("Deleted agent: {id}");
    Ok(StatusCode::NO_CONTENT)
}

/// Update an agent's configuration. Restarts the agent with new config.
///
/// Two input shapes:
/// - Structured: `body.spec` is present → validated, launch_command is
///   recomposed from the spec, and the stored `SpawnSpec` is replaced so
///   future Save / Config flows see the new structure. Other fields on
///   `body` are ignored.
/// - Legacy flat: `body.spec` is None → the individual `name` /
///   `repo_path` / `role` / `setup_commands` / `launch_command` fields
///   overlay the current `AgentConfig`. The stored `SpawnSpec` is left
///   alone (if any).
async fn update_agent(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Json(body): Json<UpdateAgentBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    let agent_id = AgentId(id.clone());

    let mut config = {
        let configs = state.agent_configs.lock().await;
        configs
            .get(&id)
            .cloned()
            .ok_or((StatusCode::NOT_FOUND, format!("agent '{id}' not found")))?
    };

    // Apply updates. Structured path takes precedence.
    let new_spec: Option<SpawnSpec> = if let Some(spec) = body.spec {
        if let Err(missing) = spec.validate() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("invalid spec — missing/invalid: {}", missing.join(", ")),
            ));
        }
        // Derive AgentConfig fields from the spec.
        config.role = parse_role(&spec.role);
        config.launch_command = spec.compose_command();
        config.name = format!("{} ({})", spec.name, spec.project_label());
        // repo_path: local → working_dir; remote → base_dir placeholder.
        config.repo_path = if spec.transport == atn_core::spawn_spec::Transport::Local {
            let wd = std::path::Path::new(&spec.working_dir);
            if wd.is_absolute() {
                wd.to_path_buf()
            } else {
                state.base_dir.join(wd)
            }
        } else {
            state.base_dir.clone()
        };
        Some(spec)
    } else {
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
        None
    };

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
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("spawn failed: {e}"),
            )
        })?;
    }

    rearm_heat_tracker(&state, &id).await;

    // Update stored config, repo path, and spec.
    {
        let mut configs = state.agent_configs.lock().await;
        configs.insert(id.clone(), config.clone());
    }
    {
        let mut repo_paths = state.agent_repo_paths.lock().await;
        repo_paths.insert(id.clone(), config.repo_path);
    }
    if let Some(spec) = new_spec {
        let mut specs = state.agent_specs.lock().await;
        specs.insert(id.clone(), spec);
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
    let specs = state.agent_specs.lock().await;

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
            let spec = specs.get(&c.id.0).cloned();
            // For spec-backed agents, blank the flat launch_command so the
            // serialized TOML is clean — on load, to_agent_config recomposes
            // the command from the spec anyway.
            let launch_command = if spec.is_some() {
                String::new()
            } else {
                c.launch_command.clone()
            };
            atn_core::config::AgentEntry {
                id: c.id.0.clone(),
                name: c.name.clone(),
                repo_path,
                role: c.role.clone(),
                setup_commands: c.setup_commands.clone(),
                launch_command,
                spec,
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
    role: String,
    state: String,
    blocked_on: Vec<String>,
}

async fn agent_dependency_graph(State(state): State<AppState>) -> Json<Vec<DepGraphNode>> {
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

    let mut nodes = Vec::with_capacity(pending.len());
    for (id, name, role, state_lock) in pending {
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
            role,
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
    let agent_specs = state.agent_specs.clone();
    let heat_map = state.heat.clone();

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

            // Update all configs, repo paths, and specs.
            {
                let mut configs = agent_configs.lock().await;
                let mut repo_paths = agent_repo_paths.lock().await;
                let mut specs = agent_specs.lock().await;
                for entry in &new_config.agents {
                    let config = entry.to_agent_config(&base_dir);
                    repo_paths.insert(entry.id.clone(), config.repo_path.clone());
                    configs.insert(entry.id.clone(), config);
                    match &entry.spec {
                        Some(s) => {
                            specs.insert(entry.id.clone(), s.clone());
                        }
                        None => {
                            // Entry no longer has a spec — drop any stale
                            // one so saved TOML stops round-tripping it.
                            specs.remove(&entry.id);
                        }
                    }
                }
                // Drop specs for agents removed from the config entirely.
                for id in &to_remove {
                    specs.remove(id);
                }
            }

            // Drop heat entries for removed agents.
            {
                let mut heat = heat_map.lock().await;
                for id in &to_remove {
                    heat.remove(id);
                }
            }

            // Spawn newly added agents.
            // Each spawn_agent call is done in a scope that drops the lock before the next.
            for entry in &to_add {
                let config = entry.to_agent_config(&base_dir);
                let id = entry.id.clone();
                let spawn_result = manager.lock().await.spawn_agent(config);
                match spawn_result {
                    Ok(_) => {
                        tracing::info!("Hot-reload: added agent '{id}'");
                        // Attach a fresh heat tracker for the new session.
                        let mgr_guard = manager.lock().await;
                        if let Ok(session) = mgr_guard.get_session(&AgentId(id.clone())) {
                            heat::spawn_heat_tracker(
                                session.output_receiver(),
                                heat_map.clone(),
                                id.clone(),
                            );
                        }
                    }
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
///
/// Honors `If-None-Match`: when the client's last-seen ETag matches
/// the current content ETag, return `304 Not Modified` with no body.
/// The wiki side-panel's 5 s poll leans on this to skip re-rendering
/// unchanged pages over the wire.
async fn wiki_get_page(
    Path(title): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, StatusCode> {
    let page = state
        .wiki
        .get_page(&title)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    let etag = content_etag(&page.content);

    // If-None-Match short-circuit: no body on a match, just 304 + ETag.
    if let Some(inm) = headers
        .get("If-None-Match")
        .and_then(|v| v.to_str().ok())
        && inm == etag
    {
        let mut resp = axum::http::Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .body(axum::body::Body::empty())
            .unwrap();
        resp.headers_mut().insert("ETag", etag.parse().unwrap());
        return Ok(resp);
    }

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

// ── Wiki HTML handlers ────────────────────────────────────────────────

/// Render a navigation sidebar listing all wiki pages.
fn wiki_nav(pages: &[String], active: &str) -> String {
    let mut nav = String::from(r#"<nav class="wiki-nav"><h2>Pages</h2><ul>"#);
    for p in pages {
        let cls = if p == active {
            r#" class="active""#
        } else {
            ""
        };
        let display = p.replace("__", " / ");
        nav.push_str(&format!(
            r#"<li{cls}><a href="/wiki/{p}">{display}</a></li>"#
        ));
    }
    nav.push_str(r#"</ul><a class="new-page-link" href="/wiki?action=new">+ New page</a></nav>"#);
    nav
}

/// CSS shared across all wiki HTML pages.
const WIKI_CSS: &str = r##"
  * { margin:0; padding:0; box-sizing:border-box; }
  body { font-family: system-ui, -apple-system, sans-serif; background:#0a0e1a; color:#c9d1d9; display:flex; min-height:100vh; }
  .wiki-nav { width:220px; background:#16213e; padding:16px; border-right:1px solid #0f3460; flex-shrink:0; overflow-y:auto; }
  .wiki-nav h2 { font-size:14px; color:#e94560; margin-bottom:12px; text-transform:uppercase; letter-spacing:1px; }
  .wiki-nav ul { list-style:none; }
  .wiki-nav li { margin-bottom:4px; }
  .wiki-nav a { color:#58a6ff; text-decoration:none; font-size:14px; display:block; padding:4px 8px; border-radius:4px; }
  .wiki-nav a:hover { background:#0f3460; }
  .wiki-nav li.active a { background:#0f3460; color:#e94560; font-weight:600; }
  .new-page-link { display:block; margin-top:12px; padding:6px 8px; color:#34d399; font-size:13px; font-weight:600; }
  .wiki-main { flex:1; padding:32px 48px; max-width:900px; overflow-y:auto; }
  .wiki-main h1 { color:#e2e8f0; margin-bottom:16px; font-size:28px; border-bottom:1px solid #0f3460; padding-bottom:8px; }
  .wiki-main h2 { color:#c9d1d9; margin:24px 0 8px; font-size:20px; }
  .wiki-main h3 { color:#9ca3af; margin:16px 0 6px; font-size:16px; }
  .wiki-main p { line-height:1.7; margin-bottom:12px; }
  .wiki-main ul, .wiki-main ol { margin:8px 0 12px 24px; line-height:1.7; }
  .wiki-main li { margin-bottom:4px; }
  .wiki-main code { background:#1e293b; padding:2px 6px; border-radius:3px; font-size:0.9em; }
  .wiki-main pre { background:#1e293b; padding:12px; border-radius:6px; overflow-x:auto; margin:12px 0; }
  .wiki-main a { color:#58a6ff; text-decoration:none; }
  .wiki-main a:hover { text-decoration:underline; }
  .page-list { list-style:none; margin:0; }
  .page-list li { padding:8px 12px; border-bottom:1px solid #1e293b; }
  .page-list a { font-size:16px; }
  .back-link { display:inline-block; margin-bottom:16px; color:#9ca3af; font-size:13px; }
  .toolbar { display:flex; gap:8px; margin-bottom:16px; align-items:center; }
  .toolbar .back-link { margin-bottom:0; }
  .btn { padding:6px 14px; border:1px solid #0f3460; border-radius:4px; background:transparent; color:#c9d1d9; cursor:pointer; font-size:13px; text-decoration:none; display:inline-block; }
  .btn:hover { background:#0f3460; }
  .btn-primary { background:#0f3460; color:#58a6ff; }
  .btn-primary:hover { background:#1a4a7a; }
  .spacer { flex:1; }
  textarea.editor { width:100%; min-height:400px; background:#1e293b; color:#c9d1d9; border:1px solid #0f3460; border-radius:6px; padding:12px; font-family:'SF Mono',Monaco,monospace; font-size:14px; line-height:1.6; resize:vertical; }
  textarea.editor:focus { outline:none; border-color:#58a6ff; }
  input.title-input { background:#1e293b; color:#c9d1d9; border:1px solid #0f3460; border-radius:4px; padding:8px 12px; font-size:16px; width:100%; margin-bottom:12px; }
  input.title-input:focus { outline:none; border-color:#58a6ff; }
  .msg { padding:8px 12px; border-radius:4px; margin-bottom:12px; font-size:13px; }
  .msg-err { background:#7f1d1d; color:#fca5a5; }
  .msg-ok { background:#064e3b; color:#6ee7b7; }
  .wiki-redlink { color:#e94560 !important; border-bottom:1px dashed #e94560; }
  .wiki-redlink:hover { color:#ff6b81 !important; }
  .create-hint { color:#9ca3af; font-style:italic; margin-bottom:16px; }
"##;

/// JS for the edit/create form — uses fetch to PUT via the JSON API.
const WIKI_EDIT_JS: &str = r##"
async function savePage(title, isNew) {
    const content = document.getElementById('editor').value;
    const etag = document.getElementById('etag')?.value || '';
    const msgEl = document.getElementById('msg');
    msgEl.textContent = '';
    msgEl.className = 'msg';

    const headers = { 'Content-Type': 'application/json' };
    if (etag) headers['If-Match'] = etag;

    try {
        const res = await fetch('/api/wiki/' + encodeURIComponent(title), {
            method: 'PUT',
            headers,
            body: JSON.stringify({ content })
        });
        if (res.ok) {
            window.location.href = '/wiki/' + encodeURIComponent(title);
        } else {
            const data = await res.json().catch(() => null);
            const detail = data?.error || res.statusText;
            msgEl.textContent = 'Save failed: ' + detail;
            msgEl.className = 'msg msg-err';
            // Update etag if server returned one (conflict recovery)
            const newEtag = res.headers.get('ETag');
            if (newEtag && document.getElementById('etag')) {
                document.getElementById('etag').value = newEtag;
            }
        }
    } catch (e) {
        msgEl.textContent = 'Network error: ' + e.message;
        msgEl.className = 'msg msg-err';
    }
}

function createPage() {
    const titleInput = document.getElementById('new-title');
    let title = titleInput.value.trim();
    if (!title) { titleInput.focus(); return; }
    // Convert spaces/slashes to __ for wiki title format
    title = title.replace(/[\s\/]+/g, '__');
    savePage(title, true);
}
"##;

/// Shared wiki page shell.
fn wiki_html_shell(nav: &str, title: &str, body: &str, extra_js: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} — ATN Wiki</title>
<style>{WIKI_CSS}</style>
</head><body>
{nav}
<main class="wiki-main">{body}</main>
<script>{extra_js}</script>
</body></html>"##
    )
}

/// GET /wiki — index page listing all wiki pages, or new-page form.
async fn wiki_html_index(
    Query(query): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Html<String> {
    let pages = state.wiki.list_pages().await;
    let nav = wiki_nav(&pages, "");

    if query.get("action").map(|s| s.as_str()) == Some("new") {
        // New page form
        let body = r#"<a class="back-link" href="/wiki">&larr; All pages</a>
<h1>New Page</h1>
<div id="msg" class="msg"></div>
<input class="title-input" id="new-title" placeholder="Page title (use __ for subpages, e.g. Coordination__Notes)" autofocus>
<textarea class="editor" id="editor" placeholder="Page content (markdown)"></textarea>
<div class="toolbar" style="margin-top:12px;">
  <button class="btn btn-primary" onclick="createPage()">Create</button>
  <a class="btn" href="/wiki">Cancel</a>
</div>"#;
        return Html(wiki_html_shell(&nav, "New Page", body, WIKI_EDIT_JS));
    }

    let mut body = String::from(
        r#"<div class="toolbar"><h1 style="flex:1">Wiki</h1><a class="btn btn-primary" href="/wiki?action=new">+ New page</a></div><ul class="page-list">"#,
    );
    for p in &pages {
        let display = p.replace("__", " / ");
        body.push_str(&format!(r#"<li><a href="/wiki/{p}">{display}</a></li>"#));
    }
    body.push_str("</ul>");
    Html(wiki_html_shell(&nav, "Wiki", &body, ""))
}

/// Post-process rendered HTML to fix wiki-link hrefs and add red-link styling.
/// The wiki-common renderer produces `<a class="wiki-link" data-wiki-link="Slug" href="#/wiki/Slug">`.
/// We rewrite hrefs to `/wiki/Slug` and add red-link class for missing pages.
fn wikify_links(html: &str, existing_pages: &[String]) -> String {
    let mut result = String::with_capacity(html.len());
    let mut rest = html;
    let marker = r#"class="wiki-link" data-wiki-link=""#;
    while let Some(pos) = rest.find(marker) {
        // Find the start of the <a tag
        let tag_start = rest[..pos].rfind("<a ").unwrap_or(pos);
        result.push_str(&rest[..tag_start]);
        let after_marker = &rest[pos + marker.len()..];
        if let Some(quote_end) = after_marker.find('"') {
            let slug = &after_marker[..quote_end];
            let exists = existing_pages.iter().any(|p| p == slug);
            let cls = if exists {
                "wiki-link"
            } else {
                "wiki-link wiki-redlink"
            };
            // Find the rest of the tag after data-wiki-link="..."
            let after_data = &after_marker[quote_end + 1..];
            // Find href="..." and replace it
            if let Some(href_start) = after_data.find("href=\"") {
                let after_href = &after_data[href_start + 6..];
                if let Some(href_end) = after_href.find('"') {
                    let after_href_attr = &after_href[href_end + 1..];
                    // Find the closing > of the <a> tag
                    if let Some(tag_end) = after_href_attr.find('>') {
                        let inner_and_rest = &after_href_attr[tag_end + 1..];
                        // Find </a>
                        if let Some(close) = inner_and_rest.find("</a>") {
                            let inner = &inner_and_rest[..close];
                            let title_attr = if exists {
                                ""
                            } else {
                                r#" title="Create this page""#
                            };
                            result.push_str(&format!(
                                r#"<a class="{cls}" href="/wiki/{slug}"{title_attr}>{inner}</a>"#
                            ));
                            rest = &inner_and_rest[close + 4..];
                            continue;
                        }
                    }
                }
            }
            // Fallback: couldn't parse, keep original
            result.push_str(&rest[tag_start..pos + marker.len() + quote_end + 1]);
            rest = &after_marker[quote_end + 1..];
        } else {
            result.push_str(&rest[tag_start..pos + marker.len()]);
            rest = after_marker;
        }
    }
    result.push_str(rest);
    result
}

/// GET /wiki/{title} — render a wiki page, edit form, or create form for new pages.
async fn wiki_html_page(
    Path(title): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Html<String> {
    let page = state.wiki.get_page(&title).await;
    let pages = state.wiki.list_pages().await;
    let nav = wiki_nav(&pages, &title);
    let display_title = title.replace("__", " / ");

    // Page does not exist — show create form
    if page.is_none() {
        let body = format!(
            r#"<div class="toolbar">
  <a class="back-link" href="/wiki">&larr; All pages</a>
  <span class="spacer"></span>
</div>
<h1>{display_title}</h1>
<p class="create-hint">This page does not exist yet. Start writing to create it.</p>
<div id="msg" class="msg"></div>
<textarea class="editor" id="editor" placeholder="Page content (markdown)" autofocus># {display_title}

</textarea>
<div class="toolbar" style="margin-top:12px;">
  <button class="btn btn-primary" onclick="savePage('{title}', true)">Create</button>
  <a class="btn" href="/wiki">Cancel</a>
</div>"#
        );
        return Html(wiki_html_shell(
            &nav,
            &format!("Create {display_title}"),
            &body,
            WIKI_EDIT_JS,
        ));
    }

    let page = page.unwrap();
    let etag = content_etag(&page.content);

    if query.get("action").map(|s| s.as_str()) == Some("edit") {
        // Edit form
        let escaped_content = page
            .content
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        let body = format!(
            r#"<div class="toolbar">
  <a class="back-link" href="/wiki/{title}">&larr; Back to page</a>
  <span class="spacer"></span>
</div>
<h1>Editing: {display_title}</h1>
<div id="msg" class="msg"></div>
<input type="hidden" id="etag" value="{etag}">
<textarea class="editor" id="editor">{escaped_content}</textarea>
<div class="toolbar" style="margin-top:12px;">
  <button class="btn btn-primary" onclick="savePage('{title}', false)">Save</button>
  <a class="btn" href="/wiki/{title}">Cancel</a>
</div>"#
        );
        return Html(wiki_html_shell(
            &nav,
            &format!("Edit {display_title}"),
            &body,
            WIKI_EDIT_JS,
        ));
    }

    // Read view with wiki-links processed
    let html = render_wiki_content(&page.content);
    let html = wikify_links(&html, &pages);
    let body = format!(
        r#"<div class="toolbar">
  <a class="back-link" href="/wiki">&larr; All pages</a>
  <span class="spacer"></span>
  <a class="btn" href="/wiki/{title}?action=edit">Edit</a>
</div>
<h1>{display_title}</h1>
<div>{html}</div>"#
    );
    Html(wiki_html_shell(&nav, &display_title, &body, ""))
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

/// Parsed CLI args.
struct ServerArgs {
    config_path: PathBuf,
    prs_dir: Option<PathBuf>,
    central_repo: Option<PathBuf>,
}

/// Parse the (small) atn-server arg shape: a positional config
/// path (defaults to `agents.toml`) plus optional `--prs-dir` and
/// `--central-repo` flags. Both `--flag value` and `--flag=value`
/// forms are accepted.
fn parse_server_args() -> ServerArgs {
    let mut iter = std::env::args().skip(1);
    let mut config_path: Option<PathBuf> = None;
    let mut prs_dir: Option<PathBuf> = None;
    let mut central_repo: Option<PathBuf> = None;
    while let Some(a) = iter.next() {
        if let Some(v) = a.strip_prefix("--prs-dir=") {
            prs_dir = Some(PathBuf::from(v));
            continue;
        }
        if let Some(v) = a.strip_prefix("--central-repo=") {
            central_repo = Some(PathBuf::from(v));
            continue;
        }
        match a.as_str() {
            "--prs-dir" => prs_dir = iter.next().map(PathBuf::from),
            "--central-repo" => central_repo = iter.next().map(PathBuf::from),
            other if other.starts_with("--") => {
                tracing::warn!("atn-server: unknown flag {other:?}, ignoring");
            }
            _ => {
                if config_path.is_none() {
                    config_path = Some(PathBuf::from(a));
                }
            }
        }
    }
    ServerArgs {
        config_path: config_path
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH)),
        prs_dir,
        central_repo,
    }
}
