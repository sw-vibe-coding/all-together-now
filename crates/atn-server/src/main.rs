use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("atn=info".parse().unwrap()))
        .init();

    tracing::info!("All Together Now — PGM server starting");

    // Phase 2 will add Axum routes, SSE streaming, and agent session management.
    // For now, just validate the workspace compiles and runs.
    tracing::info!("Server placeholder — no routes configured yet");
}
