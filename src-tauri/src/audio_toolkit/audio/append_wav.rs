//! Append-only 16 kHz mono 16-bit WAV for a meeting track.
//!
//! `save_wav_file` still writes a dictation take in one shot. A meeting cannot
//! use that: this writer emits the header, appends PCM, and every one second
//! of audio patches the RIFF and data sizes and fsyncs. Drop does not finalise
//! the header. A crash leaves sizes stale; the next open recomputes them from
//! the file length before any read.

#![cfg(feature = "meetings")]

use anyhow::{anyhow, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const SAMPLE_RATE: u32 = 16_000;
const HEADER_LEN: u64 = 44;
const FLUSH_EVERY: Duration = Duration::from_secs(1);

pub struct AppendWav {
    file: File,
    unflushed_samples: u64,
    last_patch: Instant,
    /// Set on the first write or flush error. Later appends return this and
    /// do not report success.
    stopped: Option<String>,
}

impl AppendWav {
    /// Create a new file with a valid empty header and fsync it.
    pub fn create(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.write_all(&header(0))?;
        file.sync_all()?;
        Ok(Self::new(file))
    }

    /// Open an existing file. Header sizes that do not match the file length
    /// (a crash between append and the one-second patch) are rewritten before
    /// the writer, or a later `read_wav_samples`, trusts them.
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Self::from_file(file)
    }

    /// Wrap an already-open file. A read-only file repairs only when the
    /// header already matches; the next append then fails as a write error.
    pub fn from_file(mut file: File) -> Result<Self> {
        repair_header(&mut file)?;
        Ok(Self::new(file))
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.is_some()
    }

    /// Append PCM. A full second of audio since the last patch is flushed
    /// before this returns. A write error stops the writer and is returned.
    pub fn append(&mut self, samples: &[i16]) -> Result<()> {
        self.ensure_running()?;
        let mut offset = 0;
        while offset < samples.len() {
            if self.due() {
                self.flush()?;
            }
            let room = u64::from(SAMPLE_RATE).saturating_sub(self.unflushed_samples) as usize;
            let n = (samples.len() - offset).min(room.max(1));
            if let Err(e) = self.write_pcm(&samples[offset..offset + n]) {
                return self.fail(e);
            }
            self.unflushed_samples += n as u64;
            offset += n;
            if self.due() {
                self.flush()?;
            }
        }
        Ok(())
    }

    /// Patch the RIFF and data sizes to the bytes on disk and fsync.
    pub fn flush(&mut self) -> Result<()> {
        self.ensure_running()?;
        if let Err(e) = self.patch_and_sync() {
            self.stopped = Some(e.to_string());
            return Err(anyhow!("wav flush failed: {e}"));
        }
        Ok(())
    }

    fn new(file: File) -> Self {
        Self {
            file,
            unflushed_samples: 0,
            last_patch: Instant::now(),
            stopped: None,
        }
    }

    fn ensure_running(&self) -> Result<()> {
        if let Some(prev) = &self.stopped {
            anyhow::bail!("wav writer stopped after a write error: {prev}");
        }
        Ok(())
    }

    fn due(&self) -> bool {
        if self.unflushed_samples == 0 {
            return false;
        }
        self.unflushed_samples >= u64::from(SAMPLE_RATE) || self.last_patch.elapsed() >= FLUSH_EVERY
    }

    fn write_pcm(&mut self, samples: &[i16]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::End(0))?;
        let mut buf = vec![0u8; samples.len() * 2];
        for (i, sample) in samples.iter().enumerate() {
            buf[i * 2..i * 2 + 2].copy_from_slice(&sample.to_le_bytes());
        }
        self.file.write_all(&buf)
    }

    /// Patch sizes to whatever bytes are actually in the file, then fsync.
    /// Failure here is a write error; callers must not turn it into success.
    fn patch_and_sync(&mut self) -> std::io::Result<()> {
        let len = self.file.seek(SeekFrom::End(0))?;
        if len < HEADER_LEN {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "wav shorter than a 44-byte header",
            ));
        }
        let data_bytes = data_bytes_for(len)?;
        write_u32_at(&mut self.file, 4, 36 + data_bytes)?;
        write_u32_at(&mut self.file, 40, data_bytes)?;
        self.file
            .seek(SeekFrom::Start(HEADER_LEN + u64::from(data_bytes)))?;
        self.file.sync_all()?;
        self.unflushed_samples = 0;
        self.last_patch = Instant::now();
        Ok(())
    }

    fn fail(&mut self, err: std::io::Error) -> Result<()> {
        self.stopped = Some(err.to_string());
        match self.patch_and_sync() {
            Ok(()) => Err(anyhow!("wav write failed: {err}")),
            Err(patch) => Err(anyhow!(
                "wav write failed: {err}; header patch failed: {patch}"
            )),
        }
    }
}

fn header(data_bytes: u32) -> [u8; 44] {
    let mut h = [0u8; HEADER_LEN as usize];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&SAMPLE_RATE.to_le_bytes());
    h[28..32].copy_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    h[32..34].copy_from_slice(&2u16.to_le_bytes()); // block align
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

/// Recompute RIFF and data sizes from the file length when they are stale.
fn repair_header(file: &mut File) -> Result<()> {
    let len = file.seek(SeekFrom::End(0))?;
    if len < HEADER_LEN {
        anyhow::bail!("wav shorter than a 44-byte header");
    }
    let data_bytes = data_bytes_for(len)?;
    let riff = read_u32_at(file, 4)?;
    let data = read_u32_at(file, 40)?;
    if riff == 36 + data_bytes && data == data_bytes {
        file.seek(SeekFrom::Start(HEADER_LEN + u64::from(data_bytes)))?;
        return Ok(());
    }
    write_u32_at(file, 4, 36 + data_bytes)?;
    write_u32_at(file, 40, data_bytes)?;
    file.sync_all()?;
    file.seek(SeekFrom::Start(HEADER_LEN + u64::from(data_bytes)))?;
    Ok(())
}

/// PCM bytes after the header, as the u32 the RIFF sizes hold. Past the 4 GiB
/// RIFF limit (about 37 hours of one track) this is an error, never a wrapped size.
fn data_bytes_for(len: u64) -> std::io::Result<u32> {
    u32::try_from((len - HEADER_LEN) & !1)
        .ok()
        .filter(|n| n.checked_add(36).is_some())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "wav data exceeds the 4 GiB RIFF limit",
            )
        })
}

fn read_u32_at(file: &mut File, offset: u64) -> std::io::Result<u32> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn write_u32_at(file: &mut File, offset: u64, value: u32) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(&value.to_le_bytes())
}
