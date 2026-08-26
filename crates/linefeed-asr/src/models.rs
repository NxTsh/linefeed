//! The model registry: every downloadable ASR model linefeed knows about.
//! Single source of truth for the CLI (`--model`), the GUI model picker,
//! and the first-run downloader. Sizes and archive layouts were verified
//! against the live release assets — do not guess new entries, list the
//! actual tarball first (its entries may or may not be `./`-prefixed and
//! must contain `model.int8.onnx` + `tokens.txt` at the top level).

use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub struct ModelSpec {
    /// Stable id used in config files and CLI flags ("pt-br", "en").
    pub id: &'static str,
    /// Human label for pickers.
    pub label: &'static str,
    /// BCP-47-ish language tag.
    pub lang: &'static str,
    /// Directory name inside the models dir == the archive's top-level dir.
    pub dirname: &'static str,
    pub url: &'static str,
    /// Exact archive size (Content-Length), for progress math.
    pub archive_bytes: u64,
    /// Files that must exist for the model to count as installed.
    pub files: &'static [&'static str],
}

pub const DEFAULT_MODEL_ID: &str = "pt-br";

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "pt-br",
        label: "Português (Brasil) — NeMo FastConformer large",
        lang: "pt-BR",
        dirname: "sherpa-onnx-nemo-stt_pt_fastconformer_hybrid_large_pc-int8",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-stt_pt_fastconformer_hybrid_large_pc-int8.tar.bz2",
        archive_bytes: 103_316_407,
        files: &["model.int8.onnx", "tokens.txt"],
    },
    ModelSpec {
        id: "en",
        label: "English — NeMo Conformer medium",
        lang: "en",
        dirname: "sherpa-onnx-nemo-ctc-en-conformer-medium",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-ctc-en-conformer-medium.tar.bz2",
        archive_bytes: 165_685_608,
        files: &["model.int8.onnx", "tokens.txt"],
    },
];

pub fn model_spec(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.id == id)
}

/// The spec for a config value, falling back to the default model.
pub fn model_spec_or_default(id: &str) -> &'static ModelSpec {
    model_spec(id).unwrap_or_else(|| model_spec(DEFAULT_MODEL_ID).expect("default model exists"))
}

/// Install directory of a model under [`crate::models_dir`].
pub fn model_dir(spec: &ModelSpec) -> PathBuf {
    crate::models_dir().join(spec.dirname)
}

/// Required files that are missing (empty == installed).
pub fn missing_files(spec: &ModelSpec) -> Vec<String> {
    let dir = model_dir(spec);
    spec.files
        .iter()
        .filter(|f| !dir.join(f).exists())
        .map(|f| format!("{}/{f}", spec.dirname))
        .collect()
}

pub fn model_installed(spec: &ModelSpec) -> bool {
    missing_files(spec).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_consistent() {
        assert!(model_spec(DEFAULT_MODEL_ID).is_some());
        let mut ids: Vec<&str> = MODELS.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), MODELS.len(), "model ids must be unique");
        for m in MODELS {
            assert!(m.url.ends_with(".tar.bz2"));
            assert!(m.url.contains(m.dirname), "dirname must match the asset");
            assert!(m.archive_bytes > 10_000_000);
            assert!(m.files.contains(&"model.int8.onnx"));
            assert!(m.files.contains(&"tokens.txt"));
        }
    }

    #[test]
    fn unknown_id_falls_back_to_default() {
        assert_eq!(model_spec_or_default("klingon").id, DEFAULT_MODEL_ID);
        assert_eq!(model_spec_or_default("en").id, "en");
    }
}
