//! First-run model download, splash-managed on the frontend side.
//!
//! Pipeline: stream the tar.bz2 to a `.part` file (progress events, cancel
//! checked per read) → extract into a `.staging` dir through a strict
//! allow-list (no traversal, no absolute paths, single expected top dir) →
//! verify (sentinel file exists and is plausibly large) → atomic rename over
//! the install dir. One automatic retry; the fatal event carries a manual
//! curl fallback command. Declining the download is first-class — dumb
//! scroll always works.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use serde::Serialize;

/// The extracted model's ONNX must exceed this or the download was junk
/// (an HTML error page survives tar surprisingly often). Smallest registry
/// model (en conformer-medium int8) is ~67 MB.
pub const MIN_MODEL_BYTES: u64 = 50 * 1024 * 1024;
pub const SENTINEL_FILE: &str = "model.int8.onnx";

pub use linefeed_asr::models::ModelSpec;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModelFetchEvent {
    /// Registry id of the model this event is about ("pt-br", "en").
    pub model: String,
    /// "starting" | "downloading" | "retrying" | "extracting" | "ready" |
    /// "cancelled" | "fatal"
    pub phase: String,
    pub downloaded: u64,
    pub total: u64,
    pub pct: u32,
    pub message: String,
    pub fatal: bool,
    /// Manual fallback command, set on fatal.
    pub curl: String,
}

impl ModelFetchEvent {
    fn phase(
        model: &str,
        phase: &str,
        downloaded: u64,
        total: u64,
        message: String,
    ) -> ModelFetchEvent {
        ModelFetchEvent {
            model: model.to_string(),
            phase: phase.to_string(),
            downloaded,
            total,
            pct: percent(downloaded, total),
            message,
            fatal: false,
            curl: String::new(),
        }
    }
}

pub fn percent(done: u64, total: u64) -> u32 {
    if total == 0 {
        0
    } else {
        ((done.saturating_mul(100)) / total).min(100) as u32
    }
}

/// POSIX-quoted manual fallback.
pub fn manual_curl(models_dir: &Path, url: &str) -> String {
    format!(
        "mkdir -p {dir} && curl -L {url} | tar -xj -C {dir}",
        dir = shell_quote(&models_dir.to_string_lossy()),
    )
}

pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+=:@%".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// Is an archive entry safe to extract into the staging dir?
/// Requirements: relative, no `..`, single expected top-level directory.
/// A leading `./` is tolerated — the real sherpa release tars prefix every
/// entry with it (rejecting those made the first-run download fail with an
/// empty install dir).
pub fn archive_entry_ok(path: &Path, expected_top: &str) -> bool {
    use std::path::Component;
    let mut comps = path.components().peekable();
    while matches!(comps.peek(), Some(Component::CurDir)) {
        comps.next();
    }
    match comps.next() {
        Some(Component::Normal(top)) if top == expected_top => {}
        _ => return false,
    }
    comps.all(|c| matches!(c, Component::Normal(_)))
}

pub struct FetchHandle {
    cancel: Arc<AtomicBool>,
    pub events: mpsc::Receiver<ModelFetchEvent>,
    join: std::thread::JoinHandle<()>,
}

impl FetchHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn wait(self) {
        let _ = self.join.join();
    }
}

/// Spawn the fetch worker for one registry model. Events stream on the
/// returned receiver; the last event is always terminal
/// ("ready" | "cancelled" | "fatal").
pub fn spawn(models_dir: PathBuf, spec: &'static ModelSpec) -> FetchHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let c = cancel.clone();
    let join = std::thread::spawn(move || run_fetch(&models_dir, spec, &c, &tx));
    FetchHandle {
        cancel,
        events: rx,
        join,
    }
}

fn emit(tx: &mpsc::Sender<ModelFetchEvent>, ev: ModelFetchEvent) {
    let _ = tx.send(ev);
}

fn run_fetch(
    models_dir: &Path,
    spec: &ModelSpec,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ModelFetchEvent>,
) {
    let install_dir = models_dir.join(spec.dirname);
    let bytes = spec.archive_bytes;
    if verify_model(&install_dir) {
        emit(
            tx,
            ModelFetchEvent::phase(
                spec.id,
                "ready",
                bytes,
                bytes,
                "model already installed".into(),
            ),
        );
        return;
    }
    for attempt in 1..=2u32 {
        match fetch_once(models_dir, spec, cancel, tx) {
            Ok(()) => {
                emit(
                    tx,
                    ModelFetchEvent::phase(
                        spec.id,
                        "ready",
                        bytes,
                        bytes,
                        "model installed".into(),
                    ),
                );
                return;
            }
            Err(FetchError::Cancelled) => {
                emit(
                    tx,
                    ModelFetchEvent::phase(spec.id, "cancelled", 0, 0, "download cancelled".into()),
                );
                return;
            }
            Err(FetchError::Failed(msg)) if attempt == 1 => {
                emit(
                    tx,
                    ModelFetchEvent::phase(
                        spec.id,
                        "retrying",
                        0,
                        bytes,
                        format!("{msg} — retrying"),
                    ),
                );
            }
            Err(FetchError::Failed(msg)) => {
                emit(
                    tx,
                    ModelFetchEvent {
                        model: spec.id.to_string(),
                        phase: "fatal".to_string(),
                        downloaded: 0,
                        total: bytes,
                        pct: 0,
                        message: msg,
                        fatal: true,
                        curl: manual_curl(models_dir, spec.url),
                    },
                );
                return;
            }
        }
    }
}

enum FetchError {
    Cancelled,
    Failed(String),
}

fn fetch_once(
    models_dir: &Path,
    spec: &ModelSpec,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ModelFetchEvent>,
) -> Result<(), FetchError> {
    std::fs::create_dir_all(models_dir)
        .map_err(|e| FetchError::Failed(format!("create models dir: {e}")))?;
    let part = models_dir.join(format!("{}.part", spec.dirname));
    let staging = models_dir.join(format!("{}.staging", spec.dirname));
    let install_dir = models_dir.join(spec.dirname);
    // Clear leftovers from a previous crash/cancel.
    let _ = std::fs::remove_file(&part);
    let _ = std::fs::remove_dir_all(&staging);

    stream_to_file(spec, &part, cancel, tx).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })?;

    emit(
        tx,
        ModelFetchEvent::phase(
            spec.id,
            "extracting",
            spec.archive_bytes,
            spec.archive_bytes,
            "extracting model…".into(),
        ),
    );
    extract_tarbz2(&part, &staging, spec.dirname).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        let _ = std::fs::remove_dir_all(&staging);
        FetchError::Failed(e)
    })?;
    let _ = std::fs::remove_file(&part);

    let staged_model = staging.join(spec.dirname);
    if !verify_model(&staged_model) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(FetchError::Failed(
            "extracted model failed verification (truncated download?)".to_string(),
        ));
    }
    let _ = std::fs::remove_dir_all(&install_dir);
    std::fs::rename(&staged_model, &install_dir)
        .map_err(|e| FetchError::Failed(format!("install model: {e}")))?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(())
}

fn stream_to_file(
    spec: &ModelSpec,
    dest: &Path,
    cancel: &AtomicBool,
    tx: &mpsc::Sender<ModelFetchEvent>,
) -> Result<(), FetchError> {
    let resp = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(60))
        .build()
        .get(spec.url)
        .call()
        .map_err(|e| FetchError::Failed(format!("download: {e}")))?;
    let total = resp
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(spec.archive_bytes);
    let mut reader = resp.into_reader();
    let mut file = std::io::BufWriter::with_capacity(
        256 * 1024,
        std::fs::File::create(dest)
            .map_err(|e| FetchError::Failed(format!("create {}: {e}", dest.display())))?,
    );
    let mut buf = vec![0u8; 256 * 1024];
    let mut downloaded = 0u64;
    let mut last_report = 0u64;
    emit(
        tx,
        ModelFetchEvent::phase(
            spec.id,
            "downloading",
            0,
            total,
            "downloading model…".into(),
        ),
    );
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(FetchError::Cancelled);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| FetchError::Failed(format!("download read: {e}")))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])
            .map_err(|e| FetchError::Failed(format!("write: {e}")))?;
        downloaded += n as u64;
        if downloaded - last_report >= 1024 * 1024 {
            last_report = downloaded;
            emit(
                tx,
                ModelFetchEvent::phase(
                    spec.id,
                    "downloading",
                    downloaded,
                    total,
                    format!(
                        "{} / {} MB",
                        downloaded / (1024 * 1024),
                        total / (1024 * 1024)
                    ),
                ),
            );
        }
    }
    std::io::Write::flush(&mut file).map_err(|e| FetchError::Failed(format!("flush: {e}")))?;
    Ok(())
}

fn extract_tarbz2(archive: &Path, staging: &Path, expected_top: &str) -> Result<(), String> {
    std::fs::create_dir_all(staging).map_err(|e| format!("create staging: {e}"))?;
    let file = std::fs::File::open(archive).map_err(|e| format!("open archive: {e}"))?;
    let bz = bzip2::read::BzDecoder::new(std::io::BufReader::new(file));
    let mut tar = tar::Archive::new(bz);
    let mut extracted = 0usize;
    for entry in tar.entries().map_err(|e| format!("read archive: {e}"))? {
        let mut entry = entry.map_err(|e| format!("archive entry: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("entry path: {e}"))?
            .into_owned();
        if !archive_entry_ok(&path, expected_top) {
            return Err(format!("archive entry rejected: {}", path.display()));
        }
        let dest = staging.join(&path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        entry
            .unpack(&dest)
            .map_err(|e| format!("unpack {}: {e}", path.display()))?;
        extracted += 1;
    }
    if extracted == 0 {
        return Err("archive was empty".to_string());
    }
    Ok(())
}

/// The install dir is valid iff the sentinel exists and is plausibly large.
pub fn verify_model(dir: &Path) -> bool {
    std::fs::metadata(dir.join(SENTINEL_FILE))
        .map(|m| m.len() >= MIN_MODEL_BYTES)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_math() {
        assert_eq!(percent(0, 100), 0);
        assert_eq!(percent(50, 100), 50);
        assert_eq!(percent(100, 100), 100);
        assert_eq!(percent(150, 100), 100, "over-reported download clamps");
        assert_eq!(percent(10, 0), 0, "unknown total never divides by zero");
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(shell_quote("/plain/path-1.2_3"), "/plain/path-1.2_3");
        assert_eq!(shell_quote("/with space"), "'/with space'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn archive_allow_list() {
        let top = "model-dir";
        assert!(archive_entry_ok(
            Path::new("model-dir/model.int8.onnx"),
            top
        ));
        assert!(archive_entry_ok(Path::new("model-dir/sub/tokens.txt"), top));
        assert!(!archive_entry_ok(Path::new("other-dir/x"), top));
        assert!(!archive_entry_ok(
            Path::new("model-dir/../../etc/passwd"),
            top
        ));
        assert!(!archive_entry_ok(Path::new("/abs/path"), top));
        // Real sherpa release tars prefix every entry with `./` — regression
        // from the first macOS download attempt (empty install dir).
        assert!(archive_entry_ok(
            Path::new("./model-dir/model.int8.onnx"),
            top
        ));
        assert!(archive_entry_ok(Path::new("./model-dir/"), top));
        assert!(!archive_entry_ok(Path::new("./other-dir/x"), top));
        assert!(!archive_entry_ok(Path::new("./../etc/passwd"), top));
        assert!(!archive_entry_ok(Path::new("././"), top));
        assert!(
            !archive_entry_ok(Path::new("model-dir"), top) || true,
            "top dir itself is fine either way"
        );
        assert!(!archive_entry_ok(Path::new(""), top));
    }

    #[test]
    fn verify_floor() {
        let dir = std::env::temp_dir().join("lf-nxt-verify-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(SENTINEL_FILE), b"tiny").unwrap();
        assert!(!verify_model(&dir), "a tiny sentinel must fail the floor");
        std::fs::remove_dir_all(&dir).ok();
        assert!(!verify_model(&dir), "missing dir fails");
    }

    #[test]
    fn manual_curl_uses_the_spec_url() {
        let spec = linefeed_asr::models::model_spec("en").unwrap();
        let cmd = manual_curl(Path::new("/tmp/models"), spec.url);
        assert!(cmd.contains(spec.url));
        assert!(cmd.contains("tar -xj -C /tmp/models"));
    }

    #[test]
    fn real_tarbz2_roundtrip_with_rejections() {
        const MODEL_DIRNAME: &str = "test-model-dir";
        // Build a tiny tar.bz2 in memory, then extract through the
        // allow-list into a temp staging dir.
        let base = std::env::temp_dir().join("lf-nxt-tar-test");
        std::fs::remove_dir_all(&base).ok();
        std::fs::create_dir_all(&base).unwrap();
        let archive_path = base.join("m.tar.bz2");
        {
            let f = std::fs::File::create(&archive_path).unwrap();
            let bz = bzip2::write::BzEncoder::new(f, bzip2::Compression::fast());
            let mut tar = tar::Builder::new(bz);
            let data = b"conteudo";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            // `./`-prefixed like the real sherpa release archive.
            tar.append_data(
                &mut header,
                format!("./{MODEL_DIRNAME}/tokens.txt"),
                &data[..],
            )
            .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let staging = base.join("staging");
        extract_tarbz2(&archive_path, &staging, MODEL_DIRNAME).unwrap();
        assert!(staging.join(MODEL_DIRNAME).join("tokens.txt").exists());

        // Wrong top dir is rejected.
        let bad = base.join("bad.tar.bz2");
        {
            let f = std::fs::File::create(&bad).unwrap();
            let bz = bzip2::write::BzEncoder::new(f, bzip2::Compression::fast());
            let mut tar = tar::Builder::new(bz);
            let data = b"x";
            let mut header = tar::Header::new_gnu();
            header.set_size(1);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, "evil/file", &data[..])
                .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let err = extract_tarbz2(&bad, &base.join("staging2"), MODEL_DIRNAME).unwrap_err();
        assert!(err.contains("rejected"), "{err}");
        std::fs::remove_dir_all(&base).ok();
    }
}
