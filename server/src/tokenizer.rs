use crate::cefr;

#[derive(Debug, Clone)]
pub struct WordSpan {
    pub line: u32,
    pub start_char: u32,
    pub length: u32,
    pub word: String,
}

#[derive(Debug, Clone)]
pub struct PhraseSpan {
    pub line: u32,
    pub start_char: u32,
    pub length: u32,
    pub phrase_key: String,
    pub word_count: usize,
}

#[derive(Debug, Clone)]
pub struct TokenizeResult {
    pub words: Vec<WordSpan>,
    pub phrases: Vec<PhraseSpan>,
}

fn extract_words(text: &str) -> Vec<WordSpan> {
    let mut spans = Vec::new();

    for (line_idx, line) in text.lines().enumerate() {
        let mut chars = line.char_indices().peekable();

        while let Some(&(byte_start, ch)) = chars.peek() {
            if ch.is_alphabetic() {
                let word_start_char = line[..byte_start].chars().count() as u32;
                let mut word = String::new();
                word.push(ch);
                chars.next();

                while let Some(&(_, c)) = chars.peek() {
                    if c.is_alphabetic() || c == '\'' || c == '\u{2019}' || c == '-' {
                        word.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }

                let trimmed = word
                    .trim_end_matches(|c: char| c == '\'' || c == '\u{2019}' || c == '-');
                if !trimmed.is_empty() {
                    spans.push(WordSpan {
                        line: line_idx as u32,
                        start_char: word_start_char,
                        length: trimmed.chars().count() as u32,
                        word: trimmed.to_string(),
                    });
                }
            } else {
                chars.next();
            }
        }
    }

    spans
}

/// Tokenize a document, returning words and phrases.
/// Words that are part of a detected phrase are excluded from the word list.
pub fn tokenize(text: &str) -> TokenizeResult {
    let all_words = extract_words(text);
    let phrase_keys = cefr::phrase_keys();
    let mut phrases = Vec::new();
    let mut covered: Vec<bool> = vec![false; all_words.len()];

    for (pattern_words, original_key) in phrase_keys.iter() {
        let pattern_len = pattern_words.len();
        if pattern_len < 2 || pattern_len > all_words.len() {
            continue;
        }

        for i in 0..=all_words.len() - pattern_len {
            if covered[i] {
                continue;
            }
            if all_words[i].line != all_words[i + pattern_len - 1].line {
                continue;
            }

            let matched = pattern_words
                .iter()
                .zip(&all_words[i..i + pattern_len])
                .all(|(pat, span)| span.word.to_lowercase() == *pat);

            if matched {
                let first = &all_words[i];
                let last = &all_words[i + pattern_len - 1];
                let end_char = last.start_char + last.length;

                phrases.push(PhraseSpan {
                    line: first.line,
                    start_char: first.start_char,
                    length: end_char - first.start_char,
                    phrase_key: original_key.clone(),
                    word_count: pattern_len,
                });

                for j in i..i + pattern_len {
                    covered[j] = true;
                }
            }
        }
    }

    phrases.sort_by_key(|p| (p.line, p.start_char));

    let words: Vec<WordSpan> = all_words
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !covered[*i])
        .map(|(_, w)| w)
        .collect();

    TokenizeResult { words, phrases }
}
