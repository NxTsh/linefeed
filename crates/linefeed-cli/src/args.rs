//! Hand-rolled argument parser (no deps, fully unit-tested).
//!
//! Subcommands:
//!   replay <script> --wav F | --timeline F   deterministic replay
//!   live <script> [--input-device S]         live microphone tracking
//!   devices                                  list input devices
//!   dump --wav F --out F                     record a timeline (no script)

use std::path::PathBuf;

pub const USAGE: &str = "\
linefeed — a teleprompter that scrolls as you speak (offline)

USAGE:
  linefeed replay <script.txt> (--wav <f.wav> | --timeline <f.jsonl>) [options]
  linefeed live <script.txt> [--input-device <name|index>] [options]
  linefeed devices
  linefeed dump --wav <f.wav> --out <f.jsonl> [options]

OPTIONS:
  --engine <name>        ASR engine (default: sherpa)
  --model <id>           ASR model: pt-br (default) or en
  --model-dir <dir>      explicit model directory (overrides --model;
                         default root: $LINEFEED_MODELS_DIR or the platform
                         data dir under linefeed/models)
  --threads <n>          decoder threads (default: per-engine)
  --dump-timeline <f>    also record hypotheses as replayable JSONL
  -h, --help             show this help
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaySource {
    Wav(PathBuf),
    Timeline(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineOpts {
    pub engine: String,
    pub model: String,
    pub model_dir: Option<String>,
    pub threads: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Replay {
        script: PathBuf,
        source: ReplaySource,
        opts: EngineOpts,
        dump: Option<PathBuf>,
    },
    Live {
        script: PathBuf,
        input_device: Option<String>,
        opts: EngineOpts,
        dump: Option<PathBuf>,
    },
    Devices,
    Dump {
        wav: PathBuf,
        out: PathBuf,
        opts: EngineOpts,
    },
    Help,
}

fn take_value(it: &mut std::slice::Iter<'_, String>, flag: &str) -> Result<String, String> {
    it.next()
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// Parse argv (without the binary name).
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter();
    let sub = match it.next() {
        None => return Ok(Command::Help),
        Some(s) if s == "-h" || s == "--help" || s == "help" => return Ok(Command::Help),
        Some(s) => s.clone(),
    };

    let mut script: Option<PathBuf> = None;
    let mut wav: Option<PathBuf> = None;
    let mut timeline: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut dump: Option<PathBuf> = None;
    let mut input_device: Option<String> = None;
    let mut opts = EngineOpts {
        engine: "sherpa".to_string(),
        model: "pt-br".to_string(),
        model_dir: None,
        threads: None,
    };

    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--wav" => wav = Some(PathBuf::from(take_value(&mut it, "--wav")?)),
            "--timeline" => timeline = Some(PathBuf::from(take_value(&mut it, "--timeline")?)),
            "--out" => out = Some(PathBuf::from(take_value(&mut it, "--out")?)),
            "--dump-timeline" => {
                dump = Some(PathBuf::from(take_value(&mut it, "--dump-timeline")?))
            }
            "--engine" => opts.engine = take_value(&mut it, "--engine")?,
            "--model" => opts.model = take_value(&mut it, "--model")?,
            "--model-dir" => opts.model_dir = Some(take_value(&mut it, "--model-dir")?),
            "--input-device" => input_device = Some(take_value(&mut it, "--input-device")?),
            "--threads" => {
                let v = take_value(&mut it, "--threads")?;
                let n: i32 = v
                    .parse()
                    .map_err(|_| format!("--threads: not a number: {v}"))?;
                if n < 1 {
                    return Err("--threads must be >= 1".to_string());
                }
                opts.threads = Some(n);
            }
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
            positional => {
                if script.is_some() {
                    return Err(format!("unexpected extra argument {positional:?}"));
                }
                script = Some(PathBuf::from(positional));
            }
        }
    }

    match sub.as_str() {
        "replay" => {
            let script = script.ok_or("replay needs a script path")?;
            let source = match (wav, timeline) {
                (Some(w), None) => ReplaySource::Wav(w),
                (None, Some(t)) => ReplaySource::Timeline(t),
                (Some(_), Some(_)) => {
                    return Err("replay takes --wav OR --timeline, not both".to_string())
                }
                (None, None) => return Err("replay needs --wav or --timeline".to_string()),
            };
            Ok(Command::Replay {
                script,
                source,
                opts,
                dump,
            })
        }
        "live" => {
            let script = script.ok_or("live needs a script path")?;
            Ok(Command::Live {
                script,
                input_device,
                opts,
                dump,
            })
        }
        "devices" => {
            if script.is_some() {
                return Err("devices takes no arguments".to_string());
            }
            Ok(Command::Devices)
        }
        "dump" => {
            let wav = wav.ok_or("dump needs --wav")?;
            let out = out.ok_or("dump needs --out")?;
            Ok(Command::Dump { wav, out, opts })
        }
        other => Err(format!("unknown subcommand {other:?} (see --help)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_args_is_help() {
        assert_eq!(parse(&[]).unwrap(), Command::Help);
        assert_eq!(parse(&args(&["--help"])).unwrap(), Command::Help);
        assert_eq!(parse(&args(&["replay", "-h"])).unwrap(), Command::Help);
    }

    #[test]
    fn replay_wav() {
        let c = parse(&args(&[
            "replay", "s.txt", "--wav", "f.wav", "--engine", "sherpa",
        ]))
        .unwrap();
        match c {
            Command::Replay {
                script,
                source,
                opts,
                dump,
            } => {
                assert_eq!(script, PathBuf::from("s.txt"));
                assert_eq!(source, ReplaySource::Wav(PathBuf::from("f.wav")));
                assert_eq!(opts.engine, "sherpa");
                assert_eq!(opts.model, "pt-br", "pt-BR is the default model");
                assert_eq!(dump, None);
            }
            other => panic!("wrong command: {other:?}"),
        }
    }

    #[test]
    fn replay_timeline_with_dump() {
        let c = parse(&args(&[
            "replay",
            "s.txt",
            "--timeline",
            "t.jsonl",
            "--dump-timeline",
            "copy.jsonl",
        ]))
        .unwrap();
        match c {
            Command::Replay { source, dump, .. } => {
                assert_eq!(source, ReplaySource::Timeline(PathBuf::from("t.jsonl")));
                assert_eq!(dump, Some(PathBuf::from("copy.jsonl")));
            }
            other => panic!("wrong command: {other:?}"),
        }
    }

    #[test]
    fn replay_requires_exactly_one_source() {
        assert!(parse(&args(&["replay", "s.txt"])).is_err());
        assert!(parse(&args(&["replay", "s.txt", "--wav", "a", "--timeline", "b"])).is_err());
    }

    #[test]
    fn live_with_device_and_dump() {
        let c = parse(&args(&[
            "live",
            "s.txt",
            "--input-device",
            "scarlett",
            "--dump-timeline",
            "take.jsonl",
            "--threads",
            "2",
        ]))
        .unwrap();
        match c {
            Command::Live {
                input_device,
                dump,
                opts,
                ..
            } => {
                assert_eq!(input_device.as_deref(), Some("scarlett"));
                assert_eq!(dump, Some(PathBuf::from("take.jsonl")));
                assert_eq!(opts.threads, Some(2));
            }
            other => panic!("wrong command: {other:?}"),
        }
    }

    #[test]
    fn model_flag_selects_english() {
        let c = parse(&args(&[
            "replay", "s.txt", "--wav", "f.wav", "--model", "en",
        ]))
        .unwrap();
        match c {
            Command::Replay { opts, .. } => assert_eq!(opts.model, "en"),
            other => panic!("wrong command: {other:?}"),
        }
        assert!(parse(&args(&["replay", "s.txt", "--wav", "f", "--model"])).is_err());
    }

    #[test]
    fn dump_needs_wav_and_out() {
        assert!(parse(&args(&["dump", "--wav", "f.wav"])).is_err());
        assert!(parse(&args(&["dump", "--out", "t.jsonl"])).is_err());
        let c = parse(&args(&["dump", "--wav", "f.wav", "--out", "t.jsonl"])).unwrap();
        assert!(matches!(c, Command::Dump { .. }));
    }

    #[test]
    fn errors_are_errors() {
        assert!(parse(&args(&["replay", "s.txt", "extra", "--wav", "f"])).is_err());
        assert!(parse(&args(&["replay", "s.txt", "--wav"])).is_err());
        assert!(parse(&args(&["replay", "s.txt", "--frobnicate"])).is_err());
        assert!(parse(&args(&["fly"])).is_err());
        assert!(parse(&args(&["live", "s.txt", "--threads", "zero"])).is_err());
        assert!(parse(&args(&["live", "s.txt", "--threads", "0"])).is_err());
        assert!(parse(&args(&["devices", "extra"])).is_err());
    }
}
