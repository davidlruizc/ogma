//! Integration test that exercises the real audio input device.
//! Ignored by default (requires a microphone); run explicitly with:
//!   cargo test -p ogma-core --test recorder_integration -- --ignored

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ogma_core::recording::{wav, Recorder};

#[test]
#[ignore]
fn records_two_seconds_from_default_mic() {
    let dir = tempfile::tempdir().unwrap();
    let levels = Arc::new(AtomicUsize::new(0));
    let levels_cb = Arc::clone(&levels);

    let recorder = Recorder::start(
        dir.path(),
        None,
        Box::new(move |_level| {
            levels_cb.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .expect("recorder should start on a machine with a mic");

    std::thread::sleep(Duration::from_secs(2));
    assert!(recorder.elapsed_ms() > 1000, "elapsed should advance");

    // Pause: elapsed must stop advancing.
    recorder.pause();
    std::thread::sleep(Duration::from_millis(300));
    let at_pause = recorder.elapsed_ms();
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        (recorder.elapsed_ms() - at_pause).abs() < 100,
        "elapsed should not advance while paused"
    );
    recorder.resume();
    std::thread::sleep(Duration::from_millis(500));

    let result = recorder.stop().expect("stop should finalize");
    assert_eq!(result.segments.len(), 1);
    assert!(result.duration_ms >= 2000, "got {}ms", result.duration_ms);

    let samples = wav::sample_count(&result.segments[0]).unwrap();
    // ~2.5s of audio at 16kHz (generous bounds for device startup slack)
    assert!(samples > 16_000, "only {samples} samples written");

    assert!(levels.load(Ordering::SeqCst) > 5, "level meter should tick");

    // Concat produces a playable audio.wav
    let out = dir.path().join("audio.wav");
    wav::concat(&result.segments, &out).unwrap();
    assert_eq!(wav::sample_count(&out).unwrap(), samples);
}
