//! Desktop stub. The upstream desktop backend (cpal 0.15 + hound) is stripped
//! in this fork: it cannot coexist with ogma-core's cpal 0.18 in one cargo
//! workspace (both link the native `alsa` library on Linux), and Ogma's
//! desktop recording is handled by ogma-core's own recorder anyway. Every
//! call reports the plugin as mobile-only.

use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::error::Error;
use crate::models::*;

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<AudioRecorder<R>> {
    Ok(AudioRecorder(std::marker::PhantomData))
}

/// Access to the audio-recorder APIs (inert on desktop).
/// `fn() -> R` keeps the marker `Send + Sync` without requiring `R: Sync`.
pub struct AudioRecorder<R: Runtime>(std::marker::PhantomData<fn() -> R>);

fn mobile_only<T>() -> crate::Result<T> {
    Err(Error::Recording(
        "audio-recorder is mobile-only in Ogma; desktop recording uses the built-in recorder".into(),
    ))
}

impl<R: Runtime> AudioRecorder<R> {
    pub fn start_recording(&self, _config: RecordingConfig) -> crate::Result<()> {
        mobile_only()
    }

    pub fn stop_recording(&self) -> crate::Result<RecordingResult> {
        mobile_only()
    }

    pub fn pause_recording(&self) -> crate::Result<()> {
        mobile_only()
    }

    pub fn resume_recording(&self) -> crate::Result<()> {
        mobile_only()
    }

    pub fn get_status(&self) -> crate::Result<RecordingStatus> {
        mobile_only()
    }

    pub fn get_devices(&self) -> crate::Result<AudioDevicesResponse> {
        mobile_only()
    }

    pub fn check_permission(&self) -> crate::Result<PermissionStatus> {
        mobile_only()
    }

    pub fn request_permission(&self) -> crate::Result<PermissionStatus> {
        mobile_only()
    }
}
