//! Append additional audio to a finished meeting after fresh consent, and drive
//! deterministic pause/resume timelines without splitting the meeting.

#![cfg(feature = "meetings")]

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::audio_toolkit::audio::AppendWav;
use crate::managers::meeting_chunker::{ChannelChunker, FinishedChunk, CHANNEL_ME, CHANNEL_OTHERS};
use crate::managers::meetings::{meeting_is_active, MeetingsStore};

const FRAME_SAMPLES: usize = 480;
const SAMPLE_RATE_MS: i64 = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppendAudioRefusal {
    ConsentRequired,
    MeetingActive,
    MeetingNotFound,
    Io(String),
    Transcription(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AppendAudioOutcome {
    pub meeting_id: i64,
    pub added_segments: usize,
    pub offset_ms: i64,
}

/// Single-use fresh-consent gate for adding audio after a meeting has ended.
#[derive(Default)]
pub struct AppendConsentGate {
    next_token: AtomicU64,
    pending: Mutex<Option<(i64, u64)>>,
}

impl AppendConsentGate {
    pub const fn new() -> Self {
        Self {
            next_token: AtomicU64::new(0),
            pending: Mutex::new(None),
        }
    }

    pub fn request_consent(&self, meeting_id: i64) -> u64 {
        let token = self.next_token.fetch_add(1, Ordering::Relaxed) + 1;
        *self.pending.lock().unwrap_or_else(|err| err.into_inner()) = Some((meeting_id, token));
        token
    }

    pub fn consume(&self, meeting_id: i64, token: u64) -> Result<(), AppendAudioRefusal> {
        let mut slot = self.pending.lock().unwrap_or_else(|err| err.into_inner());
        if *slot == Some((meeting_id, token)) {
            *slot = None;
            Ok(())
        } else {
            Err(AppendAudioRefusal::ConsentRequired)
        }
    }
}

fn build_chunker(
    channel: &'static str,
    silero_model: Option<&Path>,
    offset_ms: i64,
) -> Result<ChannelChunker, String> {
    let base = match silero_model {
        Some(path) => ChannelChunker::silero(channel, path).map_err(|err| err.to_string())?,
        None => ChannelChunker::energy(channel),
    };
    Ok(base.with_offset_ms(offset_ms))
}

fn to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

fn append_track_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    if samples.is_empty() {
        return Ok(());
    }
    let mut wav = if path.exists() {
        AppendWav::open(path).map_err(|err| err.to_string())?
    } else {
        AppendWav::create(path).map_err(|err| err.to_string())?
    };
    wav.append(&to_pcm16(samples))
        .and_then(|()| wav.flush())
        .map_err(|err| err.to_string())
}

fn flush_both(
    me: &mut ChannelChunker,
    others: &mut ChannelChunker,
    chunks: &mut Vec<FinishedChunk>,
) {
    for chunker in [me, others] {
        if let Some(tail) = chunker.finish() {
            chunks.push(tail);
        }
    }
}

fn store_chunks(
    store: &MeetingsStore,
    meeting_id: i64,
    mut chunks: Vec<FinishedChunk>,
    transcribe: &dyn Fn(Vec<f32>) -> Result<String, String>,
) -> Result<usize, AppendAudioRefusal> {
    chunks.sort_by_key(|c| c.start_ms);
    let mut count = 0usize;
    for chunk in chunks {
        let text = transcribe(chunk.samples).map_err(AppendAudioRefusal::Transcription)?;
        store
            .insert_segment(
                meeting_id,
                chunk.start_ms,
                chunk.end_ms,
                chunk.channel,
                &text,
            )
            .map_err(|err| AppendAudioRefusal::Io(err.to_string()))?;
        count += 1;
    }
    Ok(count)
}

/// Append `me_samples` and `others_samples` to an existing finished meeting.
/// Refuses with [`AppendAudioRefusal::ConsentRequired`] unless `fresh_consent` is true.
pub fn append_audio_to_meeting(
    root: &Path,
    meeting_id: i64,
    fresh_consent: bool,
    me_samples: &[f32],
    others_samples: &[f32],
    silero_model: Option<&Path>,
    transcribe: &dyn Fn(Vec<f32>) -> Result<String, String>,
) -> Result<AppendAudioOutcome, AppendAudioRefusal> {
    if !fresh_consent {
        return Err(AppendAudioRefusal::ConsentRequired);
    }
    if meeting_is_active() {
        return Err(AppendAudioRefusal::MeetingActive);
    }
    let store = MeetingsStore::open(root).map_err(|err| AppendAudioRefusal::Io(err.to_string()))?;
    let meeting = store
        .meeting(meeting_id)
        .map_err(|_| AppendAudioRefusal::MeetingNotFound)?;
    if meeting.status == "recording" {
        return Err(AppendAudioRefusal::MeetingActive);
    }
    let seg_end_ms = store
        .segments(meeting_id)
        .map_err(|err| AppendAudioRefusal::Io(err.to_string()))?
        .iter()
        .map(|s| s.end_ms)
        .max()
        .unwrap_or(0);
    let dur_ms = meeting
        .ended_at
        .map(|ended| (ended - meeting.started_at).max(0) * 1000)
        .unwrap_or(0);
    let offset_ms = seg_end_ms.max(dur_ms);

    let dir = root.join("meetings").join(meeting_id.to_string());
    fs::create_dir_all(&dir).map_err(|err| AppendAudioRefusal::Io(err.to_string()))?;
    append_track_wav(&dir.join("mic.wav"), me_samples).map_err(AppendAudioRefusal::Io)?;
    append_track_wav(&dir.join("system.wav"), others_samples).map_err(AppendAudioRefusal::Io)?;

    let mut me =
        build_chunker(CHANNEL_ME, silero_model, offset_ms).map_err(AppendAudioRefusal::Io)?;
    let mut others =
        build_chunker(CHANNEL_OTHERS, silero_model, offset_ms).map_err(AppendAudioRefusal::Io)?;
    let mut chunks = me.push(me_samples);
    chunks.extend(others.push(others_samples));
    flush_both(&mut me, &mut others, &mut chunks);
    let added_segments = store_chunks(&store, meeting_id, chunks, transcribe)?;

    let extra_secs = ((me_samples.len().max(others_samples.len()) as i64) + 15_999) / 16_000;
    let new_end = meeting
        .ended_at
        .unwrap_or(meeting.started_at)
        .saturating_add(extra_secs);
    let conn =
        Connection::open(store.db_path()).map_err(|err| AppendAudioRefusal::Io(err.to_string()))?;
    conn.execute(
        "UPDATE meetings SET ended_at = ?1, status = 'transcript_ready' WHERE id = ?2",
        params![new_end, meeting_id],
    )
    .map_err(|err| AppendAudioRefusal::Io(err.to_string()))?;

    Ok(AppendAudioOutcome {
        meeting_id,
        added_segments,
        offset_ms,
    })
}

/// Drive a single meeting across optional `(pause_start_ms, resume_at_ms)` windows.
/// Frames inside a pause window write zero WAV samples and emit zero segments.
#[allow(clippy::too_many_arguments)]
pub fn run_wav_tracks_with_pauses(
    root: &Path,
    title: &str,
    model_id: &str,
    me_samples: &[f32],
    others_samples: &[f32],
    pause_windows_ms: &[(i64, i64)],
    silero_model: Option<&Path>,
    transcribe: &dyn Fn(Vec<f32>) -> Result<String, String>,
) -> Result<i64, String> {
    let store = MeetingsStore::open(root).map_err(|err| err.to_string())?;
    let id = store
        .start_meeting(title, model_id)
        .map_err(|err| err.to_string())?;
    let dir = root.join("meetings").join(id.to_string());
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let mut mic_wav = AppendWav::create(&dir.join("mic.wav")).map_err(|err| err.to_string())?;
    let mut sys_wav = AppendWav::create(&dir.join("system.wav")).map_err(|err| err.to_string())?;
    let mut me = build_chunker(CHANNEL_ME, silero_model, 0)?;
    let mut others = build_chunker(CHANNEL_OTHERS, silero_model, 0)?;

    let total_len = me_samples.len().max(others_samples.len());
    let mut chunks: Vec<FinishedChunk> = Vec::new();
    let mut was_paused = false;
    let mut cursor = 0usize;
    while cursor < total_len {
        let end = (cursor + FRAME_SAMPLES).min(total_len);
        let at_ms = (cursor as i64) / SAMPLE_RATE_MS;
        let in_pause = pause_windows_ms
            .iter()
            .any(|(s, e)| at_ms >= *s && at_ms < *e);
        if in_pause && !was_paused {
            flush_both(&mut me, &mut others, &mut chunks);
        }
        was_paused = in_pause;
        let me_slice = me_samples
            .get(cursor..end.min(me_samples.len()))
            .unwrap_or(&[]);
        let sys_slice = others_samples
            .get(cursor..end.min(others_samples.len()))
            .unwrap_or(&[]);
        if in_pause {
            me.skip_paused(me_slice.len());
            others.skip_paused(sys_slice.len());
        } else {
            if !me_slice.is_empty() {
                mic_wav
                    .append(&to_pcm16(me_slice))
                    .map_err(|err| err.to_string())?;
                chunks.extend(me.push(me_slice));
            }
            if !sys_slice.is_empty() {
                sys_wav
                    .append(&to_pcm16(sys_slice))
                    .map_err(|err| err.to_string())?;
                chunks.extend(others.push(sys_slice));
            }
        }
        cursor = end;
    }
    if !was_paused {
        flush_both(&mut me, &mut others, &mut chunks);
    }
    mic_wav.flush().map_err(|err| err.to_string())?;
    sys_wav.flush().map_err(|err| err.to_string())?;
    store_chunks(&store, id, chunks, transcribe).map_err(|err| format!("{err:?}"))?;

    let ended = Utc::now().timestamp();
    let conn = Connection::open(store.db_path()).map_err(|err| err.to_string())?;
    conn.execute(
        "UPDATE meetings SET ended_at = ?1, status = 'transcript_ready' WHERE id = ?2",
        params![ended, id],
    )
    .map_err(|err| err.to_string())?;
    Ok(id)
}
