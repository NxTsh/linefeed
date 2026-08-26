//! IPC payloads. Rust owns tokenization: every word carries its token span
//! `[ts, te)` in the tracker's index space, so the frontend NEVER
//! re-tokenizes (a digit word like "23" is one rendered word spanning three
//! tokens).

use serde::Serialize;

use linefeed_core::Script;

#[derive(Debug, Clone, Serialize)]
pub struct WordPayload {
    pub raw: String,
    /// Token span [ts, te) in tracker index space.
    pub ts: usize,
    pub te: usize,
    /// Display line index.
    pub line: usize,
    /// Paragraph index.
    pub para: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParagraphPayload {
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScriptPayload {
    pub path: String,
    pub words: Vec<WordPayload>,
    pub paragraphs: Vec<ParagraphPayload>,
    pub n_tokens: usize,
    pub n_words: usize,
}

pub fn build(script: &Script, path: &str) -> ScriptPayload {
    let mut words = Vec::with_capacity(script.n_words());
    for w in 0..script.n_words() {
        let Some((ts, te)) = script.word_span(w) else {
            continue;
        };
        // A word always has >= 1 token; provenance comes from its first one.
        let (line, para) = if ts < script.n_tokens() {
            (
                script.line_of_token(ts).unwrap_or(0),
                script.paragraph_of_token(ts).unwrap_or(0),
            )
        } else {
            (0, 0)
        };
        let raw = script
            .tokens()
            .get(ts)
            .map(|t| t.raw.clone())
            .unwrap_or_default();
        words.push(WordPayload {
            raw,
            ts,
            te,
            line,
            para,
        });
    }
    ScriptPayload {
        path: path.to_string(),
        words,
        paragraphs: script
            .paragraphs()
            .iter()
            .map(|p| ParagraphPayload {
                start_line: p.start_line,
                end_line: p.end_line,
            })
            .collect(),
        n_tokens: script.n_tokens(),
        n_words: script.n_words(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digit_words_span_multiple_tokens() {
        let s = Script::parse("tenho 23 anos\n\nsegundo parágrafo");
        let p = build(&s, "/tmp/x.txt");
        assert_eq!(p.n_words, 5);
        assert_eq!(p.n_tokens, 7);
        let w23 = &p.words[1];
        assert_eq!(w23.raw, "23");
        assert_eq!((w23.ts, w23.te), (1, 4));
        // Token spans tile the token space with no gaps.
        let mut cursor = 0;
        for w in &p.words {
            assert_eq!(w.ts, cursor, "spans must tile");
            cursor = w.te;
        }
        assert_eq!(cursor, p.n_tokens);
        assert_eq!(p.paragraphs.len(), 2);
        assert_eq!(p.words[3].para, 1);
        assert_eq!(p.words[3].line, 1);
    }

    #[test]
    fn empty_script_is_safe() {
        let p = build(&Script::parse(""), "");
        assert_eq!(p.n_words, 0);
        assert!(p.words.is_empty());
    }
}
