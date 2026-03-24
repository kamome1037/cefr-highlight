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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CefrLevel {
    A1,
    A2,
    B1,
    B2,
    C1,
    C2,
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
        match self {
            Self::A1 => 0,
            Self::A2 => 1,
            Self::B1 => 2,
            Self::B2 => 3,
            Self::C1 => 4,
            Self::C2 => 5,
        }
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

pub fn index() -> &'static CefrIndex {
    &CEFR_DATA
}

/// Generate candidate lookup keys for a word, handling common English inflections.
fn candidate_keys(raw: &str) -> Vec<String> {
    let key = raw.trim().to_lowercase();
    let mut candidates = vec![key.clone()];

    if key.len() > 3 && key.ends_with("ed") {
        candidates.push(key[..key.len() - 1].to_string()); // e.g. "used" -> "use"
        candidates.push(key[..key.len() - 2].to_string()); // e.g. "played" -> "play"
    }
    if key.len() > 4 && key.ends_with("ing") {
        candidates.push(key[..key.len() - 3].to_string()); // e.g. "playing" -> "play"
        candidates.push(format!("{}e", &key[..key.len() - 3])); // e.g. "making" -> "make"
    }
    if key.len() > 2 && key.ends_with('s') && !key.ends_with("ss") {
        candidates.push(key[..key.len() - 1].to_string()); // e.g. "cats" -> "cat"
    }
    if key.len() > 3 && key.ends_with("es") {
        candidates.push(key[..key.len() - 2].to_string()); // e.g. "watches" -> "watch"
    }
    if key.len() > 3 && key.ends_with("ies") {
        let mut base = key[..key.len() - 3].to_string();
        base.push('y');
        candidates.push(base); // e.g. "countries" -> "country"
    }
    if key.len() > 3 && key.ends_with("ly") {
        candidates.push(key[..key.len() - 2].to_string()); // e.g. "quickly" -> "quick"
    }
    if key.len() > 4 && key.ends_with("er") {
        candidates.push(key[..key.len() - 2].to_string()); // e.g. "bigger" -> "bigg" (won't match, but worth trying)
        candidates.push(key[..key.len() - 1].to_string()); // e.g. "wider" -> "wide"
    }
    if key.len() > 4 && key.ends_with("est") {
        candidates.push(key[..key.len() - 3].to_string());
        candidates.push(format!("{}e", &key[..key.len() - 3]));
    }

    candidates.dedup();
    candidates
}

/// Look up a word in the CEFR index, trying inflected form fallbacks.
pub fn lookup(word: &str) -> Option<&'static Vec<CefrEntry>> {
    let idx = index();
    for key in candidate_keys(word) {
        if let Some(entries) = idx.get(&key) {
            return Some(entries);
        }
    }
    None
}

/// Get the highest-priority (first) CEFR level for a word.
pub fn lookup_level(word: &str) -> Option<CefrLevel> {
    lookup(word).and_then(|entries| {
        entries
            .first()
            .and_then(|e| CefrLevel::from_str(&e.level))
    })
}
