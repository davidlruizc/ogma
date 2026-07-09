use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// User settings, persisted as JSON in the app data dir.
///
/// The three API secrets (OpenAI / Anthropic / Notion) are kept out of the
/// JSON: `save` writes them to the OS keychain and blanks them on disk, and
/// `load` re-hydrates them from the keychain. On platforms without a native
/// keystore, or if the keychain call fails, we transparently fall back to
/// plaintext-in-JSON so keys are never lost. See the `secrets` module.
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
            let mut cfg = Config::with_defaults(Config::default());
            secrets::hydrate(&mut cfg);
            return Ok(cfg);
        }
        let raw = std::fs::read_to_string(&path)?;
        let cfg: Config = serde_json::from_str(&raw)
            .map_err(|e| Error::Config(format!("failed to parse {}: {e}", path.display())))?;
        let mut cfg = Config::with_defaults(cfg);
        // JSON wins if it still carries a plaintext key (legacy config); the
        // next `save` migrates it into the keychain and blanks it on disk.
        secrets::hydrate(&mut cfg);
        Ok(cfg)
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let path = Self::config_path(data_dir);
        // Move secrets into the keychain; the on-disk copy has them blanked
        // (or left in place if the keychain was unavailable).
        let on_disk = secrets::externalize(self);
        std::fs::write(&path, serde_json::to_string_pretty(&on_disk)?)?;
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

/// OS-keychain storage for the three API secrets. Enabled on Windows (Credential
/// Manager) and macOS (Keychain); a no-op fallback elsewhere keeps the secrets
/// in `config.json` so no target loses keys.
#[cfg(any(windows, target_os = "macos"))]
mod secrets {
    use super::Config;

    const SERVICE: &str = "com.davidruiz.ogma";

    fn entry(account: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, account)
            .map_err(|e| tracing::warn!("keychain unavailable for {account}: {e}"))
            .ok()
    }

    fn get(account: &str) -> Option<String> {
        match entry(account)?.get_password() {
            Ok(p) => Some(p),
            Err(keyring::Error::NoEntry) => None,
            Err(e) => {
                tracing::warn!("keychain read {account} failed: {e}");
                None
            }
        }
    }

    /// Store or clear one secret. Returns true when the value is safely handled
    /// by the keychain (so it can be blanked in `config.json`).
    fn put(account: &str, value: &str) -> bool {
        let Some(entry) = entry(account) else {
            return false;
        };
        if value.is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => tracing::warn!("keychain delete {account} failed: {e}"),
            }
            true
        } else {
            match entry.set_password(value) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!("keychain write {account} failed, keeping it in config.json: {e}");
                    false
                }
            }
        }
    }

    /// Fill empty secret fields from the keychain (a non-empty JSON value wins,
    /// for backward compatibility with legacy plaintext configs).
    pub fn hydrate(cfg: &mut Config) {
        if cfg.openai_api_key.is_empty() {
            if let Some(v) = get("openai_api_key") {
                cfg.openai_api_key = v;
            }
        }
        if cfg.anthropic_api_key.is_empty() {
            if let Some(v) = get("anthropic_api_key") {
                cfg.anthropic_api_key = v;
            }
        }
        if cfg.notion_api_key.is_empty() {
            if let Some(v) = get("notion_api_key") {
                cfg.notion_api_key = v;
            }
        }
    }

    /// Persist secrets to the keychain and return a copy with the stored ones
    /// blanked — safe to write to `config.json`.
    pub fn externalize(cfg: &Config) -> Config {
        let mut out = cfg.clone();
        if put("openai_api_key", &cfg.openai_api_key) {
            out.openai_api_key.clear();
        }
        if put("anthropic_api_key", &cfg.anthropic_api_key) {
            out.anthropic_api_key.clear();
        }
        if put("notion_api_key", &cfg.notion_api_key) {
            out.notion_api_key.clear();
        }
        out
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod secrets {
    use super::Config;

    pub fn hydrate(_cfg: &mut Config) {}

    pub fn externalize(cfg: &Config) -> Config {
        cfg.clone()
    }
}
