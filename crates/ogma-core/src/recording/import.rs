//! Import an external audio file as if it had been recorded live.
//!
//! Decodes WAV/M4A/MP3/FLAC/OGG via symphonia, downmixes to mono, resamples to
//! 16 kHz and writes the same rotating 5-minute `seg-NNN.wav` segments the
//! `Recorder` produces — so everything downstream (concat, Whisper chunking,
//! pipeline resume) works unchanged. This is the desktop half of the mobile
//! fallback path from PLAN.md Phase 4: record with Voice Memos (M4A), import,
//! run the normal pipeline.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{Error, Result};

use super::{wav, RecordingResult, Resampler, SegmentSink};

/// Hard ceiling on decoded import length. Meetings are 1–3 h; 6 h leaves slack
/// while bounding what a decompression bomb (tiny FLAC/OGG of silence) can
/// expand to on disk — and, downstream, what the auto-run pipeline can spend
/// on Whisper/Claude API calls.
pub const MAX_IMPORT_HOURS: u64 = 6;
const MAX_IMPORT_SAMPLES: u64 = MAX_IMPORT_HOURS * 3600 * wav::SAMPLE_RATE as u64;

/// Decode `src` into 5-minute segments under `meeting_dir` and return them
/// like a finished recording. Fails without leaving partial segments behind
/// only in the sense that callers should treat any `Err` as "discard the dir".
pub fn import_file(src: &Path, meeting_dir: &Path) -> Result<RecordingResult> {
    std::fs::create_dir_all(meeting_dir)?;

    let file = File::open(src)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = src.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| Error::Audio(format!("unsupported or unreadable audio file: {e}")))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| Error::Audio("no decodable audio track in file".into()))?;
    let track_id = track.id;
    // Container-declared length (when known), to detect silent truncation.
    let expected_ms: Option<i64> = track
        .codec_params
        .n_frames
        .zip(track.codec_params.sample_rate)
        .map(|(frames, rate)| (frames as i128 * 1000 / rate as i128) as i64);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| Error::Audio(format!("unsupported audio codec: {e}")))?;

    let mut sink = SegmentSink::new(meeting_dir).with_max_samples(MAX_IMPORT_SAMPLES);
    let mut resampler: Option<Resampler> = None;
    let mut in_rate: u32 = 0;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut mono: Vec<f32> = Vec::new();
    let mut skipped_packets: u64 = 0;
    let mut truncated = false;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            // A required reset (e.g. chained streams) ends the import early;
            // the shortfall is caught against `expected_ms` below.
            Err(SymError::ResetRequired) => {
                truncated = true;
                break;
            }
            Err(e) => return Err(Error::Audio(format!("error reading audio file: {e}"))),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // A corrupt packet is skippable; the stream stays aligned.
            Err(SymError::DecodeError(_)) => {
                skipped_packets += 1;
                continue;
            }
            Err(e) => return Err(Error::Audio(format!("error decoding audio: {e}"))),
        };
        let spec = *decoded.spec();
        if decoded.frames() == 0 {
            continue;
        }
        match resampler {
            None => {
                in_rate = spec.rate;
                resampler = Some(Resampler::new(spec.rate, wav::SAMPLE_RATE));
            }
            Some(_) if spec.rate != in_rate => {
                return Err(Error::Audio(
                    "audio files with a changing sample rate are not supported".into(),
                ));
            }
            Some(_) => {}
        }
        if sample_buf
            .as_ref()
            .map(|b| b.capacity() < decoded.capacity() * spec.channels.count())
            .unwrap_or(true)
        {
            sample_buf = Some(SampleBuffer::new(decoded.capacity() as u64, spec));
        }
        let buf = sample_buf.as_mut().unwrap();
        buf.copy_interleaved_ref(decoded);

        let channels = spec.channels.count().max(1);
        mono.clear();
        for frame in buf.samples().chunks_exact(channels) {
            mono.push(frame.iter().sum::<f32>() / channels as f32);
        }
        let out = resampler.as_mut().unwrap().process(&mono);
        sink.write(&out)?;
    }

    let result = sink.finish()?;
    if result.duration_ms == 0 {
        return Err(Error::Audio("no audio could be decoded from the file".into()));
    }
    // Don't let a partially corrupt/truncated file import silently short: the
    // pipeline would produce notes for a fraction of the meeting and nothing
    // would tell the user.
    if let Some(expected) = expected_ms.filter(|&e| e > 0) {
        if result.duration_ms < expected * 9 / 10 {
            return Err(Error::Audio(format!(
                "only {} s of the file's {} s could be decoded — the file appears corrupt or truncated",
                result.duration_ms / 1000,
                expected / 1000
            )));
        }
    }
    if truncated || skipped_packets > 0 {
        tracing::warn!(
            "import decoded with gaps: {skipped_packets} corrupt packet(s) skipped, ended early: {truncated}"
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::super::{SEGMENT_SAMPLES, SEGMENT_SECONDS};
    use super::*;
    use std::io::Write as _;

    /// Minimal PCM WAV writer at an arbitrary rate/channel count, for building
    /// import fixtures (the production `wav::WavWriter` is fixed at 16 kHz mono).
    fn write_test_wav(path: &Path, rate: u32, channels: u16, samples: &[i16]) {
        let data_len = (samples.len() * 2) as u32;
        let mut f = File::create(path).unwrap();
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
        f.write_all(b"WAVE").unwrap();
        f.write_all(b"fmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&channels.to_le_bytes()).unwrap();
        f.write_all(&rate.to_le_bytes()).unwrap();
        f.write_all(&(rate * channels as u32 * 2).to_le_bytes()).unwrap();
        f.write_all(&(channels * 2).to_le_bytes()).unwrap();
        f.write_all(&16u16.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&data_len.to_le_bytes()).unwrap();
        for s in samples {
            f.write_all(&s.to_le_bytes()).unwrap();
        }
    }

    #[test]
    fn imports_stereo_44k_wav_to_16k_mono_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("in.wav");
        // 2 seconds of stereo sine-ish ramp at 44.1 kHz.
        let frames = 44_100 * 2;
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let v = ((i % 100) as i16 - 50) * 200;
            samples.push(v);
            samples.push(v);
        }
        write_test_wav(&src, 44_100, 2, &samples);

        let dir = tmp.path().join("meeting");
        let result = import_file(&src, &dir).unwrap();
        assert_eq!(result.segments.len(), 1);
        // ~2000 ms of audio after resampling; the linear resampler eats a
        // couple of edge samples.
        assert!(
            (result.duration_ms - 2000).abs() <= 10,
            "duration {}",
            result.duration_ms
        );
        let n = wav::sample_count(&result.segments[0]).unwrap();
        assert!((n as i64 - 32_000).abs() <= 160, "samples {n}");
    }

    #[test]
    fn rotates_segments_at_five_minutes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("meeting");
        std::fs::create_dir_all(&dir).unwrap();
        let mut sink = SegmentSink::new(&dir);
        // 5 min + 1 s of 16 kHz samples in odd-sized blocks.
        let block = vec![0i16; 7000];
        let total = SEGMENT_SAMPLES + wav::SAMPLE_RATE as u64;
        let mut written = 0u64;
        while written < total {
            let take = (total - written).min(block.len() as u64) as usize;
            sink.write(&block[..take]).unwrap();
            written += take as u64;
        }
        let result = sink.finish().unwrap();
        assert_eq!(result.segments.len(), 2);
        assert_eq!(wav::sample_count(&result.segments[0]).unwrap(), SEGMENT_SAMPLES);
        assert_eq!(
            wav::sample_count(&result.segments[1]).unwrap(),
            wav::SAMPLE_RATE as u64
        );
        assert_eq!(result.duration_ms, (SEGMENT_SECONDS as i64 + 1) * 1000);
    }

    #[test]
    fn rejects_non_audio_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("not-audio.wav");
        std::fs::write(&src, b"definitely not a wav file").unwrap();
        let dir = tmp.path().join("meeting");
        assert!(import_file(&src, &dir).is_err());
    }

    #[test]
    fn rejects_zero_sample_audio() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("empty.wav");
        write_test_wav(&src, 16_000, 1, &[]);
        let dir = tmp.path().join("meeting");
        let err = import_file(&src, &dir).unwrap_err();
        assert!(
            err.to_string().contains("no audio"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_heavily_truncated_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("truncated.wav");
        // Header declares 2 s of 16 kHz mono audio…
        write_test_wav(&src, 16_000, 1, &vec![1000i16; 32_000]);
        // …but the file is cut off after 0.25 s of data.
        let full = std::fs::read(&src).unwrap();
        std::fs::write(&src, &full[..44 + 8_000]).unwrap();
        let dir = tmp.path().join("meeting");
        let err = import_file(&src, &dir).unwrap_err();
        assert!(
            err.to_string().contains("could be decoded"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_audio_longer_than_the_import_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("meeting");
        std::fs::create_dir_all(&dir).unwrap();
        let mut sink = SegmentSink::new(&dir).with_max_samples(1_000);
        sink.write(&[0i16; 800]).unwrap();
        let err = sink.write(&[0i16; 300]).unwrap_err();
        assert!(
            err.to_string().contains("maximum import length"),
            "unexpected error: {err}"
        );
    }
}
