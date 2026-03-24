use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct CefrEntry {
    pub term: String,
    pub level: String,
    #[serde(default)]
    pub part_of_speech: String,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CefrLevel {
    A1 = 0,
    A2 = 1,
    B1 = 2,
    B2 = 3,
    C1 = 4,
    C2 = 5,
}

impl CefrLevel {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "A1" => Some(Self::A1),
            "A2" => Some(Self::A2),
            "B1" => Some(Self::B1),
            "B2" => Some(Self::B2),
            "C1" => Some(Self::C1),
            "C2" => Some(Self::C2),
            _ => None,
        }
    }

    pub fn token_type_index(self) -> u32 {
        self as u32
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::A1 => "A1 (Beginner)",
            Self::A2 => "A2 (Elementary)",
            Self::B1 => "B1 (Intermediate)",
            Self::B2 => "B2 (Upper-intermediate)",
            Self::C1 => "C1 (Advanced)",
            Self::C2 => "C2 (Proficiency)",
        }
    }
}

pub type CefrIndex = HashMap<String, Vec<CefrEntry>>;

static CEFR_DATA: Lazy<CefrIndex> = Lazy::new(|| {
    let json_bytes = include_bytes!("../data/cefr_index.json");
    serde_json::from_slice(json_bytes).expect("failed to parse cefr_index.json")
});

/// All phrase keys (entries containing a space), sorted longest-first for greedy matching.
static PHRASE_KEYS: Lazy<Vec<(Vec<String>, String)>> = Lazy::new(|| {
    let idx = index();
    let mut phrases: Vec<(Vec<String>, String)> = idx
        .keys()
        .filter(|k| k.contains(' ') && !k.contains('(') && !k.contains('/'))
        .map(|k| {
            let words: Vec<String> = k.split_whitespace().map(|w| w.to_lowercase()).collect();
            (words, k.clone())
        })
        .collect();
    phrases.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    phrases
});

pub fn index() -> &'static CefrIndex {
    &CEFR_DATA
}

pub fn phrase_keys() -> &'static Vec<(Vec<String>, String)> {
    &PHRASE_KEYS
}

fn candidate_keys(raw: &str) -> Vec<String> {
    let key = raw.trim().to_lowercase();
    let mut candidates = vec![key.clone()];

    if key.len() > 3 && key.ends_with("ed") {
        candidates.push(key[..key.len() - 1].to_string());
        candidates.push(key[..key.len() - 2].to_string());
    }
    if key.len() > 4 && key.ends_with("ing") {
        candidates.push(key[..key.len() - 3].to_string());
        candidates.push(format!("{}e", &key[..key.len() - 3]));
    }
    if key.len() > 2 && key.ends_with('s') && !key.ends_with("ss") {
        candidates.push(key[..key.len() - 1].to_string());
    }
    if key.len() > 3 && key.ends_with("es") {
        candidates.push(key[..key.len() - 2].to_string());
    }
    if key.len() > 3 && key.ends_with("ies") {
        let mut base = key[..key.len() - 3].to_string();
        base.push('y');
        candidates.push(base);
    }
    if key.len() > 3 && key.ends_with("ly") {
        candidates.push(key[..key.len() - 2].to_string());
    }
    if key.len() > 4 && key.ends_with("er") {
        candidates.push(key[..key.len() - 2].to_string());
        candidates.push(key[..key.len() - 1].to_string());
    }
    if key.len() > 4 && key.ends_with("est") {
        candidates.push(key[..key.len() - 3].to_string());
        candidates.push(format!("{}e", &key[..key.len() - 3]));
    }

    candidates.dedup();
    candidates
}

pub fn lookup(word: &str) -> Option<&'static Vec<CefrEntry>> {
    let idx = index();
    for key in candidate_keys(word) {
        if let Some(entries) = idx.get(&key) {
            return Some(entries);
        }
    }
    None
}

pub fn lookup_phrase(key: &str) -> Option<&'static Vec<CefrEntry>> {
    index().get(key)
}

pub fn lookup_level(word: &str) -> Option<CefrLevel> {
    lookup(word).and_then(|entries| {
        entries
            .first()
            .and_then(|e| CefrLevel::from_str(&e.level))
    })
}

pub fn lookup_phrase_level(key: &str) -> Option<CefrLevel> {
    lookup_phrase(key).and_then(|entries| {
        entries
            .first()
            .and_then(|e| CefrLevel::from_str(&e.level))
    })
}
