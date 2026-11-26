#![cfg_attr(not(test), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};

use tauri::{AppHandle, Emitter, Manager};

use crate::audio_toolkit::audio::AppendWav;
use crate::managers::meeting_chunker::{ChannelChunker, FinishedChunk};
use crate::managers::meetings::{MeetingChannel, MeetingsStore};
use crate::managers::transcription::TranscriptionManager;
use crate::meeting_controller::{MeetingSegmentView, MeetingWarning, SEGMENT_EVENT};

static PENDING_CHUNKS: AtomicUsize = AtomicUsize::new(0);

/// Number of transcribed-audio chunks currently waiting in the meeting feed queue.
pub(crate) fn pending_chunk_count() -> usize {
    PENDING_CHUNKS.load(Ordering::Relaxed)
}

struct PendingGuard;

impl PendingGuard {
    fn acquire() -> Self {
        PENDING_CHUNKS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        let _ = PENDING_CHUNKS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
            Some(cur.saturating_sub(1))
        });
    }
}

pub(crate) type TranscribeFn = Arc<dyn Fn(Vec<f32>) -> Result<String, String> + Send + Sync>;

fn poison<T>(err: std::sync::PoisonError<T>) -> T {
    err.into_inner()
}

fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

pub(crate) struct Job {
    channel: MeetingChannel,
    start_ms: i64,
    end_ms: i64,
    samples: Vec<f32>,
    _pending: PendingGuard,
}

pub(crate) struct Feed {
    pub(crate) system_wav: Mutex<AppendWav>,
    pub(crate) me_wav: Mutex<Option<AppendWav>>,
    pub(crate) me: Mutex<ChannelChunker>,
    pub(crate) others: Mutex<ChannelChunker>,
    pub(crate) tx: Mutex<Option<Sender<Job>>>,
    pub(crate) write_failed: AtomicBool,
    pub(crate) heard: AtomicBool,
    pub(crate) pause_gate: RwLock<bool>,
    pub(crate) warnings: Mutex<Vec<MeetingWarning>>,
    pub(crate) mic_name: Mutex<Option<String>>,
}

impl Feed {
    pub(crate) fn set_paused(&self, paused: bool) {
        *self.pause_gate.write().unwrap_or_else(poison) = paused;
    }

    #[cfg(test)]
    pub(crate) fn is_paused(&self) -> bool {
        *self.pause_gate.read().unwrap_or_else(poison)
    }

    pub(crate) fn push_me(&self, samples: &[f32]) {
        let paused = self.pause_gate.read().unwrap_or_else(poison);
        if *paused {
            return;
        }
        if !write_optional(&self.me_wav, &self.write_failed, samples) {
            return;
        }
        self.push_chunker(&self.me, samples);
    }

    pub(crate) fn push_me_chunker(&self, samples: &[f32]) {
        let paused = self.pause_gate.read().unwrap_or_else(poison);
        if *paused {
            return;
        }
        self.push_chunker(&self.me, samples);
    }

    pub(crate) fn push_others(&self, samples: &[f32]) {
        let paused = self.pause_gate.read().unwrap_or_else(poison);
        if *paused {
            return;
        }
        if samples.iter().any(|sample| *sample != 0.0) {
            self.heard.store(true, Ordering::Relaxed);
        }
        if !write_wav(&self.system_wav, &self.write_failed, samples) {
            return;
        }
        self.push_chunker(&self.others, samples);
    }

    pub(crate) fn push_chunker(&self, chunker: &Mutex<ChannelChunker>, samples: &[f32]) {
        let done = chunker.lock().unwrap_or_else(poison).push(samples);
        for chunk in done {
            self.enqueue(chunk);
        }
    }

    fn enqueue(&self, chunk: FinishedChunk) {
        let Some(channel) = MeetingChannel::from_label(chunk.channel) else {
            log::warn!("meeting dropped a chunk with an unknown channel");
            return;
        };
        let job = Job {
            channel,
            start_ms: chunk.start_ms,
            end_ms: chunk.end_ms,
            samples: chunk.samples,
            _pending: PendingGuard::acquire(),
        };
        let tx = self.tx.lock().unwrap_or_else(poison);
        if let Some(tx) = tx.as_ref() {
            if tx.send(job).is_err() {
                log::warn!("meeting chunk queue closed");
            }
        }
    }

    pub(crate) fn finish_chunkers(&self) {
        for chunker in [&self.me, &self.others] {
            if let Some(chunk) = chunker.lock().unwrap_or_else(poison).finish() {
                self.enqueue(chunk);
            }
        }
    }
}

fn write_wav(wav: &Mutex<AppendWav>, failed: &AtomicBool, samples: &[f32]) -> bool {
    if failed.load(Ordering::Relaxed) {
        return false;
    }
    if let Err(err) = wav.lock().unwrap_or_else(poison).append(&to_i16(samples)) {
        failed.store(true, Ordering::Relaxed);
        log::warn!("meeting wav write failed: {err}");
        return false;
    }
    true
}

pub(crate) fn write_optional(
    slot: &Mutex<Option<AppendWav>>,
    failed: &AtomicBool,
    samples: &[f32],
) -> bool {
    let mut guard = slot.lock().unwrap_or_else(poison);
    let Some(wav) = guard.as_mut() else {
        return true;
    };
    if failed.load(Ordering::Relaxed) {
        return false;
    }
    if let Err(err) = wav.append(&to_i16(samples)) {
        failed.store(true, Ordering::Relaxed);
        log::warn!("meeting wav write failed: {err}");
        return false;
    }
    true
}

pub(crate) fn push_warning(feed: &Feed, warning: MeetingWarning) {
    let mut warnings = feed.warnings.lock().unwrap_or_else(poison);
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

pub(crate) fn chunker(
    app: Option<&AppHandle>,
    channel: &'static str,
    fake: bool,
) -> anyhow::Result<ChannelChunker> {
    #[cfg(test)]
    if fake {
        return Ok(ChannelChunker::energy(channel));
    }
    #[cfg(not(test))]
    if fake {
        anyhow::bail!("fake meeting chunker is test-only");
    }
    let app = app.ok_or_else(|| anyhow::anyhow!("meeting silero model requires an app handle"))?;
    let path = app
        .path()
        .resolve(
            "resources/models/silero_vad_v4.onnx",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|err| anyhow::anyhow!("meeting silero model path unavailable: {err}"))?;
    ChannelChunker::silero(channel, &path)
}

pub(crate) fn spawn_worker(
    store: MeetingsStore,
    id: i64,
    rx: Receiver<Job>,
    test_transcribe: Option<TranscribeFn>,
    app: Option<AppHandle>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            record_chunk(&store, id, job, test_transcribe.as_ref(), app.as_ref());
        }
    })
}

fn record_chunk(
    store: &MeetingsStore,
    id: i64,
    job: Job,
    test_transcribe: Option<&TranscribeFn>,
    app: Option<&AppHandle>,
) {
    let transcribed = if let Some(transcribe) = test_transcribe {
        transcribe(job.samples)
    } else if let Some(app) = app {
        match app.try_state::<Arc<TranscriptionManager>>() {
            Some(tm) => tm
                .transcribe_meeting_chunk(job.samples)
                .map_err(|err| err.to_string()),
            None => Err("transcription manager missing".to_string()),
        }
    } else {
        Err("transcription manager missing".to_string())
    };
    match transcribed {
        Ok(text) => {
            if store
                .insert_segment(id, job.start_ms, job.end_ms, job.channel, &text)
                .is_err()
            {
                log::warn!("meeting {id} segment insert failed");
                return;
            }
            if let Some(app) = app {
                let _ = app.emit(
                    SEGMENT_EVENT,
                    MeetingSegmentView {
                        meeting_id: id,
                        channel: job.channel,
                        start_ms: job.start_ms,
                        end_ms: job.end_ms,
                        text,
                    },
                );
            }
        }
        Err(err) => {
            // The wav already has this audio. Empty text is the gap in the row.
            log::warn!(
                "meeting {id} chunk transcription failed ({} {}-{} ms): {err}",
                job.channel.as_str(),
                job.start_ms,
                job.end_ms
            );
            if store
                .insert_segment(id, job.start_ms, job.end_ms, job.channel, "")
                .is_err()
            {
                log::warn!("meeting {id} gap insert failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::meeting_chunker::CHANNEL_ME;

    #[test]
    fn non_fake_chunker_without_app_returns_err() {
        assert!(chunker(None, CHANNEL_ME, false).is_err());
    }
}
