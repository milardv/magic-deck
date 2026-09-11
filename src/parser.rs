use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value};
use thiserror::Error;

use crate::{
    card_database,
    model::{CardMetadata, Deck, DeckCard, OwnedCard, Snapshot, Wildcards},
};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("cannot open MTGA log {path}: {source}")]
    Open {
        path: String,
        source: std::io::Error,
    },
    #[error("cannot read MTGA log: {0}")]
    Read(#[from] std::io::Error),
}

#[derive(Debug, Copy, Clone, Default)]
struct Context {
    collection: bool,
    decks: bool,
    inventory: bool,
}

pub fn parse_log(path: &Path) -> Result<Snapshot, ParseError> {
    let file = File::open(path).map_err(|source| ParseError::Open {
        path: path.display().to_string(),
        source,
    })?;
    let modified = file.metadata().ok().and_then(|m| m.modified().ok());
    let mut collection = BTreeMap::<u64, u32>::new();
    let mut decks = BTreeMap::<String, Deck>::new();
    let mut metadata = HashMap::<u64, CardMetadata>::new();
    let mut wildcards = Wildcards::default();
    let mut context = Context::default();
    let mut pending: Option<(Context, String)> = None;
    let mut malformed_json = 0usize;

    for line in BufReader::new(file).lines() {
        let line = line?;
        let lower = line.to_ascii_lowercase();
        let is_preconstructed_catalog = lower.contains("precondeck");
        let line_context = Context {
            collection: lower.contains("getplayercards") || lower.contains("cardidtoquantity"),
            decks: !is_preconstructed_catalog
                && (lower.contains("getdeck")
                    || lower.contains("decklist")
                    || lower.contains("upsertdeck")
                    || lower.contains("updatedeck")
                    || lower.contains("coursedeck")
                    || lower.contains("eventsetdeck")),
            inventory: lower.contains("getplayerinventory") || lower.contains("wildcard"),
        };
        let has_marker = line_context.collection || line_context.decks || line_context.inventory;
        if has_marker {
            context = line_context;
            pending = None;
        }

        if let Some((pending_context, buffer)) = pending.as_mut() {
            buffer.push('\n');
            buffer.push_str(&line);
            if let Some(value) = complete_json_value(buffer) {
                visit_value(
                    &value,
                    *pending_context,
                    &mut collection,
                    &mut decks,
                    &mut metadata,
                    &mut wildcards,
                    0,
                );
                pending = None;
                context = Context::default();
            }
            continue;
        }

        let values = json_values_in(&line);
        let has_values = !values.is_empty();
        for value in values {
            visit_value(
                &value,
                context,
                &mut collection,
                &mut decks,
                &mut metadata,
                &mut wildcards,
                0,
            );
        }
        if has_values {
            context = Context::default();
        } else if has_marker {
            if let Some(start) = json_start(&line, true) {
                pending = Some((context, line[start..].to_owned()));
            }
        } else if !has_marker && (context.collection || context.decks || context.inventory) {
            if let Some(start) = json_start(&line, false) {
                pending = Some((context, line[start..].to_owned()));
            }
        } else if looks_like_json_start(&line) {
            malformed_json += 1;
        }
    }

    if pending.is_some() {
        malformed_json += 1;
    }

    if decks.values().any(|deck| deck.source_priority == 3) {
        decks.retain(|_, deck| deck.source_priority == 3);
    }

    let card_ids = referenced_card_ids(&collection, &decks);
    let mut warnings = Vec::new();
    match card_database::load_metadata(path, &card_ids) {
        Ok(Some(local_metadata)) => metadata.extend(local_metadata),
        Ok(None) => warnings.push(
            "MTGA card database not found. Card names are unavailable; set MTGA_CARD_DB_PATH if MTGA is installed in another Steam library."
                .into(),
        ),
        Err(error) => warnings.push(format!("Could not read the MTGA card database: {error}")),
    }
    match card_database::load_deck_names(path, &decks) {
        Ok(Some(names)) => {
            for deck in decks.values_mut() {
                if let Some(name) = names.get(&deck.name) {
                    deck.name.clone_from(name);
                }
            }
        }
        Ok(None) => warnings.push(
            "MTGA localization database not found; some built-in deck names may remain technical."
                .into(),
        ),
        Err(error) => warnings.push(format!("Could not read MTGA deck names: {error}")),
    }

    enrich_decks(&mut decks, &metadata);
    let collection = collection
        .into_iter()
        .map(|(arena_id, quantity)| owned_card(arena_id, quantity, metadata.get(&arena_id)))
        .collect::<Vec<_>>();

    if decks.is_empty() {
        warnings.push(
            "No full deck payload found. Open the Decks screen in MTGA, then sync again.".into(),
        );
    }
    if malformed_json > 0 {
        warnings.push(format!(
            "Ignored {malformed_json} incomplete JSON log line(s)."
        ));
    }

    Ok(Snapshot {
        collection,
        decks: decks.into_values().collect(),
        wildcards,
        synced_at: Some(unix_timestamp(SystemTime::now())),
        source_modified_at: modified.map(unix_timestamp),
        warnings,
    })
}

pub(crate) fn set_collection(
    snapshot: &mut Snapshot,
    path: &Path,
    quantities: BTreeMap<u64, u32>,
) -> anyhow::Result<()> {
    let ids = quantities.keys().copied().collect::<HashSet<_>>();
    let metadata = card_database::load_metadata(path, &ids)?.unwrap_or_default();
    snapshot.collection = quantities
        .into_iter()
        .map(|(arena_id, quantity)| owned_card(arena_id, quantity, metadata.get(&arena_id)))
        .collect();
    Ok(())
}

fn visit_value(
    value: &Value,
    context: Context,
    collection: &mut BTreeMap<u64, u32>,
    decks: &mut BTreeMap<String, Deck>,
    metadata: &mut HashMap<u64, CardMetadata>,
    wildcards: &mut Wildcards,
    depth: usize,
) {
    if depth > 12 {
        return;
    }
    match value {
        Value::Object(object) => {
            collect_metadata(object, metadata);
            collect_wildcards(object, wildcards);
            collect_internal_decks(object, decks, metadata);

            if let Some(map) =
                object_value_ci(object, &["cardIdToQuantity", "cardsOwned", "playerCards"])
                    .and_then(numeric_quantity_map)
            {
                *collection = map;
            } else if context.collection {
                if let Some(map) = numeric_quantity_map(value) {
                    if !map.is_empty() {
                        *collection = map;
                    }
                }
            }

            if context.decks {
                if let Some(deck) = parse_wrapped_deck(object, metadata) {
                    merge_deck(decks, deck);
                }
            }
            if let Some(value) = object_value_ci(object, &["decks", "deckLists", "personalDecks"]) {
                collect_decks(value, decks, metadata);
            } else if context.decks && looks_like_deck(object) {
                if let Some(deck) = parse_deck(object, metadata) {
                    merge_deck(decks, deck);
                }
            }

            for nested in object.values() {
                visit_nested_string_or_value(
                    nested, context, collection, decks, metadata, wildcards, depth,
                );
            }
        }
        Value::Array(values) => {
            if context.decks {
                collect_decks(value, decks, metadata);
            }
            for nested in values {
                visit_nested_string_or_value(
                    nested, context, collection, decks, metadata, wildcards, depth,
                );
            }
        }
        Value::String(text) => {
            if let Ok(nested) = serde_json::from_str::<Value>(text) {
                visit_value(
                    &nested,
                    context,
                    collection,
                    decks,
                    metadata,
                    wildcards,
                    depth + 1,
                );
            }
        }
        _ => {}
    }
}

fn visit_nested_string_or_value(
    value: &Value,
    context: Context,
    collection: &mut BTreeMap<u64, u32>,
    decks: &mut BTreeMap<String, Deck>,
    metadata: &mut HashMap<u64, CardMetadata>,
    wildcards: &mut Wildcards,
    depth: usize,
) {
    visit_value(
        value,
        context,
        collection,
        decks,
        metadata,
        wildcards,
        depth + 1,
    );
}

fn collect_decks(
    value: &Value,
    decks: &mut BTreeMap<String, Deck>,
    metadata: &mut HashMap<u64, CardMetadata>,
) {
    match value {
        Value::Array(items) => {
            for item in items {
                if let Value::Object(object) = item {
                    if let Some(deck) = parse_deck(object, metadata) {
                        merge_deck(decks, deck);
                    }
                }
            }
        }
        Value::Object(object) => {
            if let Some(deck) = parse_deck(object, metadata) {
                merge_deck(decks, deck);
            } else {
                for nested in object.values() {
                    collect_decks(nested, decks, metadata);
                }
            }
        }
        _ => {}
    }
}

fn merge_deck(decks: &mut BTreeMap<String, Deck>, incoming: Deck) {
    match decks.get(&incoming.id) {
        Some(existing) if existing.source_priority > incoming.source_priority => {
            let mut observed = existing.clone();
            merge_observation(&mut observed, &incoming);
            decks.insert(observed.id.clone(), observed);
        }
        _ => {
            decks.insert(incoming.id.clone(), incoming);
        }
    }
}

fn merge_observation(target: &mut Deck, incoming: &Deck) {
    // CurrentWins/CurrentLosses are cumulative counters repeated by several
    // CourseDeck snapshots. Taking the maximum avoids counting the same games
    // multiple times when the log contains Play, Ladder and refresh events.
    target.wins = target.wins.max(incoming.wins);
    target.losses = target.losses.max(incoming.losses);
    target.draws = target.draws.max(incoming.draws);
    for event in &incoming.events {
        if !target.events.contains(event) {
            target.events.push(event.clone());
        }
    }
    if target.last_played.is_none() {
        target.last_played.clone_from(&incoming.last_played);
    }
    if target.last_updated.is_none() {
        target.last_updated.clone_from(&incoming.last_updated);
    }
    target.is_favorite |= incoming.is_favorite;
}

fn parse_deck(
    object: &Map<String, Value>,
    metadata: &mut HashMap<u64, CardMetadata>,
) -> Option<Deck> {
    let id = string_value_ci(object, &["id", "deckId", "DeckId", "deckID"])
        .or_else(|| string_value_ci(object, &["name", "Name"]))?;
    let name = string_value_ci(object, &["name", "Name", "deckName"])
        .unwrap_or_else(|| format!("Deck {id}"));
    let format = string_value_ci(object, &["format", "Format", "formatType", "deckFormat"])
        .or_else(|| attribute_value(object, "Format"));
    let main = object_value_ci(
        object,
        &["mainDeck", "MainDeck", "main", "deckCards", "cards"],
    )
    .map(|value| parse_card_list(value, metadata))
    .unwrap_or_default();
    let sideboard = object_value_ci(object, &["sideboard", "Sideboard", "sideBoard"])
        .map(|value| parse_card_list(value, metadata))
        .unwrap_or_default();
    let command_zone = object_value_ci(object, &["commandZone", "CommandZone", "commanders"])
        .map(|value| parse_card_list(value, metadata))
        .unwrap_or_default();
    if main.is_empty() && sideboard.is_empty() && command_zone.is_empty() {
        return None;
    }
    let card_count = main.iter().map(|card| card.quantity).sum();
    let mut colors = Vec::new();
    for card in main.iter().chain(command_zone.iter()) {
        for color in &card.colors {
            if !colors.contains(color) {
                colors.push(color.clone());
            }
        }
    }
    Some(Deck {
        id,
        name,
        format,
        colors,
        main_deck: main,
        sideboard,
        command_zone,
        card_count,
        is_user_deck: false,
        last_played: None,
        last_updated: None,
        is_favorite: false,
        wins: 0,
        losses: 0,
        draws: 0,
        events: Vec::new(),
        source_priority: 3,
    })
}

fn parse_wrapped_deck(
    object: &Map<String, Value>,
    metadata: &mut HashMap<u64, CardMetadata>,
) -> Option<Deck> {
    let summary =
        object_value_ci(object, &["summary", "deckSummary", "courseDeckSummary"])?.as_object()?;
    let contents = object_value_ci(object, &["deck", "deckContents", "courseDeck"])?.as_object()?;
    let mut combined = contents.clone();
    for (target, candidates) in [
        ("id", &["id", "deckId", "DeckId"][..]),
        ("name", &["name", "deckName"][..]),
        ("format", &["format", "formatType", "deckFormat"][..]),
    ] {
        if object_value_ci(&combined, &[target]).is_none() {
            if let Some(value) = object_value_ci(summary, candidates) {
                combined.insert(target.to_owned(), value.clone());
            }
        }
    }
    let mut deck = parse_deck(&combined, metadata)?;
    if object_value_ci(object, &["courseDeck"]).is_some() {
        deck.source_priority = 1;
        deck.wins = u32_value_ci(object, &["currentWins"]).unwrap_or_default();
        deck.losses = u32_value_ci(object, &["currentLosses"]).unwrap_or_default();
        deck.draws = u32_value_ci(object, &["currentDraws"]).unwrap_or_default();
        if let Some(event) = string_value_ci(object, &["internalEventName"]) {
            deck.events.push(event);
        }
    }
    Some(deck)
}

fn collect_internal_decks(
    object: &Map<String, Value>,
    decks: &mut BTreeMap<String, Deck>,
    metadata: &mut HashMap<u64, CardMetadata>,
) {
    let Some(summaries) = object_value_ci(object, &["deckSummaries"]).and_then(Value::as_array)
    else {
        return;
    };
    let Some(contents) = object_value_ci(object, &["decksInternal"]).and_then(Value::as_object)
    else {
        return;
    };

    for summary in summaries.iter().filter_map(Value::as_object) {
        if object_value_ci(summary, &["isNetDeck"]).and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(id) = string_value_ci(summary, &["deckIdInternal", "deckId", "id"]) else {
            continue;
        };
        let Some(deck_contents) = contents.get(&id).and_then(Value::as_object) else {
            continue;
        };
        let mut combined = deck_contents.clone();
        combined.insert("id".into(), Value::String(id));
        if let Some(name) = object_value_ci(summary, &["name"]) {
            combined.insert("name".into(), name.clone());
        }
        if let Some(attributes) = object_value_ci(summary, &["attributes"]) {
            combined.insert("attributes".into(), attributes.clone());
        }
        if let Some(mut deck) = parse_deck(&combined, metadata) {
            deck.source_priority = 3;
            deck.is_user_deck = summary_is_user_deck(summary);
            deck.last_played = attribute_value(summary, "LastPlayed");
            deck.last_updated = attribute_value(summary, "LastUpdated");
            deck.is_favorite = attribute_value(summary, "IsFavorite")
                .is_some_and(|value| value.eq_ignore_ascii_case("true"));
            merge_deck(decks, deck);
        }
    }
}

fn summary_is_user_deck(summary: &Map<String, Value>) -> bool {
    // Arena marks built-in/preconstructed decks with a zero LastUpdated value.
    // Player-created decks receive a real timestamp when saved or edited.
    if let Some(last_updated) = attribute_value(summary, "LastUpdated") {
        let normalized = last_updated.trim_matches('"');
        if !normalized.is_empty() && !normalized.starts_with("0001-01-01") {
            return true;
        }
    }

    // Custom decks generally have no localized preconstructed description and
    // are not stored under the technical Loc/Decks/Precon namespace.
    let name = string_value_ci(summary, &["name", "deckName"]).unwrap_or_default();
    let description = string_value_ci(summary, &["description", "Description"]).unwrap_or_default();
    description.is_empty() && !name.contains("Loc/Decks/Precon")
}

fn parse_card_list(value: &Value, metadata: &mut HashMap<u64, CardMetadata>) -> Vec<DeckCard> {
    let mut quantities = Vec::<(u64, u32)>::new();
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if let (Ok(id), Some(quantity)) = (key.parse::<u64>(), as_u32(value)) {
                    add_card_quantity(&mut quantities, id, quantity);
                }
            }
        }
        Value::Array(items) => {
            let flat_numbers = items.iter().all(Value::is_number);
            if flat_numbers && items.len() % 2 == 0 {
                for pair in items.chunks(2) {
                    if let (Some(id), Some(quantity)) = (pair[0].as_u64(), as_u32(&pair[1])) {
                        add_card_quantity(&mut quantities, id, quantity);
                    }
                }
            } else {
                for item in items {
                    match item {
                        Value::Object(object) => {
                            collect_metadata(object, metadata);
                            if let Some(id) =
                                u64_value_ci(object, &["cardId", "grpId", "id", "arenaId"])
                            {
                                let quantity =
                                    object_value_ci(object, &["quantity", "qty", "count"])
                                        .and_then(as_u32)
                                        .unwrap_or(1);
                                add_card_quantity(&mut quantities, id, quantity);
                            }
                        }
                        Value::Array(pair) if pair.len() >= 2 => {
                            if let (Some(id), Some(quantity)) = (pair[0].as_u64(), as_u32(&pair[1]))
                            {
                                add_card_quantity(&mut quantities, id, quantity);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    quantities
        .into_iter()
        .map(|(id, quantity)| deck_card(id, quantity, metadata.get(&id)))
        .collect()
}

fn add_card_quantity(cards: &mut Vec<(u64, u32)>, id: u64, quantity: u32) {
    if let Some((_, existing_quantity)) =
        cards.iter_mut().find(|(existing_id, _)| *existing_id == id)
    {
        *existing_quantity += quantity;
    } else {
        cards.push((id, quantity));
    }
}

fn collect_metadata(object: &Map<String, Value>, metadata: &mut HashMap<u64, CardMetadata>) {
    let Some(id) = u64_value_ci(object, &["cardId", "grpId", "arenaId"]) else {
        return;
    };
    let entry = metadata.entry(id).or_default();
    update_if_some(
        &mut entry.name,
        string_value_ci(object, &["name", "cardName", "title"]),
    );
    update_if_some(
        &mut entry.type_line,
        string_value_ci(object, &["typeLine", "type", "cardType"]),
    );
    update_if_some(
        &mut entry.set_code,
        string_value_ci(object, &["set", "setCode"]),
    );
    update_if_some(
        &mut entry.collector_number,
        string_value_ci(object, &["collectorNumber", "number"]),
    );
    if let Some(value) = object_value_ci(object, &["colors", "colorIdentity", "color"]) {
        let colors = parse_colors(value);
        if !colors.is_empty() {
            entry.colors = colors;
        }
    }
}

fn collect_wildcards(object: &Map<String, Value>, wildcards: &mut Wildcards) {
    for (key, value) in object {
        let normalized = normalize_key(key);
        let Some(quantity) = as_u32(value) else {
            continue;
        };
        if normalized.contains("wildcard") || normalized.contains("wc") {
            if normalized.contains("uncommon") {
                wildcards.uncommon = quantity;
            } else if normalized.contains("mythic") {
                wildcards.mythic = quantity;
            } else if normalized.contains("rare") {
                wildcards.rare = quantity;
            } else if normalized.contains("common") {
                wildcards.common = quantity;
            }
        }
    }
}

fn numeric_quantity_map(value: &Value) -> Option<BTreeMap<u64, u32>> {
    let Value::Object(object) = value else {
        return None;
    };
    if object.is_empty() {
        return None;
    }
    let mut result = BTreeMap::new();
    for (key, value) in object {
        let id = key.parse::<u64>().ok()?;
        let quantity = as_u32(value)?;
        result.insert(id, quantity);
    }
    Some(result)
}

fn enrich_decks(decks: &mut BTreeMap<String, Deck>, metadata: &HashMap<u64, CardMetadata>) {
    for deck in decks.values_mut() {
        for card in deck
            .main_deck
            .iter_mut()
            .chain(deck.sideboard.iter_mut())
            .chain(deck.command_zone.iter_mut())
        {
            let replacement = deck_card(card.arena_id, card.quantity, metadata.get(&card.arena_id));
            if !replacement.name.starts_with("Arena card #")
                || card.name.starts_with("Arena card #")
            {
                *card = replacement;
            }
        }
        deck.colors.clear();
        for card in deck.main_deck.iter().chain(deck.command_zone.iter()) {
            for color in &card.colors {
                if !deck.colors.contains(color) {
                    deck.colors.push(color.clone());
                }
            }
        }
    }
}

fn referenced_card_ids(
    collection: &BTreeMap<u64, u32>,
    decks: &BTreeMap<String, Deck>,
) -> HashSet<u64> {
    let mut ids = collection.keys().copied().collect::<HashSet<_>>();
    for deck in decks.values() {
        ids.extend(
            deck.main_deck
                .iter()
                .chain(deck.sideboard.iter())
                .chain(deck.command_zone.iter())
                .map(|card| card.arena_id),
        );
    }
    ids
}

fn owned_card(id: u64, quantity: u32, metadata: Option<&CardMetadata>) -> OwnedCard {
    let metadata = metadata.cloned().unwrap_or_default();
    OwnedCard {
        arena_id: id,
        name: metadata.name.unwrap_or_else(|| format!("Arena card #{id}")),
        quantity,
        type_line: metadata.type_line.unwrap_or_else(|| "Unknown type".into()),
        colors: metadata.colors,
        set_code: metadata.set_code,
        collector_number: metadata.collector_number,
        rarity: metadata.rarity,
    }
}

fn deck_card(id: u64, quantity: u32, metadata: Option<&CardMetadata>) -> DeckCard {
    let card = owned_card(id, quantity, metadata);
    DeckCard {
        arena_id: card.arena_id,
        name: card.name,
        quantity: card.quantity,
        type_line: card.type_line,
        colors: card.colors,
        set_code: card.set_code,
        collector_number: card.collector_number,
    }
}

fn parse_colors(value: &Value) -> Vec<String> {
    match value {
        Value::Array(values) => values.iter().filter_map(value_to_string).collect(),
        Value::String(value) => value
            .chars()
            .filter(|character| "WUBRG".contains(*character))
            .map(|character| character.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

fn looks_like_deck(object: &Map<String, Value>) -> bool {
    object.keys().any(|key| {
        matches!(
            normalize_key(key).as_str(),
            "maindeck" | "deckcards" | "sideboard" | "commandzone"
        )
    })
}

fn object_value_ci<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    object.iter().find_map(|(key, value)| {
        keys.iter()
            .any(|expected| normalize_key(key) == normalize_key(expected))
            .then_some(value)
    })
}

fn string_value_ci(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    object_value_ci(object, keys).and_then(value_to_string)
}

fn attribute_value(object: &Map<String, Value>, expected_name: &str) -> Option<String> {
    object_value_ci(object, &["attributes"])?
        .as_array()?
        .iter()
        .filter_map(Value::as_object)
        .find(|attribute| {
            string_value_ci(attribute, &["name"])
                .is_some_and(|name| name.eq_ignore_ascii_case(expected_name))
        })
        .and_then(|attribute| string_value_ci(attribute, &["value"]))
}

fn u64_value_ci(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    object_value_ci(object, keys)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
}

fn u32_value_ci(object: &Map<String, Value>, keys: &[&str]) -> Option<u32> {
    u64_value_ci(object, keys).and_then(|value| u32::try_from(value).ok())
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn as_u32(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn update_if_some(target: &mut Option<String>, value: Option<String>) {
    if value.as_ref().is_some_and(|value| !value.is_empty()) {
        *target = value;
    }
}

fn json_values_in(line: &str) -> Vec<Value> {
    let bytes = line.as_bytes();
    let mut values = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        while start < bytes.len() && bytes[start] != b'{' && bytes[start] != b'[' {
            start += 1;
        }
        if start == bytes.len() {
            break;
        }
        let opening = bytes[start];
        let closing = if opening == b'{' { b'}' } else { b']' };
        let mut stack = vec![closing];
        let mut quoted = false;
        let mut escaped = false;
        let mut end = start + 1;
        while end < bytes.len() && !stack.is_empty() {
            let byte = bytes[end];
            if quoted {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    quoted = false;
                }
            } else {
                match byte {
                    b'"' => quoted = true,
                    b'{' => stack.push(b'}'),
                    b'[' => stack.push(b']'),
                    byte if stack.last() == Some(&byte) => {
                        stack.pop();
                    }
                    _ => {}
                }
            }
            end += 1;
        }
        if stack.is_empty() {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes[start..end]) {
                values.push(value);
            }
            start = end;
        } else {
            start += 1;
        }
    }
    values
}

fn complete_json_value(input: &str) -> Option<Value> {
    let trimmed = input.trim_start();
    let bytes = trimmed.as_bytes();
    let closing = match bytes.first()? {
        b'{' => b'}',
        b'[' => b']',
        _ => return None,
    };
    let mut stack = vec![closing];
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate().skip(1) {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            byte if stack.last() == Some(&byte) => {
                stack.pop();
                if stack.is_empty() {
                    return serde_json::from_slice(&bytes[..=index]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

fn json_start(line: &str, marker_line: bool) -> Option<usize> {
    if let Some(position) = line.find('{') {
        return Some(position);
    }
    if marker_line {
        for separator in ["==>", "<=="] {
            if let Some(separator_position) = line.find(separator) {
                let suffix = &line[separator_position + separator.len()..];
                if let Some(relative) = suffix.find('[') {
                    return Some(separator_position + separator.len() + relative);
                }
            }
        }
        None
    } else {
        let trimmed = line.trim_start();
        trimmed
            .starts_with('[')
            .then_some(line.len() - trimmed.len())
    }
}

fn looks_like_json_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with("[{") || trimmed.starts_with("[\"")
}

fn unix_timestamp(time: SystemTime) -> String {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn parses_prefixed_collection_inventory_and_decks() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            "[Unity] Inventory.GetPlayerCardsV3 ==> {{\"100\":4,\"200\":2}}"
        )
        .unwrap();
        writeln!(file, "[Unity] Inventory.GetPlayerInventory ==> {{\"wcCommon\":12,\"wcUncommon\":8,\"wcRare\":4,\"wcMythic\":1}}").unwrap();
        writeln!(file, "[Unity] Deck.GetDeckListsV3 ==> {{\"decks\":[{{\"id\":\"deck-1\",\"name\":\"Azorius\",\"format\":\"Standard\",\"MainDeck\":[{{\"cardId\":100,\"quantity\":4,\"name\":\"Helpful Knight\",\"typeLine\":\"Creature — Knight\",\"colors\":[\"W\"]}},{{\"cardId\":200,\"quantity\":20,\"name\":\"Island\",\"typeLine\":\"Basic Land\",\"colors\":[\"U\"]}}],\"Sideboard\":{{\"300\":2}}}}]}}").unwrap();

        let snapshot = parse_log(file.path()).unwrap();

        assert_eq!(snapshot.collection.len(), 2);
        assert_eq!(snapshot.wildcards.rare, 4);
        assert_eq!(snapshot.decks.len(), 1);
        assert_eq!(snapshot.decks[0].card_count, 24);
        assert_eq!(snapshot.decks[0].main_deck[0].name, "Helpful Knight");
        assert_eq!(snapshot.decks[0].colors, ["W", "U"]);
    }

    #[test]
    fn parses_embedded_json_and_flat_card_pairs() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(
            br#"GetPlayerCards {"payload":"{\"cardIdToQuantity\":{\"42\":3}}"}
GetDeck {"id":"x","name":"Pair deck","mainDeck":[42,3,99,1]}
"#,
        )
        .unwrap();

        let snapshot = parse_log(file.path()).unwrap();

        assert_eq!(snapshot.collection[0].quantity, 3);
        assert_eq!(snapshot.decks[0].main_deck.len(), 2);
    }

    #[test]
    fn parses_multiline_collection_and_wrapped_deck() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "<== PlayerInventory.GetPlayerCardsV3").unwrap();
        writeln!(file, "{{").unwrap();
        writeln!(file, "  \"300\": 4").unwrap();
        writeln!(file, "}}").unwrap();
        file.write_all(
            br#"Deck.UpsertDeckV2 {"request":"{\"Summary\":{\"DeckId\":\"wrapped-1\",\"Name\":\"Wrapped\"},\"Deck\":{\"MainDeck\":[300,4]}}"}
"#,
        )
        .unwrap();

        let snapshot = parse_log(file.path()).unwrap();

        assert_eq!(snapshot.collection[0].arena_id, 300);
        assert_eq!(snapshot.decks[0].id, "wrapped-1");
        assert_eq!(snapshot.decks[0].card_count, 4);
    }

    #[test]
    fn current_internal_deck_keeps_order_and_wins_over_old_course_deck() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(
            br#"{"DeckSummaries":[{"DeckIdInternal":"deck-1","Name":"thalys","Attributes":[{"name":"Format","value":"Standard"}]}],"DecksInternal":{"deck-1":{"MainDeck":[{"cardId":30,"quantity":1},{"cardId":10,"quantity":2}]}}}
{"CourseDeckSummary":{"DeckId":"deck-1","Name":"thalys"},"CourseDeck":{"MainDeck":[{"cardId":99,"quantity":60}]}}
"#,
        )
        .unwrap();

        let snapshot = parse_log(file.path()).unwrap();
        let deck = &snapshot.decks[0];

        assert_eq!(deck.format.as_deref(), Some("Standard"));
        assert!(deck.is_user_deck);
        assert_eq!(deck.card_count, 3);
        assert_eq!(deck.main_deck[0].arena_id, 30);
        assert_eq!(deck.main_deck[1].arena_id, 10);
    }

    #[test]
    fn latest_collection_snapshot_wins() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "GetPlayerCards {{\"1\":1}}").unwrap();
        writeln!(file, "GetPlayerCards {{\"2\":4}}").unwrap();

        let snapshot = parse_log(file.path()).unwrap();

        assert_eq!(snapshot.collection.len(), 1);
        assert_eq!(snapshot.collection[0].arena_id, 2);
    }
}
