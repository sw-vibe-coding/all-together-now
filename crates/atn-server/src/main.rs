use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_stream::StreamExt as _;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

use atn_core::agent::{AgentId, AgentState};
use atn_core::config::load_project_config;
use atn_core::event::{InputEvent, OutputSignal};
use atn_pty::manager::SessionManager;

type AppState = Arc<Mutex<SessionManager>>;

static INDEX_HTML: &str = include_str!("../static/index.html");

const DEFAULT_CONFIG_PATH: &str = "agents.toml";

#[derive(Deserialize)]
struct InputPayload {
    text: String,
}

#[derive(Serialize)]
struct AgentInfo {
    id: String,
    name: String,
    role: String,
    state: AgentState,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("atn=info".parse().unwrap()),
        )
        .init();

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
        .map(|d| base_dir.join(d));

    let mut manager = SessionManager::new(log_dir);

    // Spawn all configured agents.
    for entry in &project_config.agents {
        let config = entry.to_agent_config(&base_dir);
        match manager.spawn_agent(config).await {
            Ok(id) => tracing::info!("Spawned agent: {id} ({})", entry.name),
            Err(e) => tracing::error!("Failed to spawn agent '{}': {e}", entry.id),
        }
    }

    tracing::info!(
        "{} agent(s) running",
        manager.len()
    );

    let state: AppState = Arc::new(Mutex::new(manager));

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/agents", get(list_agents))
        .route("/api/agents/{id}/sse", get(agent_sse))
        .route("/api/agents/{id}/input", post(agent_input))
        .route("/api/agents/{id}/ctrl-c", post(agent_ctrl_c))
        .route("/api/agents/{id}/state", get(agent_state))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = "0.0.0.0:7500";
    tracing::info!("Listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// Returns full agent info with state for all agents.
async fn list_agents(State(state): State<AppState>) -> Json<Vec<AgentInfo>> {
    let pending: Vec<_> = {
        let mgr = state.lock().await;
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
        let mgr = state.lock().await;
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
    let mgr = state.lock().await;
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
        let mgr = state.lock().await;
        let session = mgr
            .get_session(&agent_id)
            .map_err(|_| StatusCode::NOT_FOUND)?;
        session.input_sender()
    };
    tx.send(InputEvent::HumanText {
        text: payload.text,
    })
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
        let mgr = state.lock().await;
        let session = mgr
            .get_session(&agent_id)
            .map_err(|_| StatusCode::NOT_FOUND)?;
        session.input_sender()
    };
    tx.send(InputEvent::RawBytes {
        bytes: vec![0x03],
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}
