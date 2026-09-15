//! One bounded Forge worker, immutable deck snapshots and durable per-run reports.
use crate::model::{Deck, DeckCard, OwnedCard};
use anyhow::{bail, Context, Result};
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{watch, Mutex},
};

const BRIDGE: &str = include_str!("../bridge/MagicDeckSimulation.java");
const MAX_LOG_BYTES: usize = 2 * 1024 * 1024;

fn forge_worker_limit() -> usize {
    std::env::var("MAGIC_DECK_FORGE_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=16).contains(value))
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|parallelism| parallelism.get().min(8))
                .unwrap_or(4)
        })
}

fn forge_heap() -> String {
    std::env::var("MAGIC_DECK_FORGE_XMX").unwrap_or_else(|_| "2g".into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineSettings {
    pub forge_dir: String,
    pub java_path: String,
}
impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            forge_dir: String::new(),
            java_path: "java".into(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunRequest {
    pub deck_id: String,
    #[serde(default)]
    pub deck: Option<Deck>,
    #[serde(default)]
    pub opponent_id: Option<String>,
    #[serde(default)]
    pub opponent_ids: Vec<String>,
    pub games: u32,
    pub seed: u32,
    pub seconds_per_game: u32,
    #[serde(default)]
    pub parallel: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RandomLabRequest {
    #[serde(default)]
    pub deck_id: Option<String>,
    pub colors: Vec<String>,
    #[serde(default)]
    pub bicolor: bool,
    #[serde(default = "default_land_count")]
    pub land_count: u32,
    #[serde(default = "default_creature_count")]
    pub creature_count: u32,
    #[serde(default = "default_noncreature_count")]
    pub noncreature_count: u32,
    #[serde(default = "default_dual_land_count")]
    pub dual_land_count: u32,
    #[serde(default = "default_lands_per_color")]
    pub lands_per_color: u32,
    pub candidate_count: u32,
    pub opponent_ids: Vec<String>,
    #[serde(default)]
    pub seed: Option<u64>,
}

fn default_land_count() -> u32 {
    24
}
fn default_creature_count() -> u32 {
    20
}
fn default_noncreature_count() -> u32 {
    16
}
fn default_dual_land_count() -> u32 {
    8
}
fn default_lands_per_color() -> u32 {
    7
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RandomLabCandidate {
    pub id: String,
    pub deck: Deck,
    pub report_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RandomLabCampaign {
    pub id: String,
    pub created_at: u64,
    pub request: RandomLabRequest,
    pub opponents: Vec<Deck>,
    pub candidates: Vec<RandomLabCandidate>,
}

#[derive(Clone, Default)]
pub struct RandomLabStore {
    campaigns: Arc<Mutex<HashMap<String, RandomLabCampaign>>>,
}

/// Builds deterministic, collection-only deck candidates. The generator is intentionally
/// heuristic: it enforces the requested shape and card ownership, while Forge remains the
/// authority for actual game performance.
pub fn generate_random_decks(
    collection: &[OwnedCard],
    request: &RandomLabRequest,
) -> Result<Vec<Deck>> {
    let colors = request
        .colors
        .iter()
        .map(|color| color.trim().to_ascii_uppercase())
        .collect::<Vec<_>>();
    let land_count = if request.bicolor {
        request.dual_land_count + request.lands_per_color.saturating_mul(2)
    } else {
        request.land_count
    };
    let total = land_count + request.creature_count + request.noncreature_count;
    if total != 60 {
        bail!("Les contraintes doivent totaliser 60 cartes (actuellement {total}).");
    }
    let grouped = grouped_collection(collection);
    let creatures = grouped
        .iter()
        .filter(|card| is_creature(card) && compatible(card, &colors))
        .cloned()
        .collect::<Vec<_>>();
    let spells = grouped
        .iter()
        .filter(|card| !is_land(card) && !is_creature(card) && compatible(card, &colors))
        .cloned()
        .collect::<Vec<_>>();
    let lands = grouped
        .iter()
        .filter(|card| is_land(card) && compatible(card, &colors))
        .cloned()
        .collect::<Vec<_>>();
    if creatures.is_empty() || spells.is_empty() {
        bail!("La collection ne contient pas assez de créatures et de cartes non-terrain dans ces couleurs.");
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(request.seed.unwrap_or_else(rand::random));
    // Do not pre-allocate from user input: the candidate count is intentionally
    // unbounded and can be very large on long-running local experiments.
    let mut decks = Vec::new();
    let mut signatures = HashSet::new();
    for index in 0..request.candidate_count {
        let mut deck = Deck {
            id: format!("random-lab-{}-{index}", timestamp_id()),
            name: format!("Généré localement · variante {}", index + 1),
            format: Some("Construit · génération locale".into()),
            colors: colors.clone(),
            ..Default::default()
        };
        let mut remaining = grouped
            .iter()
            .map(|card| {
                (
                    card.name.to_lowercase(),
                    if is_basic(&card.name) {
                        60
                    } else {
                        card.quantity.min(4)
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        if request.bicolor {
            let duals = lands
                .iter()
                .filter(|card| is_dual_land(card))
                .cloned()
                .collect::<Vec<_>>();
            for _ in 0..request.dual_land_count {
                let card = choose(&duals, &mut remaining, &mut rng)
                    .context("Pas assez de terrains doubles possédés.")?;
                add_card(&mut deck, &card, 1);
            }
            for color in colors.iter().take(2) {
                let basic = basic_land(color);
                for _ in 0..request.lands_per_color {
                    if let Some(card) = lands
                        .iter()
                        .find(|card| basic.eq_ignore_ascii_case(&card.name))
                    {
                        add_card(&mut deck, card, 1);
                    } else {
                        add_basic(&mut deck, &basic, 1);
                    }
                }
            }
        } else {
            let basic = basic_land(colors.first().map(String::as_str).unwrap_or("W"));
            for _ in 0..land_count {
                if let Some(card) = choose(&lands, &mut remaining, &mut rng) {
                    add_card(&mut deck, &card, 1);
                } else {
                    add_basic(&mut deck, &basic, 1);
                }
            }
        }
        for _ in 0..request.creature_count {
            let card = choose(&creatures, &mut remaining, &mut rng)
                .context("Quantité de créatures insuffisante dans la collection.")?;
            add_card(&mut deck, &card, 1);
        }
        for _ in 0..request.noncreature_count {
            let card = choose(&spells, &mut remaining, &mut rng)
                .context("Quantité de cartes non-créature insuffisante dans la collection.")?;
            add_card(&mut deck, &card, 1);
        }
        deck.card_count = deck.main_deck.iter().map(|card| card.quantity).sum();
        let signature = deck
            .main_deck
            .iter()
            .map(|card| format!("{}:{}", card.name.to_lowercase(), card.quantity))
            .collect::<Vec<_>>()
            .join("|");
        if signatures.insert(signature) {
            decks.push(deck);
        }
    }
    if decks.is_empty() {
        bail!("Aucune combinaison distincte n'a pu être générée.");
    }
    Ok(decks)
}

fn grouped_collection(collection: &[OwnedCard]) -> Vec<OwnedCard> {
    let mut grouped = HashMap::<String, OwnedCard>::new();
    for card in collection {
        let key = card.name.to_lowercase();
        let entry = grouped.entry(key).or_insert_with(|| card.clone());
        entry.quantity = entry.quantity.saturating_add(card.quantity);
    }
    grouped.into_values().collect()
}

fn compatible(card: &OwnedCard, colors: &[String]) -> bool {
    card.colors.is_empty()
        || card.colors.iter().all(|color| {
            colors
                .iter()
                .any(|chosen| chosen.eq_ignore_ascii_case(color))
        })
}
fn is_land(card: &OwnedCard) -> bool {
    card.type_line.to_ascii_lowercase().contains("land")
}
fn is_creature(card: &OwnedCard) -> bool {
    card.type_line.to_ascii_lowercase().contains("creature")
}
fn is_basic(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "plains" | "island" | "swamp" | "mountain" | "forest"
    )
}
fn is_dual_land(card: &OwnedCard) -> bool {
    if is_basic(&card.name) {
        return false;
    }
    if card.colors.len() >= 2 {
        return true;
    }
    let name = card.name.to_ascii_lowercase();
    [
        "guildgate",
        "cove",
        "temple",
        "pathway",
        "triome",
        "shock",
        "check",
        "campus",
        "anchorage",
        "harbor",
        "falls",
        "grove",
        "fortress",
        "summit",
        "catacomb",
        "chamber",
        "refuge",
        "barrens",
        "crossroads",
        "gateway",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}
fn basic_land(color: &str) -> String {
    match color {
        "U" => "Island",
        "B" => "Swamp",
        "R" => "Mountain",
        "G" => "Forest",
        _ => "Plains",
    }
    .into()
}
fn add_basic(deck: &mut Deck, name: &str, quantity: u32) {
    let card = OwnedCard {
        name: name.into(),
        type_line: "Basic Land".into(),
        ..Default::default()
    };
    add_card(deck, &card, quantity);
}
fn add_card(deck: &mut Deck, card: &OwnedCard, quantity: u32) {
    if let Some(existing) = deck
        .main_deck
        .iter_mut()
        .find(|existing| existing.name.eq_ignore_ascii_case(&card.name))
    {
        existing.quantity += quantity;
    } else {
        deck.main_deck.push(DeckCard {
            arena_id: card.arena_id,
            name: card.name.clone(),
            quantity,
            type_line: card.type_line.clone(),
            colors: card.colors.clone(),
            set_code: card.set_code.clone(),
            collector_number: card.collector_number.clone(),
        });
    }
}
fn choose(
    pool: &[OwnedCard],
    remaining: &mut HashMap<String, u32>,
    rng: &mut rand::rngs::StdRng,
) -> Option<OwnedCard> {
    let available = pool
        .iter()
        .filter(|card| {
            remaining
                .get(&card.name.to_lowercase())
                .copied()
                .unwrap_or_default()
                > 0
        })
        .collect::<Vec<_>>();
    if available.is_empty() {
        return None;
    }
    let card = available[rng.random_range(0..available.len())].clone();
    if let Some(quantity) = remaining.get_mut(&card.name.to_lowercase()) {
        *quantity -= 1;
    }
    Some(card)
}

impl RandomLabStore {
    pub async fn insert(&self, campaign: RandomLabCampaign) {
        let mut campaigns = self.campaigns.lock().await;
        campaigns.insert(campaign.id.clone(), campaign);
        if campaigns.len() > 20 {
            if let Some(oldest) = campaigns
                .values()
                .min_by_key(|campaign| campaign.created_at)
                .map(|campaign| campaign.id.clone())
            {
                campaigns.remove(&oldest);
            }
        }
    }

    pub async fn get(&self, id: &str) -> Option<RandomLabCampaign> {
        self.campaigns.lock().await.get(id).cloned()
    }

    pub async fn list(&self) -> Vec<RandomLabCampaign> {
        let mut campaigns = self
            .campaigns
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        campaigns.sort_by_key(|campaign| std::cmp::Reverse(campaign.created_at));
        campaigns
    }

    pub async fn assign_report(&self, campaign_id: &str, candidate_id: &str, report_id: String) {
        if let Some(candidate) =
            self.campaigns
                .lock()
                .await
                .get_mut(campaign_id)
                .and_then(|campaign| {
                    campaign
                        .candidates
                        .iter_mut()
                        .find(|c| c.id == candidate_id)
                })
        {
            candidate.report_id = Some(report_id);
        }
    }

    pub async fn assign_error(&self, campaign_id: &str, candidate_id: &str, error: String) {
        if let Some(candidate) =
            self.campaigns
                .lock()
                .await
                .get_mut(campaign_id)
                .and_then(|campaign| {
                    campaign
                        .candidates
                        .iter_mut()
                        .find(|c| c.id == candidate_id)
                })
        {
            candidate.error = Some(error);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameResult {
    pub index: u32,
    #[serde(default)]
    pub opponent_index: usize,
    pub outcome: String,
    pub duration_ms: u64,
    pub turns: u32,
    pub seat: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub id: String,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub status: String,
    pub deck: Deck,
    pub opponent: Deck,
    #[serde(default)]
    pub opponents: Vec<Deck>,
    pub games_requested: u32,
    #[serde(default)]
    pub games_per_opponent: u32,
    pub seed: u32,
    pub seconds_per_game: u32,
    pub engine: EngineSettings,
    pub engine_version: Option<String>,
    pub adapter_version: String,
    pub results: Vec<GameResult>,
    pub error: Option<String>,
}
impl RunReport {
    pub fn summary(&self) -> Value {
        let wins = self.results.iter().filter(|g| g.outcome == "win").count();
        let losses = self.results.iter().filter(|g| g.outcome == "loss").count();
        let draws = self.results.iter().filter(|g| g.outcome == "draw").count();
        let n = wins + losses;
        json!({"id":self.id, "createdAt":self.created_at, "status":self.status,
            "deckId":self.deck.id,"deckName":self.deck.name,"opponentName":self.opponent.name,
            "opponentNames":self.opponents.iter().map(|opponent| opponent.name.clone()).collect::<Vec<_>>(),
            "gamesRequested":self.games_requested,"completed":self.results.len(),
            "wins":wins,"losses":losses,"draws":draws,
            "timeouts":self.results.iter().filter(|g|g.outcome=="timeout").count(),
            "winRate":if n==0 {None} else {Some(wins as f64 / n as f64)},
            "confidence95":wilson(wins,n),"engineVersion":self.engine_version,"error":self.error})
    }
}

// Wilson interval for decisive games only: never count a crash or timeout as a loss/draw.
fn wilson(wins: usize, n: usize) -> Option<[f64; 2]> {
    if n == 0 {
        return None;
    }
    let n = n as f64;
    let p = wins as f64 / n;
    let z2 = 1.96_f64.powi(2);
    let middle = (p + z2 / (2.0 * n)) / (1.0 + z2 / n);
    let radius = 1.96 * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt() / (1.0 + z2 / n);
    Some([(middle - radius).max(0.0), (middle + radius).min(1.0)])
}

struct Active {
    id: String,
    cancel: watch::Sender<bool>,
}
#[derive(Clone)]
pub struct SimulationService {
    root: PathBuf,
    active: Arc<Mutex<Vec<Active>>>,
}

impl SimulationService {
    pub fn open_default() -> Result<Self> {
        let root = std::env::var_os("MAGIC_DECK_SIM_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::data_local_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("magic-deck/simulations")
            });
        Self::open(root)
    }
    fn open(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        let root = root.canonicalize()?;
        // A server restart never presents an abandoned worker as still running.
        for entry in std::fs::read_dir(&root)? {
            let path = entry?.path().join("report.json");
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(mut report) = serde_json::from_slice::<RunReport>(&bytes) {
                    if matches!(report.status.as_str(), "starting" | "running") {
                        report.status = "interrupted".into();
                        report.finished_at = Some(now());
                        report.error = Some("Magic Deck a été arrêté pendant cette série.".into());
                        std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
                    }
                }
            }
        }
        Ok(Self {
            root,
            active: Arc::new(Mutex::new(Vec::new())),
        })
    }
    pub async fn settings(&self) -> Result<EngineSettings> {
        let path = self.root.join("engine.json");
        let mut settings: EngineSettings = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).context("Configuration Forge illisible")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => EngineSettings::default(),
            Err(error) => return Err(error.into()),
        };
        if let Ok(dir) = std::env::var("MAGIC_DECK_FORGE_DIR") {
            settings.forge_dir = dir;
        }
        if settings.forge_dir.is_empty() {
            if let Some(data) = dirs::data_local_dir() {
                let installed = data.join("magic-deck/forge/2.0.14");
                if installed.is_dir() {
                    settings.forge_dir = installed.to_string_lossy().into_owned();
                }
            }
        }
        Ok(settings)
    }
    pub async fn configure(&self, mut settings: EngineSettings) -> Result<()> {
        let active = self.active.lock().await;
        if !active.is_empty() {
            bail!("Attendez la fin de la série avant de modifier Forge.");
        }
        settings.forge_dir = settings.forge_dir.trim().to_owned();
        settings.java_path = settings.java_path.trim().to_owned();
        if settings.java_path.is_empty() {
            bail!("Indiquez le chemin de Java.");
        }
        let _ = engine_jar(&settings).await?;
        atomic_json(&self.root.join("engine.json"), &settings).await
    }
    pub async fn engine_status(&self) -> Result<Value> {
        let settings = self.settings().await?;
        let problem = match engine_jar(&settings).await {
            Err(e) => Some(e.to_string()),
            Ok(_) => {
                let result = tokio::time::timeout(
                    Duration::from_secs(5),
                    Command::new(&settings.java_path)
                        .arg("--list-modules")
                        .kill_on_drop(true)
                        .output(),
                )
                .await;
                match result {
                    Ok(Ok(output)) if output.status.success() && jdk_supported(&String::from_utf8_lossy(&output.stdout)) => None,
                    _ => Some("Java introuvable : installez un JDK 17 ou supérieur et indiquez son exécutable.".into()),
                }
            }
        };
        Ok(
            json!({"settings":settings,"ready":problem.is_none(),"problem":problem,
            "activeId":self.active.lock().await.first().map(|a|a.id.clone()),
            "recommendedVersion":"2.0.14"}),
        )
    }
    fn run_dir(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty() || id.len() > 40 || !id.bytes().all(|c| c.is_ascii_digit() || c == b'-') {
            bail!("Identifiant de simulation invalide.");
        }
        Ok(self.root.join(id))
    }
    pub async fn get(&self, id: &str) -> Result<RunReport> {
        let bytes = tokio::fs::read(self.run_dir(id)?.join("report.json"))
            .await
            .context("Simulation introuvable")?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub async fn logs(&self, id: &str) -> Result<String> {
        Ok(tokio::fs::read_to_string(self.run_dir(id)?.join("engine.log")).await?)
    }
    pub async fn list(&self) -> Result<Vec<Value>> {
        let mut entries = tokio::fs::read_dir(&self.root).await?;
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort_unstable_by(|a, b| b.cmp(a));
        let mut reports = Vec::new();
        for id in names.into_iter().take(100) {
            match self.get(&id).await {
                Ok(report) => reports.push(report.summary()),
                Err(error) => tracing::warn!(%id,%error,"Simulation history entry unreadable"),
            }
        }
        Ok(reports)
    }
    pub async fn cancel(&self, id: &str) -> Result<()> {
        let active = self.active.lock().await;
        let job = active
            .iter()
            .find(|a| a.id == id)
            .context("Cette série n'est plus en cours.")?;
        job.cancel.send(true)?;
        Ok(())
    }
    pub async fn shutdown(&self) {
        for active in self.active.lock().await.iter() {
            let _ = active.cancel.send(true);
        }
    }
    pub async fn start(
        &self,
        request: RunRequest,
        deck: Deck,
        opponents: Vec<Deck>,
    ) -> Result<RunReport> {
        if !(1..=1000).contains(&request.games) || !(10..=300).contains(&request.seconds_per_game) {
            bail!("Choisissez 1 à 1 000 parties et un délai de 10 à 300 secondes par partie.");
        }
        let challenger = forge_deck(&deck)?;
        if opponents.is_empty() {
            bail!("Sélectionnez au moins un deck adverse.");
        }
        let opponent_count = opponents.len() as u32;
        let opposition = opponents
            .iter()
            .map(forge_deck)
            .collect::<Result<Vec<_>>>()?;
        let mut active = self.active.lock().await;
        if (!request.parallel && !active.is_empty()) || active.len() >= forge_worker_limit() {
            bail!("Une série est déjà en cours. Annulez-la ou attendez sa fin.");
        }
        let settings = self.settings().await?;
        let jar = engine_jar(&settings).await?;
        let id = timestamp_id();
        let dir = self.run_dir(&id)?;
        tokio::fs::create_dir(&dir).await?;
        tokio::fs::create_dir(dir.join("home")).await?;
        tokio::fs::write(dir.join("challenger.dck"), challenger).await?;
        for (index, contents) in opposition.into_iter().enumerate() {
            tokio::fs::write(dir.join(format!("opponent-{index}.dck")), contents).await?;
        }
        tokio::fs::write(dir.join("MagicDeckSimulation.java"), BRIDGE).await?;
        let report = RunReport {
            id: id.clone(),
            created_at: now(),
            finished_at: None,
            status: "starting".into(),
            deck,
            opponent: opponents[0].clone(),
            opponents,
            games_requested: request.games * opponent_count,
            games_per_opponent: request.games,
            seed: request.seed,
            seconds_per_game: request.seconds_per_game,
            engine: settings,
            engine_version: None,
            adapter_version: "forge-bridge-v1".into(),
            results: Vec::new(),
            error: None,
        };
        atomic_json(&dir.join("report.json"), &report).await?;
        let (cancel, receiver) = watch::channel(false);
        active.push(Active {
            id: id.clone(),
            cancel,
        });
        let service = self.clone();
        let job = report.clone();
        tokio::spawn(async move {
            let mut job = job;
            if let Err(error) = service.run(&mut job, &jar, receiver).await {
                job.status = "failed".into();
                job.error = Some(error.to_string());
                tracing::warn!(id=%job.id,%error,"Forge simulation failed");
            }
            job.finished_at = Some(now());
            if let Err(error) = atomic_json(&dir.join("report.json"), &job).await {
                tracing::error!(%error,"Cannot persist simulation report");
            }
            service
                .active
                .lock()
                .await
                .retain(|active| active.id != job.id);
        });
        Ok(report)
    }

    async fn run(
        &self,
        report: &mut RunReport,
        jar: &Path,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<()> {
        let dir = self.run_dir(&report.id)?;
        let mut command = Command::new(&report.engine.java_path);
        command
            .current_dir(jar.parent().context("Dossier Forge invalide")?)
            .args([
                &format!("-Xmx{}", forge_heap()),
                "-XX:+UseSerialGC",
                "-Djava.awt.headless=true",
                "-Dfile.encoding=UTF-8",
            ])
            .arg(format!("-Duser.home={}", dir.join("home").display()))
            .arg("--class-path")
            .arg(jar)
            .arg(dir.join("MagicDeckSimulation.java"))
            .arg(dir.join("challenger.dck"));
        for index in 0..report.opponents.len() {
            command.arg(dir.join(format!("opponent-{index}.dck")));
        }
        command
            .arg("--")
            .arg(report.games_per_opponent.to_string())
            .arg(report.seed.to_string())
            .arg(report.seconds_per_game.to_string())
            .env("APPDATA", dir.join("home"))
            .env("LOCALAPPDATA", dir.join("home"))
            .env_remove("GEMINI_API_KEY")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .context("Impossible de lancer Java. Un JDK 17+ est nécessaire.")?;
        let mut lines =
            BufReader::new(child.stdout.take().context("stdout Forge indisponible")?).lines();
        let mut stderr = child.stderr.take().context("stderr Forge indisponible")?;
        let stderr_task = tokio::spawn(async move {
            let mut tail = Vec::new();
            let mut chunk = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut chunk).await {
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&chunk[..n]);
                if tail.len() > 16_384 {
                    tail.drain(..tail.len() - 16_384);
                }
            }
            String::from_utf8_lossy(&tail).into_owned()
        });
        let mut log = tokio::fs::File::create(dir.join("engine.log")).await?;
        let mut logged = 0;
        let mut done = false;
        let mut error = None;
        let mut deadline = tokio::time::Instant::now() + Duration::from_secs(180);
        loop {
            tokio::select! {
                _ = cancel.changed() => { report.status = "cancelled".into(); break; },
                _ = tokio::time::sleep_until(deadline) => { error = Some("Forge ne répond plus : délai de démarrage ou de partie dépassé.".to_owned()); break; },
                line = lines.next_line() => {
                    let line = match line { Ok(Some(line)) => line, Ok(None) => break,
                        Err(e) => { error=Some(e.to_string()); break; } };
                    if logged + line.len() < MAX_LOG_BYTES {
                        log.write_all(line.as_bytes()).await?; log.write_all(b"\n").await?; logged += line.len()+1;
                    }
                    if let Some(event) = line.strip_prefix("MAGIC_DECK ") {
                        match apply_event(report,event) {
                            Ok(is_done) => { done |= is_done; },
                            Err(e) => { error = Some(e.to_string()); break; }
                        }
                        // Checkpointing every event makes long campaigns spend a surprising
                        // amount of time serializing and renaming JSON files. Keep immediate
                        // visibility for lifecycle transitions, and batch game checkpoints.
                        let checkpoint = matches!(report.status.as_str(), "running" | "completed")
                            && (done || report.results.len().is_multiple_of(10));
                        if checkpoint || report.status == "starting" {
                            atomic_json(&dir.join("report.json"), report).await?;
                        }
                        deadline = tokio::time::Instant::now() + Duration::from_secs(u64::from(report.seconds_per_game)+30);
                    }
                }
            }
        }
        if error.is_some() || report.status == "cancelled" {
            let _ = child.kill().await;
        }
        let status = match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
            Ok(result) => result?,
            Err(_) => {
                child.kill().await?;
                child.wait().await?
            }
        };
        let tail = stderr_task.await?;
        log.write_all(tail.as_bytes()).await?;
        if report.status == "cancelled" {
            return Ok(());
        }
        if let Some(error) = error {
            bail!("{error}");
        }
        if !status.success() || !done || report.results.len() != report.games_requested as usize {
            bail!(
                "Forge n'a pas produit toutes les parties attendues (code {:?}). {}",
                status.code(),
                tail.chars().take(2000).collect::<String>()
            );
        }
        report.status = "completed".into();
        Ok(())
    }
}

fn jdk_supported(modules: &str) -> bool {
    modules.lines().any(|line| {
        line.strip_prefix("jdk.compiler@").is_some_and(|version| {
            version
                .split(['.', '-'])
                .next()
                .and_then(|major| major.parse::<u32>().ok())
                .is_some_and(|major| major >= 17)
        })
    })
}

async fn engine_jar(settings: &EngineSettings) -> Result<PathBuf> {
    if settings.forge_dir.trim().is_empty() {
        bail!("Installez Forge puis renseignez son dossier pour activer les simulations.");
    }
    let dir = tokio::fs::canonicalize(&settings.forge_dir)
        .await
        .context("Dossier Forge introuvable")?;
    if !dir.join("res").is_dir() {
        bail!("La distribution Forge complète est nécessaire (dossier res absent).");
    }
    if dir.join("forge.profile.properties").exists() {
        bail!("Cette installation Forge possède un profil personnalisé. Utilisez une copie dédiée sans forge.profile.properties pour isoler les simulations.");
    }
    let mut files = tokio::fs::read_dir(&dir).await?;
    let mut jars = Vec::new();
    while let Some(entry) = files.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("forge-gui-desktop-") && name.ends_with("-jar-with-dependencies.jar") {
            jars.push(entry.path());
        }
    }
    if jars.len() != 1 {
        bail!("Le dossier doit contenir un seul forge-gui-desktop-…-jar-with-dependencies.jar.");
    }
    Ok(jars.remove(0))
}

pub fn forge_deck(deck: &Deck) -> Result<String> {
    if !deck.command_zone.is_empty()
        || deck.format.as_deref().is_some_and(|f| {
            let f = f.to_lowercase();
            f.contains("brawl") || f.contains("commander") || f.contains("oathbreaker")
        })
    {
        bail!("Les simulations prennent en charge les duels construits BO1 sans commandant pour le moment.");
    }
    let count: u64 = deck.main_deck.iter().map(|c| u64::from(c.quantity)).sum();
    if !(40..=250).contains(&count) {
        bail!(
            "Le deck {} doit contenir 40 à 250 cartes principales.",
            deck.name
        );
    }
    let mut output = String::from("[metadata]\nName=Magic Deck\n[Main]\n");
    for card in &deck.main_deck {
        if card.quantity == 0
            || card.name.trim().is_empty()
            || card.name.contains(['\n', '\r', '|'])
            || card.name.starts_with("Arena card #")
        {
            bail!("Carte non résolue ou invalide : {}", card.name);
        }
        output.push_str(&format!("{} {}\n", card.quantity, card.name));
    }
    Ok(output)
}

fn apply_event(report: &mut RunReport, line: &str) -> Result<bool> {
    let event: Value = serde_json::from_str(line).context("Réponse Forge illisible")?;
    match event["kind"].as_str() {
        Some("ready") if report.status == "starting" => {
            report.engine_version = Some(
                event["version"]
                    .as_str()
                    .context("Version Forge absente")?
                    .into(),
            );
            report.status = "running".into();
        }
        Some("game") if report.status == "running" => {
            let game: GameResult = serde_json::from_value(event)?;
            if game.index as usize != report.results.len() + 1
                || game.index > report.games_requested
                || game.opponent_index >= report.opponents.len()
                || !matches!(game.outcome.as_str(), "win" | "loss" | "draw" | "timeout")
                || !(1..=2).contains(&game.seat)
            {
                bail!("Résultat Forge incohérent.");
            }
            report.results.push(game);
        }
        Some("done")
            if report.status == "running"
                && report.results.len() == report.games_requested as usize
                && report.results.iter().all(|g| g.outcome != "timeout") =>
        {
            return Ok(true)
        }
        Some("error") => bail!(
            "{}",
            event["message"]
                .as_str()
                .unwrap_or("Erreur du moteur Forge")
        ),
        _ => bail!("Événement Forge inattendu."),
    }
    Ok(false)
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn timestamp_id() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}

pub fn reference_decks() -> Vec<Deck> {
    use crate::model::DeckCard;
    [
        (
            "reference-red",
            "Référence · Aggro rouge",
            "Mountain",
            [
                "Goblin Guide",
                "Monastery Swiftspear",
                "Viashino Pyromancer",
                "Ghitu Lavarunner",
                "Lightning Bolt",
                "Shock",
                "Lightning Strike",
                "Lava Spike",
                "Skewer the Critics",
            ],
        ),
        (
            "reference-green",
            "Référence · Créatures vertes",
            "Forest",
            [
                "Llanowar Elves",
                "Elvish Mystic",
                "Leatherback Baloth",
                "Steel Leaf Champion",
                "Garruk's Companion",
                "Rancor",
                "Giant Growth",
                "Thragtusk",
                "Pelakka Wurm",
            ],
        ),
        (
            "reference-white",
            "Référence · Armée blanche",
            "Plains",
            [
                "Savannah Lions",
                "Elite Vanguard",
                "Raise the Alarm",
                "Pacifism",
                "Glorious Anthem",
                "Serra Angel",
                "White Knight",
                "Suntail Hawk",
                "Banisher Priest",
            ],
        ),
    ]
    .into_iter()
    .map(|(id, name, land, names)| {
        let mut cards = names
            .into_iter()
            .map(|name| DeckCard {
                name: name.into(),
                quantity: 4,
                ..Default::default()
            })
            .collect::<Vec<_>>();
        cards.push(DeckCard {
            name: land.into(),
            quantity: 24,
            ..Default::default()
        });
        Deck {
            id: id.into(),
            name: name.into(),
            format: Some("Entraînement libre, sans garantie de légalité Arena".into()),
            main_deck: cards,
            card_count: 60,
            ..Default::default()
        }
    })
    .collect()
}
async fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temp = path.with_extension("tmp");
    tokio::fs::write(&temp, serde_json::to_vec_pretty(value)?).await?;
    tokio::fs::rename(&temp, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DeckCard;
    fn deck() -> Deck {
        Deck {
            name: "Test".into(),
            main_deck: vec![DeckCard {
                name: "Plains".into(),
                quantity: 60,
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    #[test]
    fn exports_only_exact_main_deck_and_rejects_unsupported_modes() {
        let mut d = deck();
        assert!(forge_deck(&d).unwrap().ends_with("60 Plains\n"));
        d.main_deck[0].name = "Arena card #123".into();
        assert!(forge_deck(&d).is_err());
        d = deck();
        d.format = Some("HistoricBrawl".into());
        assert!(forge_deck(&d).is_err());
    }
    #[test]
    fn uncertainty_is_not_a_power_rating() {
        assert!(wilson(0, 0).is_none());
        let [low, high] = wilson(50, 100).unwrap();
        assert!(low < 0.41 && high > 0.59);
        let [low, high] = wilson(0, 100).unwrap();
        assert!(low < 0.001 && high > 0.03);
    }
    #[tokio::test]
    async fn snapshots_recover_and_events_cannot_invent_wins() {
        let temp = tempfile::tempdir().unwrap();
        let service = SimulationService::open(temp.path().to_owned()).unwrap();
        assert!(service.get("../config").await.is_err());
        let mut r = RunReport {
            id: "123".into(),
            created_at: now(),
            finished_at: None,
            status: "starting".into(),
            deck: deck(),
            opponent: deck(),
            opponents: vec![deck()],
            games_requested: 2,
            games_per_opponent: 2,
            seed: 1,
            seconds_per_game: 30,
            engine: EngineSettings::default(),
            engine_version: None,
            adapter_version: "test".into(),
            results: vec![],
            error: None,
        };
        assert!(apply_event(&mut r, r#"{"kind":"done"}"#).is_err());
        apply_event(&mut r, r#"{"kind":"ready","version":"test"}"#).unwrap();
        let event =
            r#"{"kind":"game","index":1,"outcome":"win","durationMs":123,"turns":5,"seat":1}"#;
        apply_event(&mut r, event).unwrap();
        assert!(apply_event(&mut r, event).is_err());
        assert!(apply_event(&mut r, r#"{"kind":"done"}"#).is_err());
        tokio::fs::create_dir(temp.path().join("123"))
            .await
            .unwrap();
        atomic_json(&temp.path().join("123/report.json"), &r)
            .await
            .unwrap();
        let restarted = SimulationService::open(temp.path().to_owned()).unwrap();
        let saved = restarted.get("123").await.unwrap();
        assert_eq!(saved.status, "interrupted");
        assert_eq!(saved.summary()["wins"], 1);
        assert_eq!(saved.deck.main_deck[0].name, "Plains");
    }

    #[test]
    fn random_generator_honours_mono_and_bicolor_shapes() {
        let mut collection = vec![
            OwnedCard {
                name: "Plains".into(),
                quantity: 60,
                type_line: "Basic Land".into(),
                colors: vec!["W".into()],
                ..Default::default()
            },
            OwnedCard {
                name: "Island".into(),
                quantity: 60,
                type_line: "Basic Land".into(),
                colors: vec!["U".into()],
                ..Default::default()
            },
            OwnedCard {
                name: "Azorius Guildgate".into(),
                quantity: 4,
                type_line: "Land".into(),
                colors: vec!["W".into(), "U".into()],
                ..Default::default()
            },
            OwnedCard {
                name: "Tranquil Cove".into(),
                quantity: 4,
                type_line: "Land".into(),
                colors: vec!["W".into(), "U".into()],
                ..Default::default()
            },
        ];
        for index in 0..6 {
            collection.push(OwnedCard {
                name: format!("Creature {index}"),
                quantity: 4,
                type_line: "Creature — Human".into(),
                colors: vec!["W".into()],
                ..Default::default()
            });
        }
        for index in 0..5 {
            collection.push(OwnedCard {
                name: format!("Spell {index}"),
                quantity: 4,
                type_line: "Instant".into(),
                colors: vec!["W".into()],
                ..Default::default()
            });
        }
        let mono = RandomLabRequest {
            colors: vec!["W".into()],
            bicolor: false,
            land_count: 24,
            creature_count: 20,
            noncreature_count: 16,
            dual_land_count: 8,
            lands_per_color: 7,
            candidate_count: 1,
            opponent_ids: vec![],
            deck_id: None,
            seed: Some(1),
        };
        let deck = generate_random_decks(&collection, &mono).unwrap().remove(0);
        assert_eq!(deck.card_count, 60);
        assert_eq!(
            deck.main_deck
                .iter()
                .filter(|card| card.type_line.contains("Creature"))
                .map(|card| card.quantity)
                .sum::<u32>(),
            20
        );
        let mut bi = mono.clone();
        bi.colors = vec!["W".into(), "U".into()];
        bi.bicolor = true;
        bi.land_count = 22;
        bi.creature_count = 21;
        bi.noncreature_count = 17;
        let deck = generate_random_decks(&collection, &bi).unwrap().remove(0);
        assert_eq!(deck.card_count, 60);
        assert_eq!(
            deck.main_deck
                .iter()
                .filter(|card| card.name == "Azorius Guildgate" || card.name == "Tranquil Cove")
                .map(|card| card.quantity)
                .sum::<u32>(),
            8
        );
    }
}
