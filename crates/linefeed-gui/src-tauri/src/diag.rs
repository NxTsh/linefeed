//! Opt-in per-session JSONL diagnostics, written next to the script
//! (temp-dir fallback): `session_start`, one `tracker` line per event, a
//! `mic_rms` line per stream-second, `session_end`. Settings are snapshotted
//! at session start so a mid-session toggle can't race the file.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use linefeed_core::TrackerEvent;

pub struct DiagLog {
    out: std::io::BufWriter<std::fs::File>,
    pub path: PathBuf,
}

impl DiagLog {
    /// `script_path` decides the directory; the file is
    /// `linefeed-diag-<epoch>.jsonl`.
    pub fn create(script_path: &Path, engine: &str, device: &str) -> Option<DiagLog> {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        let dir = script_path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("linefeed-diag-{epoch}.jsonl"));
        let file = std::fs::File::create(&path)
            .or_else(|_| {
                std::fs::File::create(
                    std::env::temp_dir().join(format!("linefeed-diag-{epoch}.jsonl")),
                )
            })
            .ok()?;
        let mut log = DiagLog {
            out: std::io::BufWriter::new(file),
            path,
        };
        log.write(serde_json::json!({
            "kind": "session_start",
            "engine": engine,
            "device": device,
            "script": script_path.to_string_lossy(),
        }));
        Some(log)
    }

    fn write(&mut self, v: serde_json::Value) {
        let _ = writeln!(self.out, "{v}");
    }

    pub fn tracker(&mut self, ev: &TrackerEvent, hyp_tail: &[String]) {
        self.write(serde_json::json!({
            "kind": "tracker",
            "t": ev.t,
            "state": ev.state.as_str(),
            "cursor": ev.cursor,
            "score": ev.score,
            "jump": ev.jump,
            "hyp_tail": hyp_tail,
        }));
    }

    pub fn mic_rms(&mut self, t: f32, rms: f32) {
        self.write(serde_json::json!({ "kind": "mic_rms", "t": t, "rms": rms }));
    }

    pub fn session_end(&mut self, cursor: usize, n_tokens: usize) {
        self.write(serde_json::json!({
            "kind": "session_end",
            "cursor": cursor,
            "n_tokens": n_tokens,
        }));
        let _ = self.out.flush();
    }
}

/// One multiply-add per sample; reports once per whole second of stream.
pub struct RmsMeter {
    sum_sq: f64,
    n: u64,
    t: f32,
    last_report: f32,
}

impl Default for RmsMeter {
    fn default() -> RmsMeter {
        RmsMeter {
            sum_sq: 0.0,
            n: 0,
            t: 0.0,
            last_report: 0.0,
        }
    }
}

impl RmsMeter {
    /// Feed samples; returns `Some((t, rms))` once per stream-second.
    pub fn feed(&mut self, samples: &[f32], sample_rate: u32) -> Option<(f32, f32)> {
        for s in samples {
            self.sum_sq += (*s as f64) * (*s as f64);
        }
        self.n += samples.len() as u64;
        self.t += samples.len() as f32 / sample_rate as f32;
        if self.t - self.last_report >= 1.0 && self.n > 0 {
            let rms = (self.sum_sq / self.n as f64).sqrt() as f32;
            self.last_report = self.t;
            self.sum_sq = 0.0;
            self.n = 0;
            return Some((self.t, rms));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_reports_once_per_second() {
        let mut m = RmsMeter::default();
        let block = vec![0.5f32; 1600]; // 100 ms at 16 kHz
        let mut reports = 0;
        for _ in 0..25 {
            if let Some((_, rms)) = m.feed(&block, 16000) {
                assert!((rms - 0.5).abs() < 1e-3);
                reports += 1;
            }
        }
        assert_eq!(reports, 2, "2.5 s of audio → 2 whole-second reports");
    }

    #[test]
    fn diag_log_writes_jsonl() {
        let script = std::env::temp_dir().join("lf-nxt-diag/script.txt");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, "olá").unwrap();
        let mut log = DiagLog::create(&script, "sherpa", "default").unwrap();
        log.mic_rms(1.0, 0.25);
        log.session_end(10, 100);
        let content = std::fs::read_to_string(&log.path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        for l in lines {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            assert!(v["kind"].is_string());
        }
        std::fs::remove_dir_all(script.parent().unwrap()).ok();
    }
}
