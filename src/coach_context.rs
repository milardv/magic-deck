//! Bounded, read-only rules context from Arena's local database.
//! Missing/unknown schemas leave context empty instead of inventing abilities.
use anyhow::Result;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Serialize)]
pub struct CardRules {
    pub mana: String,
    pub text: String,
}

pub fn load(log: &Path, ids: &[u64]) -> Result<BTreeMap<u64, CardRules>> {
    let Some(path) = crate::card_database::find_database(log) else {
        return Ok(BTreeMap::new());
    };
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    read_rules(&db, ids, 12_000)
}

fn read_rules(db: &Connection, ids: &[u64], budget: usize) -> Result<BTreeMap<u64, CardRules>> {
    let mut card_query =
        db.prepare("SELECT OldSchoolManaText, AbilityIds FROM Cards WHERE GrpId=?1")?;
    let mut ability_query = db.prepare("SELECT l.Loc FROM Abilities a JOIN Localizations_enUS l ON l.LocId=a.TextId AND l.Formatted=1 WHERE a.Id=?1 LIMIT 1")?;
    let mut rules = BTreeMap::new();
    let mut used = 0;
    for id in ids {
        if rules.contains_key(id) {
            continue;
        }
        let row = card_query
            .query_row([id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .optional()?;
        let Some((mana, abilities)) = row else {
            continue;
        };
        let mut parts = Vec::new();
        let mut complete = true;
        for entry in abilities.split(',').filter(|part| !part.is_empty()) {
            // Arena stores BaseAbilityId:variantId. Only the first identifies Abilities.Id.
            let Some(ability) = entry.split(':').next().and_then(|v| v.parse::<u64>().ok()) else {
                complete = false;
                break;
            };
            let text = ability_query
                .query_row([ability], |row| row.get::<_, String>(0))
                .optional()?;
            if let Some(text) = text {
                parts.push(crate::card_database::strip_markup(&text));
            } else {
                complete = false;
                break;
            }
        }
        let text = parts.join("\n");
        let size = text.chars().count() + mana.chars().count();
        // Include complete texts only; truncating costs could create false combos.
        if complete && !text.is_empty() && used + size <= budget {
            used += size;
            rules.insert(*id, CardRules { mana, text });
        }
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_base_ids_and_skips_partial_or_over_budget_rules() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE Cards(GrpId INT,OldSchoolManaText TEXT,AbilityIds TEXT);
          CREATE TABLE Abilities(Id INT,TextId INT);
          CREATE TABLE Localizations_enUS(LocId INT,Formatted INT,Loc TEXT);
          INSERT INTO Cards VALUES(1,'oW','10:20'),(2,'oU','10:20,99:1');
          INSERT INTO Abilities VALUES(10,100),(20,200);
          INSERT INTO Localizations_enUS VALUES(100,1,'Flying'),(200,1,'Wrong variant');",
        )
        .unwrap();
        let rules = read_rules(&db, &[1, 1, 2], 100).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[&1].text, "Flying");
        assert!(read_rules(&db, &[1], 2).unwrap().is_empty());
    }
}
