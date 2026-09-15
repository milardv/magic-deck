use crate::{
    ai_coach::{DeckLabSuggestionOptions, GeminiClient},
    model::{DeckLabCandidate, DeckLabGenerateRequest, DeckLabGeneration},
    routes::{ApiError, AppState},
    simulation::{EngineSettings, RandomLabCampaign, RandomLabRequest, RunReport, RunRequest},
};
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

pub async fn engine(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        state
            .simulations
            .engine_status()
            .await
            .map_err(ApiError::internal)?,
    ))
}
pub async fn configure(
    State(state): State<AppState>,
    Json(settings): Json<EngineSettings>,
) -> Result<Json<Value>, ApiError> {
    state
        .simulations
        .configure(settings)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    engine(State(state)).await
}
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Value>>, ApiError> {
    Ok(Json(
        state.simulations.list().await.map_err(ApiError::internal)?,
    ))
}
pub async fn opponents() -> Json<Vec<crate::model::Deck>> {
    Json(crate::simulation::reference_decks())
}

pub async fn random_lab_start(
    State(state): State<AppState>,
    Json(mut request): Json<RandomLabRequest>,
) -> Result<Json<RandomLabCampaign>, ApiError> {
    let allowed = ["W", "U", "B", "R", "G"];
    request.colors = request
        .colors
        .iter()
        .map(|color| color.trim().to_ascii_uppercase())
        .collect();
    request.colors.sort();
    request.colors.dedup();
    if (request.bicolor && request.colors.len() != 2)
        || (!request.bicolor && request.colors.len() != 1)
    {
        return Err(ApiError::bad_request(
            "Choisissez une ou deux couleurs cohérentes avec le mode sélectionné.",
        ));
    }
    if request
        .colors
        .iter()
        .any(|color| !allowed.contains(&color.as_str()))
    {
        return Err(ApiError::bad_request("Couleur invalide."));
    }
    if request.candidate_count == 0 {
        return Err(ApiError::bad_request("Générez au moins une combinaison."));
    }
    if request.opponent_ids.is_empty() || request.opponent_ids.len() > 12 {
        return Err(ApiError::bad_request(
            "Sélectionnez 1 à 12 decks de référence.",
        ));
    }
    let (collection, opponents) = {
        let snapshot = state.snapshot.read().await;
        let references = crate::simulation::reference_decks();
        let mut ids = request.opponent_ids.clone();
        ids.sort();
        ids.dedup();
        let opponents = ids
            .into_iter()
            .map(|id| {
                snapshot
                    .decks
                    .iter()
                    .find(|deck| deck.id == id)
                    .cloned()
                    .or_else(|| references.iter().find(|deck| deck.id == id).cloned())
                    .ok_or_else(|| ApiError::not_found(format!("Deck adverse introuvable : {id}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        (snapshot.collection.clone(), opponents)
    };
    let decks = crate::simulation::generate_random_decks(&collection, &request)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let campaign_id = crate::simulation::timestamp_id();
    let candidates = decks
        .into_iter()
        .map(|deck| crate::simulation::RandomLabCandidate {
            id: deck.id.clone(),
            deck,
            report_id: None,
            error: None,
        })
        .collect::<Vec<_>>();
    let campaign = RandomLabCampaign {
        id: campaign_id.clone(),
        created_at: crate::simulation::now(),
        request: request.clone(),
        opponents: opponents.clone(),
        candidates,
    };
    state.random_labs.insert(campaign.clone()).await;
    for (index, candidate) in campaign.candidates.iter().cloned().enumerate() {
        let service = state.simulations.clone();
        let store = state.random_labs.clone();
        let opponents = opponents.clone();
        let campaign_id = campaign_id.clone();
        let seed = request.seed.unwrap_or_default().wrapping_add(index as u64) as u32;
        tokio::spawn(async move {
            loop {
                let run = RunRequest {
                    deck_id: candidate.id.clone(),
                    deck: Some(candidate.deck.clone()),
                    opponent_id: None,
                    opponent_ids: Vec::new(),
                    games: 1,
                    seed,
                    seconds_per_game: 120,
                    parallel: true,
                };
                match service
                    .start(run, candidate.deck.clone(), opponents.clone())
                    .await
                {
                    Ok(report) => {
                        store
                            .assign_report(&campaign_id, &candidate.id, report.id)
                            .await;
                        break;
                    }
                    Err(error) if error.to_string().contains("déjà en cours") => {
                        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
                    }
                    Err(error) => {
                        store
                            .assign_error(&campaign_id, &candidate.id, error.to_string())
                            .await;
                        break;
                    }
                }
            }
        });
    }
    Ok(Json(campaign))
}

async fn random_lab_view(state: &AppState, campaign: RandomLabCampaign) -> Value {
    let mut candidates = Vec::with_capacity(campaign.candidates.len());
    for candidate in campaign.candidates {
        let report = match candidate.report_id.as_deref() {
            Some(report_id) => state.simulations.get(report_id).await.ok(),
            None => None,
        };
        let (wins, losses, draws) = report.as_ref().map_or((0, 0, 0), |report| {
            (
                report
                    .results
                    .iter()
                    .filter(|game| game.outcome == "win")
                    .count() as u32,
                report
                    .results
                    .iter()
                    .filter(|game| game.outcome == "loss")
                    .count() as u32,
                report
                    .results
                    .iter()
                    .filter(|game| game.outcome == "draw")
                    .count() as u32,
            )
        });
        let games = wins + losses + draws;
        let score = if games == 0 {
            None
        } else {
            Some((wins as f64 + draws as f64 * 0.5) / games as f64)
        };
        candidates.push(json!({
            "id": candidate.id,
            "deck": candidate.deck,
            "reportId": candidate.report_id,
            "status": report.as_ref().map(|report| report.status.as_str()).unwrap_or(if candidate.error.is_some() { "failed" } else { "queued" }),
            "error": candidate.error,
            "wins": wins, "losses": losses, "draws": draws, "score": score,
            "report": report,
        }));
    }
    candidates.sort_by(|left, right| {
        right["score"]
            .as_f64()
            .unwrap_or(-1.0)
            .partial_cmp(&left["score"].as_f64().unwrap_or(-1.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let completed = candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate["status"].as_str(),
                Some("completed" | "failed" | "cancelled")
            )
        })
        .count();
    let status = if completed == candidates.len() {
        "completed"
    } else {
        "running"
    };
    json!({"id":campaign.id,"createdAt":campaign.created_at,"status":status,"request":campaign.request,"opponents":campaign.opponents,"candidates":candidates})
}

pub async fn random_lab_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let campaign = state
        .random_labs
        .get(&id)
        .await
        .ok_or_else(|| ApiError::not_found("Laboratoire introuvable ou redémarré."))?;
    Ok(Json(random_lab_view(&state, campaign).await))
}

pub async fn random_lab_list(State(state): State<AppState>) -> Result<Json<Vec<Value>>, ApiError> {
    let campaigns = state.random_labs.list().await;
    let mut views = Vec::with_capacity(campaigns.len());
    for campaign in campaigns {
        views.push(random_lab_view(&state, campaign).await);
    }
    Ok(Json(views))
}

pub async fn generate_candidates(
    State(state): State<AppState>,
    Json(request): Json<DeckLabGenerateRequest>,
) -> Result<Json<DeckLabGeneration>, ApiError> {
    if !(1..=3).contains(&request.candidate_count)
        || !(2048..=65_536).contains(&request.max_output_tokens)
    {
        return Err(ApiError::bad_request(
            "Choisissez 1 à 3 candidats et un budget de 2 048 à 65 536 tokens.",
        ));
    }
    let (api_key, log_path) = {
        let settings = state.settings.read().await;
        (
            crate::routes::effective_api_key(&settings),
            PathBuf::from(&settings.log_path),
        )
    };
    let (deck, shortlist_deck, collection) = {
        let snapshot = state.snapshot.read().await;
        let shortlist_deck = snapshot
            .decks
            .iter()
            .find(|deck| deck.id == request.deck_id)
            .cloned()
            .ok_or_else(|| ApiError::not_found("Deck introuvable"))?;
        let deck = if let Some(deck) = request.base_deck.clone() {
            deck
        } else {
            snapshot
                .decks
                .iter()
                .find(|deck| deck.id == request.deck_id)
                .cloned()
                .ok_or_else(|| ApiError::not_found("Deck introuvable"))?
        };
        if !deck
            .format
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains("standard")
        {
            return Err(ApiError::bad_request(
                "Le Deck Lab prend actuellement en charge le format Standard.",
            ));
        }
        (deck, shortlist_deck, snapshot.collection.clone())
    };
    if request.base_deck.is_some() {
        let printing_sets = load_printing_sets(&log_path, &deck).await?;
        validate_generated_deck(&deck, &collection, &printing_sets)?;
    }
    let client = GeminiClient::from_api_key(api_key)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let model = client.model().to_owned();
    let batch = client
        .suggest_deck_changes(
            &deck,
            &shortlist_deck,
            &collection,
            DeckLabSuggestionOptions {
                candidate_count: request.candidate_count,
                max_output_tokens: request.max_output_tokens,
                simulation_feedback: request.simulation_feedback.as_deref(),
                cache_name: request.cache_name.as_deref(),
                cache_attempted: request.cache_attempted,
            },
        )
        .await
        .map_err(|error| ApiError::bad_gateway(error.to_string()))?;
    let mut candidates = Vec::new();
    for (index, suggestion) in batch
        .suggestions
        .into_iter()
        .take(request.candidate_count)
        .enumerate()
    {
        let mut candidate = deck.clone();
        candidate.id = format!("deck-lab-{}-{index}", crate::simulation::timestamp_id());
        candidate.name = format!("{} · variante {}", deck.name, index + 1);
        candidate.format = Some("Standard".into());
        for removal in &suggestion.card_to_remove {
            if let Some(card) = candidate
                .main_deck
                .iter_mut()
                .find(|card| card.name.eq_ignore_ascii_case(&removal.card_name))
            {
                card.quantity = card.quantity.saturating_sub(removal.quantity);
            }
        }
        candidate.main_deck.retain(|card| card.quantity > 0);
        for addition in &suggestion.card_to_add {
            if let Some(card) = candidate
                .main_deck
                .iter_mut()
                .find(|card| card.name.eq_ignore_ascii_case(&addition.card_name))
            {
                card.quantity += addition.quantity;
            } else if let Some(card) = collection
                .iter()
                .find(|card| card.name.eq_ignore_ascii_case(&addition.card_name))
            {
                candidate.main_deck.push(crate::model::DeckCard {
                    arena_id: card.arena_id,
                    name: card.name.clone(),
                    quantity: addition.quantity,
                    type_line: card.type_line.clone(),
                    colors: card.colors.clone(),
                    set_code: card.set_code.clone(),
                    collector_number: card.collector_number.clone(),
                });
            }
        }
        candidate.card_count = candidate.main_deck.iter().map(|card| card.quantity).sum();
        normalize_candidate_size(
            &mut candidate,
            deck.card_count.max(60),
            &suggestion.card_to_add,
        );
        if (60..=250).contains(&candidate.card_count) {
            candidates.push(DeckLabCandidate {
                deck: candidate,
                rationale: format!("{} — {}", suggestion.title, suggestion.reasoning),
            });
        } else {
            tracing::warn!(candidate=%candidate.name, cards=candidate.card_count, target=deck.card_count.max(60), "Gemini Deck Lab candidate rejected after size normalization");
        }
    }
    if candidates.is_empty() {
        return Err(ApiError::bad_gateway(
            "Gemini n'a produit aucune variante Standard valide d'au moins 60 cartes.",
        ));
    }
    Ok(Json(DeckLabGeneration {
        candidates,
        max_output_tokens: request.max_output_tokens,
        model,
        explicit_cache: batch.cache_name.is_some(),
        cache_name: batch.cache_name,
        cache_attempted: true,
    }))
}

fn normalize_candidate_size(
    deck: &mut crate::model::Deck,
    target: u32,
    additions: &[crate::model::CardChange],
) {
    let mut total: u32 = deck.main_deck.iter().map(|card| card.quantity).sum();
    if total > target {
        let mut excess = total - target;
        for addition in additions.iter().rev() {
            if let Some(card) = deck
                .main_deck
                .iter_mut()
                .find(|card| card.name.eq_ignore_ascii_case(&addition.card_name))
            {
                let removed = excess.min(addition.quantity).min(card.quantity);
                card.quantity -= removed;
                excess -= removed;
                if excess == 0 {
                    break;
                }
            }
        }
        deck.main_deck.retain(|card| card.quantity > 0);
        total = deck.main_deck.iter().map(|card| card.quantity).sum();
    }
    if total < target {
        let missing = target - total;
        const BASICS: [&str; 5] = ["Plains", "Island", "Swamp", "Mountain", "Forest"];
        if let Some(land) = deck
            .main_deck
            .iter_mut()
            .filter(|card| BASICS.contains(&card.name.as_str()))
            .max_by_key(|card| card.quantity)
        {
            land.quantity += missing;
        } else {
            let name = if deck.colors.iter().any(|color| color == "U") {
                "Island"
            } else if deck.colors.iter().any(|color| color == "B") {
                "Swamp"
            } else if deck.colors.iter().any(|color| color == "R") {
                "Mountain"
            } else if deck.colors.iter().any(|color| color == "G") {
                "Forest"
            } else {
                "Plains"
            };
            deck.main_deck.push(crate::model::DeckCard {
                name: name.into(),
                quantity: missing,
                type_line: "Basic Land".into(),
                ..Default::default()
            });
        }
    }
    deck.card_count = deck.main_deck.iter().map(|card| card.quantity).sum();
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RunReport>, ApiError> {
    Ok(Json(
        state
            .simulations
            .get(&id)
            .await
            .map_err(|e| ApiError::not_found(e.to_string()))?,
    ))
}
pub async fn start(
    State(state): State<AppState>,
    Json(request): Json<RunRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let log_path = PathBuf::from(&state.settings.read().await.log_path);
    let supplied_deck = request.deck.is_some();
    let (deck, opponents, collection) = {
        let snapshot = state.snapshot.read().await;
        let deck = if let Some(deck) = request.deck.clone() {
            deck
        } else {
            snapshot
                .decks
                .iter()
                .find(|d| d.id == request.deck_id)
                .cloned()
                .ok_or_else(|| ApiError::not_found("Deck introuvable"))?
        };
        let mut ids = request.opponent_ids.clone();
        if ids.is_empty() {
            if let Some(id) = &request.opponent_id {
                ids.push(id.clone());
            }
        }
        ids.sort();
        ids.dedup();
        if ids.is_empty() || ids.len() > 12 {
            return Err(ApiError::bad_request("Sélectionnez 1 à 12 decks adverses."));
        }
        let refs = crate::simulation::reference_decks();
        let opponents = ids
            .into_iter()
            .map(|id| {
                snapshot
                    .decks
                    .iter()
                    .find(|d| d.id == id)
                    .cloned()
                    .or_else(|| refs.iter().find(|d| d.id == id).cloned())
                    .ok_or_else(|| ApiError::not_found(format!("Deck adverse introuvable : {id}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        (deck, opponents, snapshot.collection.clone())
    };
    if supplied_deck {
        let printing_sets = load_printing_sets(&log_path, &deck).await?;
        validate_generated_deck(&deck, &collection, &printing_sets)?;
    }
    let report = state
        .simulations
        .start(request, deck, opponents)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok((StatusCode::ACCEPTED, Json(report)))
}

fn validate_generated_deck(
    deck: &crate::model::Deck,
    collection: &[crate::model::OwnedCard],
    printing_sets: &HashMap<String, HashSet<String>>,
) -> Result<(), ApiError> {
    if !deck
        .format
        .as_deref()
        .unwrap_or_default()
        .eq_ignore_ascii_case("standard")
    {
        return Err(ApiError::bad_request(
            "Les decks générés doivent être au format Standard.",
        ));
    }
    let owned = collection.iter().fold(
        std::collections::HashMap::<String, u32>::new(),
        |mut quantities, card| {
            *quantities.entry(card.name.to_lowercase()).or_default() += card.quantity;
            quantities
        },
    );
    for card in &deck.main_deck {
        let name = card.name.to_lowercase();
        let legal_owned_printing_exists = collection
            .iter()
            .filter(|owned| owned.name.eq_ignore_ascii_case(&card.name))
            .any(|owned| crate::candidate_selector::legal_for_format(owned, "Standard"));
        let legal_database_printing_exists = printing_sets.get(&name).is_some_and(|sets| {
            sets.iter()
                .any(|set| crate::candidate_selector::standard_set_is_legal(set))
        });
        let available = owned.get(&name).copied().unwrap_or_default();
        let basic = matches!(
            card.name.as_str(),
            "Plains" | "Island" | "Swamp" | "Mountain" | "Forest"
        );
        if !basic && card.quantity > available {
            return Err(ApiError::bad_request(format!(
                "{} dépasse la quantité possédée.",
                card.name
            )));
        }
        if !basic && !legal_owned_printing_exists && !legal_database_printing_exists {
            return Err(ApiError::bad_request(format!(
                "{} n'est pas éligible au pool Standard configuré.",
                card.name
            )));
        }
    }
    Ok(())
}

async fn load_printing_sets(
    log_path: &std::path::Path,
    deck: &crate::model::Deck,
) -> Result<HashMap<String, HashSet<String>>, ApiError> {
    let names = deck
        .main_deck
        .iter()
        .map(|card| card.name.to_lowercase())
        .collect::<HashSet<_>>();
    let log_path = log_path.to_path_buf();
    tokio::task::spawn_blocking(move || crate::card_database::load_printing_sets(&log_path, &names))
        .await
        .map_err(ApiError::internal)?
        .map_err(ApiError::internal)
        .map(Option::unwrap_or_default)
}
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .simulations
        .cancel(&id)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(json!({"ok":true})))
}
pub async fn logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let logs = state
        .simulations
        .logs(&id)
        .await
        .map_err(|e| ApiError::not_found(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=forge.log",
            ),
        ],
        logs,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_small_gemini_quantity_mismatches() {
        let mut short = crate::model::Deck {
            main_deck: vec![crate::model::DeckCard {
                name: "Plains".into(),
                quantity: 58,
                ..Default::default()
            }],
            card_count: 58,
            ..Default::default()
        };
        normalize_candidate_size(&mut short, 60, &[]);
        assert_eq!(short.card_count, 60);

        let mut long = crate::model::Deck {
            main_deck: vec![
                crate::model::DeckCard {
                    name: "Plains".into(),
                    quantity: 59,
                    ..Default::default()
                },
                crate::model::DeckCard {
                    name: "New Card".into(),
                    quantity: 4,
                    ..Default::default()
                },
            ],
            card_count: 63,
            ..Default::default()
        };
        normalize_candidate_size(
            &mut long,
            60,
            &[crate::model::CardChange {
                card_name: "New Card".into(),
                quantity: 3,
            }],
        );
        assert_eq!(long.card_count, 60);
        assert_eq!(
            long.main_deck
                .iter()
                .find(|card| card.name == "New Card")
                .unwrap()
                .quantity,
            1
        );
    }

    #[test]
    fn accepts_an_owned_printing_when_the_same_card_has_a_standard_printing() {
        let deck = crate::model::Deck {
            format: Some("Standard".into()),
            main_deck: vec![crate::model::DeckCard {
                name: "Hero's Downfall".into(),
                quantity: 3,
                ..Default::default()
            }],
            ..Default::default()
        };
        let collection = vec![crate::model::OwnedCard {
            name: "Hero's Downfall".into(),
            quantity: 3,
            set_code: Some("VOW".into()),
            ..Default::default()
        }];
        let printing_sets = HashMap::from([(
            "hero's downfall".into(),
            HashSet::from(["VOW".into(), "FDN".into()]),
        )]);
        assert!(validate_generated_deck(&deck, &collection, &printing_sets).is_ok());
    }
}
