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
    /// Folder the Markdown destination writes one `.md` per meeting into
    /// (e.g. an Obsidian vault folder). Empty = destination disabled.
    pub markdown_dir: String,
    /// Create a note per meeting in Apple Notes (macOS only; the field
    /// exists on every platform but only macOS builds act on it).
    pub apple_notes_enabled: bool,
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
        // A legacy config carries plaintext secrets directly in the JSON.
        let has_plaintext_secret = !cfg.openai_api_key.is_empty()
            || !cfg.anthropic_api_key.is_empty()
            || !cfg.notion_api_key.is_empty();
        let mut cfg = Config::with_defaults(cfg);
        // JSON wins if it still carries a plaintext key (legacy config).
        secrets::hydrate(&mut cfg);
        // Proactively migrate a legacy plaintext secret into the keychain rather
        // than leaving it on disk until some incidental future `save`. Only when
        // there is actually plaintext to move and a native keystore to move it
        // into; best-effort — a keychain failure keeps the plaintext (save never
        // drops keys), and once migrated the on-disk secrets are blank so this
        // does not fire again.
        if has_plaintext_secret && secrets::ENABLED {
            if let Err(e) = cfg.save(data_dir) {
                tracing::warn!("migrating legacy plaintext secrets to the keychain failed: {e}");
            }
        }
        Ok(cfg)
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let path = Self::config_path(data_dir);
        // Move secrets into the keychain; the on-disk copy has them blanked
        // (or left in place if the keychain was unavailable). A failed *removal*
        // of a secret surfaces as an error rather than silently succeeding.
        let on_disk = secrets::externalize(self)?;
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
    use crate::error::{Error, Result};

    /// This build has a native keystore, so `load` may proactively migrate
    /// legacy plaintext secrets into it.
    pub const ENABLED: bool = true;

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

    /// Store or clear one secret.
    ///
    /// * `Ok(true)`  — the value is safely in the keychain (or was removed), so
    ///   it can be blanked in `config.json`.
    /// * `Ok(false)` — a keychain *write* failed; the value is kept in
    ///   `config.json` as a fallback so a key is never lost.
    /// * `Err(_)`    — a keychain *removal* failed. We must NOT report the
    ///   secret as gone: doing so blanks it on disk while it survives in the
    ///   keychain, and the next `hydrate` would silently resurrect it. Surface
    ///   the failure instead.
    fn put(account: &str, value: &str) -> Result<bool> {
        let Some(entry) = entry(account) else {
            // No keychain available: keep any non-empty value in config.json.
            return Ok(value.is_empty());
        };
        if value.is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(true),
                Err(e) => Err(Error::Config(format!(
                    "failed to remove {account} from the keychain: {e}"
                ))),
            }
        } else {
            match entry.set_password(value) {
                Ok(()) => Ok(true),
                Err(e) => {
                    tracing::warn!("keychain write {account} failed, keeping it in config.json: {e}");
                    Ok(false)
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
    /// blanked — safe to write to `config.json`. Errors only if a secret could
    /// not be *removed* (see `put`).
    pub fn externalize(cfg: &Config) -> Result<Config> {
        let mut out = cfg.clone();
        if put("openai_api_key", &cfg.openai_api_key)? {
            out.openai_api_key.clear();
        }
        if put("anthropic_api_key", &cfg.anthropic_api_key)? {
            out.anthropic_api_key.clear();
        }
        if put("notion_api_key", &cfg.notion_api_key)? {
            out.notion_api_key.clear();
        }
        Ok(out)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod secrets {
    use super::Config;
    use crate::error::Result;

    /// No native keystore on this target, so there is nothing to migrate into.
    pub const ENABLED: bool = false;

    pub fn hydrate(_cfg: &mut Config) {}

    pub fn externalize(cfg: &Config) -> Result<Config> {
        Ok(cfg.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_secrets(openai: &str, anthropic: &str, notion: &str) -> Config {
        Config {
            openai_api_key: openai.to_string(),
            anthropic_api_key: anthropic.to_string(),
            notion_api_key: notion.to_string(),
            ..Config::default()
        }
    }

    /// Precedence: a non-empty (legacy plaintext) secret must never be
    /// overwritten by `hydrate`. Because the field is non-empty, `hydrate` must
    /// not consult the keychain at all — so this holds on every platform without
    /// touching any keystore.
    #[test]
    fn hydrate_keeps_existing_plaintext_secret() {
        let mut cfg = cfg_with_secrets("plaintext-openai", "plaintext-anthropic", "plaintext-notion");
        secrets::hydrate(&mut cfg);
        assert_eq!(cfg.openai_api_key, "plaintext-openai");
        assert_eq!(cfg.anthropic_api_key, "plaintext-anthropic");
        assert_eq!(cfg.notion_api_key, "plaintext-notion");
    }

    /// On a target with no native keystore, secrets stay in `config.json` and
    /// `hydrate` is a no-op, so keys are never lost. This is the module compiled
    /// on a Linux CI runner.
    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn fallback_keeps_secrets_in_config() {
        assert!(!secrets::ENABLED);
        let cfg = cfg_with_secrets("sk-o", "sk-a", "sk-n");
        let on_disk = secrets::externalize(&cfg).unwrap();
        assert_eq!(on_disk.openai_api_key, "sk-o");
        assert_eq!(on_disk.anthropic_api_key, "sk-a");
        assert_eq!(on_disk.notion_api_key, "sk-n");
    }

    /// With a native keystore, `externalize` stores the secrets and blanks the
    /// on-disk copy. Uses keyring's in-memory mock so the real OS keychain is
    /// never touched (the mock keeps no cross-entry state, so we assert the
    /// blanking of the returned on-disk config rather than a get round-trip).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn externalize_blanks_stored_secrets() {
        assert!(secrets::ENABLED);
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let cfg = cfg_with_secrets("sk-openai", "sk-anthropic", "sk-notion");
        let on_disk = secrets::externalize(&cfg).unwrap();
        assert!(on_disk.openai_api_key.is_empty());
        assert!(on_disk.anthropic_api_key.is_empty());
        assert!(on_disk.notion_api_key.is_empty());
    }
}
