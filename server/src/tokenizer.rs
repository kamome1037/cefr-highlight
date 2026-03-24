/// A word span in a document.
#[derive(Debug, Clone)]
pub struct WordSpan {
    pub line: u32,
    pub start_char: u32,
    pub length: u32,
    pub word: String,
}

/// Split document text into word spans.
/// Words are sequences of ASCII alphabetic characters (plus apostrophes for contractions).
pub fn tokenize(text: &str) -> Vec<WordSpan> {
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

                let trimmed = word.trim_end_matches(|c: char| c == '\'' || c == '\u{2019}' || c == '-');
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
