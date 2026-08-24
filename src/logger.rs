use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

pub struct Logger {
    file: Mutex<File>,
    date: String,
}

impl Logger {
    pub fn new(date: &str) -> Result<Self> {
        std::fs::create_dir_all("log").context("create log dir")?;
        let path = format!("log/{date}.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open {path}"))?;
        Ok(Self {
            file: Mutex::new(file),
            date: date.to_string(),
        })
    }

    pub fn log(&self, msg: &str) {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let line = format!("[{ts}] {msg}");
        eprintln!("{line}");
        if let Ok(mut f) = self.file.lock() {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }

    pub fn date(&self) -> &str {
        &self.date
    }
}
