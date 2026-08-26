//! sherpa-onnx engine: NeMo FastConformer CTC (offline) with trailing-window
//! re-decode streaming emulation.
//!
//! Why offline: the sherpa-onnx 1.13.x pt exports are rejected by the
//! online/streaming API (missing ONNX metadata), so streaming is emulated —
//! every [`HOP_S`] seconds the trailing [`WINDOW_S`] seconds of retained
//! audio are re-decoded from scratch. The tracker's dedupe stage reconciles
//! the ~11 s of overlap between consecutive hypotheses.
//!
//! The nemo_ctc result carries no assembled text or word list — words are
//! rebuilt from BPE token pieces + timestamps (a piece with a leading space,
//! or one following a whitespace-only piece, starts a new word).
//!
//! Timestamp contract: word times are anchored to the ACTUAL start of the
//! decoded segment, clamped to the retained buffer — a request that predates
//! the buffer shifts the anchor forward with the data instead of silently
//! shifting every word time (a latent bug in the first implementation).

use crate::engine::{AsrEngine, EngineConfig, Error, Hypothesis, Word};
use sherpa_onnx::{
    OfflineModelConfig, OfflineNemoEncDecCtcModelConfig, OfflineRecognizer,
    OfflineRecognizerConfig,
};

/// Seconds between re-decodes. Cost scales ~1/hop; ~0.22 RTF-equivalent at
/// 4 threads was measured for the 1 s hop in the first implementation.
pub const HOP_S: f32 = 1.0;

/// Trailing re-decode window, seconds. Shorter = fresher context per decode
/// (stale leading words can outbid fresh ones near the cursor). NOTE: the
/// tracker's base backward window (60 tokens ≈ 20 s of speech) exceeds this;
/// deep rereads rely on being re-fed by subsequent hops.
pub const WINDOW_S: f32 = 12.0;

pub struct SherpaEngine {
    recognizer: OfflineRecognizer,
    /// Retained samples (amortized trim keeps 13–26 s; not a ring buffer).
    audio: Vec<f32>,
    /// Absolute sample offset of `audio[0]` in the stream.
    audio_start_sample: usize,
    sample_rate: u32,
    last_emit: f32,
    t_fed: f32,
}

impl SherpaEngine {
    pub fn new(cfg: &EngineConfig) -> Result<SherpaEngine, Error> {
        let model_path = std::path::Path::new(&cfg.model_dir).join("model.int8.onnx");
        let tokens_path = std::path::Path::new(&cfg.model_dir).join("tokens.txt");
        for (what, p) in [("model", &model_path), ("tokens", &tokens_path)] {
            if !p.exists() {
                return Err(Error::ModelMissing(format!("{what}: {}", p.display())));
            }
        }
        let mut config = OfflineRecognizerConfig::default();
        config.model_config = OfflineModelConfig {
            nemo_ctc: OfflineNemoEncDecCtcModelConfig {
                model: Some(model_path.to_string_lossy().into_owned()),
            },
            tokens: Some(tokens_path.to_string_lossy().into_owned()),
            num_threads: cfg.num_threads,
            debug: false,
            ..Default::default()
        };
        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| Error::Engine("sherpa recognizer init failed".into()))?;
        Ok(SherpaEngine {
            recognizer,
            audio: Vec::new(),
            audio_start_sample: 0,
            sample_rate: cfg.sample_rate,
            last_emit: 0.0,
            t_fed: 0.0,
        })
    }

    /// Decode the absolute stream range `[start_s, end_s)`, clamped to the
    /// retained buffer, and rebuild words with times anchored to the actual
    /// clamped segment start.
    fn decode_words(&self, start_s: f32, end_s: f32) -> Result<Vec<(String, f32)>, Error> {
        let sr = self.sample_rate as usize;
        let abs_start = ((start_s * sr as f32) as usize).max(self.audio_start_sample);
        let abs_end = (end_s * sr as f32) as usize;
        let lo = abs_start - self.audio_start_sample;
        let hi = abs_end
            .saturating_sub(self.audio_start_sample)
            .min(self.audio.len());
        if hi.saturating_sub(lo) < sr / 4 {
            return Ok(Vec::new());
        }
        // Anchor = the ACTUAL first decoded sample, not the requested start.
        let t0 = abs_start as f32 / sr as f32;
        let seg = &self.audio[lo..hi];
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(self.sample_rate as i32, seg);
        self.recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or_else(|| Error::Engine("sherpa decode: no result".into()))?;
        Ok(rebuild_words(&result.tokens, result.timestamps.as_deref(), t0))
    }

    fn maybe_emit(&mut self) -> Result<Vec<Hypothesis>, Error> {
        let mut out = Vec::new();
        while self.last_emit + HOP_S <= self.t_fed + 1e-9 {
            let next_emit = self.last_emit + HOP_S;
            let w_start = (next_emit - WINDOW_S).max(0.0);
            let words: Vec<Word> = self
                .decode_words(w_start, next_emit)?
                .into_iter()
                .filter(|(w, _)| !w.trim().is_empty())
                .map(|(w, t)| Word::new(w, t))
                .collect();
            out.push(Hypothesis {
                words,
                t: next_emit,
            });
            self.last_emit = next_emit;
        }
        Ok(out)
    }

    /// Amortized front-trim: retain WINDOW_S + 1 s; drain only when the
    /// buffer doubles, so the memmove happens at most once per ~13 s.
    fn trim_audio(&mut self) {
        let keep = (WINDOW_S * self.sample_rate as f32) as usize + self.sample_rate as usize;
        if self.audio.len() > keep * 2 {
            let cut = self.audio.len() - keep;
            self.audio.drain(0..cut);
            self.audio_start_sample += cut;
        }
    }
}

/// Rebuild word strings + start times from BPE token pieces.
fn rebuild_words(tokens: &[String], timestamps: Option<&[f32]>, t0: f32) -> Vec<(String, f32)> {
    let tss = timestamps.unwrap_or(&[]);
    let mut words: Vec<(String, f32)> = Vec::new();
    let mut cur = String::new();
    let mut cur_t: Option<f32> = None;
    for (i, tok) in tokens.iter().enumerate() {
        let ts = tss.get(i).copied().unwrap_or(0.0);
        if tok.trim().is_empty() {
            if !cur.is_empty() {
                words.push((std::mem::take(&mut cur), cur_t.unwrap_or(t0)));
                cur_t = None;
            }
            continue;
        }
        if tok.starts_with(' ') && !cur.is_empty() {
            words.push((std::mem::take(&mut cur), cur_t.unwrap_or(t0)));
            cur_t = None;
        }
        if cur_t.is_none() {
            cur_t = Some(t0 + ts);
        }
        cur.push_str(tok.trim());
    }
    if !cur.is_empty() {
        words.push((cur, cur_t.unwrap_or(t0)));
    }
    words
}

impl AsrEngine for SherpaEngine {
    fn name(&self) -> &'static str {
        "sherpa"
    }

    fn feed(&mut self, samples: &[f32]) -> Result<Vec<Hypothesis>, Error> {
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        self.audio.extend_from_slice(samples);
        self.t_fed += samples.len() as f32 / self.sample_rate as f32;
        let hyps = self.maybe_emit()?;
        self.trim_audio();
        Ok(hyps)
    }

    fn flush(&mut self) -> Result<Vec<Hypothesis>, Error> {
        // Final decode to the true end of stream; overlaps the last periodic
        // emission by design — the tracker's dedupe absorbs it.
        let end = self.t_fed;
        let w_start = (end - WINDOW_S).max(0.0);
        let words: Vec<Word> = self
            .decode_words(w_start, end)?
            .into_iter()
            .filter(|(w, _)| !w.trim().is_empty())
            .map(|(w, t)| Word::new(w, t))
            .collect();
        if words.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(vec![Hypothesis { words, t: end }])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuild_words_bpe_boundaries() {
        let toks: Vec<String> = ["Bo", "m", " di", "a", " pes", "soal"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let ts = [0.0, 0.05, 0.2, 0.28, 0.45, 0.55];
        let words = rebuild_words(&toks, Some(&ts), 5.0);
        assert_eq!(words.len(), 3);
        assert_eq!(words[0], ("Bom".to_string(), 5.0));
        assert_eq!(words[1], ("dia".to_string(), 5.2));
        assert_eq!(words[2], ("pessoal".to_string(), 5.45));
    }

    #[test]
    fn rebuild_words_whitespace_token_is_boundary() {
        let toks: Vec<String> = ["Oi", " ", "gente"].iter().map(|s| s.to_string()).collect();
        let ts = [0.0, 0.1, 0.2];
        let words = rebuild_words(&toks, Some(&ts), 0.0);
        assert_eq!(words.len(), 2);
        assert_eq!(words[1], ("gente".to_string(), 0.2));
    }

    /// Real-model streaming test: runs only when LINEFEED_MODELS_DIR points
    /// at a directory containing the sherpa pt-BR model and LINEFEED_TEST_WAV
    /// names a 16 kHz mono WAV. Prints SKIP otherwise (models are not in git).
    #[test]
    fn streams_wav_and_produces_words() {
        let Ok(models) = std::env::var("LINEFEED_MODELS_DIR") else {
            eprintln!("SKIP: LINEFEED_MODELS_DIR not set");
            return;
        };
        let dir = std::path::Path::new(&models)
            .join("sherpa-onnx-nemo-stt_pt_fastconformer_hybrid_large_pc-int8");
        if !dir.exists() {
            eprintln!("SKIP: sherpa model not present in {models}");
            return;
        }
        let Ok(wav_path) = std::env::var("LINEFEED_TEST_WAV") else {
            eprintln!("SKIP: LINEFEED_TEST_WAV not set");
            return;
        };
        let mut reader = hound::WavReader::open(&wav_path).expect("open wav");
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.expect("sample") as f32 / 32768.0)
            .collect();
        let mut eng = SherpaEngine::new(&EngineConfig {
            model_dir: dir.to_string_lossy().into_owned(),
            num_threads: 4,
            sample_rate: 16000,
        })
        .expect("engine init");
        let mut n_hyps = 0usize;
        let mut n_words = 0usize;
        for c in samples.chunks(8000) {
            for h in eng.feed(c).expect("feed") {
                n_hyps += 1;
                n_words += h.words.len();
            }
        }
        for h in eng.flush().expect("flush") {
            n_hyps += 1;
            n_words += h.words.len();
        }
        eprintln!("sherpa: {n_hyps} hypotheses, {n_words} word-observations");
        assert!(n_hyps >= 10, "expected ~1 hypothesis per second, got {n_hyps}");
        assert!(n_words > 0, "expected word observations");
    }
}
