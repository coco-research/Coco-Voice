//! Notes runner: turn a stopped meeting's transcript and notepad into saved,
//! cited notes using one-shot generation when the transcript fits or map-merge
//! across pieces when longer.

#![cfg(feature = "meetings")]

use std::ops::Range;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use log::{debug, warn};

use crate::managers::meetings::{MeetingsStore, StoredNote};
use crate::meeting_notes::{build_notes_prompt, parse_notes_reply, NoteKind, PromptSegment};
use crate::meeting_notes_merge::merge_pieces;
use crate::meeting_notes_pieces::{
    estimate_tokens, is_one_shot, piece_budget, plan_pieces, SEGMENT_TAG_TOKENS,
};

/// Stored kind of a notepad point the model dropped and the runner preserved.
pub const NOTEPAD_KIND: &str = "notepad";

/// One model call on a full notes prompt.
pub trait NotesGenerator {
    fn generate(&mut self, prompt: &str) -> Result<String>;
}

/// Sent after each model call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotesProgress {
    pub calls_done: usize,
    pub calls_total: usize,
    pub elapsed_secs: u64,
}

impl NotesProgress {
    pub fn label(&self) -> String {
        format!("Finishing notes: {} s", self.elapsed_secs)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct NotesOutcome {
    pub notes: Vec<StoredNote>,
    pub oversized_segments: Vec<i64>,
    pub calls_total: usize,
}

/// Write and save the notes of `meeting_id`.
///
/// A generator error, or no segment the model can read (none yet, or all
/// oversized), returns `Err` before anything is saved so earlier notes and the
/// transcript stay intact.
pub fn generate_meeting_notes<G: NotesGenerator>(
    store: &MeetingsStore,
    meeting_id: i64,
    generator: &mut G,
    on_progress: &mut dyn FnMut(NotesProgress),
) -> Result<NotesOutcome> {
    let started = Instant::now();
    let mut raw_segments = store.segments(meeting_id)?;
    raw_segments.sort_by_key(|s| (s.start_ms, s.id));
    let segments: Vec<PromptSegment> = raw_segments
        .into_iter()
        .map(|s| PromptSegment {
            id: s.id,
            channel: s.channel,
            text: s.text,
        })
        .collect();
    let notepad: Vec<String> = store
        .notepad(meeting_id)?
        .into_iter()
        .map(|(_, text)| text)
        .collect();

    let prompt_tokens = estimate_tokens(&build_notes_prompt(&[], &[]));
    let notepad_tokens =
        estimate_tokens(&build_notes_prompt(&[], &notepad)).saturating_sub(prompt_tokens);
    let segment_tokens: Vec<usize> = segments.iter().map(|s| estimate_tokens(&s.text)).collect();
    let budget = piece_budget(prompt_tokens).saturating_sub(notepad_tokens);
    let cost = |r: &Range<usize>| -> usize {
        segment_tokens[r.clone()]
            .iter()
            .fold(0, |acc, n| acc.saturating_add(n + SEGMENT_TAG_TOKENS))
    };

    let (pieces, oversized): (Vec<_>, Vec<_>) =
        plan_pieces(&segment_tokens, notepad_tokens, prompt_tokens)
            .into_iter()
            .partition(|r| cost(r) <= budget);
    let oversized_segments: Vec<i64> = oversized
        .into_iter()
        .flatten()
        .map(|i| segments[i].id)
        .collect();
    if pieces.is_empty() {
        return Err(anyhow!(
            "meeting {meeting_id}: no transcript the notes model can read (too long: {oversized_segments:?}); earlier notes kept"
        ));
    }
    if !oversized_segments.is_empty() {
        warn!(
            "meeting {meeting_id}: segments {oversized_segments:?} are too long for the notes model and were left out"
        );
    }
    debug!(
        "meeting {meeting_id}: notes in {} call(s), one-shot: {}",
        pieces.len(),
        is_one_shot(&pieces)
    );

    let calls_total = pieces.len();
    let mut mapped = Vec::with_capacity(calls_total);
    for (done, range) in pieces.iter().enumerate() {
        let piece = &segments[range.clone()];
        let reply = generator
            .generate(&build_notes_prompt(piece, &notepad))
            .with_context(|| format!("meeting {meeting_id}: notes model failed, nothing saved"))?;
        mapped.push(
            parse_notes_reply(&reply, piece)
                .into_iter()
                .map(|l| ((l.kind, l.owner), l.text, l.citations))
                .collect(),
        );
        on_progress(NotesProgress {
            calls_done: done + 1,
            calls_total,
            elapsed_secs: started.elapsed().as_secs(),
        });
    }

    let mut notes: Vec<StoredNote> = merge_pieces(mapped)
        .into_iter()
        .map(|((kind, owner), text, citations)| StoredNote {
            kind: kind_name(kind).to_string(),
            text,
            owner,
            citations,
        })
        .collect();

    for point in &notepad {
        let needle = normalise(point);
        if !needle.is_empty() && !notes.iter().any(|n| normalise(&n.text).contains(&needle)) {
            notes.push(StoredNote {
                kind: NOTEPAD_KIND.to_string(),
                text: point.clone(),
                owner: None,
                citations: Vec::new(),
            });
        }
    }

    store.save_notes(meeting_id, &notes)?;
    Ok(NotesOutcome {
        notes,
        oversized_segments,
        calls_total,
    })
}

fn kind_name(kind: NoteKind) -> &'static str {
    match kind {
        NoteKind::Summary => "summary",
        NoteKind::Decision => "decision",
        NoteKind::Action => "action",
        NoteKind::Question => "question",
    }
}

fn normalise(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The on-device model adapter for meeting notes generation.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub struct LocalLlmNotes {
    pub model_path: std::path::PathBuf,
}

#[cfg(target_os = "macos")]
impl NotesGenerator for LocalLlmNotes {
    fn generate(&mut self, prompt: &str) -> Result<String> {
        use crate::meeting_notes_pieces::NOTES_MAX_TOKENS;
        tauri::async_runtime::block_on(crate::local_llm::generate_text(
            &self.model_path,
            "You write meeting notes.",
            prompt,
            NOTES_MAX_TOKENS as i32,
            None,
            None,
        ))
        .map_err(|err| anyhow!("{}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ReplyGen(Result<String, &'static str>);

    impl NotesGenerator for ReplyGen {
        fn generate(&mut self, _prompt: &str) -> Result<String> {
            match &self.0 {
                Ok(reply) => Ok(reply.clone()),
                Err(msg) => Err(anyhow!("{}", msg)),
            }
        }
    }

    #[test]
    fn preserves_dropped_notepad_points_and_keeps_prior_notes_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let id = store.start_meeting("Sync", "turbo").unwrap();
        let s1 = store
            .insert_segment(
                id,
                0,
                1_000,
                crate::managers::meetings::MeetingChannel::Me,
                "We decided to ship Friday",
            )
            .unwrap();
        store
            .add_notepad_point(id, 500, "Follow up on metrics")
            .unwrap();

        let out = generate_meeting_notes(
            &store,
            id,
            &mut ReplyGen(Ok(format!("DECISION: Ship Friday [S{s1}]"))),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(out.calls_total, 1);
        assert_eq!(out.notes.len(), 2);
        assert!(out
            .notes
            .iter()
            .any(|n| n.kind == NOTEPAD_KIND && n.text == "Follow up on metrics"));

        assert!(
            generate_meeting_notes(&store, id, &mut ReplyGen(Err("failed")), &mut |_| {}).is_err()
        );
        assert_eq!(store.notes(id).unwrap().len(), 2);
    }
}
