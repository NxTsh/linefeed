//! linefeed-core — pure script/speech alignment for the linefeed teleprompter.
//!
//! Pipeline: [`Script::parse`] normalizes the script into a token stream;
//! ASR word hypotheses are matched onto a token window by the DP in
//! [`matcher`]; [`Tracker`] turns accepted alignments into a monotone cursor
//! with a four-state machine (TRACKING / HOLDING / LOST / BACKTRACK).
//!
//! Design-around note (deliberate, keep it): alignment operates on
//! normalized token indices only — never font size, visible-word counts, or
//! any rendering metric. That keeps the core engine-agnostic (any ASR that
//! emits word hypotheses works) and renderer-agnostic (CLI line numbers or
//! GUI visual lines).
//!
//! This crate has no audio and no UI dependencies, must not panic on bad
//! input, and is dependency-free by default (`serde` is opt-in for GUI IPC).

pub mod matcher;
pub mod script;
pub mod tracker;

pub use matcher::{align_window, similarity, Alignment, MatchParams, Scratch};
pub use script::{expand_word, is_filler, normalize_word, Paragraph, Script, ScriptToken};
pub use tracker::{Hypothesis, State, Tracker, TrackerConfig, TrackerEvent, Word};
