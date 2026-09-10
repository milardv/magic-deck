use std::{path::PathBuf, sync::Arc};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::RwLock;

use crate::{
    ai_coach::{CoachError, GeminiClient},
    analysis_store::{self, AnalysisStore},
    config,
    export::{collection_arena, collection_csv, deck_arena, deck_csv},
    memory_collection::{self, ScanResult},
    model::{
        AnalysisReport, AnalysisReportSummary, AnalyzeDeckRequest, OwnedCard, Settings, Snapshot,
        StatusResponse, UpdateSettings,
    },
    parser,
};

#[derive(Clone)]
pub struct AppState {
    pub settings: Arc<RwLock<Settings>>,
    pub snapshot: Arc<RwLock<Snapshot>>,
    pub analysis_store: AnalysisStore,
}

impl AppState {
    pub fn new(settings: Settings) -> anyhow::Result<Self> {
        Ok(Self {
            settings: Arc::new(RwLock::new(settings)),
            snapshot: Arc::new(RwLock::new(Snapshot::default())),
            analysis_store: AnalysisStore::open_default()?,
        })
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CollectionQuery {
    search: Option<String>,
    color: Option<String>,
    card_type: Option<String>,
    min_quantity: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    format: Option<String>,
}

pub async fn index() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}

pub async fn health() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

pub async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    Json(build_status(&state).await)
}

pub async fn sync(State(state): State<AppState>) -> Result<Json<StatusResponse>, ApiError> {
    sync_state(&state).await?;
    Ok(Json(build_status(&state).await))
}

pub async fn get_settings(State(state): State<AppState>) -> Json<Settings> {
    Json(state.settings.read().await.clone())
}

pub async fn update_settings(
    State(state): State<AppState>,
    Json(update): Json<UpdateSettings>,
) -> Result<Json<Settings>, ApiError> {
    let value = update.log_path.trim();
    if value.is_empty() {
        return Err(ApiError::bad_request("The log path cannot be empty."));
    }
    let expanded = expand_home(value);
    let settings = Settings {
        log_path: expanded.to_string_lossy().into_owned(),
    };
    config::save(&settings).map_err(ApiError::internal)?;
    *state.settings.write().await = settings.clone();
    Ok(Json(settings))
}

pub async fn decks(State(state): State<AppState>) -> Json<Vec<crate::model::Deck>> {
    Json(state.snapshot.read().await.decks.clone())
}

pub async fn deck(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::model::Deck>, ApiError> {
    state
        .snapshot
        .read()
        .await
        .decks
        .iter()
        .find(|deck| deck.id == id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found("Deck not found."))
}

pub async fn collection(
    State(state): State<AppState>,
    Query(query): Query<CollectionQuery>,
) -> Json<Vec<OwnedCard>> {
    let snapshot = state.snapshot.read().await;
    Json(filter_collection(&snapshot.collection, &query))
}

pub async fn export_deck(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let snapshot = state.snapshot.read().await;
    let deck = snapshot
        .decks
        .iter()
        .find(|deck| deck.id == id)
        .ok_or_else(|| ApiError::not_found("Deck not found."))?;
    match query.format.as_deref().unwrap_or("arena") {
        "arena" | "txt" => Ok(download(
            deck_arena(deck),
            "text/plain; charset=utf-8",
            "deck.txt",
        )),
        "csv" => Ok(download(
            deck_csv(deck),
            "text/csv; charset=utf-8",
            "deck.csv",
        )),
        "json" => Ok(download(
            serde_json::to_string_pretty(deck).map_err(ApiError::internal)?,
            "application/json; charset=utf-8",
            "deck.json",
        )),
        _ => Err(ApiError::bad_request(
            "Supported formats: arena, json, csv.",
        )),
    }
}

pub async fn export_collection(
    State(state): State<AppState>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let snapshot = state.snapshot.read().await;
    match query.format.as_deref().unwrap_or("csv") {
        "arena" | "txt" => Ok(download(
            collection_arena(&snapshot.collection),
            "text/plain; charset=utf-8",
            "collection.txt",
        )),
        "csv" => Ok(download(
            collection_csv(&snapshot.collection),
            "text/csv; charset=utf-8",
            "collection.csv",
        )),
        "json" => Ok(download(
            serde_json::to_string_pretty(&snapshot.collection).map_err(ApiError::internal)?,
            "application/json; charset=utf-8",
            "collection.json",
        )),
        _ => Err(ApiError::bad_request(
            "Supported formats: arena, json, csv.",
        )),
    }
}

pub async fn analyze_deck(
    State(state): State<AppState>,
    Json(request): Json<AnalyzeDeckRequest>,
) -> Result<Json<AnalysisReport>, ApiError> {
    let (deck, collection) = {
        let snapshot = state.snapshot.read().await;
        let deck = snapshot
            .decks
            .iter()
            .find(|deck| deck.id == request.deck_id)
            .cloned()
            .ok_or_else(|| ApiError::not_found("Deck not found."))?;
        (deck, snapshot.collection.clone())
    };
    if collection.is_empty() {
        return Err(ApiError::conflict(
            "The collection is empty. Start MTGA and synchronize before requesting an analysis.",
        ));
    }
    let client = GeminiClient::from_env().map_err(coach_error)?;
    let fingerprint = analysis_store::fingerprint(&deck, &collection, client.model())
        .map_err(ApiError::internal)?;
    let store = state.analysis_store.clone();
    let deck_id = deck.id.clone();
    let model = client.model().to_owned();
    let fingerprint_for_cache = fingerprint.clone();
    let model_for_cache = model.clone();
    if let Some(report) = tokio::task::spawn_blocking(move || {
        store.find_cached(&deck_id, &fingerprint_for_cache, &model_for_cache)
    })
    .await
    .map_err(ApiError::internal)?
    .map_err(ApiError::internal)?
    {
        return Ok(Json(report));
    }
    let analysis = client
        .analyze(&deck, &collection)
        .await
        .map_err(coach_error)?;
    let store = state.analysis_store.clone();
    let report = tokio::task::spawn_blocking(move || {
        store.save(&deck, &collection, &model, &fingerprint, &analysis)
    })
    .await
    .map_err(ApiError::internal)?
    .map_err(ApiError::internal)?;
    Ok(Json(report))
}

pub async fn deck_analyses(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<AnalysisReportSummary>>, ApiError> {
    let store = state.analysis_store.clone();
    let reports = tokio::task::spawn_blocking(move || store.list_for_deck(&id))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::internal)?;
    Ok(Json(reports))
}

pub async fn analysis_report(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<AnalysisReport>, ApiError> {
    let store = state.analysis_store.clone();
    tokio::task::spawn_blocking(move || store.get(id))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::internal)?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("Analysis report not found."))
}

fn coach_error(error: CoachError) -> ApiError {
    match error {
        CoachError::MissingApiKey => ApiError::service_unavailable(
            "AI Coach is not configured. Set GEMINI_API_KEY and restart Magic Deck.",
        ),
        CoachError::InvalidSuggestion(_) | CoachError::InvalidResponse(_) => {
            tracing::warn!(error = %error, "Gemini analysis rejected");
            ApiError::bad_gateway("Gemini returned advice that failed validation. Try again.")
        }
        CoachError::Api { status, .. }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                || status == reqwest::StatusCode::SERVICE_UNAVAILABLE =>
        {
            ApiError::service_unavailable(
                "Gemini is temporarily busy or rate-limited. Try again in a moment.",
            )
        }
        error => {
            tracing::error!(error = %error, "Gemini analysis failed");
            ApiError::bad_gateway("Gemini could not analyze this deck. Try again later.")
        }
    }
}

pub async fn sync_state(state: &AppState) -> Result<(), ApiError> {
    let log_path = state.settings.read().await.log_path.clone();
    let path = PathBuf::from(log_path);
    let snapshot = tokio::task::spawn_blocking(move || {
        let mut snapshot = parser::parse_log(&path)?;
        if snapshot.collection.is_empty() {
            match memory_collection::scan(&path) {
                Ok(ScanResult::Collection(cards)) => {
                    if let Err(error) = parser::set_collection(&mut snapshot, &path, cards) {
                        snapshot
                            .warnings
                            .push(format!("Could not resolve collection cards: {error}"));
                    }
                }
                Ok(ScanResult::GameNotRunning) => snapshot.warnings.push(
                    "The current MTGA client does not write owned cards to Player.log. Start MTGA, then sync again."
                        .into(),
                ),
                Ok(ScanResult::NoCollectionFound) => snapshot.warnings.push(
                    "MTGA is running, but its collection was not found in memory. Open Collection, then sync again."
                        .into(),
                ),
                Err(error) => snapshot
                    .warnings
                    .push(format!("Could not read the live MTGA collection: {error}")),
            }
        }
        Ok::<_, parser::ParseError>(snapshot)
    })
        .await
        .map_err(ApiError::internal)?
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    *state.snapshot.write().await = snapshot;
    Ok(())
}

async fn build_status(state: &AppState) -> StatusResponse {
    let settings = state.settings.read().await.clone();
    let snapshot = state.snapshot.read().await;
    StatusResponse {
        log_exists: PathBuf::from(&settings.log_path).is_file(),
        log_path: settings.log_path,
        synced_at: snapshot.synced_at.clone(),
        source_modified_at: snapshot.source_modified_at.clone(),
        cards_owned: snapshot.collection.len(),
        total_copies: snapshot.collection.iter().map(|card| card.quantity).sum(),
        deck_count: snapshot.decks.len(),
        wildcards: snapshot.wildcards.clone(),
        warnings: snapshot.warnings.clone(),
    }
}

fn filter_collection(cards: &[OwnedCard], query: &CollectionQuery) -> Vec<OwnedCard> {
    let search = query
        .search
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let color = query
        .color
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    let card_type = query
        .card_type
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    cards
        .iter()
        .filter(|card| {
            (search.is_empty()
                || card.name.to_ascii_lowercase().contains(&search)
                || card.arena_id.to_string().contains(&search))
                && (color.is_empty() || card.colors.iter().any(|item| item == &color))
                && (card_type.is_empty()
                    || card.type_line.to_ascii_lowercase().contains(&card_type))
                && query
                    .min_quantity
                    .is_none_or(|minimum| card.quantity >= minimum)
        })
        .cloned()
        .collect()
}

fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(value)
}

fn download(content: String, content_type: &'static str, filename: &'static str) -> Response {
    let mut response = Response::new(Body::from(content));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .expect("static filename is a valid header"),
    );
    response
}

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }

    fn bad_gateway(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: message.into(),
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "request failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "Internal server error.".into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": self.message}))).into_response()
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_by_name_color_type_and_quantity() {
        let cards = vec![OwnedCard {
            arena_id: 42,
            name: "Helpful Knight".into(),
            quantity: 4,
            type_line: "Creature — Knight".into(),
            colors: vec!["W".into()],
            ..Default::default()
        }];
        let query = CollectionQuery {
            search: Some("knight".into()),
            color: Some("w".into()),
            card_type: Some("creature".into()),
            min_quantity: Some(4),
        };

        assert_eq!(filter_collection(&cards, &query).len(), 1);
    }
}
