//! linefeed GUI backend (Tauri v2).
//!
//! Architecture: Rust owns tokenization and the tracker; the frontend is a
//! pure renderer consuming events. Long-running work (engine load, session
//! shutdown, model download) always runs off the main thread — `start`,
//! `stop`, `download_model` and `cancel_model_fetch` are async commands, so
//! the UI never stalls on a join or a model load.

mod config;
mod diag;
mod model_fetch;
mod payload;
#[cfg(feature = "mic")]
mod session;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use tauri::{Emitter, Manager};

pub use config::GuiConfig;
use payload::ScriptPayload;

pub const EV_TRACKER: &str = "linefeed://tracker";
pub const EV_DUMB: &str = "linefeed://dumb";
pub const EV_CONFIG: &str = "linefeed://config";
pub const EV_STATUS: &str = "linefeed://status";
pub const EV_FETCH: &str = "linefeed://model-fetch";

#[derive(Debug, Clone, Serialize, Default)]
pub struct StatusPayload {
    pub running: bool,
    pub engine: String,
    pub device: String,
    /// "idle" | "loading-model" | "listening" | "stopped" | "error"
    pub state: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DumbEvent {
    pub t: f64,
    pub cursor: usize,
    pub wpm: u32,
    pub playing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DevicePayload {
    pub index: usize,
    pub name: String,
    pub default: bool,
    pub configs: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfoPayload {
    pub id: String,
    pub label: String,
    pub lang: String,
    pub archive_bytes: u64,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartupInfoPayload {
    pub engines: Vec<String>,
    pub engine: String,
    /// Selected model id from the registry.
    pub model: String,
    /// Every model linefeed can download, with install state.
    pub models: Vec<ModelInfoPayload>,
    pub models_dir: String,
    /// True when the SELECTED model is installed.
    pub models_ok: bool,
    pub missing: Vec<String>,
    /// True when the missing selected model can be downloaded by the splash.
    pub fetchable: bool,
    pub fetch_url: String,
    pub fetch_bytes: u64,
    pub last_script: String,
}

struct DumbState {
    /// Fractional token position.
    pos: f64,
    playing: bool,
}

#[cfg(feature = "mic")]
struct Session {
    stop: Arc<AtomicBool>,
    join: std::thread::JoinHandle<()>,
}

struct Shared {
    script: Mutex<Option<(linefeed_core::Script, ScriptPayload)>>,
    #[cfg(feature = "mic")]
    session: Mutex<Option<Session>>,
    dumb: Mutex<DumbState>,
    config: Mutex<GuiConfig>,
    status: Mutex<StatusPayload>,
    fetch: Mutex<Option<model_fetch::FetchHandle>>,
    /// Lock-free idle gate for the dumb ticker.
    dumb_active: AtomicBool,
    config_path: PathBuf,
}

/// Poison-safe lock: a panicking holder must not wedge the app.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    fn emit_config(&self, app: &tauri::AppHandle) -> GuiConfig {
        let cfg = lock(&self.config).clone();
        config::save(&self.config_path, &cfg);
        let _ = app.emit(EV_CONFIG, cfg.clone());
        cfg
    }
}

type AppState<'a> = tauri::State<'a, Arc<Shared>>;

// ---------------------------------------------------------------- commands

#[tauri::command]
fn load_script(
    app: tauri::AppHandle,
    state: AppState,
    path: String,
) -> Result<ScriptPayload, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))?;
    let script = linefeed_core::Script::parse(&text);
    if script.n_tokens() == 0 {
        return Err(format!("{path}: script has no words"));
    }
    let payload = payload::build(&script, &path);
    *lock(&state.script) = Some((script, payload.clone()));
    lock(&state.dumb).pos = 0.0;
    lock(&state.config).last_script = path;
    state.emit_config(&app);
    Ok(payload)
}

#[tauri::command]
fn available_engines() -> Vec<String> {
    linefeed_asr::available_engines()
        .into_iter()
        .map(String::from)
        .collect()
}

#[cfg(feature = "mic")]
#[tauri::command]
fn list_devices() -> Result<Vec<DevicePayload>, String> {
    let devices = linefeed_asr::mic::list_input_devices().map_err(|e| e.to_string())?;
    Ok(devices
        .into_iter()
        .map(|d| DevicePayload {
            index: d.index,
            name: d.name,
            default: d.default,
            configs: d.configs,
        })
        .collect())
}

#[cfg(not(feature = "mic"))]
#[tauri::command]
fn list_devices() -> Result<Vec<DevicePayload>, String> {
    Err("microphone support not built in — rebuild with the `mic` feature".to_string())
}

#[cfg(feature = "mic")]
#[tauri::command(async)]
fn start(app: tauri::AppHandle, state: AppState) -> Result<(), String> {
    session::start(&app, &state)
}

#[cfg(not(feature = "mic"))]
#[tauri::command(async)]
fn start(_app: tauri::AppHandle, _state: AppState) -> Result<(), String> {
    Err("microphone support not built in — rebuild with the `mic` feature".to_string())
}

#[cfg(feature = "mic")]
#[tauri::command(async)]
fn stop(app: tauri::AppHandle, state: AppState) -> Result<(), String> {
    session::stop(&app, &state);
    Ok(())
}

#[cfg(not(feature = "mic"))]
#[tauri::command(async)]
fn stop(_app: tauri::AppHandle, _state: AppState) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
fn get_config(state: AppState) -> GuiConfig {
    lock(&state.config).clone()
}

#[tauri::command]
fn get_status(state: AppState) -> StatusPayload {
    lock(&state.status).clone()
}

/// One macro-free setter path: mutate, sanitize, persist, emit, return.
fn update_config(
    app: &tauri::AppHandle,
    state: &AppState,
    f: impl FnOnce(&mut GuiConfig),
) -> GuiConfig {
    {
        let mut cfg = lock(&state.config);
        f(&mut cfg);
        config::sanitize(&mut cfg);
    }
    state.emit_config(app)
}

#[tauri::command]
fn set_scroll_mode(app: tauri::AppHandle, state: AppState, mode: String) -> GuiConfig {
    let cfg = update_config(&app, &state, |c| c.scroll_mode = mode);
    if cfg.scroll_mode != "dumb" {
        lock(&state.dumb).playing = false;
        state.dumb_active.store(false, Ordering::SeqCst);
    }
    cfg
}

#[tauri::command]
fn set_speed(app: tauri::AppHandle, state: AppState, wpm: u32) -> GuiConfig {
    update_config(&app, &state, |c| c.wpm = wpm)
}

#[tauri::command]
fn set_mirror(app: tauri::AppHandle, state: AppState, h: bool, v: bool) -> GuiConfig {
    update_config(&app, &state, |c| {
        c.mirror_h = h;
        c.mirror_v = v;
    })
}

#[tauri::command]
fn set_font(app: tauri::AppHandle, state: AppState, px: u32) -> GuiConfig {
    update_config(&app, &state, |c| c.font_px = px)
}

#[tauri::command]
fn set_reading_font(app: tauri::AppHandle, state: AppState, id: String) -> GuiConfig {
    update_config(&app, &state, |c| c.reading_font = id)
}

#[tauri::command]
fn set_reading_zone(app: tauri::AppHandle, state: AppState, width: u32, height: u32) -> GuiConfig {
    update_config(&app, &state, |c| {
        c.reading_width = width;
        c.reading_height = height;
    })
}

#[tauri::command]
fn set_lead(app: tauri::AppHandle, state: AppState, lines: u32) -> GuiConfig {
    update_config(&app, &state, |c| c.lead_lines = lines)
}

#[tauri::command]
fn set_engine(app: tauri::AppHandle, state: AppState, engine: String) -> GuiConfig {
    update_config(&app, &state, |c| c.engine = engine)
}

#[tauri::command]
fn set_device(app: tauri::AppHandle, state: AppState, device: String) -> GuiConfig {
    update_config(&app, &state, |c| c.device = device)
}

#[tauri::command]
fn set_debug_log(app: tauri::AppHandle, state: AppState, on: bool) -> GuiConfig {
    update_config(&app, &state, |c| c.debug_log = on)
}

/// Play/pause dumb scroll (and force dumb mode — the frontend only calls
/// this from dumb-mode controls).
#[tauri::command]
fn dumb_play(app: tauri::AppHandle, state: AppState, playing: bool) -> GuiConfig {
    {
        let mut dumb = lock(&state.dumb);
        dumb.playing = playing;
    }
    state.dumb_active.store(playing, Ordering::SeqCst);
    let cfg = update_config(&app, &state, |c| c.scroll_mode = "dumb".to_string());
    emit_dumb(&app, &state);
    cfg
}

#[tauri::command]
fn dumb_seek(app: tauri::AppHandle, state: AppState, cursor: usize) {
    lock(&state.dumb).pos = cursor as f64;
    emit_dumb(&app, &state);
}

fn emit_dumb(app: &tauri::AppHandle, state: &Shared) {
    let (pos, playing) = {
        let d = lock(&state.dumb);
        (d.pos, d.playing)
    };
    let wpm = lock(&state.config).wpm;
    let _ = app.emit(
        EV_DUMB,
        DumbEvent {
            t: pos,
            cursor: pos as usize,
            wpm,
            playing,
        },
    );
}

#[tauri::command]
fn set_model(app: tauri::AppHandle, state: AppState, model: String) -> GuiConfig {
    update_config(&app, &state, |c| c.model = model)
}

#[tauri::command]
fn startup_probe(state: AppState) -> StartupInfoPayload {
    let cfg = lock(&state.config).clone();
    let models_dir = linefeed_asr::models_dir();
    let spec = linefeed_asr::models::model_spec_or_default(&cfg.model);
    let missing = linefeed_asr::models::missing_files(spec);
    let fetchable = linefeed_asr::engine_available("sherpa") && !missing.is_empty();
    let models = linefeed_asr::models::MODELS
        .iter()
        .map(|m| ModelInfoPayload {
            id: m.id.to_string(),
            label: m.label.to_string(),
            lang: m.lang.to_string(),
            archive_bytes: m.archive_bytes,
            installed: linefeed_asr::models::model_installed(m),
        })
        .collect();
    StartupInfoPayload {
        engines: available_engines(),
        engine: cfg.engine,
        model: spec.id.to_string(),
        models,
        models_dir: models_dir.to_string_lossy().into_owned(),
        models_ok: missing.is_empty(),
        missing,
        fetchable,
        fetch_url: spec.url.to_string(),
        fetch_bytes: spec.archive_bytes,
        last_script: cfg.last_script,
    }
}

/// Start downloading a registry model — `model: None` means the selected
/// one. One download at a time.
#[tauri::command(async)]
fn download_model(
    app: tauri::AppHandle,
    state: AppState,
    model: Option<String>,
) -> Result<(), String> {
    let id = model.unwrap_or_else(|| lock(&state.config).model.clone());
    let spec =
        linefeed_asr::models::model_spec(&id).ok_or_else(|| format!("unknown model {id:?}"))?;
    {
        let mut slot = lock(&state.fetch);
        if slot.is_some() {
            return Err("a model download is already running".to_string());
        }
        *slot = Some(model_fetch::spawn(linefeed_asr::models_dir(), spec));
    }
    // Forward worker events until the terminal one, then clear the slot.
    let shared = state.inner().clone();
    std::thread::spawn(move || {
        loop {
            let ev = {
                let slot = lock(&shared.fetch);
                match slot.as_ref() {
                    Some(h) => h.events.recv().ok(),
                    None => None,
                }
            };
            let Some(ev) = ev else { break };
            let terminal = matches!(ev.phase.as_str(), "ready" | "cancelled" | "fatal");
            let _ = app.emit(EV_FETCH, ev);
            if terminal {
                break;
            }
        }
        if let Some(h) = lock(&shared.fetch).take() {
            h.wait();
        }
    });
    Ok(())
}

#[tauri::command(async)]
fn cancel_model_fetch(state: AppState) {
    if let Some(h) = lock(&state.fetch).as_ref() {
        h.cancel();
    }
}

// -------------------------------------------------------------- dumb ticker

/// 10 Hz advance for dumb-scroll mode. Idles on an atomic — zero locks
/// taken while not playing.
fn dumb_ticker(app: tauri::AppHandle, shared: Arc<Shared>) {
    let mut last = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !shared.dumb_active.load(Ordering::SeqCst) {
            last = std::time::Instant::now();
            continue;
        }
        let dt = last.elapsed().as_secs_f64();
        last = std::time::Instant::now();
        let (wpm, tokens_per_word) = {
            let cfg = lock(&shared.config);
            let tpw = lock(&shared.script)
                .as_ref()
                .map(|(s, _)| {
                    if s.n_words() == 0 {
                        1.0
                    } else {
                        s.n_tokens() as f64 / s.n_words() as f64
                    }
                })
                .unwrap_or(1.0);
            (cfg.wpm, tpw)
        };
        let n_tokens = lock(&shared.script)
            .as_ref()
            .map(|(s, _)| s.n_tokens())
            .unwrap_or(0);
        {
            let mut dumb = lock(&shared.dumb);
            if !dumb.playing {
                continue;
            }
            let rate = wpm as f64 / 60.0 * tokens_per_word;
            dumb.pos = (dumb.pos + rate * dt).min(n_tokens as f64);
            if dumb.pos >= n_tokens as f64 {
                dumb.playing = false;
                shared.dumb_active.store(false, Ordering::SeqCst);
            }
        }
        emit_dumb(&app, &shared);
    }
}

// --------------------------------------------------------------------- run

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_path = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("config.json");
            let cfg = config::load(&config_path);
            let shared = Arc::new(Shared {
                script: Mutex::new(None),
                #[cfg(feature = "mic")]
                session: Mutex::new(None),
                dumb: Mutex::new(DumbState {
                    pos: 0.0,
                    playing: false,
                }),
                config: Mutex::new(cfg),
                status: Mutex::new(StatusPayload {
                    state: "idle".to_string(),
                    ..StatusPayload::default()
                }),
                fetch: Mutex::new(None),
                dumb_active: AtomicBool::new(false),
                config_path,
            });
            app.manage(shared.clone());
            let handle = app.handle().clone();
            std::thread::spawn(move || dumb_ticker(handle, shared));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_script,
            available_engines,
            list_devices,
            start,
            stop,
            get_config,
            get_status,
            set_scroll_mode,
            set_speed,
            set_mirror,
            set_font,
            set_reading_font,
            set_reading_zone,
            set_lead,
            set_engine,
            set_model,
            set_device,
            set_debug_log,
            dumb_play,
            dumb_seek,
            startup_probe,
            download_model,
            cancel_model_fetch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
