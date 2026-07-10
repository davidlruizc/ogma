//! Recording engine: cpal capture → 16 kHz mono s16 → rotating WAV segments.
//!
//! Threading model: cpal streams are !Send, so a dedicated audio thread owns
//! the stream and forwards raw blocks over a channel to a writer thread that
//! does downmix/resample/segmentation. The UI talks to `Recorder` (Send),
//! which only holds flags, counters and join handles.

pub mod import;
pub mod wake;
pub mod wav;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::error::{Error, Result};

pub const SEGMENT_SECONDS: u64 = 300; // 5-minute rotation, per PLAN.md
const SEGMENT_SAMPLES: u64 = SEGMENT_SECONDS * wav::SAMPLE_RATE as u64;
/// Flush cadence: at most ~1s of audio is buffered (not yet at the OS).
const FLUSH_EVERY_SAMPLES: u64 = wav::SAMPLE_RATE as u64;

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct LevelUpdate {
    /// RMS of the last block, 0.0..=1.0.
    pub rms: f32,
    pub peak: f32,
    /// Milliseconds of audio actually recorded (excludes pauses).
    pub elapsed_ms: i64,
    pub paused: bool,
}

pub type LevelCallback = Box<dyn Fn(LevelUpdate) + Send + 'static>;

/// A finished recording, ready for the pipeline.
#[derive(Debug, Clone)]
pub struct RecordingResult {
    pub segments: Vec<PathBuf>,
    pub duration_ms: i64,
}

enum AudioBlock {
    /// Mono f32 samples at the device rate.
    Samples(Vec<f32>),
    Finish,
}

pub struct Recorder {
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    elapsed_samples: Arc<AtomicI64>,
    audio_thread: Option<JoinHandle<()>>,
    writer_thread: Option<JoinHandle<Result<Vec<PathBuf>>>>,
    /// Filled by the audio thread if the device fails at startup.
    startup_err: Receiver<Option<String>>,
}

impl Recorder {
    /// Start capturing into `meeting_dir/seg-NNN.wav`. Level updates arrive on
    /// `on_level` (~10 Hz) from a background thread.
    ///
    /// `device_name` selects the input device by its cpal name; `None` or an
    /// empty string uses the host default. An unknown name falls back to the
    /// default rather than failing, so a saved device that's since been
    /// unplugged doesn't block recording.
    pub fn start(
        meeting_dir: &Path,
        device_name: Option<String>,
        on_level: LevelCallback,
    ) -> Result<Recorder> {
        std::fs::create_dir_all(meeting_dir)?;

        let paused = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let elapsed_samples = Arc::new(AtomicI64::new(0));

        // Audio thread → writer thread. Bounded so a stuck writer can't eat RAM;
        // at 48 kHz stereo the callback fires every ~10 ms, 512 blocks ≈ 5 s.
        let (block_tx, block_rx) = mpsc::sync_channel::<AudioBlock>(512);
        let (startup_tx, startup_rx) = mpsc::channel::<Option<String>>();

        let audio_thread = spawn_audio_thread(
            device_name.filter(|n| !n.is_empty()),
            block_tx,
            startup_tx,
            Arc::clone(&paused),
            Arc::clone(&stop),
        );

        // Wait for the device to actually open so `start` fails loudly when
        // there's no mic instead of recording silence.
        let (device_rate, err) = match startup_rx.recv() {
            Ok(None) => {
                return Err(Error::Audio("audio thread died before reporting".into()))
            }
            Ok(Some(msg)) => match msg.strip_prefix("ok:") {
                Some(rate) => (rate.parse::<u32>().unwrap_or(48_000), None),
                None => (0, Some(msg)),
            },
            Err(_) => (0, Some("audio thread died before reporting".to_string())),
        };
        if let Some(msg) = err {
            let _ = audio_thread.join();
            return Err(Error::Audio(msg));
        }

        let writer_thread = spawn_writer_thread(
            meeting_dir.to_path_buf(),
            device_rate,
            block_rx,
            on_level,
            Arc::clone(&paused),
            Arc::clone(&elapsed_samples),
        );

        Ok(Recorder {
            paused,
            stop,
            elapsed_samples,
            audio_thread: Some(audio_thread),
            writer_thread: Some(writer_thread),
            startup_err: startup_rx,
        })
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn elapsed_ms(&self) -> i64 {
        self.elapsed_samples.load(Ordering::SeqCst) * 1000 / wav::SAMPLE_RATE as i64
    }

    /// Runtime device failure (unplugged mic). Non-blocking check.
    pub fn runtime_error(&self) -> Option<String> {
        match self.startup_err.try_recv() {
            Ok(Some(msg)) if !msg.starts_with("ok:") => Some(msg),
            _ => None,
        }
    }

    /// Stop capture, finalize the last segment and return the segment list.
    pub fn stop(mut self) -> Result<RecordingResult> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.audio_thread.take() {
            let _ = h.join();
        }
        let segments = match self.writer_thread.take() {
            Some(h) => h
                .join()
                .map_err(|_| Error::Audio("writer thread panicked".into()))??,
            None => Vec::new(),
        };
        let duration_ms = self.elapsed_ms();
        Ok(RecordingResult {
            segments,
            duration_ms,
        })
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.audio_thread.take() {
            let _ = h.join();
        }
        if let Some(h) = self.writer_thread.take() {
            let _ = h.join();
        }
    }
}

fn spawn_audio_thread(
    device_name: Option<String>,
    block_tx: SyncSender<AudioBlock>,
    startup_tx: Sender<Option<String>>,
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("ogma-audio".into())
        .spawn(move || {
            // Keep the machine awake for the lifetime of the capture.
            let _wake = wake::WakeGuard::new();

            let host = cpal::default_host();
            let device = match find_input_device(&host, device_name.as_deref()) {
                Some(d) => d,
                None => {
                    let _ = startup_tx.send(Some("no input device found".into()));
                    return;
                }
            };
            let supported = match device.default_input_config() {
                Ok(c) => c,
                Err(e) => {
                    let _ = startup_tx.send(Some(format!("input config error: {e}")));
                    return;
                }
            };
            let sample_format = supported.sample_format();
            let config: cpal::StreamConfig = supported.into();
            let channels = config.channels as usize;
            let rate = config.sample_rate;

            let err_tx = startup_tx.clone();
            let on_err = move |e: cpal::Error| {
                let _ = err_tx.send(Some(format!("stream error: {e}")));
            };

            macro_rules! build {
                ($t:ty) => {
                    build_stream::<$t>(&device, &config, channels, block_tx.clone(), on_err)
                };
            }
            let stream = match sample_format {
                cpal::SampleFormat::F32 => build!(f32),
                cpal::SampleFormat::I16 => build!(i16),
                cpal::SampleFormat::U16 => build!(u16),
                cpal::SampleFormat::I32 => build!(i32),
                other => Err(Error::Audio(format!("unsupported sample format {other}"))),
            };
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    let _ = startup_tx.send(Some(e.to_string()));
                    return;
                }
            };
            if let Err(e) = stream.play() {
                let _ = startup_tx.send(Some(format!("failed to start stream: {e}")));
                return;
            }
            let _ = startup_tx.send(Some(format!("ok:{rate}")));

            let _ = &paused; // pause handled in writer; kept for future use
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(50));
            }
            drop(stream);
            let _ = block_tx.send(AudioBlock::Finish);
        })
        .expect("failed to spawn audio thread")
}

/// Resolve an input device by cpal name, falling back to the host default when
/// the name is `None`/empty or no longer matches a connected device.
///
/// cpal 0.18 exposes the device name through `Display` (`to_string`), not a
/// `name()` method.
fn find_input_device(host: &cpal::Host, name: Option<&str>) -> Option<cpal::Device> {
    if let Some(name) = name.filter(|n| !n.is_empty()) {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(dev) = devices.find(|d| d.to_string() == name) {
                return Some(dev);
            }
        }
    }
    host.default_input_device()
}

/// Names of the available input devices, for the settings picker. The host
/// default (when it can be identified) is listed first.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().map(|d| d.to_string());
    let mut names: Vec<String> = match host.input_devices() {
        Ok(devices) => devices.map(|d| d.to_string()).collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names.dedup();
    if let Some(default) = default_name {
        names.retain(|n| n != &default);
        names.insert(0, default);
    }
    names
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    tx: SyncSender<AudioBlock>,
    on_err: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let stream = device
        .build_input_stream(
            config.clone(),
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                // Downmix to mono here; resampling happens on the writer thread.
                let mut mono = Vec::with_capacity(data.len() / channels);
                for frame in data.chunks_exact(channels) {
                    let sum: f32 = frame
                        .iter()
                        .map(|s| <f32 as cpal::FromSample<T>>::from_sample_(*s))
                        .sum();
                    mono.push(sum / channels as f32);
                }
                // try_send: dropping a block under backpressure beats blocking
                // the realtime callback.
                let _ = tx.try_send(AudioBlock::Samples(mono));
            },
            on_err,
            None,
        )
        .map_err(|e| Error::Audio(format!("failed to open input stream: {e}")))?;
    Ok(stream)
}

fn spawn_writer_thread(
    dir: PathBuf,
    device_rate: u32,
    rx: Receiver<AudioBlock>,
    on_level: LevelCallback,
    paused: Arc<AtomicBool>,
    elapsed_samples: Arc<AtomicI64>,
) -> JoinHandle<Result<Vec<PathBuf>>> {
    std::thread::Builder::new()
        .name("ogma-writer".into())
        .spawn(move || {
            let mut resampler = Resampler::new(device_rate, wav::SAMPLE_RATE);
            let mut segments: Vec<PathBuf> = Vec::new();
            let mut writer: Option<wav::WavWriter> = None;
            let mut seg_samples: u64 = 0;
            let mut since_flush: u64 = 0;
            let mut last_level = Instant::now();

            loop {
                let block = match rx.recv() {
                    Ok(AudioBlock::Samples(s)) => s,
                    Ok(AudioBlock::Finish) | Err(_) => break,
                };

                // Level meter runs even while paused so the user can see the
                // mic is alive; audio is simply not written.
                if last_level.elapsed() >= Duration::from_millis(100) {
                    last_level = Instant::now();
                    let peak = block.iter().fold(0f32, |m, s| m.max(s.abs()));
                    let rms = (block.iter().map(|s| s * s).sum::<f32>()
                        / block.len().max(1) as f32)
                        .sqrt();
                    on_level(LevelUpdate {
                        rms,
                        peak,
                        elapsed_ms: elapsed_samples.load(Ordering::SeqCst) * 1000
                            / wav::SAMPLE_RATE as i64,
                        paused: paused.load(Ordering::SeqCst),
                    });
                }

                if paused.load(Ordering::SeqCst) {
                    resampler.reset();
                    continue;
                }

                let out = resampler.process(&block);
                if out.is_empty() {
                    continue;
                }

                let mut offset = 0usize;
                while offset < out.len() {
                    if writer.is_none() {
                        let path = dir.join(format!("seg-{:03}.wav", segments.len()));
                        writer = Some(wav::WavWriter::create(&path)?);
                        segments.push(path);
                        seg_samples = 0;
                    }
                    let room = (SEGMENT_SAMPLES - seg_samples) as usize;
                    let take = room.min(out.len() - offset);
                    let w = writer.as_mut().unwrap();
                    w.write_samples(&out[offset..offset + take])?;
                    seg_samples += take as u64;
                    since_flush += take as u64;
                    elapsed_samples.fetch_add(take as i64, Ordering::SeqCst);
                    offset += take;

                    if seg_samples >= SEGMENT_SAMPLES {
                        writer.take().unwrap().finalize()?;
                    } else if since_flush >= FLUSH_EVERY_SAMPLES {
                        w.flush()?;
                        since_flush = 0;
                    }
                }
            }

            if let Some(w) = writer.take() {
                w.finalize()?;
            }
            Ok(segments)
        })
        .expect("failed to spawn writer thread")
}

/// Linear-interpolation resampler, mono f32 → i16. Speech-grade quality is
/// plenty for STT; keeps us off heavyweight DSP crates.
struct Resampler {
    ratio: f64, // in-rate / out-rate
    pos: f64,   // fractional read position into the current input stream
    prev: f32,  // last sample of the previous block, for interpolation
    has_prev: bool,
}

impl Resampler {
    fn new(in_rate: u32, out_rate: u32) -> Resampler {
        Resampler {
            ratio: in_rate as f64 / out_rate as f64,
            pos: 0.0,
            prev: 0.0,
            has_prev: false,
        }
    }

    /// Drop continuity across a pause so we don't interpolate over the gap.
    fn reset(&mut self) {
        self.pos = 0.0;
        self.has_prev = false;
    }

    fn process(&mut self, input: &[f32]) -> Vec<i16> {
        if input.is_empty() {
            return Vec::new();
        }
        // Virtual input: [prev, input...] so interpolation spans block edges.
        let mut out = Vec::with_capacity((input.len() as f64 / self.ratio) as usize + 2);
        let base: f64 = if self.has_prev { 0.0 } else { 1.0 };
        if !self.has_prev {
            self.pos = self.pos.max(1.0);
        }
        let get = |i: usize| -> f32 {
            if i == 0 {
                self.prev
            } else {
                input[i - 1]
            }
        };
        let virtual_len = input.len() + 1;
        let mut pos = self.pos.max(base);
        while pos + 1.0 < virtual_len as f64 {
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = get(idx);
            let b = get(idx + 1);
            let sample = a + (b - a) * frac;
            out.push((sample.clamp(-1.0, 1.0) * 32767.0) as i16);
            pos += self.ratio;
        }
        self.pos = pos - input.len() as f64; // rebase against next block
        self.prev = input[input.len() - 1];
        self.has_prev = true;
        out
    }
}

/// Repair all segments of a meeting directory after a crash and return
/// (segment paths, total duration). Used for meetings stuck in `recording`.
pub fn recover_segments(dir: &Path) -> Result<RecordingResult> {
    let mut segments = list_segments(dir)?;
    segments.retain(|p| match wav::repair(p) {
        Ok(n) => n > 0,
        Err(_) => false, // unreadable/empty stub: exclude from the recording
    });
    let mut total_samples: u64 = 0;
    for seg in &segments {
        total_samples += wav::sample_count(seg)?;
    }
    Ok(RecordingResult {
        segments,
        duration_ms: (total_samples * 1000 / wav::SAMPLE_RATE as u64) as i64,
    })
}

/// seg-*.wav files of a meeting dir, in recording order.
pub fn list_segments(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut segments: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("seg-") && n.ends_with(".wav"))
                .unwrap_or(false)
        })
        .collect();
    segments.sort();
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_downsamples_3_to_1() {
        let mut r = Resampler::new(48_000, 16_000);
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 / 4800.0) * 0.5).collect();
        let out = r.process(&input);
        // 4800 in @ 3:1 → ~1600 out
        assert!((out.len() as i64 - 1600).abs() <= 2, "got {}", out.len());
        // Monotonic ramp stays monotonic after resampling
        assert!(out.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn resampler_passthrough_at_same_rate() {
        let mut r = Resampler::new(16_000, 16_000);
        let input = vec![0.25f32; 1600];
        let n: usize = (0..10).map(|_| r.process(&input).len()).sum();
        assert!((n as i64 - 16_000).abs() <= 10, "got {n}");
    }
}
