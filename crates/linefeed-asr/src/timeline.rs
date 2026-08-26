//! Deterministic timeline replay: an [`AsrEngine`] backed by a JSONL file
//! of recorded hypotheses, plus the writer that produces such files.
//!
//! Line format (compatible with the first implementation's dumps):
//! `{"t_fed": <seconds>, "words": [["texto", <t>], ...]}`
//!
//! Replay emits exactly one recorded snapshot per `feed()` call, regardless
//! of the audio passed — deterministic by construction, no models needed.

use std::io::{BufRead, Write as _};

use crate::engine::{AsrEngine, Error, Hypothesis, Word};

pub struct TimelineReplay {
    snapshots: std::vec::IntoIter<Hypothesis>,
}

impl TimelineReplay {
    pub fn from_path(path: &std::path::Path) -> Result<TimelineReplay, Error> {
        let file = std::fs::File::open(path)
            .map_err(|e| Error::Engine(format!("open timeline {}: {e}", path.display())))?;
        let mut snapshots = Vec::new();
        for (ln, line) in std::io::BufReader::new(file).lines().enumerate() {
            let line = line.map_err(|e| Error::Engine(format!("read timeline: {e}")))?;
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(&line)
                .map_err(|e| Error::Engine(format!("timeline line {}: {e}", ln + 1)))?;
            let t = v["t_fed"].as_f64().unwrap_or(0.0) as f32;
            let words = v["words"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|w| {
                            let text = w.get(0)?.as_str()?;
                            let wt = w.get(1)?.as_f64()? as f32;
                            Some(Word::new(text, wt))
                        })
                        .collect()
                })
                .unwrap_or_default();
            snapshots.push(Hypothesis { words, t });
        }
        Ok(TimelineReplay {
            snapshots: snapshots.into_iter(),
        })
    }

    pub fn remaining(&self) -> usize {
        self.snapshots.len()
    }
}

impl AsrEngine for TimelineReplay {
    fn name(&self) -> &'static str {
        "timeline"
    }

    fn feed(&mut self, _samples: &[f32]) -> Result<Vec<Hypothesis>, Error> {
        Ok(self.snapshots.next().into_iter().collect())
    }
}

/// Writes hypotheses in the replayable JSONL format.
pub struct TimelineWriter {
    out: std::io::BufWriter<std::fs::File>,
}

impl TimelineWriter {
    pub fn create(path: &std::path::Path) -> Result<TimelineWriter, Error> {
        let file = std::fs::File::create(path)
            .map_err(|e| Error::Engine(format!("create timeline {}: {e}", path.display())))?;
        Ok(TimelineWriter {
            out: std::io::BufWriter::new(file),
        })
    }

    pub fn write(&mut self, hyp: &Hypothesis) -> Result<(), Error> {
        let words: Vec<serde_json::Value> = hyp
            .words
            .iter()
            .map(|w| serde_json::json!([w.text, w.t]))
            .collect();
        let line = serde_json::json!({ "t_fed": hyp.t, "words": words });
        writeln!(self.out, "{line}").map_err(|e| Error::Engine(format!("write timeline: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_write_then_replay() {
        let dir = std::env::temp_dir().join("linefeed-nxt-timeline-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        {
            let mut w = TimelineWriter::create(&path).unwrap();
            w.write(&Hypothesis {
                words: vec![Word::new("Olá", 0.5), Word::new("pessoal", 0.9)],
                t: 1.0,
            })
            .unwrap();
            w.write(&Hypothesis { words: vec![], t: 2.0 }).unwrap();
        }
        let mut r = TimelineReplay::from_path(&path).unwrap();
        assert_eq!(r.remaining(), 2);
        let h1 = r.feed(&[]).unwrap();
        assert_eq!(h1.len(), 1);
        assert_eq!(h1[0].words.len(), 2);
        assert_eq!(h1[0].words[0].key, "ola");
        assert_eq!(h1[0].t, 1.0);
        let h2 = r.feed(&[]).unwrap();
        assert_eq!(h2.len(), 1);
        assert!(h2[0].words.is_empty());
        assert!(r.feed(&[]).unwrap().is_empty(), "exhausted replay is empty");
        std::fs::remove_file(&path).ok();
    }
}
