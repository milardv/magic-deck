use std::{collections::HashMap, time::Duration};

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::candidate_selector;
use crate::model::{Deck, DeckAnalysisResponse, DeckCard, OwnedCard};

const DEFAULT_MODEL: &str = "gemini-3.6-flash";
const API_ROOT: &str = "https://generativelanguage.googleapis.com/v1beta/models";

pub const SYSTEM_INSTRUCTION: &str = include_str!("prompts/deck_coach.txt");
pub const COACH_VERSION: &str = "combos-v1";

#[derive(Debug, Error)]
pub enum CoachError {
    #[error("GEMINI_API_KEY is not configured")]
    MissingApiKey,
    #[error("invalid Gemini model name")]
    InvalidModel,
    #[error("Gemini request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Gemini returned HTTP {status}: {message}")]
    Api { status: StatusCode, message: String },
    #[error("Gemini returned no analysis")]
    EmptyResponse,
    #[error("Gemini returned invalid analysis JSON: {0}")]
    InvalidResponse(#[from] serde_json::Error),
    #[error("Gemini suggested an invalid deck change: {0}")]
    InvalidSuggestion(String),
}

pub struct GeminiClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoachInput<'a> {
    deck_name: &'a str,
    format: &'a Option<String>,
    deck_cards: Vec<CardEntry<'a>>,
    candidate_cards: Vec<CardEntry<'a>>,
    rules: std::collections::BTreeMap<u64, crate::coach_context::CardRules>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CardEntry<'a> {
    arena_id: u64,
    name: &'a str,
    quantity: u32,
    set_code: &'a Option<String>,
    collector_number: &'a Option<String>,
    section: &'static str,
    type_line: &'a str,
}

#[derive(Debug, Deserialize)]
struct GenerateResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Option<Content>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Content {
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Debug, Deserialize)]
struct Part {
    text: Option<String>,
    #[serde(default)]
    thought: bool,
}

impl GeminiClient {
    pub fn from_api_key(api_key: Option<String>) -> Result<Self, CoachError> {
        let api_key = api_key
            .filter(|value| !value.trim().is_empty())
            .ok_or(CoachError::MissingApiKey)?;
        let model = std::env::var("GEMINI_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into());
        if !model
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '.'))
        {
            return Err(CoachError::InvalidModel);
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            http,
            api_key,
            model,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub async fn analyze(
        &self,
        deck: &Deck,
        collection: &[OwnedCard],
        log_path: std::path::PathBuf,
    ) -> Result<DeckAnalysisResponse, CoachError> {
        let candidates = candidate_selector::select(deck, collection, 80);
        let ids = deck_entries(deck)
            .iter()
            .map(|card| card.arena_id)
            .chain(candidates.iter().map(|card| card.arena_id))
            .collect::<Vec<_>>();
        let rules =
            match tokio::task::spawn_blocking(move || crate::coach_context::load(&log_path, &ids))
                .await
            {
                Ok(Ok(rules)) => rules,
                result => {
                    warn!(?result, "Local card rules unavailable for coaching");
                    Default::default()
                }
            };
        let input = CoachInput {
            rules,
            deck_name: &deck.name,
            format: &deck.format,
            deck_cards: deck_entries(deck),
            candidate_cards: candidates
                .iter()
                .map(|card| CardEntry {
                    arena_id: card.arena_id,
                    name: &card.name,
                    quantity: card.quantity,
                    set_code: &card.set_code,
                    collector_number: &card.collector_number,
                    section: "collection",
                    type_line: &card.type_line,
                })
                .collect(),
        };
        let prompt = serde_json::to_string(&input)?;
        let payload = json!({
            "systemInstruction": { "parts": [{ "text": SYSTEM_INSTRUCTION }] },
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }],
            "generationConfig": {
                "temperature": 0.3,
                // Keep enough headroom for the required JSON object. Gemini may
                // otherwise stop in the middle of a string (MAX_TOKENS).
                "maxOutputTokens": 4096,
                "responseMimeType": "application/json",
                "responseJsonSchema": response_schema()
            }
        });
        let payload_json = serde_json::to_string(&payload)?;
        info!(
            model = %self.model,
            deck = %deck.name,
            deck_cards = input.deck_cards.len(),
            candidate_cards = candidates.len(),
            prompt_bytes = prompt.len(),
            payload_bytes = payload_json.len(),
            "Preparing Gemini request"
        );
        debug!(prompt = %prompt, "Gemini request prompt (API key excluded)");
        let url = format!("{API_ROOT}/{}:generateContent", self.model);
        debug!(url = %url, "Sending Gemini request");
        let response = self
            .http
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let response_body = response.text().await?;
        info!(status = %status, response_bytes = response_body.len(), "Gemini response received");
        debug!(body = %truncate_for_log(&response_body, 8_000), "Gemini response body");
        if !status.is_success() {
            let message = truncate_for_log(&response_body, 500);
            return Err(CoachError::Api { status, message });
        }
        let response: GenerateResponse = serde_json::from_str(&response_body).map_err(|error| {
            warn!(error = %error, body = %truncate_for_log(&response_body, 8_000), "Gemini envelope was not valid JSON");
            error
        })?;
        let finish_reasons = response
            .candidates
            .iter()
            .filter_map(|candidate| candidate.finish_reason.as_deref())
            .collect::<Vec<_>>();
        if !finish_reasons.is_empty() {
            debug!(reasons = ?finish_reasons, "Gemini candidate finish reasons");
        }
        if response
            .candidates
            .first()
            .and_then(|c| c.finish_reason.as_deref())
            != Some("STOP")
        {
            return Err(CoachError::InvalidSuggestion(
                "La réponse Gemini est incomplète ou bloquée. Réessayez.".into(),
            ));
        }
        let text = response
            .candidates
            .into_iter()
            .take(1)
            .filter_map(|candidate| candidate.content)
            .flat_map(|content| content.parts)
            .filter(|part| !part.thought)
            .filter_map(|part| part.text)
            .collect::<String>();
        if text.trim().is_empty() {
            return Err(CoachError::EmptyResponse);
        }
        let analysis: DeckAnalysisResponse = serde_json::from_str(&text).map_err(|error| {
            warn!(
                error = %error,
                analysis_bytes = text.len(),
                analysis = %truncate_for_log(&text, 8_000),
                "Gemini analysis JSON could not be parsed"
            );
            error
        })?;
        validate_shortlist(&analysis, deck, &candidates)?;
        validate_analysis(&analysis, deck, collection)?;
        Ok(analysis)
    }
}

fn truncate_for_log(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("…[truncated]");
    }
    output
}

fn deck_entries(deck: &Deck) -> Vec<CardEntry<'_>> {
    let mut entries = Vec::new();
    add_deck_section(&mut entries, &deck.main_deck, "main_deck");
    add_deck_section(&mut entries, &deck.sideboard, "sideboard");
    add_deck_section(&mut entries, &deck.command_zone, "command_zone");
    entries
}

fn add_deck_section<'a>(
    entries: &mut Vec<CardEntry<'a>>,
    cards: &'a [DeckCard],
    section: &'static str,
) {
    entries.extend(cards.iter().map(|card| CardEntry {
        arena_id: card.arena_id,
        name: &card.name,
        quantity: card.quantity,
        set_code: &card.set_code,
        collector_number: &card.collector_number,
        section,
        type_line: &card.type_line,
    }));
}

fn validate_analysis(
    analysis: &DeckAnalysisResponse,
    deck: &Deck,
    collection: &[OwnedCard],
) -> Result<(), CoachError> {
    if analysis.improvement_suggestions.len() > 3 {
        return Err(CoachError::InvalidSuggestion(
            "more than three suggestions were returned".into(),
        ));
    }
    let deck_counts =
        deck_entries(deck)
            .into_iter()
            .fold(HashMap::<String, u32>::new(), |mut counts, card| {
                *counts.entry(normalize(card.name)).or_default() += card.quantity;
                counts
            });
    let collection_counts =
        collection
            .iter()
            .fold(HashMap::<String, u32>::new(), |mut counts, card| {
                *counts.entry(normalize(&card.name)).or_default() += card.quantity;
                counts
            });

    for suggestion in &analysis.improvement_suggestions {
        let mut removed = HashMap::<String, u32>::new();
        for card in &suggestion.card_to_remove {
            validate_quantity(card.quantity, &card.card_name)?;
            let name = normalize(&card.card_name);
            let total = removed.entry(name.clone()).or_default();
            *total += card.quantity;
            if *total > deck_counts.get(&name).copied().unwrap_or_default() {
                return Err(CoachError::InvalidSuggestion(format!(
                    "{} is not available in that quantity in the deck",
                    card.card_name
                )));
            }
        }
        let mut added = HashMap::<String, u32>::new();
        for card in &suggestion.card_to_add {
            validate_quantity(card.quantity, &card.card_name)?;
            let name = normalize(&card.card_name);
            let owned = collection_counts.get(&name).copied().ok_or_else(|| {
                CoachError::InvalidSuggestion(format!(
                    "{} is not present in the collection snapshot",
                    card.card_name
                ))
            })?;
            *added.entry(name.clone()).or_default() += card.quantity;
            let final_quantity = deck_counts.get(&name).copied().unwrap_or_default()
                - removed.get(&name).copied().unwrap_or_default()
                + added[&name];
            if final_quantity > owned {
                return Err(CoachError::InvalidSuggestion(format!(
                    "{} would exceed the owned quantity",
                    card.card_name
                )));
            }
        }
    }
    if analysis.combos.len() > 3 {
        return Err(CoachError::InvalidSuggestion(
            "more than three combos".into(),
        ));
    }
    for combo in &analysis.combos {
        let distinct = combo
            .cards
            .iter()
            .map(|c| normalize(&c.card_name))
            .collect::<std::collections::HashSet<_>>();
        if !(2..=4).contains(&combo.cards.len())
            || distinct.len() != combo.cards.len()
            || !(2..=5).contains(&combo.steps.len())
            || [
                &combo.title,
                &combo.prerequisites,
                &combo.payoff,
                &combo.limitations,
            ]
            .iter()
            .any(|s| s.trim().is_empty())
            || combo.steps.iter().any(|s| s.trim().is_empty())
        {
            return Err(CoachError::InvalidSuggestion(
                "combo must contain distinct cards, prerequisites, steps and limitations".into(),
            ));
        }
        for card in &combo.cards {
            validate_quantity(card.quantity, &card.card_name)?;
            let name = normalize(&card.card_name);
            let available = collection_counts.get(&name).copied().unwrap_or_default();
            if card.quantity > available {
                return Err(CoachError::InvalidSuggestion(format!(
                    "{} is unavailable for this combo",
                    card.card_name
                )));
            }
        }
    }
    Ok(())
}

fn validate_shortlist(
    analysis: &DeckAnalysisResponse,
    deck: &Deck,
    candidates: &[OwnedCard],
) -> Result<(), CoachError> {
    let names = candidates
        .iter()
        .map(|card| normalize(&card.name))
        .collect::<std::collections::HashSet<_>>();
    let supplied = names
        .iter()
        .cloned()
        .chain(deck_entries(deck).iter().map(|card| normalize(card.name)))
        .collect::<std::collections::HashSet<_>>();
    if analysis
        .improvement_suggestions
        .iter()
        .flat_map(|s| &s.card_to_add)
        .any(|card| !names.contains(&normalize(&card.card_name)))
        || analysis
            .combos
            .iter()
            .flat_map(|c| &c.cards)
            .any(|card| !supplied.contains(&normalize(&card.card_name)))
    {
        return Err(CoachError::InvalidSuggestion(
            "card was not included in the supplied shortlist or deck".into(),
        ));
    }
    Ok(())
}

fn validate_quantity(quantity: u32, name: &str) -> Result<(), CoachError> {
    if quantity == 0 {
        Err(CoachError::InvalidSuggestion(format!(
            "{name} has a zero quantity"
        )))
    } else {
        Ok(())
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

fn response_schema() -> Value {
    let card_change = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "card_name": { "type": "string", "description": "Exact English card name from the supplied data." },
            "quantity": { "type": "integer", "minimum": 1 }
        },
        "required": ["card_name", "quantity"]
    });
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "deck_summary": { "type": "string" },
            "game_plan": { "type": "string" },
            "play_challenge": { "type": "string" },
            "combos": {
                "type": "array", "maxItems": 3,
                "items": {
                    "type": "object", "additionalProperties": false,
                    "properties": {
                        "title": { "type": "string" },
                        "kind": { "type": "string", "enum": ["synergy", "sequence", "repeatable_loop", "infinite_loop"] },
                        "cards": { "type": "array", "minItems": 2, "maxItems": 4, "items": card_change.clone() },
                        "prerequisites": { "type": "string" },
                        "steps": { "type": "array", "minItems": 2, "maxItems": 5, "items": { "type": "string" } },
                        "payoff": { "type": "string" },
                        "limitations": { "type": "string" }
                    },
                    "required": ["title", "kind", "cards", "prerequisites", "steps", "payoff", "limitations"]
                }
            },
            "strengths": { "type": "array", "maxItems": 5, "items": { "type": "string" } },
            "weaknesses": { "type": "array", "maxItems": 5, "items": { "type": "string" } },
            "improvement_suggestions": {
                "type": "array",
                "maxItems": 3,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "title": { "type": "string" },
                        "priority": { "type": "string", "enum": ["high", "medium", "low"] },
                        "card_to_remove": { "type": "array", "items": card_change.clone() },
                        "card_to_add": { "type": "array", "items": card_change },
                        "reasoning": { "type": "string" }
                    },
                    "required": ["title", "priority", "card_to_remove", "card_to_add", "reasoning"]
                }
            }
        },
        "required": ["deck_summary", "game_plan", "strengths", "weaknesses", "improvement_suggestions", "combos", "play_challenge"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CardChange, ImprovementSuggestion, SuggestionPriority};

    fn combo_report() -> DeckAnalysisResponse {
        serde_json::from_value(json!({
            "deck_summary":"Résumé", "game_plan":"Plan", "strengths":[], "weaknesses":[],
            "improvement_suggestions":[], "play_challenge":"Essayer une séquence",
            "combos":[{"title":"Interaction", "kind":"synergy",
                "cards":[{"card_name":"First", "quantity":1},{"card_name":"Second", "quantity":1}],
                "prerequisites":"Deux permanents en jeu", "steps":["Jouer First", "Déclencher Second"],
                "payoff":"Une ressource", "limitations":"Interrompu par un retrait"}]
        })).unwrap()
    }

    #[test]
    fn validates_combo_ownership_and_minimum_pieces() {
        let deck = Deck {
            main_deck: vec![DeckCard {
                name: "First".into(),
                quantity: 1,
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut candidates = vec![OwnedCard {
            name: "Second".into(),
            quantity: 1,
            ..Default::default()
        }];
        let mut report = combo_report();
        assert!(validate_analysis(&report, &deck, &candidates).is_err());
        candidates.push(OwnedCard {
            name: "First".into(),
            quantity: 1,
            ..Default::default()
        });
        assert!(validate_analysis(&report, &deck, &candidates).is_ok());
        assert!(validate_shortlist(&report, &deck, &candidates).is_ok());
        assert!(validate_shortlist(&report, &deck, &[]).is_err());
        report.combos[0].cards[1].card_name = "Not supplied".into();
        assert!(validate_analysis(&report, &deck, &candidates).is_err());
        report.combos[0].cards[1].card_name = "Second".into();
        report.combos[0].cards[1].quantity = 2;
        assert!(validate_analysis(&report, &deck, &candidates).is_err());
        report.combos[0].cards[1].quantity = 1;
        report.combos[0].limitations.clear();
        assert!(validate_analysis(&report, &deck, &candidates).is_err());
    }

    #[test]
    fn old_reports_remain_readable() {
        let report: DeckAnalysisResponse = serde_json::from_value(json!({
            "deck_summary":"Old", "game_plan":"Plan", "strengths":[], "weaknesses":[], "improvement_suggestions":[]
        })).unwrap();
        assert!(report.combos.is_empty());
        assert!(report.play_challenge.is_empty());
    }

    #[test]
    fn rejects_duplicate_combo_pieces_and_invalid_kinds() {
        let mut report = combo_report();
        report.combos[0].cards[1].card_name = "First".into();
        assert!(validate_analysis(&report, &Deck::default(), &[]).is_err());
        let mut json = serde_json::to_value(combo_report()).unwrap();
        json["combos"][0]["kind"] = json!("guaranteed_win");
        assert!(serde_json::from_value::<DeckAnalysisResponse>(json).is_err());
    }

    #[test]
    fn rejects_a_card_outside_the_collection() {
        let deck = Deck {
            main_deck: vec![DeckCard {
                name: "Old Card".into(),
                quantity: 2,
                ..Default::default()
            }],
            ..Default::default()
        };
        let analysis = DeckAnalysisResponse {
            combos: vec![],
            play_challenge: String::new(),
            deck_summary: "Summary".into(),
            game_plan: "Plan".into(),
            strengths: vec![],
            weaknesses: vec![],
            improvement_suggestions: vec![ImprovementSuggestion {
                title: "Change".into(),
                priority: SuggestionPriority::High,
                card_to_remove: vec![],
                card_to_add: vec![CardChange {
                    card_name: "Imaginary Card".into(),
                    quantity: 1,
                }],
                reasoning: "Reason".into(),
            }],
        };
        assert!(validate_analysis(&analysis, &deck, &[]).is_err());
    }
}
