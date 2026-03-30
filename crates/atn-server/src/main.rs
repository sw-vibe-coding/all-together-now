use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
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

use atn_core::agent::{AgentId, AgentState};
use atn_core::config::load_project_config;
use atn_core::event::{InputEvent, OutputSignal};
use atn_pty::manager::SessionManager;
use atn_wiki::storage::FileWikiStorage;
use wiki_common::async_storage::AsyncWikiStorage;
use wiki_common::etag::content_etag;
use wiki_common::model::WikiPage;
use wiki_common::parser::render_wiki_content;
use wiki_common::patch::{apply_ops, PatchRequest};

#[derive(Clone)]
struct SharedState {
    manager: Arc<Mutex<SessionManager>>,
    wiki: Arc<FileWikiStorage>,
}

type AppState = SharedState;

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

    // Initialize wiki storage and seed coordination pages.
    let wiki_dir = base_dir.join(".atn").join("wiki");
    let wiki = Arc::new(FileWikiStorage::new(&wiki_dir));
    let now = wiki_common::time::now();
    atn_wiki::coordination::seed_coordination_pages(wiki.as_ref(), now).await;
    tracing::info!("Wiki storage initialized at {}", wiki_dir.display());

    let state = SharedState {
        manager: Arc::new(Mutex::new(manager)),
        wiki,
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/agents", get(list_agents))
        .route("/api/agents/{id}/sse", get(agent_sse))
        .route("/api/agents/{id}/input", post(agent_input))
        .route("/api/agents/{id}/ctrl-c", post(agent_ctrl_c))
        .route("/api/agents/{id}/state", get(agent_state))
        .route("/api/wiki", get(wiki_list_pages))
        .route(
            "/api/wiki/{*title}",
            get(wiki_get_page)
                .put(wiki_put_page)
                .patch(wiki_patch_page)
                .delete(wiki_delete_page),
        )
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
        let mgr = state.manager.lock().await;
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
    response
        .headers_mut()
        .insert("ETag", etag.parse().unwrap());
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
    response
        .headers_mut()
        .insert("ETag", etag.parse().unwrap());
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
    response
        .headers_mut()
        .insert("ETag", etag.parse().unwrap());
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
