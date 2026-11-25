//! Notes runner: turn a stopped meeting's transcript and notepad into saved,
//! cited notes using one-shot generation when the transcript fits or map-merge
//! across pieces when longer.

#![cfg(feature = "meetings")]

use std::ops::Range;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use log::{debug, warn};

use crate::managers::meetings::{MeetingsStore, StoredNote};
use crate::meeting_notes::{
    build_custom_template_notes_prompt, build_merge_notes_prompt, build_notes_prompt,
    parse_custom_template_reply, parse_notes_reply, NoteKind, PromptSegment,
};
use crate::meeting_notes_merge::merge_pieces;
use crate::meeting_notes_pieces::{
    estimate_tokens, is_one_shot, piece_budget, plan_pieces, SEGMENT_TAG_TOKENS,
};

/// Stored kind of a notepad point the model dropped and the runner preserved.
pub const NOTEPAD_KIND: &str = "notepad";

/// Target context window (tokens) opened for local meeting notes. Dictation
/// clean-up keeps its own 2,048 window; both reuse the same cached GGUF weights.
pub const DEFAULT_LOCAL_NOTES_N_CTX: usize = 4096;

/// Existing `LocalLlmError::OverBudget` reason string, returned verbatim when
/// a meeting requires more than one piece under the 4,096 one-shot guard.
pub const NOTES_OVER_BUDGET_REASON: &str =
    "Local LLM prompt exceeds the context budget; pasting the raw transcript";

/// Enforce the one-shot guard (`piece_count <= 1`) shared across after-Stop,
/// Retry, and rolling notes paths.
///
/// ponytail: ceiling = 1 piece on a 4,096 context; upgrade path = P2 decides
/// whether map-merge comes back or is deleted.
pub(crate) fn require_one_shot_pieces(meeting_id: i64, piece_count: usize) -> Result<()> {
    if piece_count > 1 {
        return Err(anyhow!(NOTES_OVER_BUDGET_REASON))
            .with_context(|| format!("meeting {meeting_id}: notes model failed, nothing saved"));
    }
    Ok(())
}

/// One model call on a full notes prompt.
pub trait NotesGenerator {
    fn generate(&mut self, prompt: &str) -> Result<String>;

    /// Effective context window (tokens) of this generator.
    ///
    /// Local llama.cpp generators override this to read back `ctx.n_ctx()` from
    /// the loaded context so piece planning and prompt enforcement share one
    /// source of truth.
    fn context_window(&mut self) -> Result<usize> {
        Ok(DEFAULT_LOCAL_NOTES_N_CTX)
    }
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
    generate_meeting_notes_with_headings(store, meeting_id, &[], generator, on_progress)
}

/// Write and save the notes of `meeting_id`, optionally using custom template
/// headings (`custom_headings`) as prompt line prefixes and parsed note kinds.
pub fn generate_meeting_notes_with_headings<G: NotesGenerator>(
    store: &MeetingsStore,
    meeting_id: i64,
    custom_headings: &[String],
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

    let n_ctx = generator
        .context_window()
        .with_context(|| format!("meeting {meeting_id}: failed to inspect notes model context"))?;

    let build_piece_prompt = |piece: &[PromptSegment], np: &[String]| -> String {
        if custom_headings.is_empty() {
            build_notes_prompt(piece, np)
        } else {
            build_custom_template_notes_prompt(piece, np, custom_headings)
        }
    };

    let prompt_tokens = estimate_tokens(&build_piece_prompt(&[], &[]));
    let notepad_tokens =
        estimate_tokens(&build_piece_prompt(&[], &notepad)).saturating_sub(prompt_tokens);
    let segment_tokens: Vec<usize> = segments.iter().map(|s| estimate_tokens(&s.text)).collect();
    let budget = piece_budget(n_ctx, prompt_tokens).saturating_sub(notepad_tokens);
    let cost = |r: &Range<usize>| -> usize {
        segment_tokens[r.clone()]
            .iter()
            .fold(0, |acc, n| acc.saturating_add(n + SEGMENT_TAG_TOKENS))
    };

    let planned = plan_pieces(&segment_tokens, notepad_tokens, prompt_tokens, n_ctx);
    // ponytail: ceiling = 1 piece on a 4,096 context; upgrade path = P2 decides
    // whether map-merge comes back or is deleted.
    require_one_shot_pieces(meeting_id, planned.len())?;

    let (pieces, oversized): (Vec<_>, Vec<_>) =
        planned.into_iter().partition(|r| cost(r) <= budget);
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
    let needs_merge_call = pieces.len() > 1;
    let calls_total = if needs_merge_call {
        pieces.len() + 1
    } else {
        pieces.len()
    };
    debug!(
        "meeting {meeting_id}: notes in {} call(s) (n_ctx={n_ctx}), one-shot: {}",
        calls_total,
        is_one_shot(&pieces)
    );

    type MappedNoteLine = ((String, Option<String>), String, Vec<i64>);
    type MergeDraftLine = (String, Option<String>, String, Vec<i64>);

    let parse_piece = |reply: &str, segs: &[PromptSegment]| -> Vec<MappedNoteLine> {
        if custom_headings.is_empty() {
            parse_notes_reply(reply, segs)
                .into_iter()
                .map(|l| {
                    (
                        (kind_name(l.kind).to_string(), l.owner),
                        l.text,
                        l.citations,
                    )
                })
                .collect()
        } else {
            parse_custom_template_reply(reply, segs, custom_headings)
                .into_iter()
                .map(|l| ((l.kind, l.owner), l.text, l.citations))
                .collect()
        }
    };

    let mut mapped = Vec::with_capacity(pieces.len());
    for (done, range) in pieces.iter().enumerate() {
        let piece = &segments[range.clone()];
        let reply = generator
            .generate(&build_piece_prompt(piece, &notepad))
            .with_context(|| format!("meeting {meeting_id}: notes model failed, nothing saved"))?;
        mapped.push(parse_piece(&reply, piece));
        on_progress(NotesProgress {
            calls_done: done + 1,
            calls_total,
            elapsed_secs: started.elapsed().as_secs(),
        });
    }

    let merged_deterministic = merge_pieces(mapped);
    let final_lines = if needs_merge_call && !merged_deterministic.is_empty() {
        let draft_for_merge: Vec<MergeDraftLine> = merged_deterministic
            .iter()
            .map(|((kind, owner), text, citations)| {
                (kind.clone(), owner.clone(), text.clone(), citations.clone())
            })
            .collect();
        let merge_prompt = build_merge_notes_prompt(&draft_for_merge, &notepad, custom_headings);
        let merge_reply = generator
            .generate(&merge_prompt)
            .with_context(|| format!("meeting {meeting_id}: notes merge failed, nothing saved"))?;
        on_progress(NotesProgress {
            calls_done: calls_total,
            calls_total,
            elapsed_secs: started.elapsed().as_secs(),
        });
        let reconciled = parse_piece(&merge_reply, &segments);
        if reconciled.is_empty() {
            merged_deterministic
        } else {
            merge_pieces(vec![reconciled])
        }
    } else {
        merged_deterministic
    };

    let mut notes: Vec<StoredNote> = final_lines
        .into_iter()
        .map(|((kind, owner), text, citations)| StoredNote {
            kind,
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
    cached_n_ctx: Option<usize>,
}

#[cfg(target_os = "macos")]
impl LocalLlmNotes {
    #[allow(dead_code)]
    pub fn new(model_path: std::path::PathBuf) -> Self {
        Self {
            model_path,
            cached_n_ctx: None,
        }
    }
}

#[cfg(target_os = "macos")]
impl NotesGenerator for LocalLlmNotes {
    fn context_window(&mut self) -> Result<usize> {
        if let Some(n_ctx) = self.cached_n_ctx {
            return Ok(n_ctx);
        }
        let actual = crate::local_llm::effective_context_size(
            &self.model_path,
            DEFAULT_LOCAL_NOTES_N_CTX as u32,
        )
        .map_err(|err| anyhow!("{}", err))?;
        self.cached_n_ctx = Some(actual);
        Ok(actual)
    }

    fn generate(&mut self, prompt: &str) -> Result<String> {
        use crate::meeting_notes_pieces::NOTES_MAX_TOKENS;
        let requested_n_ctx = self.cached_n_ctx.unwrap_or(DEFAULT_LOCAL_NOTES_N_CTX) as u32;
        tauri::async_runtime::block_on(crate::local_llm::generate_text_with_ctx(
            &self.model_path,
            "You write meeting notes.",
            prompt,
            NOTES_MAX_TOKENS as i32,
            requested_n_ctx,
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

    #[test]
    fn notes_context_budget_matches_local_llm_context() {
        assert_eq!(DEFAULT_LOCAL_NOTES_N_CTX, 4096);

        struct RecordingBoundedGen {
            n_ctx: usize,
            prompts: Vec<String>,
        }
        impl NotesGenerator for RecordingBoundedGen {
            fn context_window(&mut self) -> Result<usize> {
                Ok(self.n_ctx)
            }
            fn generate(&mut self, prompt: &str) -> Result<String> {
                let est = estimate_tokens(prompt);
                let limit = self.n_ctx - crate::meeting_notes_pieces::NOTES_MAX_TOKENS;
                if est > limit {
                    return Err(anyhow!("prompt {est} exceeds context limit {limit}"));
                }
                self.prompts.push(prompt.to_string());
                Ok("DECISION: Launch September 1 free [S1]".to_string())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();

        // 1. A short meeting fits in one shot at 4,096 and runs one call.
        let short_id = store.start_meeting("Short Sync", "turbo").unwrap();
        store
            .insert_segment(
                short_id,
                0,
                5_000,
                crate::managers::meetings::MeetingChannel::Me,
                "We decided to launch September 1 free.",
            )
            .unwrap();
        let mut ok_gen = RecordingBoundedGen {
            n_ctx: DEFAULT_LOCAL_NOTES_N_CTX,
            prompts: Vec::new(),
        };
        let out = generate_meeting_notes(&store, short_id, &mut ok_gen, &mut |_| {}).unwrap();
        assert_eq!(out.calls_total, 1);
        assert_eq!(ok_gen.prompts.len(), 1);
        assert_eq!(out.notes.len(), 1);

        // 2. A multi-piece meeting fails with the existing OverBudget reason,
        //    never invokes the generator (no map or merge prompt), and keeps
        //    both the transcript and any earlier saved notes intact.
        let long_id = store.start_meeting("Long Sync", "turbo").unwrap();
        let body =
            "We reviewed the migration schedule and deployment checklist in detail. ".repeat(14);
        for i in 0..16 {
            store
                .insert_segment(
                    long_id,
                    i * 10_000,
                    i * 10_000 + 8_000,
                    crate::managers::meetings::MeetingChannel::Me,
                    &body,
                )
                .unwrap();
        }
        let mut blocked_gen = RecordingBoundedGen {
            n_ctx: DEFAULT_LOCAL_NOTES_N_CTX,
            prompts: Vec::new(),
        };
        let err = generate_meeting_notes(&store, long_id, &mut blocked_gen, &mut |_| {})
            .expect_err("multi-piece meeting must fail under the 4,096 one-shot guard");
        let full_err = format!("{err:#}");
        assert!(
            full_err.contains(NOTES_OVER_BUDGET_REASON),
            "expected existing OverBudget reason, got: {full_err}"
        );
        assert!(
            blocked_gen.prompts.is_empty(),
            "multi-piece meeting must never send map or merge prompts, sent {}",
            blocked_gen.prompts.len()
        );
        assert_eq!(store.segments(long_id).unwrap().len(), 16);
        assert!(store.notes(long_id).unwrap().is_empty());
    }
}
