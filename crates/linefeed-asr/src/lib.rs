//! linefeed-asr — speech recognition engines and microphone capture.
//!
//! [`make_engine`] is the single place feature gates decide which engines
//! exist: downstream binaries compile under every feature combination, and a
//! missing engine is a runtime error, never a compile error.

pub mod engine;
pub mod timeline;

#[cfg(feature = "mic")]
pub mod mic;
#[cfg(feature = "sherpa")]
pub mod sherpa;

pub use engine::{AsrEngine, EngineConfig, Error, Hypothesis, Word};
pub use timeline::{TimelineReplay, TimelineWriter};

use anyhow::bail;

/// Engines compiled into this build, preferred first.
pub fn available_engines() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut v = Vec::new();
    #[cfg(feature = "sherpa")]
    v.push("sherpa");
    v
}

pub fn engine_available(name: &str) -> bool {
    available_engines().contains(&name)
}

/// Recommended thread count per engine — kept here so the CLI and GUI can't
/// drift apart.
pub fn engine_num_threads(name: &str) -> i32 {
    match name {
        "sherpa" => 4,
        _ => 1,
    }
}

/// Construct an engine by name. A name that isn't compiled in returns an
/// error naming the feature to enable.
#[cfg_attr(not(feature = "sherpa"), allow(unused_variables))]
pub fn make_engine(name: &str, cfg: &EngineConfig) -> anyhow::Result<Box<dyn AsrEngine>> {
    match name {
        #[cfg(feature = "sherpa")]
        "sherpa" => Ok(Box::new(sherpa::SherpaEngine::new(cfg)?)),
        #[cfg(not(feature = "sherpa"))]
        "sherpa" => bail!("engine 'sherpa' not built in — rebuild with the `sherpa` feature"),
        other => bail!(
            "unknown engine {other:?} (built in: {})",
            available_engines().join(", ")
        ),
    }
}

/// Resolve the models directory: `LINEFEED_MODELS_DIR` wins; otherwise the
/// platform data dir (`$XDG_DATA_HOME`/`~/.local/share` on Linux,
/// `~/Library/Application Support` on macOS) under `linefeed/models`.
pub fn models_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("LINEFEED_MODELS_DIR") {
        if !dir.trim().is_empty() {
            return std::path::PathBuf::from(dir);
        }
    }
    platform_data_dir().join("linefeed").join("models")
}

fn platform_data_dir() -> std::path::PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home).join("Library/Application Support");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            let p = std::path::PathBuf::from(xdg);
            if p.is_absolute() {
                return p;
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home).join(".local/share");
        }
    }
    std::env::temp_dir()
}

/// Directory name of the sherpa pt-BR model inside the models dir.
pub const SHERPA_PT_MODEL_DIRNAME: &str =
    "sherpa-onnx-nemo-stt_pt_fastconformer_hybrid_large_pc-int8";

/// Default model dir for an engine, under [`models_dir`].
pub fn default_model_dir(engine: &str) -> std::path::PathBuf {
    match engine {
        "sherpa" => models_dir().join(SHERPA_PT_MODEL_DIRNAME),
        other => models_dir().join(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_engine_is_runtime_error() {
        let err = match make_engine("whisper", &EngineConfig::default()) {
            Ok(_) => panic!("whisper must not be constructible"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("unknown engine"));
    }

    #[test]
    fn models_dir_env_override() {
        // Serialize env mutation within this test only.
        let prev = std::env::var_os("LINEFEED_MODELS_DIR");
        std::env::set_var("LINEFEED_MODELS_DIR", "/tmp/somewhere/models");
        assert_eq!(
            models_dir(),
            std::path::PathBuf::from("/tmp/somewhere/models")
        );
        match prev {
            Some(v) => std::env::set_var("LINEFEED_MODELS_DIR", v),
            None => std::env::remove_var("LINEFEED_MODELS_DIR"),
        }
    }

    #[test]
    fn thread_defaults_shared_by_cli_and_gui() {
        assert_eq!(engine_num_threads("sherpa"), 4);
        assert_eq!(engine_num_threads("timeline"), 1);
    }
}
