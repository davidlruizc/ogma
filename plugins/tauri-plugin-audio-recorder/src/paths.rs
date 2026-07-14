// Path management utilities based on tauri-plugin-sql patterns
// See: https://github.com/tauri-apps/plugins-workspace/tree/v2/plugins/sql

use std::fs::create_dir_all;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};

use crate::error::Error;

/// Default subdirectory for recordings within app_data_dir
const RECORDINGS_SUBDIR: &str = "recordings";

/// Gets the recordings directory, creating it if it doesn't exist.
///
/// Uses `app_data_dir()` as base directory, following the pattern from tauri-plugin-sql.
///
/// # Example
/// ```rust,ignore
/// let recordings_dir = get_recordings_dir(&app)?;
/// let output_path = recordings_dir.join("my_recording.wav");
/// ```
#[allow(dead_code)]
pub fn get_recordings_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, Error> {
    get_plugin_subdir(app, RECORDINGS_SUBDIR)
}

/// Gets a subdirectory within app_data_dir, creating it if necessary.
///
/// This follows the pattern from tauri-plugin-sql which uses `app_config_dir()`.
/// We use `app_data_dir()` for user data like recordings.
///
/// # Arguments
/// * `app` - The Tauri app handle
/// * `subdir` - Subdirectory name within the app data directory
///
/// # Returns
/// The full path to the subdirectory
#[allow(dead_code)]
pub fn get_plugin_subdir<R: Runtime>(app: &AppHandle<R>, subdir: &str) -> Result<PathBuf, Error> {
    let base_path = app
        .path()
        .app_data_dir()
        .map_err(|e| Error::InvalidPath(format!("Could not determine app data directory: {e}")))?;

    let full_path = base_path.join(subdir);

    create_dir_all(&full_path).map_err(|e| {
        Error::InvalidPath(format!(
            "Could not create directory {}: {}",
            full_path.display(),
            e
        ))
    })?;

    Ok(full_path)
}

/// Gets the app cache directory for temporary files.
///
/// Use this for files that can be regenerated or are temporary.
///
/// # Example
/// ```rust,ignore
/// let cache_dir = get_cache_dir(&app, "temp")?;
/// ```
#[allow(dead_code)]
pub fn get_cache_dir<R: Runtime>(app: &AppHandle<R>, subdir: &str) -> Result<PathBuf, Error> {
    let base_path = app
        .path()
        .app_cache_dir()
        .map_err(|e| Error::InvalidPath(format!("Could not determine app cache directory: {e}")))?;

    let full_path = base_path.join(subdir);

    create_dir_all(&full_path).map_err(|e| {
        Error::InvalidPath(format!(
            "Could not create cache directory {}: {}",
            full_path.display(),
            e
        ))
    })?;

    Ok(full_path)
}

/// Resolves a relative path to an absolute path within the recordings directory.
///
/// If the path is already absolute, it's returned as-is (for custom locations).
/// If relative, it's resolved against the default recordings directory.
///
/// # Arguments
/// * `app` - The Tauri app handle
/// * `path` - The path string (can be relative or absolute)
///
/// # Example
/// ```rust,ignore
/// let path = resolve_output_path(&app, "recording.wav")?;
/// // Returns: /path/to/app_data/recordings/recording.wav
///
/// let path = resolve_output_path(&app, "/custom/path/recording.wav")?;
/// // Returns: /custom/path/recording.wav
/// ```
#[allow(dead_code)]
pub fn resolve_output_path<R: Runtime>(app: &AppHandle<R>, path: &str) -> Result<PathBuf, Error> {
    let path_buf = PathBuf::from(path);

    if path_buf.is_absolute() {
        // For absolute paths, just ensure parent directory exists
        if let Some(parent) = path_buf.parent() {
            create_dir_all(parent).map_err(|e| {
                Error::InvalidPath(format!(
                    "Could not create parent directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        Ok(path_buf)
    } else {
        // For relative paths, use the recordings directory
        let recordings_dir = get_recordings_dir(app)?;
        Ok(recordings_dir.join(path))
    }
}

/// Validates that a path doesn't contain path traversal attacks.
///
/// Checks for ".." components that could escape the intended directory.
///
/// # Security
/// Always call this before writing to user-provided paths.
#[allow(dead_code)]
pub fn validate_path(path: &str) -> Result<(), Error> {
    let path_buf = PathBuf::from(path);

    for component in path_buf.components() {
        if let std::path::Component::ParentDir = component {
            return Err(Error::InvalidPath(
                "Path traversal not allowed (contains '..')".to_string(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path_normal() {
        assert!(validate_path("recording.wav").is_ok());
        assert!(validate_path("subdir/recording.wav").is_ok());
        assert!(validate_path("/absolute/path/recording.wav").is_ok());
    }

    #[test]
    fn test_validate_path_traversal() {
        assert!(validate_path("../recording.wav").is_err());
        assert!(validate_path("subdir/../recording.wav").is_err());
        assert!(validate_path("subdir/../../recording.wav").is_err());
    }
}
