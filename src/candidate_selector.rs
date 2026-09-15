use std::collections::{HashMap, HashSet};

use crate::model::{Deck, OwnedCard};

/// Select a compact, relevant slice of the collection before sending an AI
/// request. The complete collection remains available to server validation.
pub fn select(deck: &Deck, collection: &[OwnedCard], limit: usize) -> Vec<OwnedCard> {
    let format = deck.format.as_deref().unwrap_or_default();
    let deck_colors: HashSet<&str> = deck.colors.iter().map(String::as_str).collect();
    let mut deck_types = HashMap::<&str, u32>::new();
    for card in deck
        .main_deck
        .iter()
        .chain(deck.sideboard.iter())
        .chain(deck.command_zone.iter())
    {
        let kind = card.type_line.split('—').next().unwrap_or_default().trim();
        *deck_types.entry(kind).or_default() += card.quantity;
    }
    let deck_names: HashSet<String> = deck
        .main_deck
        .iter()
        .chain(deck.sideboard.iter())
        .chain(deck.command_zone.iter())
        .map(|card| card.name.to_lowercase())
        .collect();

    let mut ranked = collection
        .iter()
        .filter(|card| legal_for_format(card, format))
        .filter(|card| {
            card.colors
                .iter()
                .all(|color| deck_colors.contains(color.as_str()))
        })
        .cloned()
        .map(|card| {
            let type_score = deck_types
                .iter()
                .filter(|(kind, _)| !kind.is_empty() && card.type_line.starts_with(**kind))
                .map(|(_, count)| 2 + (*count).min(4))
                .max()
                .unwrap_or(0);
            let color_score = if card.colors.is_empty() {
                if card.type_line.to_ascii_lowercase().contains("land") {
                    3
                } else {
                    0
                }
            } else {
                card.colors
                    .iter()
                    .filter(|color| deck_colors.contains(color.as_str()))
                    .count() as u32
                    * 3
            };
            let existing_score = deck_names.contains(&card.name.to_lowercase()) as u32;
            (type_score + color_score + existing_score, card)
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.arena_id.cmp(&right.arena_id))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, card)| card)
        .collect()
}

pub(crate) fn legal_for_format(card: &OwnedCard, format: &str) -> bool {
    let normalized = format
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if normalized.contains("standard") || normalized.contains("alchemy") {
        return card.set_code.as_deref().is_some_and(standard_set_is_legal);
    }
    true
}

pub(crate) fn standard_set_is_legal(set_code: &str) -> bool {
    let allowed = std::env::var("MAGIC_DECK_STANDARD_SETS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(|set| set.trim().to_ascii_uppercase())
                .filter(|set| !set.is_empty())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_else(|| {
            // Keep this list explicit and overridable: MTGA's SQLite
            // database has no format-legality table.
            [
                "WOE", "LCI", "MKM", "OTJ", "BIG", "BLB", "DSK", "FDN", "DFT", "TDM", "FIN", "EOE",
                "SPM", "OM1", "TLA", "ECL", "TMT", "SOS", "MSH", "HOB",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        });
    allowed.contains(&set_code.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Deck, DeckCard};

    #[test]
    fn prioritizes_cards_matching_deck_colors_and_types() {
        let deck = Deck {
            colors: vec!["W".into()],
            main_deck: vec![DeckCard {
                type_line: "Creature — Human".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let cards = vec![
            OwnedCard {
                name: "Blue Spell".into(),
                colors: vec!["U".into()],
                type_line: "Instant".into(),
                ..Default::default()
            },
            OwnedCard {
                name: "White Creature".into(),
                colors: vec!["W".into()],
                type_line: "Creature — Human".into(),
                ..Default::default()
            },
        ];
        assert_eq!(select(&deck, &cards, 1)[0].name, "White Creature");
    }

    #[test]
    fn filters_standard_cards_by_set() {
        let deck = Deck {
            format: Some("Standard".into()),
            ..Default::default()
        };
        let cards = vec![
            OwnedCard {
                name: "Old Card".into(),
                set_code: Some("M20".into()),
                ..Default::default()
            },
            OwnedCard {
                name: "Current Card".into(),
                set_code: Some("FDN".into()),
                ..Default::default()
            },
        ];
        assert_eq!(
            select(&deck, &cards, 10)
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["Current Card"]
        );
        assert!(standard_set_is_legal("DFT"));
        assert!(standard_set_is_legal("TDM"));
        assert!(standard_set_is_legal("SPM"));
        assert!(standard_set_is_legal("OM1"));
        assert!(standard_set_is_legal("SOS"));
        assert!(!standard_set_is_legal("SOA"));
    }
}
