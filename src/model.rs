use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub collection: Vec<OwnedCard>,
    pub decks: Vec<Deck>,
    pub wildcards: Wildcards,
    pub synced_at: Option<String>,
    pub source_modified_at: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedCard {
    pub arena_id: u64,
    pub name: String,
    pub quantity: u32,
    pub type_line: String,
    pub colors: Vec<String>,
    pub set_code: Option<String>,
    pub collector_number: Option<String>,
    #[serde(default)]
    pub rarity: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckCard {
    pub arena_id: u64,
    pub name: String,
    pub quantity: u32,
    pub type_line: String,
    pub colors: Vec<String>,
    pub set_code: Option<String>,
    pub collector_number: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deck {
    pub id: String,
    pub name: String,
    pub format: Option<String>,
    pub colors: Vec<String>,
    pub main_deck: Vec<DeckCard>,
    pub sideboard: Vec<DeckCard>,
    pub command_zone: Vec<DeckCard>,
    pub card_count: u32,
    /// True for decks created or edited by the player, false for Arena-provided decks.
    pub is_user_deck: bool,
    pub last_played: Option<String>,
    pub last_updated: Option<String>,
    pub is_favorite: bool,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub events: Vec<String>,
    #[serde(skip)]
    pub(crate) source_priority: u8,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wildcards {
    pub common: u32,
    pub uncommon: u32,
    pub rare: u32,
    pub mythic: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub log_path: String,
    pub log_exists: bool,
    pub synced_at: Option<String>,
    pub source_modified_at: Option<String>,
    pub cards_owned: usize,
    pub total_copies: u32,
    pub deck_count: usize,
    pub wildcards: Wildcards,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub log_path: String,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub gemini_api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub log_path: String,
    pub gemini_configured: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettings {
    pub log_path: String,
    pub gemini_api_key: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CardMetadata {
    pub name: Option<String>,
    pub type_line: Option<String>,
    pub colors: Vec<String>,
    pub set_code: Option<String>,
    pub collector_number: Option<String>,
    pub rarity: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisReport {
    pub id: i64,
    pub deck_id: String,
    pub deck_name: String,
    pub created_at: i64,
    pub model: String,
    pub deck_card_count: u32,
    pub collection_card_count: u32,
    pub analysis: DeckAnalysisResponse,
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisReportSummary {
    pub id: i64,
    pub deck_id: String,
    pub deck_name: String,
    pub created_at: i64,
    pub model: String,
    pub deck_card_count: u32,
    pub collection_card_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DeckAnalysisResponse {
    pub deck_summary: String,
    pub game_plan: String,
    pub strengths: Vec<String>,
    pub weaknesses: Vec<String>,
    pub improvement_suggestions: Vec<ImprovementSuggestion>,
    /// Defaults keep historical reports readable after the coaching upgrade.
    #[serde(default)]
    pub combos: Vec<DeckCombo>,
    #[serde(default)]
    pub play_challenge: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckCombo {
    pub title: String,
    pub kind: ComboKind,
    pub cards: Vec<CardChange>,
    pub prerequisites: String,
    pub steps: Vec<String>,
    pub payoff: String,
    pub limitations: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComboKind {
    Synergy,
    Sequence,
    RepeatableLoop,
    InfiniteLoop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ImprovementSuggestion {
    pub title: String,
    pub priority: SuggestionPriority,
    pub card_to_remove: Vec<CardChange>,
    pub card_to_add: Vec<CardChange>,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CardChange {
    pub card_name: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionPriority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeDeckRequest {
    pub deck_id: String,
}
