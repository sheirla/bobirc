use crate::app::ChatMessage;
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

fn history_path() -> Result<PathBuf> {
    let base = crate::config::config_path()?
        .parent()
        .context("no parent dir for config")?
        .to_path_buf();
    std::fs::create_dir_all(&base).context("create config dir")?;
    Ok(base.join("history.jsonl"))
}

pub fn append_message(msg: &ChatMessage) -> Result<()> {
    let path = history_path()?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .context("open history")?;
    let line = serde_json::to_string(msg).context("serialize message")?;
    writeln!(f, "{}", line).context("write history")?;
    Ok(())
}

pub fn clear() -> Result<()> {
    let path = history_path()?;
    if path.exists() {
        std::fs::remove_file(&path).context("remove history")?;
    }
    Ok(())
}
