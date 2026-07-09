use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// User settings, persisted as JSON in the app data dir.
///
/// Plaintext on disk for v1 (same trust level as the audio recordings next to
/// it); OS-keychain storage is a listed follow-up in PLAN.md.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub openai_api_key: String,
    pub anthropic_api_key: String,
    pub notion_api_key: String,
    /// Data-source/database that meeting pages get created in. Set via Notion
    /// setup (create) or pasted by the user (select existing).
    pub notion_database_id: String,
    pub notes_model: String,
    pub whisper_model: String,
    /// Optional language hint for Whisper (e.g. "en", "es"). Empty = auto.
    pub language: String,
    /// cpal name of the microphone to record from. Empty = host default.
    pub input_device: String,
}

impl Config {
    pub fn config_path(data_dir: &Path) -> PathBuf {
        data_dir.join("config.json")
    }

    pub fn load(data_dir: &Path) -> Result<Config> {
        let path = Self::config_path(data_dir);
        if !path.exists() {
            return Ok(Config::with_defaults(Config::default()));
        }
        let raw = std::fs::read_to_string(&path)?;
        let cfg: Config = serde_json::from_str(&raw)
            .map_err(|e| Error::Config(format!("failed to parse {}: {e}", path.display())))?;
        Ok(Config::with_defaults(cfg))
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let path = Self::config_path(data_dir);
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    fn with_defaults(mut cfg: Config) -> Config {
        if cfg.notes_model.is_empty() {
            cfg.notes_model = "claude-sonnet-5".to_string();
        }
        if cfg.whisper_model.is_empty() {
            cfg.whisper_model = "whisper-1".to_string();
        }
        cfg
    }
}
