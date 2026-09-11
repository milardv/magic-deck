use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::model::{AnalysisReport, AnalysisReportSummary, Deck, DeckAnalysisResponse, OwnedCard};

#[derive(Clone)]
pub struct AnalysisStore {
    path: PathBuf,
}

impl AnalysisStore {
    pub fn open_default() -> Result<Self> {
        let path = std::env::var_os("MAGIC_DECK_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::data_local_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("magic-deck/reports.sqlite3")
            });
        Self::open(path)
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let store = Self { path };
        store.connection()?.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS deck_analysis_reports (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 deck_id TEXT NOT NULL,
                 deck_name TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 model TEXT NOT NULL,
                 deck_card_count INTEGER NOT NULL,
                 collection_card_count INTEGER NOT NULL,
                 fingerprint TEXT NOT NULL DEFAULT '',
                 deck_snapshot_json TEXT NOT NULL,
                 collection_snapshot_json TEXT NOT NULL,
                 analysis_json TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_deck_analysis_reports_deck_date
                 ON deck_analysis_reports(deck_id, created_at DESC, id DESC);",
        )?;
        let has_fingerprint = store
            .connection()?
            .prepare("PRAGMA table_info(deck_analysis_reports)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|name| name == "fingerprint");
        if !has_fingerprint {
            store.connection()?.execute(
                "ALTER TABLE deck_analysis_reports ADD COLUMN fingerprint TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        store.connection()?.execute(
            "CREATE INDEX IF NOT EXISTS idx_deck_analysis_reports_fingerprint
             ON deck_analysis_reports(deck_id, fingerprint, model)",
            [],
        )?;
        #[cfg(unix)]
        std::fs::set_permissions(&store.path, std::fs::Permissions::from_mode(0o600))?;
        Ok(store)
    }

    pub fn save(
        &self,
        deck: &Deck,
        collection: &[OwnedCard],
        model: &str,
        fingerprint: &str,
        analysis: &DeckAnalysisResponse,
    ) -> Result<AnalysisReport> {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system time is before Unix epoch")?
            .as_secs() as i64;
        let deck_card_count = deck.card_count;
        let collection_card_count = collection.iter().map(|card| card.quantity).sum();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO deck_analysis_reports (
                deck_id, deck_name, created_at, model, deck_card_count,
                collection_card_count, fingerprint, deck_snapshot_json,
                collection_snapshot_json, analysis_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                deck.id,
                deck.name,
                created_at,
                model,
                deck_card_count,
                collection_card_count,
                fingerprint,
                serde_json::to_string(deck)?,
                serde_json::to_string(collection)?,
                serde_json::to_string(analysis)?,
            ],
        )?;
        Ok(AnalysisReport {
            id: connection.last_insert_rowid(),
            deck_id: deck.id.clone(),
            deck_name: deck.name.clone(),
            created_at,
            model: model.into(),
            deck_card_count,
            collection_card_count,
            analysis: analysis.clone(),
            cached: false,
        })
    }

    pub fn find_cached(
        &self,
        deck_id: &str,
        fingerprint: &str,
        model: &str,
    ) -> Result<Option<AnalysisReport>> {
        let connection = self.connection()?;
        let id = connection
            .query_row(
                "SELECT id FROM deck_analysis_reports
                 WHERE deck_id = ?1 AND fingerprint = ?2 AND model = ?3
                 ORDER BY id DESC LIMIT 1",
                params![deck_id, fingerprint, model],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        match id {
            Some(id) => self.get(id).map(|report| {
                report.map(|mut report| {
                    report.cached = true;
                    report
                })
            }),
            None => Ok(None),
        }
    }

    pub fn list_for_deck(&self, deck_id: &str) -> Result<Vec<AnalysisReportSummary>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, deck_id, deck_name, created_at, model,
                    deck_card_count, collection_card_count
             FROM deck_analysis_reports
             WHERE deck_id = ?1
             ORDER BY created_at DESC, id DESC",
        )?;
        let reports = statement
            .query_map([deck_id], |row| {
                Ok(AnalysisReportSummary {
                    id: row.get(0)?,
                    deck_id: row.get(1)?,
                    deck_name: row.get(2)?,
                    created_at: row.get(3)?,
                    model: row.get(4)?,
                    deck_card_count: row.get(5)?,
                    collection_card_count: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(reports)
    }

    pub fn get(&self, id: i64) -> Result<Option<AnalysisReport>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT id, deck_id, deck_name, created_at, model,
                        deck_card_count, collection_card_count, analysis_json
                 FROM deck_analysis_reports WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(
                id,
                deck_id,
                deck_name,
                created_at,
                model,
                deck_card_count,
                collection_card_count,
                analysis_json,
            )| {
                Ok(AnalysisReport {
                    id,
                    deck_id,
                    deck_name,
                    created_at,
                    model,
                    deck_card_count,
                    collection_card_count,
                    analysis: serde_json::from_str(&analysis_json)?,
                    cached: false,
                })
            },
        )
        .transpose()
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)
            .with_context(|| format!("cannot open analysis database {}", self.path.display()))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(connection)
    }

    #[cfg(test)]
    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

pub fn fingerprint(
    deck: &Deck,
    collection: &[OwnedCard],
    model: &str,
) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(&(
        deck,
        collection,
        model,
        crate::ai_coach::COACH_VERSION,
        crate::ai_coach::SYSTEM_INSTRUCTION,
    ))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DeckAnalysisResponse;

    #[test]
    fn stores_and_loads_a_dated_report() {
        let directory = tempfile::tempdir().unwrap();
        let store = AnalysisStore::open(directory.path().join("reports.sqlite3")).unwrap();
        let deck = Deck {
            id: "deck-1".into(),
            name: "Test".into(),
            card_count: 60,
            ..Default::default()
        };
        let analysis = DeckAnalysisResponse {
            combos: vec![],
            play_challenge: String::new(),
            deck_summary: "Summary".into(),
            game_plan: "Plan".into(),
            strengths: vec!["Strong".into()],
            weaknesses: vec![],
            improvement_suggestions: vec![],
        };
        let saved = store
            .save(&deck, &[], "gemini-test", "fingerprint", &analysis)
            .unwrap();

        assert!(store.path().is_file());
        assert_eq!(store.list_for_deck("deck-1").unwrap().len(), 1);
        assert_eq!(
            store.get(saved.id).unwrap().unwrap().analysis.deck_summary,
            "Summary"
        );
        assert!(
            store
                .find_cached("deck-1", "fingerprint", "gemini-test")
                .unwrap()
                .unwrap()
                .cached
        );
    }
}
