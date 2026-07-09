//! Keep the system awake while recording.
//!
//! Windows: `SetThreadExecutionState` (per-thread; the guard must live on a
//! thread that stays alive for the whole recording — the audio thread does).
//! macOS: an IOKit power assertion (`IOPMAssertionCreateWithName`) held for the
//! life of the guard; process-wide, so it doesn't matter which thread owns it.
//! Other platforms no-op.
//!
//! `WakeGuard` is a thin RAII wrapper over a per-platform `imp::Inner`, which
//! carries whatever state the platform needs (nothing on Windows, the assertion
//! id on macOS) and releases it on drop.

pub struct WakeGuard {
    _inner: imp::Inner,
}

impl WakeGuard {
    pub fn new() -> WakeGuard {
        WakeGuard {
            _inner: imp::Inner::acquire(),
        }
    }
}

impl Default for WakeGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
mod imp {
    const ES_CONTINUOUS: u32 = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetThreadExecutionState(es_flags: u32) -> u32;
    }

    pub struct Inner;

    impl Inner {
        pub fn acquire() -> Inner {
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
            }
            Inner
        }
    }

    impl Drop for Inner {
        fn drop(&mut self) {
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    //! IOKit power assertion. We hand-declare the handful of externs we need
    //! (matching the Windows path's style) rather than pull in `core-foundation`
    //! + `io-kit-sys` just for two calls.
    use std::os::raw::{c_char, c_void};
    use std::ptr;

    type IOPMAssertionID = u32;
    type IOReturn = i32;
    type CFStringRef = *const c_void;

    // `kIOPMAssertPreventUserIdleSystemSleep`: block system idle sleep while
    // allowing the display to sleep — the audio-recording analogue of the
    // Windows `ES_SYSTEM_REQUIRED` (without `ES_DISPLAY_REQUIRED`).
    const ASSERTION_TYPE: &[u8] = b"PreventUserIdleSystemSleep\0";
    const ASSERTION_NAME: &[u8] = b"Ogma is recording a meeting\0";
    const KIOPM_ASSERTION_LEVEL_ON: u32 = 255;
    const KCF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const KIO_RETURN_SUCCESS: IOReturn = 0;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            assertion_level: u32,
            assertion_name: CFStringRef,
            assertion_id: *mut IOPMAssertionID,
        ) -> IOReturn;
        fn IOPMAssertionRelease(assertion_id: IOPMAssertionID) -> IOReturn;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(cf: *const c_void);
    }

    fn cfstr(bytes: &[u8]) -> CFStringRef {
        unsafe {
            CFStringCreateWithCString(ptr::null(), bytes.as_ptr() as *const c_char, KCF_STRING_ENCODING_UTF8)
        }
    }

    pub struct Inner {
        id: Option<IOPMAssertionID>,
    }

    impl Inner {
        pub fn acquire() -> Inner {
            let id = unsafe {
                let assertion_type = cfstr(ASSERTION_TYPE);
                let assertion_name = cfstr(ASSERTION_NAME);
                let mut id: IOPMAssertionID = 0;
                let rc = IOPMAssertionCreateWithName(
                    assertion_type,
                    KIOPM_ASSERTION_LEVEL_ON,
                    assertion_name,
                    &mut id,
                );
                if !assertion_type.is_null() {
                    CFRelease(assertion_type);
                }
                if !assertion_name.is_null() {
                    CFRelease(assertion_name);
                }
                if rc == KIO_RETURN_SUCCESS {
                    Some(id)
                } else {
                    tracing::warn!("IOPMAssertionCreateWithName failed (rc={rc}); system may sleep while recording");
                    None
                }
            };
            Inner { id }
        }
    }

    impl Drop for Inner {
        fn drop(&mut self) {
            if let Some(id) = self.id {
                unsafe {
                    IOPMAssertionRelease(id);
                }
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub struct Inner;

    impl Inner {
        pub fn acquire() -> Inner {
            Inner
        }
    }
}
