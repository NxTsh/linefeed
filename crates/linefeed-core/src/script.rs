//! Script parsing and pt-BR-first text normalization.
//!
//! Every whitespace-separated word in the source text yields one or more
//! tokens: normalization lowercases, strips everything but alphanumerics and
//! apostrophes, folds Latin diacritics through a hand-rolled table (kept
//! table-based so the crate stays dependency-free — non-Latin diacritics
//! pass through unfolded), and expands ASCII digit runs into their pt-BR
//! spelled-out words (`"23"` → `vinte`/`e`/`tres`, three tokens). A word that
//! normalizes to nothing (pure punctuation) still yields a token with an
//! empty key so raw text and token indices stay in lockstep; the matcher
//! skips empty keys for free.

use std::collections::HashSet;
use std::sync::OnceLock;

/// Fold one lowercase char through the Latin diacritic table.
fn fold_char(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' => 'a',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

/// Lowercase, strip to alphanumerics + apostrophe, fold diacritics.
/// Digit runs are NOT expanded here — see [`expand_word`].
pub fn normalize_word(raw: &str) -> String {
    raw.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .map(fold_char)
        .collect()
}

const UNITS: [&str; 20] = [
    "zero",
    "um",
    "dois",
    "tres",
    "quatro",
    "cinco",
    "seis",
    "sete",
    "oito",
    "nove",
    "dez",
    "onze",
    "doze",
    "treze",
    "quatorze",
    "quinze",
    "dezesseis",
    "dezessete",
    "dezoito",
    "dezenove",
];
const TENS: [&str; 8] = [
    "vinte",
    "trinta",
    "quarenta",
    "cinquenta",
    "sessenta",
    "setenta",
    "oitenta",
    "noventa",
];
const HUNDREDS: [&str; 9] = [
    "cento",
    "duzentos",
    "trezentos",
    "quatrocentos",
    "quinhentos",
    "seiscentos",
    "setecentos",
    "oitocentos",
    "novecentos",
];

/// Spell out 0..=999 in pt-BR (folded keys, no accents).
fn number_under_1000(n: u32, out: &mut Vec<String>) {
    debug_assert!(n < 1000);
    if n == 100 {
        out.push("cem".to_string());
        return;
    }
    let h = n / 100;
    let rem = n % 100;
    if h > 0 {
        out.push(HUNDREDS[(h - 1) as usize].to_string());
        if rem > 0 {
            out.push("e".to_string());
        }
    }
    if rem == 0 {
        if h == 0 {
            out.push(UNITS[0].to_string());
        }
    } else if rem < 20 {
        out.push(UNITS[rem as usize].to_string());
    } else {
        out.push(TENS[(rem / 10 - 2) as usize].to_string());
        if rem % 10 > 0 {
            out.push("e".to_string());
            out.push(UNITS[(rem % 10) as usize].to_string());
        }
    }
}

/// Spell out 0..=999_999 in pt-BR. Larger numbers return None (the digit
/// string is kept as the key so it still matches an ASR emitting digits).
fn digits_to_words(digits: &str) -> Option<Vec<String>> {
    let n: u64 = digits.parse().ok()?;
    if n > 999_999 {
        return None;
    }
    let mut out = Vec::new();
    let thousands = (n / 1000) as u32;
    let rem = (n % 1000) as u32;
    if thousands > 0 {
        if thousands == 1 {
            out.push("mil".to_string());
        } else {
            number_under_1000(thousands, &mut out);
            out.push("mil".to_string());
        }
        if rem > 0 {
            // "e" joins when the remainder is < 100 or a round hundred.
            if rem < 100 || rem % 100 == 0 {
                out.push("e".to_string());
            }
            number_under_1000(rem, &mut out);
        }
    } else {
        number_under_1000(rem, &mut out);
    }
    Some(out)
}

/// Normalize one whitespace word into 1..n token keys.
pub fn expand_word(raw: &str) -> Vec<String> {
    let key = normalize_word(raw);
    if !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) {
        if let Some(words) = digits_to_words(&key) {
            return words;
        }
    }
    vec![key]
}

/// Filler / function words that are de-weighted during alignment: pt-BR
/// function words, interjections and ASR hallucination artifacts, plus a
/// small English set. Entries are written naturally (accents allowed) and
/// folded through the same normalization as script tokens when the set is
/// built, so lookups always compare folded keys against folded keys.
const FILLERS: &[&str] = &[
    // pt-BR articles / prepositions / conjunctions / pronouns
    "a", "o", "as", "os", "um", "uma", "uns", "umas", "de", "do", "da", "dos", "das", "em", "no",
    "na", "nos", "nas", "num", "numa", "por", "pelo", "pela", "pelos", "pelas", "com", "sem",
    "para", "pra", "pro", "que", "e", "ou", "mas", "se", "ao", "aos", "à", "às", "é", "são", "foi",
    "ser", "está", "estão", "tá", "tem", "têm", "tinha", "há", "havia", "eu", "tu", "ele", "ela",
    "nós", "você", "vocês", "eles", "elas", "me", "te", "lhe", "isso", "isto", "aquilo", "esse",
    "essa", "este", "esta", "aquele", "aquela", "não", "sim", "já", "só", "também", "então",
    "porque", "quando", "onde", "quem", "como", "muito", "muita", "muitos", "muitas", "mais",
    "menos", "bem", "mal", "aqui", "ali", "lá", "tão", "cada", "todo", "toda", "tudo", "nada",
    // interjections & hallucination artifacts
    "ah", "eh", "hã", "han", "hum", "hmm", "né", "tipo", "ok", "ó", "olha", "alô", "viu", "assim",
    "bom", "certo", "beleza", "pois", "poisé", "aí", "ué", "opa", "ta",
    // small English set (mixed-language scripts)
    "the", "a", "an", "of", "to", "in", "and", "or", "is", "are", "was", "it", "that", "this", "uh",
    "um", "yeah", "so", "like", "well",
];

fn filler_set() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| {
        FILLERS
            .iter()
            .map(|f| normalize_word(f))
            .filter(|k| !k.is_empty())
            .collect()
    })
}

/// Is this (already-normalized, folded) key a filler word?
pub fn is_filler(key: &str) -> bool {
    filler_set().contains(key)
}

/// One script token: a normalized key plus enough provenance to render.
#[derive(Debug, Clone)]
pub struct ScriptToken {
    /// The raw whitespace word this token came from (shared by all tokens of
    /// an expanded digit word).
    pub raw: String,
    /// Normalized, folded key. Empty for punctuation-only words.
    pub key: String,
    /// Display-line index (blank lines dropped).
    pub line: u32,
    /// Physical line index in the source file.
    pub file_line: u32,
    /// True when `key` is a filler word.
    pub filler: bool,
}

/// A paragraph: a run of display lines that were contiguous in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paragraph {
    pub start_line: usize,
    pub end_line: usize, // inclusive
    pub tok_start: usize,
    pub tok_end: usize, // exclusive
}

/// A parsed script: token stream plus line/paragraph/word index maps.
#[derive(Debug, Clone)]
pub struct Script {
    tokens: Vec<ScriptToken>,
    lines: Vec<String>,
    line_file: Vec<u32>,
    paragraphs: Vec<Paragraph>,
    /// Whitespace-word index → token span `[start, end)`.
    word_spans: Vec<(usize, usize)>,
}

impl Script {
    pub fn parse(text: &str) -> Script {
        let mut tokens = Vec::new();
        let mut lines = Vec::new();
        let mut line_file = Vec::new();
        let mut word_spans = Vec::new();

        for (file_idx, raw_line) in text.lines().enumerate() {
            if raw_line.trim().is_empty() {
                continue;
            }
            let display_idx = lines.len() as u32;
            lines.push(raw_line.trim_end().to_string());
            line_file.push(file_idx as u32);
            for word in raw_line.split_whitespace() {
                let start = tokens.len();
                for key in expand_word(word) {
                    let filler = !key.is_empty() && is_filler(&key);
                    tokens.push(ScriptToken {
                        raw: word.to_string(),
                        key,
                        line: display_idx,
                        file_line: file_idx as u32,
                        filler,
                    });
                }
                word_spans.push((start, tokens.len()));
            }
        }

        // Paragraphs: display lines whose file lines were contiguous.
        let mut paragraphs: Vec<Paragraph> = Vec::new();
        for li in 0..lines.len() {
            let new_para = match li.checked_sub(1) {
                Some(prev) => line_file[prev] + 1 != line_file[li],
                None => true,
            };
            let tok_start = tokens
                .iter()
                .position(|t| t.line as usize == li)
                .unwrap_or(tokens.len());
            let tok_end = tokens
                .iter()
                .rposition(|t| t.line as usize == li)
                .map(|i| i + 1)
                .unwrap_or(tok_start);
            if new_para {
                paragraphs.push(Paragraph {
                    start_line: li,
                    end_line: li,
                    tok_start,
                    tok_end,
                });
            } else if let Some(p) = paragraphs.last_mut() {
                p.end_line = li;
                p.tok_end = p.tok_end.max(tok_end);
            }
        }

        Script {
            tokens,
            lines,
            line_file,
            paragraphs,
            word_spans,
        }
    }

    pub fn n_tokens(&self) -> usize {
        self.tokens.len()
    }

    pub fn tokens(&self) -> &[ScriptToken] {
        &self.tokens
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn line_text(&self, line: usize) -> Option<&str> {
        self.lines.get(line).map(String::as_str)
    }

    pub fn line_file_index(&self, line: usize) -> Option<u32> {
        self.line_file.get(line).copied()
    }

    pub fn line_of_token(&self, tok: usize) -> Option<usize> {
        self.tokens.get(tok).map(|t| t.line as usize)
    }

    pub fn paragraphs(&self) -> &[Paragraph] {
        &self.paragraphs
    }

    /// Paragraph containing this token, or None for an out-of-range token
    /// (including any token of an empty script — no sentinel indices).
    pub fn paragraph_of_token(&self, tok: usize) -> Option<usize> {
        if tok >= self.tokens.len() {
            return None;
        }
        match self.paragraphs.binary_search_by(|p| {
            if tok < p.tok_start {
                std::cmp::Ordering::Greater
            } else if tok >= p.tok_end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(i) => Some(i),
            Err(_) => None,
        }
    }

    pub fn n_words(&self) -> usize {
        self.word_spans.len()
    }

    /// Token span `[start, end)` of the nth whitespace word.
    pub fn word_span(&self, word: usize) -> Option<(usize, usize)> {
        self.word_spans.get(word).copied()
    }

    /// Inverse of [`word_span`]: which whitespace word owns this token.
    pub fn word_of_token(&self, tok: usize) -> Option<usize> {
        if tok >= self.tokens.len() {
            return None;
        }
        match self.word_spans.binary_search_by(|&(s, e)| {
            if tok < s {
                std::cmp::Ordering::Greater
            } else if tok >= e {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(i) => Some(i),
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_strips_punct_and_folds() {
        assert_eq!(normalize_word("Bem-vindos!"), "bemvindos");
        assert_eq!(normalize_word("d'água"), "d'agua");
        assert_eq!(normalize_word("—"), "");
        assert_eq!(normalize_word("NÃO"), "nao");
        assert_eq!(normalize_word("coração,"), "coracao");
    }

    #[test]
    fn digits_expand() {
        assert_eq!(expand_word("23"), vec!["vinte", "e", "tres"]);
        assert_eq!(expand_word("100"), vec!["cem"]);
        assert_eq!(expand_word("101"), vec!["cento", "e", "um"]);
        assert_eq!(expand_word("1000"), vec!["mil"]);
        assert_eq!(
            expand_word("2024"),
            vec!["dois", "mil", "e", "vinte", "e", "quatro"]
        );
        assert_eq!(expand_word("0"), vec!["zero"]);
        // > 999_999 keeps the digit key
        assert_eq!(expand_word("1000000"), vec!["1000000"]);
    }

    #[test]
    fn fillers_are_folded_at_build_time() {
        // The regression that motivated the rewrite: accented list entries
        // must match the folded keys the matcher actually sees.
        for key in [
            "nao", "ja", "ha", "tao", "ne", "la", "e", "sao", "esta", "tambem",
        ] {
            assert!(is_filler(key), "expected folded key {key:?} to be a filler");
        }
        // Unfolded/accented lookups never happen in production; the folded
        // set intentionally does not contain them.
        assert!(!is_filler("não"));
        assert!(!is_filler("também"));
        // Content words stay content.
        for key in ["teleprompter", "servidor", "kubernetes"] {
            assert!(!is_filler(key));
        }
    }

    #[test]
    fn parse_counts_lines_paragraphs() {
        let s = Script::parse("Olá mundo!\nSegunda linha.\n\nNovo parágrafo aqui.\n");
        assert_eq!(s.lines().len(), 3);
        assert_eq!(s.paragraphs().len(), 2);
        assert_eq!(s.n_words(), 7);
        assert_eq!(s.n_tokens(), 7);
        assert_eq!(s.line_of_token(0), Some(0));
        assert_eq!(s.paragraph_of_token(6), Some(1));
        assert_eq!(s.tokens()[0].key, "ola");
    }

    #[test]
    fn digit_word_expands_token_count() {
        let s = Script::parse("tenho 23 anos");
        assert_eq!(s.n_words(), 3);
        assert_eq!(s.n_tokens(), 5); // tenho vinte e tres anos
        assert_eq!(s.word_span(1), Some((1, 4)));
        assert_eq!(s.word_of_token(3), Some(1));
        assert_eq!(s.word_of_token(4), Some(2));
    }

    #[test]
    fn empty_script_is_safe() {
        let s = Script::parse("");
        assert_eq!(s.n_tokens(), 0);
        assert_eq!(s.paragraph_of_token(0), None);
        assert_eq!(s.word_of_token(0), None);
        assert_eq!(s.line_of_token(0), None);
        assert_eq!(s.line_text(0), None);
    }

    #[test]
    fn punctuation_only_word_keeps_empty_key_token() {
        let s = Script::parse("pausa — depois");
        assert_eq!(s.n_words(), 3);
        assert_eq!(s.n_tokens(), 3);
        assert_eq!(s.tokens()[1].key, "");
        assert!(!s.tokens()[1].filler);
    }
}
