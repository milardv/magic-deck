use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::OwnedCard;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedCollection {
    pub collection: Vec<OwnedCard>,
    pub synced_at: Option<String>,
    pub source_modified_at: Option<String>,
}

fn path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("magic-deck/collection.json")
}

pub fn load() -> Result<Option<CachedCollection>> {
    let path = path();
    if !path.is_file() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("cannot read collection cache {}", path.display()))?;
    let cached = serde_json::from_str(&content)
        .with_context(|| format!("cannot parse collection cache {}", path.display()))?;
    Ok(Some(cached))
}

pub fn save(
    collection: &[OwnedCard],
    synced_at: Option<String>,
    source_modified_at: Option<String>,
) -> Result<()> {
    let path = path();
    let parent = path.parent().expect("collection cache has a parent");
    fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    let temporary = path.with_extension("json.tmp");
    let cached = CachedCollection {
        collection: collection.to_vec(),
        synced_at,
        source_modified_at,
    };
    fs::write(&temporary, serde_json::to_vec_pretty(&cached)?)
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&temporary, &path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_serializes_owned_cards() {
        let value = CachedCollection {
            collection: vec![OwnedCard {
                arena_id: 42,
                name: "Test Card".into(),
                quantity: 2,
                ..Default::default()
            }],
            synced_at: Some("2026-01-01T00:00:00Z".into()),
            source_modified_at: None,
        };
        let json = serde_json::to_string(&value).unwrap();
        let decoded: CachedCollection = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.collection[0].quantity, 2);
    }
}
