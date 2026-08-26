//! linefeed CLI: deterministic replay, live microphone tracking, device
//! listing, timeline recording. Exit codes: 0 success (including a clean
//! Ctrl-C in live mode), 1 runtime failure, 2 usage error.

mod args;
mod status;
mod wav;

use std::path::Path;

use anyhow::{bail, Context, Result};
use linefeed_asr::{AsrEngine, EngineConfig, TimelineReplay, TimelineWriter};
use linefeed_core::{Script, Tracker, TrackerConfig};

use args::{Command, EngineOpts, ReplaySource};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match args::parse(&argv) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}\n\n{}", args::USAGE);
            std::process::exit(2);
        }
    };
    let code = match run(cmd) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

fn run(cmd: Command) -> Result<()> {
    match cmd {
        Command::Help => {
            print!("{}", args::USAGE);
            Ok(())
        }
        Command::Devices => devices(),
        Command::Replay {
            script,
            source,
            opts,
            dump,
        } => replay(&script, source, &opts, dump.as_deref()),
        Command::Live {
            script,
            input_device,
            opts,
            dump,
        } => live(&script, input_device.as_deref(), &opts, dump.as_deref()),
        Command::Dump { wav, out, opts } => dump_timeline(&wav, &out, &opts),
    }
}

fn load_script(path: &Path) -> Result<Script> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("read script {}", path.display()))?;
    let script = Script::parse(&text);
    if script.n_tokens() == 0 {
        bail!("{}: script has no words", path.display());
    }
    Ok(script)
}

fn engine_config(opts: &EngineOpts) -> EngineConfig {
    let model_dir = opts.model_dir.clone().unwrap_or_else(|| {
        linefeed_asr::default_model_dir(&opts.engine)
            .to_string_lossy()
            .into_owned()
    });
    EngineConfig {
        model_dir,
        num_threads: opts
            .threads
            .unwrap_or_else(|| linefeed_asr::engine_num_threads(&opts.engine)),
        sample_rate: 16000,
    }
}

fn open_dump(path: Option<&Path>) -> Result<Option<TimelineWriter>> {
    match path {
        Some(p) => Ok(Some(TimelineWriter::create(p)?)),
        None => Ok(None),
    }
}

/// Feed one batch of samples: engine → tracker → status/dump.
fn pump(
    engine: &mut dyn AsrEngine,
    tracker: &mut Tracker,
    dump: &mut Option<TimelineWriter>,
    samples: &[f32],
) -> Result<()> {
    for hyp in engine.feed(samples)? {
        if let Some(w) = dump {
            w.write(&hyp)?;
        }
        if let Some(ev) = tracker.feed(&hyp) {
            println!("{}", status::render(tracker, &ev));
        }
    }
    Ok(())
}

fn flush(
    engine: &mut dyn AsrEngine,
    tracker: &mut Tracker,
    dump: &mut Option<TimelineWriter>,
) -> Result<()> {
    for hyp in engine.flush()? {
        if let Some(w) = dump {
            w.write(&hyp)?;
        }
        if let Some(ev) = tracker.feed(&hyp) {
            println!("{}", status::render(tracker, &ev));
        }
    }
    Ok(())
}

fn replay(
    script_path: &Path,
    source: ReplaySource,
    opts: &EngineOpts,
    dump_path: Option<&Path>,
) -> Result<()> {
    let script = load_script(script_path)?;
    let mut tracker = Tracker::new(script, TrackerConfig::default());
    let mut dump = open_dump(dump_path)?;

    match source {
        ReplaySource::Timeline(path) => {
            let mut engine = TimelineReplay::from_path(&path)?;
            while engine.remaining() > 0 {
                pump(&mut engine, &mut tracker, &mut dump, &[])?;
            }
        }
        ReplaySource::Wav(path) => {
            let samples = wav::read_mono_16k(&path)?;
            let mut engine = linefeed_asr::make_engine(&opts.engine, &engine_config(opts))?;
            for chunk in samples.chunks(8000) {
                pump(engine.as_mut(), &mut tracker, &mut dump, chunk)?;
            }
            flush(engine.as_mut(), &mut tracker, &mut dump)?;
        }
    }
    println!("{}", status::final_summary(&tracker));
    Ok(())
}

fn dump_timeline(wav_path: &Path, out: &Path, opts: &EngineOpts) -> Result<()> {
    let samples = wav::read_mono_16k(wav_path)?;
    let mut engine = linefeed_asr::make_engine(&opts.engine, &engine_config(opts))?;
    let mut writer = TimelineWriter::create(out)?;
    let mut n = 0usize;
    for chunk in samples.chunks(8000) {
        for hyp in engine.feed(chunk)? {
            writer.write(&hyp)?;
            n += 1;
        }
    }
    for hyp in engine.flush()? {
        writer.write(&hyp)?;
        n += 1;
    }
    println!("wrote {n} hypotheses to {}", out.display());
    Ok(())
}

#[cfg(feature = "mic")]
fn devices() -> Result<()> {
    let devices = linefeed_asr::mic::list_input_devices()?;
    if devices.is_empty() {
        println!("no input devices found");
        return Ok(());
    }
    println!("{:>3}  {:<40} configs", "#", "name (default marked *)");
    for d in &devices {
        let star = if d.default { "*" } else { " " };
        println!("{:>3}{star} {:<40} {}", d.index, d.name, d.configs);
    }
    println!("\nselect with --input-device <substring> or --input-device <#>");
    Ok(())
}

#[cfg(feature = "mic")]
fn live(
    script_path: &Path,
    input_device: Option<&str>,
    opts: &EngineOpts,
    dump_path: Option<&Path>,
) -> Result<()> {
    use linefeed_asr::mic::{Chunk, MicStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    let script = load_script(script_path)?;
    let mut tracker = Tracker::new(script, TrackerConfig::default());
    let mut dump = open_dump(dump_path)?;

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst))
            .context("install Ctrl-C handler")?;
    }

    // Engine first (slow model load), then open + start capture.
    eprintln!("loading model ({})…", opts.engine);
    let mut engine = linefeed_asr::make_engine(&opts.engine, &engine_config(opts))?;
    let mic = MicStream::open(input_device)?;
    eprintln!("listening on {} — Ctrl-C to stop", mic.describe());
    mic.play()?;

    let mut closed = false;
    while !stop.load(Ordering::SeqCst) {
        match mic.read(Duration::from_millis(250)) {
            Chunk::Samples(samples) => pump(engine.as_mut(), &mut tracker, &mut dump, &samples)?,
            Chunk::Timeout => continue,
            Chunk::Closed => {
                closed = true;
                break;
            }
        }
    }

    // Clean shutdown: capture tail → engine tail → final summary, exit 0.
    let dropped = mic.dropped_samples();
    let tail = mic.finish();
    if !tail.is_empty() {
        pump(engine.as_mut(), &mut tracker, &mut dump, &tail)?;
    }
    flush(engine.as_mut(), &mut tracker, &mut dump)?;
    if dropped > 0 {
        eprintln!(
            "note: {dropped} samples (~{:.1}s) were dropped because decoding fell behind",
            dropped as f32 / 16000.0
        );
    }
    println!("{}", status::final_summary(&tracker));
    if closed {
        bail!("input stream ended unexpectedly (device unplugged?)");
    }
    Ok(())
}

#[cfg(not(feature = "mic"))]
fn devices() -> Result<()> {
    bail!("microphone support not built in — rebuild with the `mic` feature")
}

#[cfg(not(feature = "mic"))]
fn live(
    _script_path: &Path,
    _input_device: Option<&str>,
    _opts: &EngineOpts,
    _dump_path: Option<&Path>,
) -> Result<()> {
    bail!("microphone support not built in — rebuild with the `mic` feature")
}
