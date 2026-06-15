//! On-disk storage for chat sessions.
//!
//! Each session lives in its own JSON file under
//! `~/.config/bobirc/sessions/<id>.json`. The file holds the full
//! session metadata + the message array + the per-session system
//! prompt. We deliberately don't keep a separate index file: the
//! `list_sessions` call just globs the directory and parses each
//! file's metadata. With at most a few hundred sessions, this is
//! fast enough and avoids a stale-index sync problem.

use crate::app::ChatMessage;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMeta {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Session {
    #[serde(flatten)]
    pub meta: SessionMeta,
    pub messages: Vec<ChatMessage>,
    pub system_prompt: String,
}

fn sessions_dir() -> Result<PathBuf> {
    let base = crate::config::config_path()?
        .parent()
        .context("no parent dir for config")?
        .to_path_buf();
    let dir = base.join("sessions");
    std::fs::create_dir_all(&dir).context("create sessions dir")?;
    Ok(dir)
}

fn path_for(id: &str) -> Result<PathBuf> {
    Ok(sessions_dir()?.join(format!("{}.json", id)))
}

pub fn new_session_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{:x}", ts)
}

pub fn list_sessions() -> Result<Vec<SessionMeta>> {
    let dir = sessions_dir()?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Ok(sess) = serde_json::from_str::<Session>(&text) {
            out.push(sess.meta);
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

pub fn load_session(id: &str) -> Result<Session> {
    let path = path_for(id)?;
    let text = std::fs::read_to_string(&path).context("read session file")?;
    serde_json::from_str(&text).context("parse session file")
}

pub fn save_session(session: &Session) -> Result<()> {
    let path = path_for(&session.meta.id)?;
    let text = serde_json::to_string_pretty(session).context("serialize session")?;
    std::fs::write(&path, text).context("write session file")?;
    Ok(())
}

pub fn delete_session(id: &str) -> Result<()> {
    let path = path_for(id)?;
    if path.exists() {
        std::fs::remove_file(&path).context("remove session file")?;
    }
    Ok(())
}

pub fn auto_name(first_user_msg: &str) -> String {
    let flat: String = first_user_msg
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let trimmed = flat.trim();
    if trimmed.is_empty() {
        return "New chat".to_string();
    }
    let max = 30usize;
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        let cut: String = trimmed.chars().take(max.saturating_sub(1)).collect();
        let last_space = cut.rfind(' ').unwrap_or(cut.len());
        let trimmed_cut = cut[..last_space].trim_end();
        if trimmed_cut.is_empty() {
            format!("{}…", cut)
        } else {
            format!("{}…", trimmed_cut)
        }
    }
}
