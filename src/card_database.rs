use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Context, Result};
use regex::Regex;
use rusqlite::{params_from_iter, Connection, OpenFlags};

use crate::model::{CardMetadata, Deck};

const DATABASE_PREFIX: &str = "Raw_CardDatabase_";
const LOCALIZATION_PREFIX: &str = "Raw_ClientLocalization_";
const DATABASE_SUFFIX: &str = ".mtga";
const LOCALES: [&str; 8] = [
    "enUS", "frFR", "deDE", "esES", "itIT", "ptBR", "jaJP", "koKR",
];

pub fn load_metadata(
    log_path: &Path,
    ids: &HashSet<u64>,
) -> Result<Option<HashMap<u64, CardMetadata>>> {
    let Some(database_path) = find_database(log_path) else {
        return Ok(None);
    };
    let connection = Connection::open_with_flags(
        &database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("cannot open card database {}", database_path.display()))?;
    let has_rarity = connection
        .prepare("PRAGMA table_info(Cards)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name.eq_ignore_ascii_case("Rarity"));
    let mut metadata = HashMap::with_capacity(ids.len());

    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let rarity_column = if has_rarity { "c.Rarity" } else { "NULL" };
        let sql = format!(
            "SELECT c.GrpId, title.Loc, card_type.Loc, subtype.Loc, \
             c.ExpansionCode, c.CollectorNumber, c.Colors, {rarity_column} \
             FROM Cards c \
             LEFT JOIN Localizations_enUS title \
               ON title.LocId = c.TitleId AND title.Formatted = 1 \
             LEFT JOIN Localizations_enUS card_type \
               ON card_type.LocId = c.TypeTextId AND card_type.Formatted = 1 \
             LEFT JOIN Localizations_enUS subtype \
               ON subtype.LocId = c.SubtypeTextId AND subtype.Formatted = 1 \
             WHERE c.GrpId IN ({placeholders})"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(chunk.iter()), |row| {
            let card_type: Option<String> = row.get(2)?;
            let subtype: Option<String> = row.get(3)?;
            Ok((
                row.get::<_, u64>(0)?,
                CardMetadata {
                    name: row
                        .get::<_, Option<String>>(1)?
                        .map(|name| strip_markup(&name)),
                    type_line: type_line(card_type, subtype),
                    colors: row
                        .get::<_, Option<String>>(6)?
                        .as_deref()
                        .map(parse_colors)
                        .unwrap_or_default(),
                    set_code: row.get(4)?,
                    collector_number: row.get(5)?,
                    rarity: row
                        .get::<_, Option<i32>>(7)
                        .ok()
                        .flatten()
                        .and_then(rarity_name),
                },
            ))
        })?;
        for row in rows {
            let (id, card) = row?;
            metadata.insert(id, card);
        }
    }
    Ok(Some(metadata))
}

fn rarity_name(value: i32) -> Option<String> {
    Some(
        match value {
            1 => "Basic Land",
            2 => "Common",
            3 => "Uncommon",
            4 => "Rare",
            5 => "Mythic Rare",
            _ => "Special",
        }
        .into(),
    )
}

pub fn load_known_card_ids(log_path: &Path) -> Result<Option<HashSet<u32>>> {
    let Some(database_path) = find_database(log_path) else {
        return Ok(None);
    };
    let connection = Connection::open_with_flags(
        &database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("cannot open card database {}", database_path.display()))?;
    let mut statement = connection.prepare("SELECT GrpId FROM Cards")?;
    let ids = statement
        .query_map([], |row| row.get::<_, u32>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    Ok(Some(ids))
}

pub fn load_printing_sets(
    log_path: &Path,
    names: &HashSet<String>,
) -> Result<Option<HashMap<String, HashSet<String>>>> {
    let Some(database_path) = find_database(log_path) else {
        return Ok(None);
    };
    let connection = Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut result = HashMap::<String, HashSet<String>>::new();
    let values = names
        .iter()
        .map(|name| name.to_lowercase())
        .collect::<Vec<_>>();
    for chunk in values.chunks(400) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT lower(l.Loc), c.ExpansionCode FROM Cards c JOIN Localizations_enUS l ON l.LocId = c.TitleId AND l.Formatted = 1 WHERE lower(l.Loc) IN ({placeholders})");
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(chunk.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (name, set) = row?;
            result
                .entry(name)
                .or_default()
                .insert(set.to_ascii_uppercase());
        }
    }
    Ok(Some(result))
}

pub fn load_deck_names(
    log_path: &Path,
    decks: &BTreeMap<String, Deck>,
) -> Result<Option<HashMap<String, String>>> {
    let technical_names = decks
        .values()
        .filter_map(|deck| localization_key(&deck.name).map(|key| (deck.name.clone(), key)))
        .collect::<Vec<_>>();
    if technical_names.is_empty() {
        return Ok(Some(HashMap::new()));
    }
    let Some(database_path) = find_localization_database(log_path) else {
        return Ok(None);
    };
    let connection = Connection::open_with_flags(
        &database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| {
        format!(
            "cannot open localization database {}",
            database_path.display()
        )
    })?;
    let locale = preferred_locale(&connection, decks)?;
    let placeholders = std::iter::repeat_n("?", technical_names.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT Key, COALESCE(NULLIF({locale}, ''), enUS) FROM Loc WHERE Key IN ({placeholders})"
    );
    let mut statement = connection.prepare(&sql)?;
    let translated = statement
        .query_map(
            params_from_iter(technical_names.iter().map(|(_, key)| key)),
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;

    Ok(Some(
        technical_names
            .into_iter()
            .filter_map(|(raw_name, key)| {
                translated
                    .get(&key)
                    .map(|name| (raw_name, strip_markup(name).trim().to_owned()))
            })
            .collect(),
    ))
}

pub(crate) fn find_database(log_path: &Path) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MTGA_CARD_DB_PATH").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }

    find_download_database(log_path, DATABASE_PREFIX)
}

fn find_localization_database(log_path: &Path) -> Option<PathBuf> {
    find_download_database(log_path, LOCALIZATION_PREFIX)
}

fn find_download_database(log_path: &Path, prefix: &str) -> Option<PathBuf> {
    let mut raw_directories = Vec::new();
    for ancestor in log_path.ancestors() {
        if ancestor.file_name().is_some_and(|name| name == "steamapps") {
            raw_directories.push(ancestor.join("common/MTGA/MTGA_Data/Downloads/Raw"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        raw_directories
            .push(home.join(".local/share/Steam/steamapps/common/MTGA/MTGA_Data/Downloads/Raw"));
        raw_directories
            .push(home.join(".steam/steam/steamapps/common/MTGA/MTGA_Data/Downloads/Raw"));
    }
    raw_directories
        .into_iter()
        .find_map(|directory| newest_database(directory, prefix))
}

fn newest_database(directory: PathBuf, prefix: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(directory).ok()?;
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            if path.is_file() && name.starts_with(prefix) && name.ends_with(DATABASE_SUFFIX) {
                let modified = entry.metadata().ok()?.modified().ok()?;
                Some((modified, path))
            } else {
                None
            }
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn localization_key(name: &str) -> Option<String> {
    name.strip_prefix("?=?Loc/")
        .or_else(|| name.strip_prefix("Loc/"))
        .map(str::to_owned)
}

fn preferred_locale(
    connection: &Connection,
    decks: &BTreeMap<String, Deck>,
) -> Result<&'static str> {
    if let Some(locale) = std::env::var("MAGIC_DECK_LOCALE")
        .ok()
        .and_then(|value| supported_locale(&value))
    {
        return Ok(locale);
    }

    let known_names = decks
        .values()
        .filter(|deck| localization_key(&deck.name).is_none())
        .map(|deck| deck.name.as_str())
        .take(100)
        .collect::<HashSet<_>>();
    if !known_names.is_empty() {
        let placeholders = std::iter::repeat_n("?", known_names.len())
            .collect::<Vec<_>>()
            .join(",");
        let predicates = LOCALES
            .iter()
            .map(|locale| format!("{locale} IN ({placeholders})"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let sql = format!("SELECT {} FROM Loc WHERE {predicates}", LOCALES.join(","));
        let parameters = LOCALES
            .iter()
            .flat_map(|_| known_names.iter().copied())
            .collect::<Vec<_>>();
        let mut scores = [0usize; LOCALES.len()];
        let mut statement = connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(parameters))?;
        while let Some(row) = rows.next()? {
            for (index, score) in scores.iter_mut().enumerate() {
                let value = row.get::<_, Option<String>>(index)?;
                if value
                    .as_deref()
                    .is_some_and(|name| known_names.contains(name))
                {
                    *score += 1;
                }
            }
        }
        if let Some((index, score)) = scores
            .iter()
            .copied()
            .enumerate()
            .max_by_key(|(_, score)| *score)
        {
            if score > 0 {
                return Ok(LOCALES[index]);
            }
        }
    }

    let environment_locale = std::env::var("LC_ALL")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_default();
    Ok(supported_locale(&environment_locale).unwrap_or("enUS"))
}

fn supported_locale(value: &str) -> Option<&'static str> {
    let normalized = value
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_lowercase();
    LOCALES
        .iter()
        .copied()
        .find(|locale| normalized.starts_with(&locale.to_ascii_lowercase()[..2]))
}

fn type_line(card_type: Option<String>, subtype: Option<String>) -> Option<String> {
    let card_type = card_type.filter(|value| !value.is_empty())?;
    match subtype.filter(|value| !value.is_empty()) {
        Some(subtype) => Some(format!(
            "{} — {}",
            strip_markup(&card_type),
            strip_markup(&subtype)
        )),
        None => Some(strip_markup(&card_type)),
    }
}

fn parse_colors(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter_map(|value| match value.trim() {
            "1" => Some("W"),
            "2" => Some("U"),
            "3" => Some("B"),
            "4" => Some("R"),
            "5" => Some("G"),
            _ => None,
        })
        .map(str::to_owned)
        .collect()
}

pub(crate) fn strip_markup(value: &str) -> String {
    static TAGS: OnceLock<Regex> = OnceLock::new();
    TAGS.get_or_init(|| Regex::new("<[^>]+>").expect("valid static regex"))
        .replace_all(value, "")
        .into_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn loads_card_metadata_and_localized_deck_names() {
        let root = TempDir::new().unwrap();
        let raw = root
            .path()
            .join("steamapps/common/MTGA/MTGA_Data/Downloads/Raw");
        let log = root
            .path()
            .join("steamapps/compatdata/2141910/pfx/Player.log");
        fs::create_dir_all(&raw).unwrap();
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        let database = raw.join("Raw_CardDatabase_test.mtga");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE Cards(GrpId INT, TitleId INT, TypeTextId INT, SubtypeTextId INT, ExpansionCode TEXT, CollectorNumber TEXT, Colors TEXT); \
                 CREATE TABLE Localizations_enUS(LocId INT, Formatted INT, Loc TEXT); \
                 INSERT INTO Cards VALUES(42, 1, 2, 3, 'TST', '7', '1,2'); \
                 INSERT INTO Localizations_enUS VALUES(1, 1, '<nobr>Test Card</nobr>'); \
                 INSERT INTO Localizations_enUS VALUES(2, 1, 'Creature'); \
                 INSERT INTO Localizations_enUS VALUES(3, 1, 'Wizard');",
            )
            .unwrap();
        drop(connection);

        let cards = load_metadata(&log, &HashSet::from([42])).unwrap().unwrap();
        let card = &cards[&42];
        assert_eq!(card.name.as_deref(), Some("Test Card"));
        assert_eq!(card.type_line.as_deref(), Some("Creature — Wizard"));
        assert_eq!(card.colors, ["W", "U"]);
        assert_eq!(card.set_code.as_deref(), Some("TST"));

        let localization_database = raw.join("Raw_ClientLocalization_test.mtga");
        let localization = Connection::open(localization_database).unwrap();
        localization
            .execute_batch(
                "CREATE TABLE Loc(Key TEXT, enUS TEXT, frFR TEXT, deDE TEXT, esES TEXT, itIT TEXT, ptBR TEXT, jaJP TEXT, koKR TEXT); \
                 INSERT INTO Loc VALUES('Decks/Precon/Test', 'Test Deck', 'Deck de test', NULL, NULL, NULL, NULL, NULL, NULL); \
                 INSERT INTO Loc VALUES('Decks/Precon/Known', 'Keep the Peace', 'Gardien de la paix', NULL, NULL, NULL, NULL, NULL, NULL);",
            )
            .unwrap();
        drop(localization);
        let decks = BTreeMap::from([
            (
                "technical".into(),
                Deck {
                    name: "?=?Loc/Decks/Precon/Test".into(),
                    ..Default::default()
                },
            ),
            (
                "known".into(),
                Deck {
                    name: "Gardien de la paix".into(),
                    ..Default::default()
                },
            ),
        ]);

        let names = load_deck_names(&log, &decks).unwrap().unwrap();
        assert_eq!(
            names.get("?=?Loc/Decks/Precon/Test").map(String::as_str),
            Some("Deck de test")
        );
    }
}
