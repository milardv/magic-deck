use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    os::unix::fs::FileExt,
    path::Path,
};

use anyhow::{bail, Context, Result};

use crate::card_database;

const MIN_CARD_ID: u32 = 1_000;
const MAX_CARD_ID: u32 = 500_000;
const MAX_QUANTITY: u32 = 400;
const MIN_BLOCK: usize = 40;
const MAX_REGION_SIZE: u64 = 512 * 1024 * 1024;

pub enum ScanResult {
    Collection(BTreeMap<u64, u32>),
    GameNotRunning,
    NoCollectionFound,
}

#[derive(Default)]
struct Candidate {
    valid_entries: usize,
    total_entries: usize,
    cards: BTreeMap<u64, u32>,
}

impl Candidate {
    fn is_better_than(&self, other: &Self) -> bool {
        let self_ratio = self.valid_entries * other.total_entries.max(1);
        let other_ratio = other.valid_entries * self.total_entries.max(1);
        self_ratio > other_ratio
            || (self_ratio == other_ratio && self.valid_entries > other.valid_entries)
    }

    fn is_collection(&self) -> bool {
        self.total_entries >= MIN_BLOCK && self.valid_entries * 100 >= self.total_entries * 90
    }
}

pub fn scan(log_path: &Path) -> Result<ScanResult> {
    let Some(pid) = find_mtga_pid()? else {
        return Ok(ScanResult::GameNotRunning);
    };
    let Some(valid_ids) = card_database::load_known_card_ids(log_path)? else {
        bail!("MTGA card database not found");
    };
    let maps = fs::read_to_string(format!("/proc/{pid}/maps"))
        .with_context(|| format!("cannot read memory map for MTGA process {pid}"))?;
    let memory = File::open(format!("/proc/{pid}/mem")).with_context(|| {
        format!("cannot read MTGA process {pid}; check kernel.yama.ptrace_scope")
    })?;
    let mut best = Candidate::default();

    for line in maps.lines() {
        let Some((start, end)) = readable_private_region(line) else {
            continue;
        };
        let size = end - start;
        if size == 0 || size > MAX_REGION_SIZE {
            continue;
        }
        let Ok(mut bytes) = usize::try_from(size).map(Vec::with_capacity) else {
            continue;
        };
        bytes.resize(size as usize, 0);
        if memory.read_exact_at(&mut bytes, start).is_err() {
            continue;
        }
        scan_dictionary_layout(&bytes, &valid_ids, &mut best);
    }

    if !best.is_collection() {
        // Some MTGA builds use adjacent (card id, quantity) pairs rather than
        // Mono Dictionary entries. Only pay for this second pass when needed.
        for line in maps.lines() {
            let Some((start, end)) = readable_private_region(line) else {
                continue;
            };
            let size = end - start;
            if size == 0 || size > MAX_REGION_SIZE {
                continue;
            }
            let mut bytes = vec![0; size as usize];
            if memory.read_exact_at(&mut bytes, start).is_err() {
                continue;
            }
            scan_packed_layout(&bytes, &valid_ids, &mut best);
        }
    }

    if best.is_collection() {
        Ok(ScanResult::Collection(best.cards))
    } else {
        Ok(ScanResult::NoCollectionFound)
    }
}

fn find_mtga_pid() -> Result<Option<u32>> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        if fs::read_to_string(format!("/proc/{pid}/comm"))
            .is_ok_and(|name| name.trim() == "MTGA.exe")
        {
            let rss = fs::read_to_string(format!("/proc/{pid}/statm"))
                .ok()
                .and_then(|value| value.split_whitespace().nth(1)?.parse::<u64>().ok())
                .unwrap_or_default();
            candidates.push((rss, pid));
        }
    }
    Ok(candidates.into_iter().max().map(|(_, pid)| pid))
}

fn readable_private_region(line: &str) -> Option<(u64, u64)> {
    let mut fields = line.split_whitespace();
    let range = fields.next()?;
    let permissions = fields.next()?;
    if !permissions.starts_with("rw-p") {
        return None;
    }
    let path = fields.nth(3).unwrap_or_default();
    if path.starts_with('/') && path.ends_with(".so") {
        return None;
    }
    let (start, end) = range.split_once('-')?;
    Some((
        u64::from_str_radix(start, 16).ok()?,
        u64::from_str_radix(end, 16).ok()?,
    ))
}

fn scan_dictionary_layout(bytes: &[u8], valid_ids: &HashSet<u32>, best: &mut Candidate) {
    let words = bytes.len() / 4;
    for phase in 0..4 {
        let mut index = phase;
        while index + 4 <= words {
            if let Some((card_id, quantity)) = dictionary_entry(bytes, index) {
                let start = index;
                let mut entries = Vec::new();
                let mut seen = HashSet::new();
                entries.push((card_id, quantity));
                seen.insert(card_id);
                index += 4;
                while index + 4 <= words {
                    let Some((next_id, next_quantity)) = dictionary_entry(bytes, index) else {
                        break;
                    };
                    if !seen.insert(next_id) {
                        break;
                    }
                    entries.push((next_id, next_quantity));
                    index += 4;
                }
                consider(entries, valid_ids, best);
                if index == start {
                    index += 4;
                }
            } else {
                index += 4;
            }
        }
    }
}

fn scan_packed_layout(bytes: &[u8], valid_ids: &HashSet<u32>, best: &mut Candidate) {
    let words = bytes.len() / 4;
    for phase in 0..2 {
        let mut index = phase;
        while index + 2 <= words {
            if let Some(pair) = packed_entry(bytes, index) {
                let mut entries = vec![pair];
                let mut seen = HashSet::from([pair.0]);
                index += 2;
                while index + 2 <= words {
                    let Some(next) = packed_entry(bytes, index) else {
                        break;
                    };
                    if !seen.insert(next.0) {
                        break;
                    }
                    entries.push(next);
                    index += 2;
                }
                consider(entries, valid_ids, best);
            } else {
                index += 2;
            }
        }
    }
}

fn dictionary_entry(bytes: &[u8], word: usize) -> Option<(u32, u32)> {
    let hash = word_at(bytes, word);
    let next = word_at(bytes, word + 1);
    let card_id = word_at(bytes, word + 2);
    let quantity = word_at(bytes, word + 3);
    (hash == card_id && plausible_pair(card_id, quantity) && (next == u32::MAX || next < 0x10_0000))
        .then_some((card_id, quantity))
}

fn packed_entry(bytes: &[u8], word: usize) -> Option<(u32, u32)> {
    let card_id = word_at(bytes, word);
    let quantity = word_at(bytes, word + 1);
    plausible_pair(card_id, quantity).then_some((card_id, quantity))
}

fn plausible_pair(card_id: u32, quantity: u32) -> bool {
    (MIN_CARD_ID..MAX_CARD_ID).contains(&card_id) && (1..=MAX_QUANTITY).contains(&quantity)
}

fn word_at(bytes: &[u8], word: usize) -> u32 {
    let offset = word * 4;
    u32::from_ne_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

fn consider(entries: Vec<(u32, u32)>, valid_ids: &HashSet<u32>, best: &mut Candidate) {
    if entries.len() < MIN_BLOCK {
        return;
    }
    let valid_entries = entries
        .iter()
        .filter(|(card_id, _)| valid_ids.contains(card_id))
        .count();
    let candidate = Candidate {
        valid_entries,
        total_entries: entries.len(),
        cards: entries
            .into_iter()
            .filter(|(card_id, _)| valid_ids.contains(card_id))
            .map(|(card_id, quantity)| (u64::from(card_id), quantity))
            .collect(),
    };
    if candidate.is_better_than(best) {
        *best = candidate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dictionary_collection() {
        let valid = (70_000..70_050).collect::<HashSet<_>>();
        let mut bytes = Vec::new();
        for id in 70_000..70_050u32 {
            bytes.extend(id.to_ne_bytes());
            bytes.extend(u32::MAX.to_ne_bytes());
            bytes.extend(id.to_ne_bytes());
            bytes.extend(2u32.to_ne_bytes());
        }
        let mut best = Candidate::default();
        scan_dictionary_layout(&bytes, &valid, &mut best);
        assert!(best.is_collection());
        assert_eq!(best.cards.len(), 50);
    }
}
