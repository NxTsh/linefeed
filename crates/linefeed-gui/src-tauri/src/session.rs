//! The live session thread: mic → engine → tracker → events.
//! Compiled only with the `mic` feature.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::Emitter;

use linefeed_asr::mic::{Chunk, MicStream};
use linefeed_core::{Tracker, TrackerConfig};

use crate::diag::{DiagLog, RmsMeter};
use crate::{lock, AppState, Session, Shared, StatusPayload, EV_STATUS, EV_TRACKER};

fn status(app: &tauri::AppHandle, shared: &Shared, state: &str, message: &str) {
    let (engine, device) = {
        let cfg = lock(&shared.config);
        (cfg.engine.clone(), cfg.device.clone())
    };
    let payload = StatusPayload {
        running: matches!(state, "loading-model" | "listening"),
        engine,
        device,
        state: state.to_string(),
        message: message.to_string(),
    };
    *lock(&shared.status) = payload.clone();
    let _ = app.emit(EV_STATUS, payload);
}

pub fn start(app: &tauri::AppHandle, state: &AppState) -> Result<(), String> {
    if lock(&state.script).is_none() {
        return Err("load a script first".to_string());
    }
    let mut slot = lock(&state.session);
    if slot.is_some() {
        return Err("a session is already running".to_string());
    }
    let stop_flag = Arc::new(AtomicBool::new(false));
    let shared: Arc<Shared> = state.inner().clone();
    let handle = app.clone();
    let flag = stop_flag.clone();
    let join = std::thread::spawn(move || session_thread(handle, shared, flag));
    *slot = Some(Session {
        stop: stop_flag,
        join,
    });
    Ok(())
}

/// Runs on an async command thread — joining here never stalls the UI.
pub fn stop(app: &tauri::AppHandle, state: &AppState) {
    let session = lock(&state.session).take();
    if let Some(s) = session {
        s.stop.store(true, Ordering::SeqCst);
        let _ = s.join.join();
    }
    status(app, state, "stopped", "");
}

fn session_thread(app: tauri::AppHandle, shared: Arc<Shared>, stop_flag: Arc<AtomicBool>) {
    let outcome = run_session(&app, &shared, &stop_flag);
    // Clear the slot if we exited on our own (device gone / error);
    // a stop() call has already taken it.
    if let Some(s) = lock(&shared.session).take() {
        // Don't join ourselves; just drop the handle.
        drop(s.join);
    }
    match outcome {
        Ok(()) => status(&app, &shared, "stopped", ""),
        Err(e) => status(&app, &shared, "error", &e),
    }
}

fn run_session(
    app: &tauri::AppHandle,
    shared: &Shared,
    stop_flag: &AtomicBool,
) -> Result<(), String> {
    let (engine_name, model_id, device_sel, debug_log, script_path) = {
        let cfg = lock(&shared.config);
        (
            cfg.engine.clone(),
            cfg.model.clone(),
            cfg.device.clone(),
            cfg.debug_log,
            cfg.last_script.clone(),
        )
    };
    let script = {
        let guard = lock(&shared.script);
        let (script, _) = guard.as_ref().ok_or("no script loaded")?;
        script.clone()
    };

    status(app, shared, "loading-model", "loading model…");
    let engine_cfg = linefeed_asr::EngineConfig {
        model_dir: linefeed_asr::default_model_dir(&model_id)
            .to_string_lossy()
            .into_owned(),
        num_threads: linefeed_asr::engine_num_threads(&engine_name),
        sample_rate: 16000,
    };
    let mut engine =
        linefeed_asr::make_engine(&engine_name, &engine_cfg).map_err(|e| e.to_string())?;

    let selector = if device_sel.trim().is_empty() {
        None
    } else {
        Some(device_sel.as_str())
    };
    let mic = MicStream::open(selector).map_err(|e| e.to_string())?;
    let mut tracker = Tracker::new(script, TrackerConfig::default());

    // Diagnostics are snapshotted at session start (no mid-session races).
    let mut diag: Option<DiagLog> = if debug_log {
        DiagLog::create(
            std::path::Path::new(&script_path),
            &engine_name,
            mic.describe(),
        )
    } else {
        None
    };
    let mut rms = RmsMeter::default();
    let mut last_words: Vec<String> = Vec::new();

    let listening_msg = match diag.as_ref() {
        Some(log) => format!("{} (diag: {})", mic.describe(), log.path.display()),
        None => mic.describe().to_string(),
    };
    status(app, shared, "listening", &listening_msg);
    mic.play().map_err(|e| e.to_string())?;

    let mut closed = false;
    while !stop_flag.load(Ordering::SeqCst) {
        match mic.read(Duration::from_millis(250)) {
            Chunk::Timeout => continue,
            Chunk::Closed => {
                closed = true;
                break;
            }
            Chunk::Samples(samples) => {
                if let Some(log) = diag.as_mut() {
                    if let Some((t, v)) = rms.feed(&samples, 16000) {
                        log.mic_rms(t, v);
                    }
                }
                feed(
                    app,
                    &mut *engine,
                    &mut tracker,
                    &mut diag,
                    &mut last_words,
                    &samples,
                )?;
            }
        }
    }

    let tail = mic.finish();
    if !tail.is_empty() {
        feed(
            app,
            &mut *engine,
            &mut tracker,
            &mut diag,
            &mut last_words,
            &tail,
        )?;
    }
    for hyp in engine.flush().map_err(|e| e.to_string())? {
        deliver(app, &mut tracker, &mut diag, &mut last_words, &hyp);
    }
    if let Some(log) = diag.as_mut() {
        log.session_end(tracker.cursor(), tracker.n_tokens());
    }
    if closed {
        return Err("input stream ended unexpectedly (device unplugged?)".to_string());
    }
    Ok(())
}

fn feed(
    app: &tauri::AppHandle,
    engine: &mut dyn linefeed_asr::AsrEngine,
    tracker: &mut Tracker,
    diag: &mut Option<DiagLog>,
    last_words: &mut Vec<String>,
    samples: &[f32],
) -> Result<(), String> {
    for hyp in engine.feed(samples).map_err(|e| e.to_string())? {
        deliver(app, tracker, diag, last_words, &hyp);
    }
    Ok(())
}

fn deliver(
    app: &tauri::AppHandle,
    tracker: &mut Tracker,
    diag: &mut Option<DiagLog>,
    last_words: &mut Vec<String>,
    hyp: &linefeed_core::Hypothesis,
) {
    if !hyp.words.is_empty() {
        *last_words = hyp
            .words
            .iter()
            .rev()
            .take(6)
            .rev()
            .map(|w| w.text.clone())
            .collect();
    }
    if let Some(ev) = tracker.feed(hyp) {
        if let Some(log) = diag.as_mut() {
            log.tracker(&ev, last_words);
        }
        let _ = app.emit(EV_TRACKER, ev);
    }
}
