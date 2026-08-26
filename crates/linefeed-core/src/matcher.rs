//! Windowed alignment of heard words onto script tokens.
//!
//! A full dynamic program over the window (not Needleman–Wunsch banded by
//! diagonal — "banded" refers to the token window the tracker selects).
//! Design rules carried over from the validated first implementation:
//!
//! - Leading token skips are free (row 0 is all zeros): the alignment may
//!   start anywhere in the window. Trailing skips are effectively free too,
//!   because the end cell is chosen by argmax over the last row.
//! - Ties in that argmax break toward the LARGER token index, so a phrase
//!   repeated inside the window resolves to the occurrence closest to the
//!   cursor (prevents the repeated-phrase scroll lurch).
//! - Empty token keys (punctuation-only words) skip for free.
//! - Filler tokens are de-weighted: cheap to skip, reduced match reward, so
//!   hallucinated filler runs can't drag the cursor forward.
//!
//! Perf contract: all DP and similarity state lives in a caller-owned
//! [`Scratch`]; steady-state alignment performs no per-cell allocation.

use crate::script::ScriptToken;

/// Cost/threshold knobs for one alignment call.
#[derive(Debug, Clone, Copy)]
pub struct MatchParams {
    /// Minimum per-word similarity for a match cell to be allowed.
    pub min_sim: f32,
    /// Cost of skipping a heard word.
    pub skip_word: f32,
    /// Cost of skipping a content token.
    pub skip_token_content: f32,
    /// Cost of skipping a filler token.
    pub skip_token_filler: f32,
    /// Match reward multiplier for filler tokens.
    pub filler_scale: f32,
}

/// Result of one window alignment. Token indices are GLOBAL script indices.
#[derive(Debug, Clone, Default)]
pub struct Alignment {
    /// Matched `(word_idx, token_idx)` pairs in ascending order.
    pub pairs: Vec<(usize, usize)>,
    /// Sum of match rewards along the traceback.
    pub score: f32,
    /// One past the last matched token, or the window start when nothing
    /// matched.
    pub end_pos: usize,
    /// True when the last matched token is non-empty and non-filler.
    pub ends_on_content: bool,
    /// Number of matched pairs whose token is non-empty and non-filler.
    pub n_content_pairs: usize,
}

/// Reusable buffers for [`align_window`] and [`similarity`].
#[derive(Debug, Default)]
pub struct Scratch {
    d: Vec<f32>,
    bp: Vec<u8>,
    sim_prev: Vec<u16>,
    sim_cur: Vec<u16>,
    sim_b: Vec<char>,
}

impl Scratch {
    pub fn new() -> Scratch {
        Scratch::default()
    }
}

/// LCS ratio in `[0, 1]`: `2·LCS(a,b) / (|a| + |b|)` over chars.
/// Equivalent to rapidfuzz's `ratio`. Zero if either side is empty.
pub fn similarity(a: &str, b: &str, scratch: &mut Scratch) -> f32 {
    similarity_split(
        a,
        b,
        &mut scratch.sim_prev,
        &mut scratch.sim_cur,
        &mut scratch.sim_b,
    )
}

const BP_MATCH: u8 = 0;
const BP_SKIP_WORD: u8 = 1;
const BP_SKIP_TOKEN: u8 = 2;

/// Align `word_keys` onto `tokens[start..end]`.
///
/// `tokens` is the FULL script token slice; `start`/`end` select the window.
/// Word indices in the result refer to positions in `word_keys`; token
/// indices are global.
pub fn align_window(
    word_keys: &[&str],
    tokens: &[ScriptToken],
    start: usize,
    end: usize,
    params: &MatchParams,
    scratch: &mut Scratch,
) -> Alignment {
    let end = end.min(tokens.len());
    let start = start.min(end);
    let window = &tokens[start..end];
    let n = word_keys.len();
    let m = window.len();

    if n == 0 || m == 0 {
        return Alignment {
            end_pos: start,
            ..Alignment::default()
        };
    }

    let stride = m + 1;
    let cells = (n + 1) * stride;
    scratch.d.clear();
    scratch.d.resize(cells, 0.0);
    scratch.bp.clear();
    scratch.bp.resize(cells, BP_SKIP_TOKEN);

    // Row 0: free leading token skips (all zeros, bp = skip-token).
    // Column 0: each unmatched heard word costs skip_word.
    for i in 1..=n {
        scratch.d[i * stride] = -params.skip_word * i as f32;
        scratch.bp[i * stride] = BP_SKIP_WORD;
    }

    // Split scratch borrows: the similarity buffers are disjoint from d/bp,
    // so compute similarities into a closure-free inner loop via indices.
    for i in 1..=n {
        for j in 1..=m {
            let tok = &window[j - 1];
            let tok_cost = if tok.key.is_empty() {
                0.0
            } else if tok.filler {
                params.skip_token_filler
            } else {
                params.skip_token_content
            };

            let idx = i * stride + j;
            let up = scratch.d[idx - stride] - params.skip_word;
            let left = scratch.d[idx - 1] - tok_cost;
            let (mut best, mut bp) = if left >= up {
                (left, BP_SKIP_TOKEN)
            } else {
                (up, BP_SKIP_WORD)
            };

            if !tok.key.is_empty() {
                // similarity() borrows scratch mutably; read tok key first.
                let sim = {
                    let (a, b) = (word_keys[i - 1], tok.key.as_str());
                    similarity_split(
                        a,
                        b,
                        &mut scratch.sim_prev,
                        &mut scratch.sim_cur,
                        &mut scratch.sim_b,
                    )
                };
                if sim >= params.min_sim {
                    let reward = if tok.filler {
                        sim * params.filler_scale
                    } else {
                        sim
                    };
                    let diag = scratch.d[idx - stride - 1] + reward;
                    if diag >= best {
                        best = diag;
                        bp = BP_MATCH;
                    }
                }
            }

            scratch.d[idx] = best;
            scratch.bp[idx] = bp;
        }
    }

    // End cell: argmax over the last row, ties toward LARGER j.
    let mut best_j = 0usize;
    let mut best_v = f32::NEG_INFINITY;
    for j in 0..=m {
        let v = scratch.d[n * stride + j];
        if v >= best_v {
            best_v = v;
            best_j = j;
        }
    }

    // Traceback from (n, best_j).
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let (mut i, mut j) = (n, best_j);
    while i > 0 || j > 0 {
        match scratch.bp[i * stride + j] {
            BP_MATCH => {
                pairs.push((i - 1, start + j - 1));
                i -= 1;
                j -= 1;
            }
            BP_SKIP_WORD => i -= 1,
            _ => {
                if j == 0 {
                    // Row-0 cells left of the alignment: done.
                    break;
                }
                j -= 1;
            }
        }
    }
    pairs.reverse();

    // Score = sum of match rewards along the traceback.
    let mut score = 0.0f32;
    let mut n_content_pairs = 0usize;
    for &(wi, ti) in &pairs {
        let tok = &tokens[ti];
        let sim = similarity_split(
            word_keys[wi],
            tok.key.as_str(),
            &mut scratch.sim_prev,
            &mut scratch.sim_cur,
            &mut scratch.sim_b,
        );
        score += if tok.filler {
            sim * params.filler_scale
        } else {
            sim
        };
        if !tok.key.is_empty() && !tok.filler {
            n_content_pairs += 1;
        }
    }

    let (end_pos, ends_on_content) = match pairs.last() {
        Some(&(_, ti)) => {
            let tok = &tokens[ti];
            (ti + 1, !tok.key.is_empty() && !tok.filler)
        }
        None => (start, false),
    };

    Alignment {
        pairs,
        score,
        end_pos,
        ends_on_content,
        n_content_pairs,
    }
}

/// similarity() body over explicitly-split buffers, so `align_window` can
/// borrow the DP tables and the similarity buffers simultaneously.
fn similarity_split(
    a: &str,
    b: &str,
    prev: &mut Vec<u16>,
    cur: &mut Vec<u16>,
    bbuf: &mut Vec<char>,
) -> f32 {
    bbuf.clear();
    bbuf.extend(b.chars());
    let m = bbuf.len();
    let mut n = 0usize;
    prev.clear();
    prev.resize(m + 1, 0);
    cur.clear();
    cur.resize(m + 1, 0);
    for ca in a.chars() {
        n += 1;
        for j in 1..=m {
            cur[j] = if ca == bbuf[j - 1] {
                prev[j - 1] + 1
            } else {
                prev[j].max(cur[j - 1])
            };
        }
        std::mem::swap(prev, cur);
        cur[0] = 0;
    }
    if n == 0 || m == 0 {
        return 0.0;
    }
    2.0 * prev[m] as f32 / (n + m) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Script;

    fn params() -> MatchParams {
        MatchParams {
            min_sim: 0.6,
            skip_word: 1.0,
            skip_token_content: 1.0,
            skip_token_filler: 0.35,
            filler_scale: 0.5,
        }
    }

    fn toks(text: &str) -> Vec<ScriptToken> {
        Script::parse(text).tokens().to_vec()
    }

    #[test]
    fn similarity_basics() {
        let mut s = Scratch::new();
        assert_eq!(similarity("casa", "casa", &mut s), 1.0);
        assert_eq!(similarity("", "casa", &mut s), 0.0);
        assert_eq!(similarity("casa", "", &mut s), 0.0);
        let v = similarity("casa", "caza", &mut s);
        assert!(v > 0.7 && v < 1.0, "got {v}");
        // Buffers are reused across calls without cross-talk.
        assert_eq!(similarity("casa", "casa", &mut s), 1.0);
    }

    #[test]
    fn align_exact_sequence() {
        let t = toks("meu servidor roda kubernetes hoje");
        let words = ["servidor", "roda", "kubernetes"];
        let al = align_window(&words, &t, 0, t.len(), &params(), &mut Scratch::new());
        assert_eq!(al.pairs.len(), 3);
        assert_eq!(al.end_pos, 4);
        assert!(al.ends_on_content);
        assert_eq!(al.n_content_pairs, 3);
    }

    #[test]
    fn align_tolerates_word_gap_and_token_gap() {
        let t = toks("primeiro segundo terceiro quarto quinto");
        // Heard an extra word not in the script.
        let al = align_window(
            &["primeiro", "banana", "segundo"],
            &t,
            0,
            t.len(),
            &params(),
            &mut Scratch::new(),
        );
        assert_eq!(al.pairs.len(), 2);
        assert_eq!(al.end_pos, 2);
        // Skipped a script token.
        let al2 = align_window(
            &["primeiro", "terceiro"],
            &t,
            0,
            t.len(),
            &params(),
            &mut Scratch::new(),
        );
        assert_eq!(al2.pairs.len(), 2);
        assert_eq!(al2.end_pos, 3);
    }

    #[test]
    fn leading_token_skips_free() {
        let t = toks("um dois tres quatro cinco seis sete oito nove dez");
        let al = align_window(
            &["oito", "nove"],
            &t,
            0,
            t.len(),
            &params(),
            &mut Scratch::new(),
        );
        assert_eq!(al.pairs, vec![(0, 7), (1, 8)]);
        assert!(al.score > 1.9);
    }

    #[test]
    fn window_offset_maps_global_indices() {
        let t = toks("um dois tres quatro cinco seis sete oito nove dez");
        let al = align_window(&["seis", "sete"], &t, 4, 8, &params(), &mut Scratch::new());
        assert_eq!(al.pairs, vec![(0, 5), (1, 6)]);
        assert_eq!(al.end_pos, 7);
    }

    #[test]
    fn repeated_phrase_ties_break_toward_larger_index() {
        let t = toks("marca zebra marca zebra");
        let al = align_window(
            &["marca", "zebra"],
            &t,
            0,
            t.len(),
            &params(),
            &mut Scratch::new(),
        );
        assert_eq!(al.end_pos, 4, "should land on the later occurrence");
    }

    #[test]
    fn empty_inputs() {
        let t = toks("um dois");
        let al = align_window(&[], &t, 0, t.len(), &params(), &mut Scratch::new());
        assert_eq!(al.end_pos, 0);
        assert!(al.pairs.is_empty());
        let al2 = align_window(&["um"], &t, 2, 2, &params(), &mut Scratch::new());
        assert_eq!(al2.end_pos, 2);
        let al3 = align_window(&["um"], &t, 5, 9, &params(), &mut Scratch::new());
        assert_eq!(al3.end_pos, 2, "clamped window collapses to end");
    }

    #[test]
    fn filler_matches_are_deweighted() {
        // "que" is a filler; a filler-only alignment scores under the
        // content equivalent and never counts as a content pair.
        let t = toks("que que que verdade");
        let al = align_window(
            &["que", "que"],
            &t,
            0,
            t.len(),
            &params(),
            &mut Scratch::new(),
        );
        assert_eq!(al.n_content_pairs, 0);
        assert!(!al.ends_on_content);
        assert!(al.score <= 1.0);
    }

    #[test]
    fn punctuation_tokens_skip_free() {
        let t = toks("casa — jardim");
        assert_eq!(t.len(), 3);
        let al = align_window(
            &["casa", "jardim"],
            &t,
            0,
            t.len(),
            &params(),
            &mut Scratch::new(),
        );
        assert_eq!(al.pairs, vec![(0, 0), (1, 2)]);
        assert!(
            (al.score - 2.0).abs() < 1e-6,
            "empty-key token must cost nothing, score {}",
            al.score
        );
    }
}
