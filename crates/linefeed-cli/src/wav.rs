//! WAV loading: 16 kHz mono, i16 or f32 samples. Errors, never panics.

use anyhow::{bail, Context, Result};

pub fn read_mono_16k(path: &std::path::Path) -> Result<Vec<f32>> {
    let reader =
        hound::WavReader::open(path).with_context(|| format!("open wav {}", path.display()))?;
    let spec = reader.spec();
    if spec.channels != 1 {
        bail!(
            "{}: {} channels — the CLI needs mono (downmix it first)",
            path.display(),
            spec.channels
        );
    }
    if spec.sample_rate != 16000 {
        bail!(
            "{}: {} Hz — the CLI needs 16 kHz (resample it first)",
            path.display(),
            spec.sample_rate
        );
    }
    let samples: Result<Vec<f32>> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => reader
            .into_samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32768.0).map_err(Into::into))
            .collect(),
        (hound::SampleFormat::Float, 32) => reader
            .into_samples::<f32>()
            .map(|s| s.map_err(Into::into))
            .collect(),
        (fmt, bits) => bail!(
            "{}: unsupported sample format {fmt:?}/{bits}-bit (need i16 or f32)",
            path.display()
        ),
    };
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(spec: hound::WavSpec, dir: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(dir);
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        if spec.sample_format == hound::SampleFormat::Float {
            for i in 0..160 {
                w.write_sample(i as f32 / 160.0).unwrap();
            }
        } else {
            for i in 0..160i16 {
                w.write_sample(i * 100).unwrap();
            }
        }
        w.finalize().unwrap();
        path
    }

    #[test]
    fn reads_i16_and_f32() {
        let p = write_wav(
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
            "lf-nxt-i16.wav",
        );
        assert_eq!(read_mono_16k(&p).unwrap().len(), 160);
        let p = write_wav(
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
            "lf-nxt-f32.wav",
        );
        assert_eq!(read_mono_16k(&p).unwrap().len(), 160);
    }

    #[test]
    fn rejects_wrong_rate_and_channels_without_panicking() {
        let p = write_wav(
            hound::WavSpec {
                channels: 1,
                sample_rate: 44100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
            "lf-nxt-441.wav",
        );
        let e = read_mono_16k(&p).unwrap_err().to_string();
        assert!(e.contains("44100"), "{e}");
    }
}
