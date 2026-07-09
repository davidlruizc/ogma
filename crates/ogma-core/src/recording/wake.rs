//! Keep the system awake while recording.
//!
//! Windows: `SetThreadExecutionState` (per-thread; the guard must live on a
//! thread that stays alive for the whole recording — the audio thread does).
//! macOS/iOS get an IOKit equivalent in Phase 4; other platforms no-op.

pub struct WakeGuard {
    _private: (),
}

#[cfg(windows)]
mod imp {
    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetThreadExecutionState(es_flags: u32) -> u32;
    }

    pub fn acquire() {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        }
    }

    pub fn release() {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn acquire() {}
    pub fn release() {}
}

impl WakeGuard {
    pub fn new() -> WakeGuard {
        imp::acquire();
        WakeGuard { _private: () }
    }
}

impl Default for WakeGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WakeGuard {
    fn drop(&mut self) {
        imp::release();
    }
}
