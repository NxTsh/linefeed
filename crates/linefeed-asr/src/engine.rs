//! The engine abstraction every recognizer implements.

pub use linefeed_core::{Hypothesis, Word};

/// Engine errors. Library code returns these — it never panics on bad input.
#[derive(Debug)]
pub enum Error {
    /// A required model file is missing (path in the message).
    ModelMissing(String),
    /// The underlying engine failed.
    Engine(String),
    /// Audio capture/processing failed.
    Audio(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ModelMissing(p) => write!(f, "model missing: {p}"),
            Error::Engine(m) => write!(f, "engine error: {m}"),
            Error::Audio(m) => write!(f, "audio error: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// Configuration shared by all engines.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Directory holding this engine's model files.
    pub model_dir: String,
    pub num_threads: i32,
    pub sample_rate: u32,
}

impl Default for EngineConfig {
    fn default() -> EngineConfig {
        EngineConfig {
            model_dir: String::new(),
            num_threads: 4,
            sample_rate: 16000,
        }
    }
}

/// A streaming speech recognizer: 16 kHz mono f32 in, word hypotheses out.
///
/// Engines may emit different hypothesis shapes — cumulative from the
/// utterance start, or a re-decoded trailing-window view. Both are legal:
/// the tracker's dedupe stage reconciles overlap.
pub trait AsrEngine: Send {
    fn name(&self) -> &'static str;

    /// Feed samples; returns zero or more new hypotheses.
    fn feed(&mut self, samples: &[f32]) -> Result<Vec<Hypothesis>, Error>;

    /// Signal end of stream; returns any final hypotheses.
    fn flush(&mut self) -> Result<Vec<Hypothesis>, Error> {
        Ok(Vec::new())
    }
}
