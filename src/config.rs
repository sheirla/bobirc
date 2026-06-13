use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub nick: String,
}

impl Config {
    pub fn is_configured(&self) -> bool {
        !self.base_url.trim().is_empty() && !self.api_key.trim().is_empty()
    }
}

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("no config dir on this platform")?;
    let dir = base.join("bobric");
    std::fs::create_dir_all(&dir).context("create config dir")?;
    Ok(dir.join("config.json"))
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config {
            nick: "bob".to_string(),
            ..Default::default()
        });
    }
    let text = std::fs::read_to_string(&path).context("read config")?;
    let cfg: Config = serde_json::from_str(&text).context("parse config")?;
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    let text = serde_json::to_string_pretty(cfg).context("serialize config")?;
    std::fs::write(&path, text).context("write config")?;
    Ok(())
}

pub fn normalize_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return trimmed;
    }
    // accept either ".../v1" or just root; keep as-is, api layer will append /models
    trimmed
}
