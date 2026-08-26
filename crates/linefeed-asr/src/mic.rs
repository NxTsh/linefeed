//! Microphone capture: cpal input → channel selection → 16 kHz resample →
//! bounded sample buffer.
//!
//! Shared verbatim by the CLI and the GUI. Design points:
//!
//! - Config negotiation: real devices refuse a fixed 16 kHz/mono request;
//!   we rank the device's own supported configs (F32 before I16, then the
//!   highest rate ≤ 48 kHz, then fewest channels).
//! - Channel handling is max-RMS SELECTION with hysteresis, not averaging:
//!   averaging scales the one live channel of an N-channel interface by 1/N
//!   (−12 dB on a 4-input interface). Comparison runs in energy space (no
//!   sqrt in the audio callback).
//! - Back-pressure by design: the callback pushes into a BOUNDED buffer;
//!   when the consumer falls behind, the OLDEST samples are dropped and
//!   counted, so latency stays bounded instead of drifting forever. The
//!   consumer drains everything available per read (coalescing).
//!
//! HUMAN-TEST NOTE: the capture path (negotiation, mux, resampler) is
//! validated by offline unit tests; live behavior needs real hardware.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};

use crate::engine::Error;

/// Everything downstream runs at 16 kHz mono.
pub const TARGET_RATE: u32 = 16000;
const MIN_DEVICE_RATE: u32 = 16000;
const MAX_DEVICE_RATE: u32 = 48000;

/// Bounded capture buffer: 30 s of 16 kHz audio (~1.9 MB).
const MAX_BUFFERED_SAMPLES: usize = 30 * TARGET_RATE as usize;

/// Energy ratio ≈ +6 dB a challenger must sustain before we switch channels.
const SWITCH_MARGIN_ENERGY: f32 = 3.98;
/// Blocks (~20 ms each, ≈0.5 s) the challenger must stay ahead.
const STICK_BLOCKS: u32 = 25;
/// Below this mean energy a channel counts as digitally silent.
const SILENCE_ENERGY: f32 = 1e-9;

/// Max-RMS channel selector with hysteresis.
struct ChannelMux {
    channels: usize,
    current: usize,
    challenger: usize,
    challenger_blocks: u32,
    energies: Vec<f32>,
}

impl ChannelMux {
    fn new(channels: usize) -> ChannelMux {
        ChannelMux {
            channels: channels.max(1),
            current: 0,
            challenger: 0,
            challenger_blocks: 0,
            energies: vec![0.0; channels.max(1)],
        }
    }

    /// Pick the live channel for this block and append it to `out`.
    fn extract(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        let ch = self.channels;
        if ch == 1 {
            out.extend_from_slice(interleaved);
            return;
        }
        let frames = interleaved.len() / ch;
        if frames == 0 {
            return;
        }
        self.energies.iter_mut().for_each(|e| *e = 0.0);
        for f in 0..frames {
            for c in 0..ch {
                let s = interleaved[f * ch + c];
                self.energies[c] += s * s;
            }
        }
        for e in self.energies.iter_mut() {
            *e /= frames as f32;
        }

        let best = (0..ch)
            .max_by(|&a, &b| self.energies[a].total_cmp(&self.energies[b]))
            .unwrap_or(0);
        if best != self.current {
            let cur_e = self.energies[self.current];
            let best_e = self.energies[best];
            if cur_e <= SILENCE_ENERGY && best_e > SILENCE_ENERGY {
                // Incumbent is digitally silent: adopt immediately.
                self.current = best;
                self.challenger_blocks = 0;
            } else if best_e > cur_e * SWITCH_MARGIN_ENERGY {
                if best == self.challenger {
                    self.challenger_blocks += 1;
                } else {
                    self.challenger = best;
                    self.challenger_blocks = 1;
                }
                if self.challenger_blocks >= STICK_BLOCKS {
                    self.current = best;
                    self.challenger_blocks = 0;
                }
            } else {
                self.challenger_blocks = 0;
            }
        } else {
            self.challenger_blocks = 0;
        }

        for f in 0..frames {
            out.push(interleaved[f * ch + self.current]);
        }
    }
}

/// Mux + resample pipeline: interleaved device-rate frames in, 16 kHz mono
/// out. One persistent resampler; buffers reused across callbacks.
struct Pipeline {
    mux: ChannelMux,
    resampler: Option<FftFixedIn<f32>>,
    chunk_in: usize,
    /// Mono staging at device rate awaiting a full resampler chunk.
    staging: Vec<f32>,
    in_buf: Vec<Vec<f32>>,
    out_buf: Vec<Vec<f32>>,
    mono_scratch: Vec<f32>,
}

impl Pipeline {
    fn new(device_rate: u32, channels: usize) -> Result<Pipeline, Error> {
        let (resampler, chunk_in) = if device_rate == TARGET_RATE {
            (None, 0)
        } else {
            // ~20 ms input chunks.
            let chunk = (device_rate as usize / 50).max(64);
            let r = FftFixedIn::<f32>::new(device_rate as usize, TARGET_RATE as usize, chunk, 4, 1)
                .map_err(|e| Error::Audio(format!("resampler init: {e}")))?;
            (Some(r), chunk)
        };
        let out_max = resampler
            .as_ref()
            .map(|r| r.output_frames_max())
            .unwrap_or(0);
        Ok(Pipeline {
            mux: ChannelMux::new(channels),
            resampler,
            chunk_in,
            staging: Vec::new(),
            in_buf: vec![vec![0.0; chunk_in.max(1)]],
            out_buf: vec![vec![0.0; out_max.max(1)]],
            mono_scratch: Vec::new(),
        })
    }

    fn push_f32(&mut self, interleaved: &[f32]) -> Vec<f32> {
        self.mono_scratch.clear();
        let mut mono = std::mem::take(&mut self.mono_scratch);
        self.mux.extract(interleaved, &mut mono);
        let out = self.process_mono(&mono);
        self.mono_scratch = mono;
        out
    }

    fn push_i16(&mut self, interleaved: &[i16]) -> Vec<f32> {
        self.mono_scratch.clear();
        let mut mono = std::mem::take(&mut self.mono_scratch);
        // Convert in place through a temporary f32 view of each frame.
        let ch = self.mux.channels;
        let frames = interleaved.len() / ch.max(1);
        let mut floats = Vec::with_capacity(interleaved.len());
        floats.extend(interleaved.iter().map(|&s| s as f32 / 32768.0));
        let _ = frames;
        self.mux.extract(&floats, &mut mono);
        let out = self.process_mono(&mono);
        self.mono_scratch = mono;
        out
    }

    fn process_mono(&mut self, mono: &[f32]) -> Vec<f32> {
        let Some(resampler) = self.resampler.as_mut() else {
            return mono.to_vec();
        };
        self.staging.extend_from_slice(mono);
        let mut out = Vec::new();
        let mut offset = 0usize;
        while self.staging.len() - offset >= self.chunk_in {
            self.in_buf[0].clear();
            self.in_buf[0].extend_from_slice(&self.staging[offset..offset + self.chunk_in]);
            offset += self.chunk_in;
            match resampler.process_into_buffer(&self.in_buf, &mut self.out_buf, None) {
                Ok((_, n_out)) => out.extend_from_slice(&self.out_buf[0][..n_out]),
                Err(_) => break,
            }
        }
        self.staging.drain(..offset);
        out
    }

    /// Drain the resampler's internal overlap (end of stream).
    fn flush(&mut self) -> Vec<f32> {
        let Some(resampler) = self.resampler.as_mut() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        // Push the final partial staging chunk, then drain twice.
        if !self.staging.is_empty() {
            self.in_buf[0].clear();
            self.in_buf[0].extend_from_slice(&self.staging);
            self.staging.clear();
            if let Ok((_, n_out)) =
                resampler.process_partial_into_buffer(Some(&self.in_buf), &mut self.out_buf, None)
            {
                out.extend_from_slice(&self.out_buf[0][..n_out]);
            }
        }
        for _ in 0..2 {
            if let Ok((_, n_out)) =
                resampler.process_partial_into_buffer(None::<&[Vec<f32>]>, &mut self.out_buf, None)
            {
                out.extend_from_slice(&self.out_buf[0][..n_out]);
            }
        }
        out
    }
}

struct SharedInner {
    buf: VecDeque<f32>,
    dropped: u64,
    closed: bool,
}

type Shared = Arc<(Mutex<SharedInner>, Condvar)>;

/// Append with bounded occupancy: overflow drops the OLDEST samples.
fn push_bounded(inner: &mut SharedInner, samples: &[f32]) {
    inner.buf.extend(samples.iter().copied());
    if inner.buf.len() > MAX_BUFFERED_SAMPLES {
        let cut = inner.buf.len() - MAX_BUFFERED_SAMPLES;
        inner.buf.drain(..cut);
        inner.dropped += cut as u64;
    }
}

/// Recover from a poisoned lock: a panicking callback must not kill capture.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One read from the capture buffer.
pub enum Chunk {
    /// Everything buffered since the last read (coalesced).
    Samples(Vec<f32>),
    /// Nothing arrived within the timeout.
    Timeout,
    /// The stream ended (device unplugged / host error).
    Closed,
}

/// An open (possibly not yet started) input stream.
pub struct MicStream {
    stream: Option<cpal::Stream>,
    shared: Shared,
    pipeline: Arc<Mutex<Pipeline>>,
    desc: String,
}

impl MicStream {
    /// Open the selected (or default) input device. The stream is built but
    /// NOT started — create the engine first, then call [`play`].
    pub fn open(selector: Option<&str>) -> Result<MicStream, Error> {
        let host = cpal::default_host();
        let device = match selector {
            Some(sel) => {
                let devices: Vec<cpal::Device> = host
                    .input_devices()
                    .map_err(|e| Error::Audio(format!("enumerate inputs: {e}")))?
                    .collect();
                let names: Vec<String> = devices
                    .iter()
                    .map(|d| d.name().unwrap_or_else(|_| "?".into()))
                    .collect();
                let idx = match_index(&names, sel).ok_or_else(|| {
                    Error::Audio(format!(
                        "no input device matches {sel:?} (have: {})",
                        names.join(", ")
                    ))
                })?;
                devices.into_iter().nth(idx).expect("index from match")
            }
            None => host
                .default_input_device()
                .ok_or_else(|| Error::Audio("no default input device".into()))?,
        };
        let name = device.name().unwrap_or_else(|_| "?".into());
        let config = pick_input_config(&device)?;
        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        let channels = stream_config.channels as usize;
        let rate = stream_config.sample_rate.0;

        let desc = format!("{name} @ {rate} Hz, {channels} ch, {sample_format:?}");
        let pipeline = Arc::new(Mutex::new(Pipeline::new(rate, channels)?));
        let shared: Shared = Arc::new((
            Mutex::new(SharedInner {
                buf: VecDeque::new(),
                dropped: 0,
                closed: false,
            }),
            Condvar::new(),
        ));

        let err_shared = shared.clone();
        let err_fn = move |e: cpal::StreamError| {
            let (m, cv) = &*err_shared;
            lock(m).closed = true;
            cv.notify_all();
            eprintln!("mic stream error: {e}");
        };

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                let p = pipeline.clone();
                let sh = shared.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        let out = lock(&p).push_f32(data);
                        if !out.is_empty() {
                            let (m, cv) = &*sh;
                            push_bounded(&mut lock(m), &out);
                            cv.notify_one();
                        }
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let p = pipeline.clone();
                let sh = shared.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        let out = lock(&p).push_i16(data);
                        if !out.is_empty() {
                            let (m, cv) = &*sh;
                            push_bounded(&mut lock(m), &out);
                            cv.notify_one();
                        }
                    },
                    err_fn,
                    None,
                )
            }
            other => {
                return Err(Error::Audio(format!(
                    "unsupported sample format {other:?} (negotiation should have rejected it)"
                )))
            }
        }
        .map_err(|e| Error::Audio(format!("build input stream: {e}")))?;

        Ok(MicStream {
            stream: Some(stream),
            shared,
            pipeline,
            desc,
        })
    }

    pub fn play(&self) -> Result<(), Error> {
        self.stream
            .as_ref()
            .ok_or_else(|| Error::Audio("stream already finished".into()))?
            .play()
            .map_err(|e| Error::Audio(format!("start stream: {e}")))
    }

    /// Device + negotiated config, for status lines and diagnostics.
    pub fn describe(&self) -> &str {
        &self.desc
    }

    /// Samples dropped so far because the consumer fell behind.
    pub fn dropped_samples(&self) -> u64 {
        lock(&self.shared.0).dropped
    }

    /// Drain everything buffered, waiting up to `timeout` if empty.
    pub fn read(&self, timeout: Duration) -> Chunk {
        let (m, cv) = &*self.shared;
        let mut inner = lock(m);
        if inner.buf.is_empty() && !inner.closed {
            let (guard, _) = cv
                .wait_timeout(inner, timeout)
                .unwrap_or_else(|e| e.into_inner());
            inner = guard;
        }
        if !inner.buf.is_empty() {
            return Chunk::Samples(inner.buf.drain(..).collect());
        }
        if inner.closed {
            Chunk::Closed
        } else {
            Chunk::Timeout
        }
    }

    /// Stop capture and return the resampler tail plus anything left in the
    /// buffer.
    pub fn finish(mut self) -> Vec<f32> {
        self.stream.take(); // drop → stop the callback
        let mut tail: Vec<f32> = lock(&self.shared.0).buf.drain(..).collect();
        tail.extend(lock(&self.pipeline).flush());
        tail
    }
}

/// Rank the device's supported configs: F32 before I16, then the highest
/// rate ≤ 48 kHz, then the fewest channels. Rates below 16 kHz are rejected.
fn pick_input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, Error> {
    let ranges: Vec<cpal::SupportedStreamConfigRange> = device
        .supported_input_configs()
        .map_err(|e| Error::Audio(format!("query input configs: {e}")))?
        .collect();
    let mut best: Option<(u8, u32, u16, cpal::SupportedStreamConfigRange)> = None;
    for r in &ranges {
        let fmt_rank = match r.sample_format() {
            cpal::SampleFormat::F32 => 2u8,
            cpal::SampleFormat::I16 => 1,
            _ => continue,
        };
        let max_r = r.max_sample_rate().0;
        let min_r = r.min_sample_rate().0;
        if max_r < MIN_DEVICE_RATE {
            continue;
        }
        let rate = max_r.min(MAX_DEVICE_RATE).max(min_r);
        let key = (fmt_rank, rate, u16::MAX - r.channels());
        let better = match &best {
            Some((f, rt, ch, _)) => key > (*f, *rt, *ch),
            None => true,
        };
        if better {
            best = Some((key.0, key.1, key.2, r.clone()));
        }
    }
    match best {
        Some((_, rate, _, range)) => Ok(range.with_sample_rate(cpal::SampleRate(rate))),
        None => {
            let offered: Vec<String> = ranges.iter().take(5).map(summarize_range).collect();
            Err(Error::Audio(format!(
                "no usable input config (need F32/I16, ≥16 kHz); device offers: {}",
                offered.join("; ")
            )))
        }
    }
}

fn summarize_range(r: &cpal::SupportedStreamConfigRange) -> String {
    format!(
        "{}–{} Hz, {} ch, {:?}",
        r.min_sample_rate().0,
        r.max_sample_rate().0,
        r.channels(),
        r.sample_format()
    )
}

/// One entry from [`list_input_devices`].
pub struct MicDevice {
    /// 1-based index (numeric selectors are index-based, shadow-proof
    /// against device names that contain digits).
    pub index: usize,
    pub name: String,
    pub default: bool,
    pub configs: String,
}

pub fn list_input_devices() -> Result<Vec<MicDevice>, Error> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();
    let mut out = Vec::new();
    let devices = host
        .input_devices()
        .map_err(|e| Error::Audio(format!("enumerate inputs: {e}")))?;
    for (i, d) in devices.enumerate() {
        let name = d.name().unwrap_or_else(|_| "?".into());
        let configs = d
            .supported_input_configs()
            .map(|it| {
                it.take(4)
                    .map(|r| summarize_range(&r))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_else(|e| format!("unavailable: {e}"));
        out.push(MicDevice {
            index: i + 1,
            default: name == default_name,
            name,
            configs,
        });
    }
    Ok(out)
}

/// Resolve a device selector: a numeric selector is a 1-based index;
/// anything else is a case-insensitive substring match (first hit wins).
pub fn match_index(names: &[String], selector: &str) -> Option<usize> {
    if let Ok(n) = selector.trim().parse::<usize>() {
        return (1..=names.len()).contains(&n).then(|| n - 1);
    }
    let needle = selector.to_lowercase();
    names
        .iter()
        .position(|n| n.to_lowercase().contains(&needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interleave(chans: &[Vec<f32>]) -> Vec<f32> {
        let frames = chans[0].len();
        let mut out = Vec::with_capacity(frames * chans.len());
        for f in 0..frames {
            for c in chans {
                out.push(c[f]);
            }
        }
        out
    }

    #[test]
    fn mux_adopts_immediately_from_silence() {
        let mut mux = ChannelMux::new(2);
        let silent = vec![0.0f32; 320];
        let live: Vec<f32> = (0..320).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
        let mut out = Vec::new();
        mux.extract(&interleave(&[silent, live.clone()]), &mut out);
        assert_eq!(mux.current, 1, "silent incumbent must be replaced at once");
        assert_eq!(out, live);
    }

    #[test]
    fn mux_does_not_flap_on_brief_spike() {
        let mut mux = ChannelMux::new(2);
        let quiet: Vec<f32> = (0..320).map(|i| (i as f32 * 0.1).sin() * 0.3).collect();
        let loud: Vec<f32> = (0..320).map(|i| (i as f32 * 0.1).sin() * 0.9).collect();
        let mut out = Vec::new();
        // Establish channel 0 as live.
        mux.extract(&interleave(&[quiet.clone(), vec![0.001; 320]]), &mut out);
        assert_eq!(mux.current, 0);
        // A few loud blocks on channel 1 — fewer than STICK_BLOCKS.
        for _ in 0..5 {
            mux.extract(&interleave(&[quiet.clone(), loud.clone()]), &mut out);
        }
        assert_eq!(mux.current, 0, "brief spike must not switch");
    }

    #[test]
    fn mux_switches_after_sustained_challenger() {
        let mut mux = ChannelMux::new(2);
        let quiet: Vec<f32> = (0..320).map(|i| (i as f32 * 0.1).sin() * 0.2).collect();
        let loud: Vec<f32> = (0..320).map(|i| (i as f32 * 0.1).sin() * 0.9).collect();
        let mut out = Vec::new();
        mux.extract(&interleave(&[quiet.clone(), vec![0.001; 320]]), &mut out);
        assert_eq!(mux.current, 0);
        for _ in 0..STICK_BLOCKS + 1 {
            mux.extract(&interleave(&[quiet.clone(), loud.clone()]), &mut out);
        }
        assert_eq!(mux.current, 1, "sustained +6 dB challenger must win");
    }

    #[test]
    fn resample_48k_stereo_to_16k() {
        let mut p = Pipeline::new(48000, 2).unwrap();
        let n = 48000; // 1 s
        let left: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let right = vec![0.0f32; n];
        let mut total = 0usize;
        for chunk in interleave(&[left, right]).chunks(2 * 480) {
            total += p.push_f32(chunk).len();
        }
        total += p.flush().len();
        let expected = 16000;
        assert!(
            (total as i64 - expected as i64).abs() < 800,
            "expected ≈{expected} samples out, got {total}"
        );
    }

    #[test]
    fn passthrough_at_16k() {
        let mut p = Pipeline::new(16000, 1).unwrap();
        let data: Vec<f32> = (0..1600).map(|i| i as f32 / 1600.0).collect();
        let out = p.push_f32(&data);
        assert_eq!(out, data);
        assert!(p.flush().is_empty());
    }

    #[test]
    fn i16_conversion() {
        let mut p = Pipeline::new(16000, 1).unwrap();
        let out = p.push_i16(&[0i16, 16384, -16384, 32767]);
        assert_eq!(out.len(), 4);
        assert!((out[1] - 0.5).abs() < 1e-3);
        assert!((out[2] + 0.5).abs() < 1e-3);
    }

    #[test]
    fn bounded_buffer_drops_oldest() {
        let mut inner = SharedInner {
            buf: VecDeque::new(),
            dropped: 0,
            closed: false,
        };
        let block: Vec<f32> = (0..TARGET_RATE as usize).map(|i| i as f32).collect();
        for _ in 0..35 {
            push_bounded(&mut inner, &block);
        }
        assert_eq!(inner.buf.len(), MAX_BUFFERED_SAMPLES);
        assert_eq!(inner.dropped, 5 * TARGET_RATE as u64);
        // The newest samples survive.
        assert_eq!(*inner.buf.back().unwrap(), (TARGET_RATE - 1) as f32);
    }

    #[test]
    fn match_index_rules() {
        let names = vec![
            "OBSBOT Tiny 2".to_string(),
            "Scarlett 4i4 USB".to_string(),
            "Monitor of Built-in 2ch".to_string(),
        ];
        assert_eq!(match_index(&names, "scarlett"), Some(1));
        assert_eq!(match_index(&names, "2"), Some(1), "numeric = 1-based index");
        assert_eq!(match_index(&names, "1"), Some(0));
        assert_eq!(match_index(&names, "4"), None, "out of range");
        assert_eq!(match_index(&names, "zzz"), None);
    }
}
