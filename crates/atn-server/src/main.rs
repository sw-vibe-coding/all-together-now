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
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio_stream::StreamExt as _;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

use atn_core::agent::{AgentConfig, AgentId, AgentRole};
use atn_core::event::{InputEvent, OutputSignal};
use atn_pty::manager::SessionManager;

type AppState = Arc<Mutex<SessionManager>>;

static INDEX_HTML: &str = include_str!("../static/index.html");

#[derive(Deserialize)]
struct InputPayload {
    text: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("atn=info".parse().unwrap()),
        )
        .init();

    tracing::info!("All Together Now — PGM server starting");

    let mut manager = SessionManager::new(None);

    // Auto-spawn a demo bash agent.
    let demo_config = AgentConfig {
        id: AgentId("demo".to_string()),
        name: "Demo Agent".to_string(),
        repo_path: PathBuf::from("."),
        role: AgentRole::Developer,
        setup_commands: vec![],
        launch_command: String::new(),
    };

    match manager.spawn_agent(demo_config).await {
        Ok(id) => tracing::info!("Spawned demo agent: {id}"),
        Err(e) => tracing::error!("Failed to spawn demo agent: {e}"),
    }

    let state: AppState = Arc::new(Mutex::new(manager));

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/agents", get(list_agents))
        .route("/api/agents/{id}/sse", get(agent_sse))
        .route("/api/agents/{id}/input", post(agent_input))
        .route("/api/agents/{id}/ctrl-c", post(agent_ctrl_c))
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

async fn list_agents(State(state): State<AppState>) -> Json<Vec<String>> {
    let mgr = state.lock().await;
    let ids: Vec<String> = mgr.agent_ids().iter().map(|id| id.0.clone()).collect();
    Json(ids)
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
