mod ai_coach;
mod analysis_store;
mod candidate_selector;
mod card_database;
mod coach_context;
mod collection_cache;
mod config;
mod export;
mod memory_collection;
mod model;
mod parser;
mod routes;

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{
    routing::{get, post},
    Router,
};
use routes::AppState;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "magic_deck=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let settings = config::load();
    let state = AppState::new(settings)?;
    if let Err(error) = routes::sync_state(&state).await {
        tracing::warn!(error = %error, "initial sync skipped");
    }

    let app = Router::new()
        .route("/", get(routes::index))
        .route("/assets/magic-deck-icon.png", get(routes::icon))
        .route("/assets/{name}", get(routes::web_asset))
        .route("/health", get(routes::health))
        .route("/api/status", get(routes::status))
        .route("/api/sync", post(routes::sync))
        .route(
            "/api/settings",
            get(routes::get_settings).put(routes::update_settings),
        )
        .route("/api/decks", get(routes::decks))
        .route("/api/decks/{id}", get(routes::deck))
        .route("/api/decks/{id}/export", get(routes::export_deck))
        .route("/api/decks/{id}/analyses", get(routes::deck_analyses))
        .route("/api/analyses/{id}", get(routes::analysis_report))
        .route("/api/analyze-deck", post(routes::analyze_deck))
        .route("/api/collection", get(routes::collection))
        .route("/api/collection/export", get(routes::export_collection))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port = std::env::var("MAGIC_DECK_PORT")
        .or_else(|_| std::env::var("PORT"))
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8092);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("cannot listen on http://{address}"))?;
    tracing::info!("Magic Deck available at http://{address}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Ctrl+C handler failed");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler failed")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
