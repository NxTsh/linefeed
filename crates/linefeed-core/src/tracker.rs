//! The cursor tracker: turns a stream of ASR word hypotheses into a script
//! cursor via a four-state machine (TRACKING / HOLDING / LOST / BACKTRACK).
//!
//! Contract (relied on by every renderer): the cursor never decreases except
//! on a `State::Backtrack` event.
//!
//! Engines may emit hypotheses of different shapes — cumulative from the
//! utterance start (true-streaming engines) or a re-decoded trailing window
//! (sherpa-style emulated streaming). Both are legal: the dedupe stage
//! reconciles overlap, and the freshness gates below key advancement to
//! genuinely new audio.
//!
//! Memory contract: the retained word stream is bounded. Entries are held in
//! a deque with a monotonically increasing base offset so all anchors are
//! absolute indices; the front is trimmed as soon as entries fall out of
//! every window that could still need them.

use std::collections::VecDeque;

use crate::matcher::{align_window, MatchParams, Scratch};
use crate::script::{normalize_word, Script};

/// One recognized word.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Word {
    /// Raw engine text.
    pub text: String,
    /// Stream time of the word start, seconds.
    pub t: f32,
    /// Normalized, folded key.
    pub key: String,
}

impl Word {
    pub fn new(text: impl Into<String>, t: f32) -> Word {
        let text = text.into();
        let key = normalize_word(&text);
        Word { text, t, key }
    }
}

/// One engine emission: a batch of words plus the stream time it was made.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Hypothesis {
    pub words: Vec<Word>,
    pub t: f32,
}

/// Tracker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum State {
    Tracking,
    Holding,
    Lost,
    Backtrack,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Tracking => "TRACKING",
            State::Holding => "HOLDING",
            State::Lost => "LOST",
            State::Backtrack => "BACKTRACK",
        }
    }
}

/// Emitted whenever there is news: the cursor moved or the state changed.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TrackerEvent {
    pub t: f32,
    pub state: State,
    pub cursor: usize,
    pub score: f32,
    /// Cursor delta relative to the previous event-worthy position.
    pub jump: i64,
    /// Consecutive hypotheses without an accepted advance.
    pub held_for: u32,
}

/// All tracker knobs. Defaults were validated on recorded pt-BR takes in the
/// first implementation; every constant that used to be a magic literal
/// (dedupe lookback, LOST scan growth) now lives here.
#[derive(Debug, Clone)]
pub struct TrackerConfig {
    /// Base token window behind the cursor.
    pub window_back: usize,
    /// Base token window ahead of the cursor.
    pub window_ahead: usize,
    /// Growth added to each side per held hypothesis.
    pub window_growth: usize,
    /// Cap on the grown backward window.
    pub window_back_max: usize,
    /// Cap on the grown forward window.
    pub window_ahead_max: usize,
    /// Minimum per-word similarity for a match.
    pub min_sim: f32,
    /// DP skip costs / filler handling (see matcher).
    pub skip_word: f32,
    pub skip_token_content: f32,
    pub skip_token_filler: f32,
    pub filler_scale: f32,
    /// Pass-B lock: minimum score per hypothesis word.
    pub track_score_per_word: f32,
    /// Pass-B lock: minimum absolute score.
    pub min_track_score: f32,
    /// Held hypotheses before HOLDING becomes LOST.
    pub lost_after: u32,
    /// Consecutive behind-votes required to accept a backtrack.
    pub backtrack_confirm: u32,
    /// Dead band: a target this close behind the cursor is overshoot
    /// jitter, not a reread.
    pub backtrack_min_tokens: usize,
    /// Max tokens the cursor may advance on a single event.
    pub max_forward_jump: usize,
    /// Extra retained words beyond the window size.
    pub stream_margin_words: usize,
    /// Two identical keys this close in time are one utterance.
    pub dedupe_window_s: f32,
    /// How many trailing stream entries the dedupe check scans.
    pub dedupe_lookback: usize,
    /// Drop single-char keys at ingest (ASR noise).
    pub drop_single_char: bool,
    /// Advances need at least this many content pairs.
    pub min_content_pairs: usize,
}

impl Default for TrackerConfig {
    fn default() -> TrackerConfig {
        TrackerConfig {
            window_back: 60,
            window_ahead: 90,
            window_growth: 30,
            window_back_max: 400,
            window_ahead_max: 400,
            min_sim: 0.60,
            skip_word: 1.0,
            skip_token_content: 1.0,
            skip_token_filler: 0.35,
            filler_scale: 0.5,
            track_score_per_word: 0.35,
            min_track_score: 1.2,
            lost_after: 12,
            backtrack_confirm: 2,
            backtrack_min_tokens: 6,
            max_forward_jump: 120,
            stream_margin_words: 24,
            dedupe_window_s: 0.35,
            dedupe_lookback: 64,
            drop_single_char: true,
            min_content_pairs: 2,
        }
    }
}

impl TrackerConfig {
    fn match_params(&self) -> MatchParams {
        MatchParams {
            min_sim: self.min_sim,
            skip_word: self.skip_word,
            skip_token_content: self.skip_token_content,
            skip_token_filler: self.skip_token_filler,
            filler_scale: self.filler_scale,
        }
    }
}

/// Diagnostics for the last feed, captured only when enabled.
#[derive(Debug, Clone, Default)]
pub struct LastAlignment {
    pub pairs: Vec<(usize, usize)>,
    pub end_pos: usize,
    pub n_content_pairs: usize,
}

pub struct Tracker {
    script: Script,
    cfg: TrackerConfig,
    cursor: usize,
    state: State,
    /// Bounded retained word stream; `stream_base` is the absolute index of
    /// `stream[0]`. All anchors below are absolute.
    stream: VecDeque<(String, f32)>,
    stream_base: usize,
    /// Max word timestamp ever ingested (freshness boundary).
    max_t: f32,
    held: u32,
    ahead: usize,
    back: usize,
    scan_reach: usize,
    backtrack_votes: u32,
    /// Absolute stream index of the first word heard after the last
    /// accepted position; while LOST, pass A only sees the stream from here.
    lost_anchor: usize,
    scratch: Scratch,
    diagnostics: bool,
    last_alignment: Option<LastAlignment>,
    pub n_hypotheses: u64,
    pub n_words: u64,
}

impl Tracker {
    pub fn new(script: Script, cfg: TrackerConfig) -> Tracker {
        Tracker {
            script,
            cfg,
            cursor: 0,
            state: State::Tracking,
            stream: VecDeque::new(),
            stream_base: 0,
            max_t: f32::NEG_INFINITY,
            held: 0,
            ahead: 0,
            back: 0,
            scan_reach: 0,
            backtrack_votes: 0,
            lost_anchor: 0,
            scratch: Scratch::new(),
            diagnostics: false,
            last_alignment: None,
            n_hypotheses: 0,
            n_words: 0,
        }
    }

    pub fn script(&self) -> &Script {
        &self.script
    }

    pub fn config(&self) -> &TrackerConfig {
        &self.cfg
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn n_tokens(&self) -> usize {
        self.script.n_tokens()
    }

    pub fn percent(&self) -> f32 {
        let n = self.script.n_tokens();
        if n == 0 {
            0.0
        } else {
            100.0 * self.cursor as f32 / n as f32
        }
    }

    /// Display line of the token just before the cursor (the line being read).
    pub fn current_line(&self) -> Option<usize> {
        let tok = self.cursor.checked_sub(1)?;
        self.script.line_of_token(tok)
    }

    pub fn line_text(&self) -> Option<&str> {
        self.script.line_text(self.current_line()?)
    }

    /// Enable capture of per-feed alignment diagnostics (off by default so
    /// steady-state feeds never clone pair vectors nobody reads).
    pub fn set_diagnostics(&mut self, on: bool) {
        self.diagnostics = on;
        if !on {
            self.last_alignment = None;
        }
    }

    pub fn last_alignment(&self) -> Option<&LastAlignment> {
        self.last_alignment.as_ref()
    }

    /// Absolute length of the ingested stream (trimmed entries included).
    fn stream_len(&self) -> usize {
        self.stream_base + self.stream.len()
    }

    /// Feed one hypothesis. Returns an event when the cursor moved or the
    /// state changed; `None` on no news.
    pub fn feed(&mut self, hyp: &Hypothesis) -> Option<TrackerEvent> {
        self.n_hypotheses += 1;
        let n_tok = self.script.n_tokens();
        if n_tok == 0 {
            return None;
        }

        let cursor_before = self.cursor;
        let state_before = self.state;

        // ---- Phase 1: ingest + dedupe -------------------------------------
        // Freshness boundary: strictly newer than anything already ingested.
        let t_cutoff = self.max_t;
        let mut fresh_added = 0usize;
        for w in &hyp.words {
            if w.key.is_empty() {
                continue;
            }
            if self.cfg.drop_single_char && w.key.chars().count() == 1 {
                continue;
            }
            let dup = self
                .stream
                .iter()
                .rev()
                .take(self.cfg.dedupe_lookback)
                .any(|(k, t)| *k == w.key && (w.t - *t).abs() <= self.cfg.dedupe_window_s);
            if dup {
                continue;
            }
            self.stream.push_back((w.key.clone(), w.t));
            self.max_t = self.max_t.max(w.t);
            fresh_added += 1;
            self.n_words += 1;
        }
        let total = self.stream_len();
        let fresh_from = total - fresh_added; // absolute index of first fresh word

        // ---- Phase 2: window ----------------------------------------------
        let lost = self.state == State::Lost;
        let w_back = (self.cfg.window_back + self.back).min(self.cfg.window_back_max)
            + if lost { self.scan_reach } else { 0 };
        let w_ahead = (self.cfg.window_ahead + self.ahead).min(self.cfg.window_ahead_max)
            + if lost { self.scan_reach } else { 0 };
        let start = self.cursor.saturating_sub(w_back);
        let end = (self.cursor + w_ahead).min(n_tok);
        if end <= start {
            return None;
        }

        // Retained slice for pass A: the trailing window's worth of words,
        // additionally anchored at `lost_anchor` while LOST so stale
        // pre-skip audio can't outbid the re-entry chain.
        let keep = (end - start) + self.cfg.stream_margin_words;
        let mut slice_from = total.saturating_sub(keep);
        if lost {
            slice_from = slice_from.max(self.lost_anchor);
        }

        // Trim everything no window can still need: pass A never looks
        // before min(lost_anchor, total - keep_max).
        let keep_max = (self.cfg.window_back_max + self.cfg.window_ahead_max + n_tok)
            .min(2 * n_tok)
            + self.cfg.stream_margin_words;
        let retain_from = self.lost_anchor.min(total.saturating_sub(keep_max));
        while self.stream_base < retain_from {
            self.stream.pop_front();
            self.stream_base += 1;
        }

        // ---- Phase 3: pass A — anchored advance ---------------------------
        let params = self.cfg.match_params();
        // Split field borrows: word keys borrow `stream`, the DP mutates
        // `scratch`, tokens borrow `script`.
        let al = {
            let stream = &self.stream;
            let base = self.stream_base;
            let word_keys: Vec<&str> = (slice_from..total)
                .filter_map(|abs| stream.get(abs - base))
                .map(|(k, _)| k.as_str())
                .collect();
            align_window(
                &word_keys,
                self.script.tokens(),
                start,
                end,
                &params,
                &mut self.scratch,
            )
        };

        if self.diagnostics {
            self.last_alignment = Some(LastAlignment {
                pairs: al.pairs.clone(),
                end_pos: al.end_pos,
                n_content_pairs: al.n_content_pairs,
            });
        }

        let has_fresh_match = al
            .pairs
            .iter()
            .any(|&(wi, ti)| slice_from + wi >= fresh_from && ti + 1 > cursor_before);
        let accept_advance = has_fresh_match
            && al.ends_on_content
            && al.n_content_pairs >= self.cfg.min_content_pairs
            && al.end_pos > self.cursor;

        if accept_advance {
            let target = al.end_pos;
            let clamped = target > self.cursor + self.cfg.max_forward_jump;
            self.cursor = target.min(self.cursor + self.cfg.max_forward_jump);
            self.backtrack_votes = 0;
            if lost && clamped {
                // Re-acquisition staircase: keep LOST, keep the scan window
                // and anchor; each event moves at most max_forward_jump.
            } else {
                self.state = State::Tracking;
                self.held = 0;
                self.ahead = 0;
                self.back = 0;
                self.scan_reach = 0;
                self.lost_anchor = self.stream_len();
            }
            return self.event(hyp.t, cursor_before, state_before, al.score);
        }

        // ---- Phase 4: hold ------------------------------------------------
        if self.stream.is_empty() {
            // Nothing heard yet: idle, not holding.
            return None;
        }
        self.held += 1;
        self.ahead = (self.ahead + self.cfg.window_growth).min(
            self.cfg
                .window_ahead_max
                .saturating_sub(self.cfg.window_ahead),
        );
        self.back = (self.back + self.cfg.window_growth).min(
            self.cfg
                .window_back_max
                .saturating_sub(self.cfg.window_back),
        );
        if lost {
            self.scan_reach = if self.scan_reach == 0 {
                self.cfg.window_ahead.max(self.cfg.window_back)
            } else {
                (self.scan_reach * 2).min(n_tok)
            };
        }
        self.state = if lost || self.held >= self.cfg.lost_after {
            State::Lost
        } else {
            State::Holding
        };

        // ---- Phase 5: pass B — backtrack voting ---------------------------
        let hyp_keys: Vec<&str> = hyp
            .words
            .iter()
            .filter(|w| {
                !w.key.is_empty() && !(self.cfg.drop_single_char && w.key.chars().count() == 1)
            })
            .map(|w| w.key.as_str())
            .collect();
        if hyp_keys.len() >= 2 {
            let al_b = align_window(
                &hyp_keys,
                self.script.tokens(),
                start,
                end,
                &params,
                &mut self.scratch,
            );
            // Fresh pairs: matched words strictly newer than everything
            // ingested before this feed (a re-emitted last word is stale).
            let usable: Vec<&Word> = hyp
                .words
                .iter()
                .filter(|w| {
                    !w.key.is_empty() && !(self.cfg.drop_single_char && w.key.chars().count() == 1)
                })
                .collect();
            let fresh_pairs = al_b
                .pairs
                .iter()
                .filter(|&&(wi, _)| usable.get(wi).is_some_and(|w| w.t > t_cutoff))
                .count();
            let len = hyp_keys.len() as f32;
            let locked = al_b.pairs.len() >= 2
                && fresh_pairs >= self.cfg.min_content_pairs
                && al_b.score / len >= self.cfg.track_score_per_word
                && al_b.score >= self.cfg.min_track_score
                && al_b.ends_on_content
                && al_b.n_content_pairs >= self.cfg.min_content_pairs;
            let behind = al_b.end_pos + self.cfg.backtrack_min_tokens < cursor_before;
            if locked && behind {
                self.backtrack_votes += 1;
                if self.backtrack_votes >= self.cfg.backtrack_confirm {
                    self.cursor = al_b.end_pos;
                    self.state = State::Backtrack;
                    self.backtrack_votes = 0;
                    self.held = 0;
                    self.ahead = 0;
                    self.back = 0;
                    self.scan_reach = 0;
                    self.lost_anchor = self.stream_len();
                    return self.event(hyp.t, cursor_before, state_before, al_b.score);
                }
            } else {
                self.backtrack_votes = 0;
            }
        }

        self.event(hyp.t, cursor_before, state_before, 0.0)
    }

    /// Flush end-of-session bookkeeping (placeholder for symmetry; the
    /// tracker itself is stateless across feeds beyond its fields).
    pub fn finish(&mut self) {}

    fn event(
        &self,
        t: f32,
        cursor_before: usize,
        state_before: State,
        score: f32,
    ) -> Option<TrackerEvent> {
        if self.cursor == cursor_before && self.state == state_before {
            return None;
        }
        Some(TrackerEvent {
            t,
            state: self.state,
            cursor: self.cursor,
            score,
            jump: self.cursor as i64 - cursor_before as i64,
            held_for: self.held,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic vocabulary where only exact keys match.
    fn vocab_word(i: usize) -> String {
        format!("palavra{i:04}")
    }

    fn synth_script(n: usize) -> Script {
        let text = (0..n).map(vocab_word).collect::<Vec<_>>().join(" ");
        Script::parse(&text)
    }

    fn strict_cfg() -> TrackerConfig {
        TrackerConfig {
            min_sim: 0.95,
            ..TrackerConfig::default()
        }
    }

    fn hyp_at(t: f32, words: &[String]) -> Hypothesis {
        Hypothesis {
            words: words
                .iter()
                .enumerate()
                .map(|(i, w)| Word::new(w.clone(), t + i as f32 * 0.3))
                .collect(),
            t,
        }
    }

    /// Feed a contiguous run of script words in chunks of `chunk`.
    fn read_run(tr: &mut Tracker, from: usize, to: usize, chunk: usize, t0: f32) -> f32 {
        let mut t = t0;
        let mut i = from;
        while i < to {
            let hi = (i + chunk).min(to);
            let words: Vec<String> = (i..hi).map(vocab_word).collect();
            tr.feed(&hyp_at(t, &words));
            t += chunk as f32 * 0.3;
            i = hi;
        }
        t
    }

    #[test]
    fn tracks_clean_reading() {
        let mut tr = Tracker::new(synth_script(60), strict_cfg());
        read_run(&mut tr, 0, 60, 4, 0.0);
        assert_eq!(tr.cursor(), 60);
        assert_eq!(tr.state(), State::Tracking);
    }

    #[test]
    fn holds_on_adlib_then_recovers() {
        let mut tr = Tracker::new(synth_script(80), strict_cfg());
        let t = read_run(&mut tr, 0, 20, 4, 0.0);
        let at_20 = tr.cursor();
        assert!(at_20 >= 18);
        // Ad-lib: off-script words hold position (no creep).
        let mut t = t;
        for _ in 0..5 {
            let adlib: Vec<String> = (0..4).map(|i| format!("improviso{i}")).collect();
            tr.feed(&hyp_at(t, &adlib));
            t += 1.2;
        }
        assert_eq!(tr.cursor(), at_20, "ad-lib must not move the cursor");
        assert!(matches!(tr.state(), State::Holding | State::Lost));
        // Resume reading where we left off.
        read_run(&mut tr, 20, 40, 4, t + 1.0);
        assert_eq!(tr.cursor(), 40);
        assert_eq!(tr.state(), State::Tracking);
    }

    #[test]
    fn silence_holds_position() {
        let mut tr = Tracker::new(synth_script(40), strict_cfg());
        read_run(&mut tr, 0, 12, 4, 0.0);
        let pos = tr.cursor();
        // Empty hypotheses (silence): no state churn, no movement.
        for k in 0..10 {
            let ev = tr.feed(&Hypothesis {
                words: vec![],
                t: 10.0 + k as f32,
            });
            assert!(ev.is_none() || ev.unwrap().cursor == pos);
        }
        assert_eq!(tr.cursor(), pos);
    }

    #[test]
    fn backtrack_needs_two_confirmations() {
        let mut tr = Tracker::new(synth_script(120), strict_cfg());
        let t = read_run(&mut tr, 0, 60, 4, 0.0);
        assert_eq!(tr.cursor(), 60);
        // Reread from token 20: first behind-vote must NOT move the cursor.
        let reread: Vec<String> = (20..26).map(vocab_word).collect();
        tr.feed(&hyp_at(t + 1.0, &reread));
        assert_eq!(tr.cursor(), 60, "one vote must not backtrack");
        // Second consecutive vote confirms.
        let reread2: Vec<String> = (26..32).map(vocab_word).collect();
        let ev = tr.feed(&hyp_at(t + 3.0, &reread2)).expect("event");
        assert_eq!(ev.state, State::Backtrack);
        assert!(tr.cursor() < 60, "cursor moved back, got {}", tr.cursor());
        assert!(ev.jump < 0);
    }

    #[test]
    fn filler_hallucination_does_not_creep() {
        // Script full of fillers ahead; hallucinated filler runs must not
        // advance the cursor (fillers never count as content pairs).
        let s = Script::parse("marco um que de a o que de a o que de a o marco dois");
        let mut tr = Tracker::new(s, TrackerConfig::default());
        for k in 0..8 {
            let words: Vec<String> = vec!["que".into(), "de".into(), "a".into(), "o".into()];
            tr.feed(&hyp_at(k as f32 * 1.3, &words));
        }
        assert_eq!(tr.cursor(), 0, "filler-only audio must not advance");
    }

    #[test]
    fn big_skip_reacquires_via_lost() {
        let n = 2000;
        let mut tr = Tracker::new(synth_script(n), strict_cfg());
        let t = read_run(&mut tr, 0, 100, 4, 0.0);
        assert_eq!(tr.cursor(), 100);
        // Jump to token 1000 — far outside every base window.
        let mut t = t + 2.0;
        let mut reacquired = false;
        let mut i = 1000;
        for _ in 0..200 {
            let hi = (i + 4).min(n);
            let words: Vec<String> = (i..hi).map(vocab_word).collect();
            tr.feed(&hyp_at(t, &words));
            t += 1.2;
            i = hi;
            if tr.state() == State::Tracking && tr.cursor() >= 1000 {
                reacquired = true;
                break;
            }
            if i >= n {
                i = 1000; // loop the passage until the scan window reaches it
            }
        }
        assert!(
            reacquired,
            "must exit LOST and land after the skip target; state={:?} cursor={}",
            tr.state(),
            tr.cursor()
        );
    }

    #[test]
    fn repeated_phrase_backtracks_to_nearest_occurrence() {
        // The same phrase at tokens 10, 70 and 130; a reread while the
        // cursor sits at ~140 must resolve to the occurrence nearest the
        // cursor (130), not teleport to the first one.
        let mut words: Vec<String> = (0..160).map(vocab_word).collect();
        for base in [10usize, 70, 130] {
            for (k, w) in ["frase", "repetida", "especial", "aqui"].iter().enumerate() {
                words[base + k] = (*w).to_string();
            }
        }
        let script = Script::parse(&words.join(" "));
        let mut tr = Tracker::new(script, strict_cfg());
        let mut t = 0.0;
        for i in (0..140).step_by(4) {
            let chunk: Vec<String> = words[i..i + 4].to_vec();
            tr.feed(&hyp_at(t, &chunk));
            t += 1.2;
        }
        assert!(tr.cursor() >= 136);
        let cursor_before = tr.cursor();
        // Reread the repeated phrase twice (two votes).
        let phrase: Vec<String> = words[130..136].to_vec();
        tr.feed(&hyp_at(t + 1.0, &phrase));
        let ev = tr.feed(&hyp_at(t + 3.5, &phrase));
        if let Some(ev) = ev {
            if ev.state == State::Backtrack {
                assert!(
                    ev.cursor >= 130 && ev.cursor < cursor_before,
                    "backtrack landed at {} — must be the nearest occurrence",
                    ev.cursor
                );
            }
        }
        assert!(tr.cursor() >= 130, "never teleport to an early occurrence");
    }

    #[test]
    fn degenerate_scripts_never_panic() {
        for text in ["", "uma", "— — —", "23"] {
            let mut tr = Tracker::new(Script::parse(text), TrackerConfig::default());
            for k in 0..5 {
                let words: Vec<String> = vec!["qualquer".into(), "coisa".into()];
                let _ = tr.feed(&hyp_at(k as f32, &words));
            }
            let _ = tr.percent();
            let _ = tr.current_line();
            let _ = tr.line_text();
        }
    }

    #[test]
    fn stream_stays_bounded() {
        let n = 200;
        let cfg = TrackerConfig {
            window_back_max: 100,
            window_ahead_max: 100,
            ..strict_cfg()
        };
        let mut tr = Tracker::new(synth_script(n), cfg);
        // Long session: read the script over and over with unique times.
        let mut t = 0.0;
        for lap in 0..8 {
            for i in (0..n).step_by(4) {
                let hi = (i + 4).min(n);
                let words: Vec<String> = (i..hi).map(vocab_word).collect();
                tr.feed(&hyp_at(t + lap as f32 * 1000.0, &words));
                t += 1.2;
            }
        }
        let bound = 2 * n
            + tr.config().stream_margin_words
            + tr.config().window_back_max
            + tr.config().window_ahead_max;
        assert!(
            tr.stream.len() <= bound,
            "retained stream {} exceeds bound {bound}",
            tr.stream.len()
        );
        assert!(tr.n_words > 1000, "ingested plenty of words");
    }

    #[test]
    fn cursor_monotone_except_backtrack() {
        let mut tr = Tracker::new(synth_script(200), strict_cfg());
        let mut cursor = 0usize;
        let mut t = 0.0;
        let mut check = |tr: &mut Tracker, words: &[String], t: f32| {
            if let Some(ev) = tr.feed(&hyp_at(t, words)) {
                if ev.state != State::Backtrack {
                    assert!(ev.cursor >= cursor, "non-backtrack regression");
                }
                cursor = ev.cursor;
            }
        };
        for i in (0..120).step_by(4) {
            let words: Vec<String> = (i..i + 4).map(vocab_word).collect();
            check(&mut tr, &words, t);
            t += 1.2;
        }
        // Interleave ad-libs, rereads, and resumes.
        for k in 0..6 {
            let adlib: Vec<String> = (0..3).map(|i| format!("solto{k}x{i}")).collect();
            check(&mut tr, &adlib, t);
            t += 1.2;
        }
        for i in (60..80).step_by(4) {
            let words: Vec<String> = (i..i + 4).map(vocab_word).collect();
            check(&mut tr, &words, t);
            t += 1.2;
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn tracker_event_serde_roundtrip() {
        let ev = TrackerEvent {
            t: 12.5,
            state: State::Backtrack,
            cursor: 42,
            score: 3.25,
            jump: -6,
            held_for: 2,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let back: TrackerEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, back);
        let h = Hypothesis {
            words: vec![Word::new("Olá", 1.0)],
            t: 1.5,
        };
        let json = serde_json::to_string(&h).unwrap();
        let back: Hypothesis = serde_json::from_str(&json).unwrap();
        assert_eq!(h, back);
    }
}
