use crate::model::{Deck, DeckCard, OwnedCard};

pub fn deck_arena(deck: &Deck) -> String {
    let mut output = String::from("Deck\n");
    append_arena_cards(&mut output, &deck.main_deck);
    if !deck.command_zone.is_empty() {
        output.push_str("\nCommander\n");
        append_arena_cards(&mut output, &deck.command_zone);
    }
    if !deck.sideboard.is_empty() {
        output.push_str("\nSideboard\n");
        append_arena_cards(&mut output, &deck.sideboard);
    }
    output
}

pub fn deck_csv(deck: &Deck) -> String {
    let mut output =
        String::from("section,quantity,arena_id,name,type,colors,set,collector_number\n");
    append_deck_csv(&mut output, "main", &deck.main_deck);
    append_deck_csv(&mut output, "commander", &deck.command_zone);
    append_deck_csv(&mut output, "sideboard", &deck.sideboard);
    output
}

pub fn collection_csv(cards: &[OwnedCard]) -> String {
    let mut output = String::from("quantity,arena_id,name,type,colors,set,collector_number\n");
    for card in cards {
        output.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            card.quantity,
            card.arena_id,
            csv(&card.name),
            csv(&card.type_line),
            csv(&card.colors.join("")),
            csv(card.set_code.as_deref().unwrap_or("")),
            csv(card.collector_number.as_deref().unwrap_or("")),
        ));
    }
    output
}

pub fn collection_arena(cards: &[OwnedCard]) -> String {
    let mut output = String::new();
    for card in cards {
        output.push_str(&arena_line(
            card.quantity,
            &card.name,
            card.set_code.as_deref(),
            card.collector_number.as_deref(),
        ));
    }
    output
}

fn append_arena_cards(output: &mut String, cards: &[DeckCard]) {
    for card in cards {
        output.push_str(&arena_line(
            card.quantity,
            &card.name,
            card.set_code.as_deref(),
            card.collector_number.as_deref(),
        ));
    }
}

fn arena_line(quantity: u32, name: &str, set: Option<&str>, collector: Option<&str>) -> String {
    match (set, collector) {
        (Some(set), Some(collector)) if !set.is_empty() && !collector.is_empty() => {
            format!(
                "{quantity} {name} ({}) {collector}\n",
                set.to_ascii_uppercase()
            )
        }
        _ => format!("{quantity} {name}\n"),
    }
}

fn append_deck_csv(output: &mut String, section: &str, cards: &[DeckCard]) {
    for card in cards {
        output.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            section,
            card.quantity,
            card.arena_id,
            csv(&card.name),
            csv(&card.type_line),
            csv(&card.colors.join("")),
            csv(card.set_code.as_deref().unwrap_or("")),
            csv(card.collector_number.as_deref().unwrap_or("")),
        ));
    }
}

fn csv(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{Deck, DeckCard};

    use super::*;

    #[test]
    fn arena_export_has_sections_and_printing() {
        let deck = Deck {
            name: "Test".into(),
            main_deck: vec![DeckCard {
                quantity: 4,
                name: "Lightning Strike".into(),
                set_code: Some("dmu".into()),
                collector_number: Some("137".into()),
                ..Default::default()
            }],
            sideboard: vec![DeckCard {
                quantity: 1,
                name: "Negate".into(),
                ..Default::default()
            }],
            ..Default::default()
        };

        assert_eq!(
            deck_arena(&deck),
            "Deck\n4 Lightning Strike (DMU) 137\n\nSideboard\n1 Negate\n"
        );
    }

    #[test]
    fn csv_escapes_special_characters() {
        assert_eq!(csv("One, \"Two\""), "\"One, \"\"Two\"\"\"");
    }
}
