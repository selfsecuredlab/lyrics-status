//! Port of src/Debug.ts — best-effort append-only diagnostics log. Never panics.

use std::path::PathBuf;
use std::sync::OnceLock;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init(dir: PathBuf) {
    let _ = LOG_DIR.set(dir);
}

pub fn write(text: &str) {
    let dir = match LOG_DIR.get() {
        Some(d) => d.clone(),
        None => PathBuf::from("."),
    };
    let stamp = chrono::Local::now().format("%H:%M:%S");
    let line = format!("[{stamp}]: {text}\n");
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("log.txt"))
    {
        let _ = f.write_all(line.as_bytes());
    }
}
