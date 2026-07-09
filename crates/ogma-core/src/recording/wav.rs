//! Minimal WAV read/write for 16-bit mono PCM.
//!
//! Hand-rolled instead of `hound` because crash-safety is the whole point of
//! segmented recording: if the process dies, the segment on disk has a stale
//! (zero-length) header, and we must be able to repair it from the file size
//! on recovery. Only supports the one format Ogma records: 16 kHz mono s16le.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::{Error, Result};

pub const SAMPLE_RATE: u32 = 16_000;
const HEADER_LEN: u64 = 44;

fn header(data_len: u32) -> [u8; HEADER_LEN as usize] {
    let mut h = [0u8; HEADER_LEN as usize];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&SAMPLE_RATE.to_le_bytes());
    h[28..32].copy_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    h[32..34].copy_from_slice(&2u16.to_le_bytes()); // block align
    h[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

/// Streaming writer for one segment. Header is written up-front with a zero
/// data length and patched on `finalize()`; `repair()` handles the crash case.
pub struct WavWriter {
    w: BufWriter<File>,
    samples_written: u64,
}

impl WavWriter {
    pub fn create(path: &Path) -> Result<WavWriter> {
        let file = File::create(path)?;
        let mut w = BufWriter::new(file);
        w.write_all(&header(0))?;
        Ok(WavWriter {
            w,
            samples_written: 0,
        })
    }

    pub fn write_samples(&mut self, samples: &[i16]) -> Result<()> {
        // i16::to_le_bytes per sample; chunked through a byte buffer.
        let mut buf = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        self.w.write_all(&buf)?;
        self.samples_written += samples.len() as u64;
        Ok(())
    }

    /// Push buffered samples to the OS so a crash loses at most the buffer.
    pub fn flush(&mut self) -> Result<()> {
        self.w.flush()?;
        Ok(())
    }

    pub fn samples_written(&self) -> u64 {
        self.samples_written
    }

    /// Patch the header with the real data length and close the file.
    pub fn finalize(mut self) -> Result<()> {
        self.w.flush()?;
        let mut file = self.w.into_inner().map_err(|e| Error::Other(e.to_string()))?;
        let data_len = (self.samples_written * 2) as u32;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header(data_len))?;
        file.sync_all()?;
        Ok(())
    }
}

/// Fix a segment whose header was never finalized (crash mid-recording):
/// recompute the data length from the file size and rewrite the header.
/// Safe to call on healthy files too (idempotent). Returns sample count.
pub fn repair(path: &Path) -> Result<u64> {
    let len = std::fs::metadata(path)?.len();
    if len < HEADER_LEN {
        return Err(Error::Other(format!(
            "{} is too short to be a WAV segment",
            path.display()
        )));
    }
    // Truncate a trailing odd byte (partial sample from a crash mid-write).
    let data_len = (len - HEADER_LEN) & !1;
    let file = OpenOptions::new().read(true).write(true).open(path)?;
    file.set_len(HEADER_LEN + data_len)?;
    let mut file = file;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&header(data_len as u32))?;
    file.sync_all()?;
    Ok(data_len / 2)
}

/// Number of samples in a (healthy) segment, from the file size.
pub fn sample_count(path: &Path) -> Result<u64> {
    let len = std::fs::metadata(path)?.len();
    Ok(len.saturating_sub(HEADER_LEN) / 2)
}

pub fn duration_ms(path: &Path) -> Result<i64> {
    Ok((sample_count(path)? * 1000 / SAMPLE_RATE as u64) as i64)
}

/// Concatenate segments into one WAV. Streams raw PCM, so memory stays flat
/// regardless of meeting length.
pub fn concat(segments: &[std::path::PathBuf], out: &Path) -> Result<()> {
    let mut writer = BufWriter::new(File::create(out)?);
    writer.write_all(&header(0))?;
    let mut total: u64 = 0;
    let mut buf = vec![0u8; 256 * 1024];
    for seg in segments {
        let mut f = File::open(seg)?;
        f.seek(SeekFrom::Start(HEADER_LEN))?;
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n])?;
            total += n as u64;
        }
    }
    writer.flush()?;
    let mut file = writer.into_inner().map_err(|e| Error::Other(e.to_string()))?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&header(total as u32))?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_finalize_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seg.wav");
        let mut w = WavWriter::create(&path).unwrap();
        w.write_samples(&[1i16, -2, 3, -4]).unwrap();
        w.finalize().unwrap();
        assert_eq!(sample_count(&path).unwrap(), 4);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);
    }

    #[test]
    fn repair_fixes_crashed_segment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seg.wav");
        let mut w = WavWriter::create(&path).unwrap();
        w.write_samples(&[10i16; 1000]).unwrap();
        w.flush().unwrap();
        drop(w); // simulate crash: header still says data_len = 0

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 0);

        let samples = repair(&path).unwrap();
        assert_eq!(samples, 1000);
        assert_eq!(sample_count(&path).unwrap(), 1000);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 2000);
    }

    #[test]
    fn concat_streams_all_segments() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..3 {
            let p = dir.path().join(format!("seg-{i}.wav"));
            let mut w = WavWriter::create(&p).unwrap();
            w.write_samples(&vec![i as i16; 100]).unwrap();
            w.finalize().unwrap();
            paths.push(p);
        }
        let out = dir.path().join("audio.wav");
        concat(&paths, &out).unwrap();
        assert_eq!(sample_count(&out).unwrap(), 300);
    }
}
