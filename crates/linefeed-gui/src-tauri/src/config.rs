//! Persisted GUI configuration: serde-defaulted, clamped on every load and
//! mutation, written pretty-printed to the app config dir.

use serde::{Deserialize, Serialize};

/// Reading-font ids the frontend dropdown offers; the single source of
/// truth mirrored by the frontend's READING_FONTS (contract-tested there).
pub const READING_FONT_IDS: &[&str] = &[
    "inter",
    "atkinson",
    "source-sans-3",
    "noto-sans",
    "georgia",
    "system",
];

pub const FONT_PX_RANGE: (u32, u32) = (24, 200);
pub const READING_WIDTH_RANGE: (u32, u32) = (40, 100);
pub const READING_HEIGHT_RANGE: (u32, u32) = (30, 100);
pub const WPM_RANGE: (u32, u32) = (40, 400);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GuiConfig {
    pub engine: String,
    /// Input device selector (substring or 1-based index); empty = default.
    pub device: String,
    /// "voice" or "dumb".
    pub scroll_mode: String,
    pub wpm: u32,
    pub font_px: u32,
    pub reading_font: String,
    pub mirror_h: bool,
    pub mirror_v: bool,
    /// Reading zone, percent of the stage.
    pub reading_width: u32,
    pub reading_height: u32,
    /// Lookahead lines below the reading anchor (0..=3).
    pub lead_lines: u32,
    pub debug_log: bool,
    pub last_script: String,
}

impl Default for GuiConfig {
    fn default() -> GuiConfig {
        GuiConfig {
            engine: "sherpa".to_string(),
            device: String::new(),
            scroll_mode: "voice".to_string(),
            wpm: 140,
            font_px: 56,
            reading_font: "inter".to_string(),
            mirror_h: false,
            mirror_v: false,
            reading_width: 90,
            reading_height: 80,
            lead_lines: 1,
            debug_log: false,
            last_script: String::new(),
        }
    }
}

fn clamp(v: u32, (lo, hi): (u32, u32)) -> u32 {
    v.clamp(lo, hi)
}

/// Clamp every field into its legal range and coerce the engine to one that
/// is actually compiled in (guards the "persisted engine no longer built"
/// crash class).
pub fn sanitize(cfg: &mut GuiConfig) {
    cfg.font_px = clamp(cfg.font_px, FONT_PX_RANGE);
    cfg.reading_width = clamp(cfg.reading_width, READING_WIDTH_RANGE);
    cfg.reading_height = clamp(cfg.reading_height, READING_HEIGHT_RANGE);
    cfg.wpm = clamp(cfg.wpm, WPM_RANGE);
    cfg.lead_lines = cfg.lead_lines.min(3);
    if !READING_FONT_IDS.contains(&cfg.reading_font.as_str()) {
        cfg.reading_font = "inter".to_string();
    }
    if cfg.scroll_mode != "dumb" {
        cfg.scroll_mode = "voice".to_string();
    }
    let engines = linefeed_asr::available_engines();
    if !engines.contains(&cfg.engine.as_str()) {
        cfg.engine = engines.first().copied().unwrap_or("sherpa").to_string();
    }
}

pub fn load(path: &std::path::Path) -> GuiConfig {
    let mut cfg = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    sanitize(&mut cfg);
    cfg
}

pub fn save(path: &std::path::Path, cfg: &GuiConfig) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let mut cfg = GuiConfig::default();
        sanitize(&mut cfg);
        assert_eq!(cfg.scroll_mode, "voice");
        assert_eq!(cfg.reading_font, "inter");
        assert!(cfg.font_px >= FONT_PX_RANGE.0 && cfg.font_px <= FONT_PX_RANGE.1);
    }

    #[test]
    fn sanitize_clamps_everything() {
        let mut cfg = GuiConfig {
            font_px: 9999,
            reading_width: 1,
            reading_height: 1,
            wpm: 1,
            lead_lines: 99,
            reading_font: "comic-sans".to_string(),
            scroll_mode: "warp".to_string(),
            engine: "whisper".to_string(),
            ..GuiConfig::default()
        };
        sanitize(&mut cfg);
        assert_eq!(cfg.font_px, FONT_PX_RANGE.1);
        assert_eq!(cfg.reading_width, READING_WIDTH_RANGE.0);
        assert_eq!(cfg.reading_height, READING_HEIGHT_RANGE.0);
        assert_eq!(cfg.wpm, WPM_RANGE.0);
        assert_eq!(cfg.lead_lines, 3);
        assert_eq!(cfg.reading_font, "inter");
        assert_eq!(cfg.scroll_mode, "voice");
        assert_ne!(cfg.engine, "whisper", "engine coerced to a compiled one");
    }

    #[test]
    fn legacy_partial_json_gets_defaults() {
        let cfg: GuiConfig = serde_json::from_str(r#"{"font_px": 72}"#).unwrap();
        assert_eq!(cfg.font_px, 72);
        assert_eq!(cfg.engine, "sherpa");
        assert_eq!(cfg.reading_width, 90);
    }

    #[test]
    fn save_load_roundtrip() {
        let path = std::env::temp_dir().join("lf-nxt-config-test/config.json");
        let mut cfg = GuiConfig::default();
        cfg.font_px = 64;
        cfg.last_script = "/tmp/x.txt".to_string();
        save(&path, &cfg);
        let back = load(&path);
        assert_eq!(back, cfg);
        std::fs::remove_file(&path).ok();
    }
}
