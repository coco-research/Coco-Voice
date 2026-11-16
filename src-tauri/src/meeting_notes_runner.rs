//! Notes runner: turn a stopped meeting's transcript and notepad into saved,
//! cited notes. The pieces plan picks the path: one model call when everything
//! fits the notes context, else one call per piece and a merge in code. The
//! model sits behind [`NotesGenerator`]; the app uses [`LocalLlmNotes`].

#![cfg_attr(not(test), allow(dead_code))] // The Stop flow calls this later.

use std::ops::Range;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use log::{debug, warn};
use rusqlite::{params, Connection};

use crate::managers::meetings::{MeetingsStore, StoredNote};
use crate::meeting_notes::{build_notes_prompt, parse_notes_reply, NoteKind, PromptSegment};
use crate::meeting_notes_merge::merge_pieces;
use crate::meeting_notes_pieces::{
    estimate_tokens, is_one_shot, piece_budget, plan_pieces, SEGMENT_TAG_TOKENS,
};

/// Stored kind of a notepad point the model dropped and the runner put back.
pub const NOTEPAD_KIND: &str = "notepad";

/// One model call on a full notes prompt.
pub trait NotesGenerator {
    /// The model's reply, or why it failed (shown to the user with `{:#}`).
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
    /// `Finishing notes: N s`, N = seconds since the run started.
    pub fn label(&self) -> String {
        format!("Finishing notes: {} s", self.elapsed_secs)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct NotesOutcome {
    /// What was saved, in order.
    pub notes: Vec<StoredNote>,
    /// Segments too long for any piece: reported to the user, never sent.
    pub oversized_segments: Vec<i64>,
}

/// Write and save the notes of `meeting_id`. Blocks on `generator`, so run it
/// on a blocking thread.
///
/// A generator error, or no segment the model can read (none yet, or all
/// oversized), returns `Err` before anything is saved: earlier notes and the transcript stay as they were.
pub fn generate_meeting_notes<G: NotesGenerator>(
    store: &MeetingsStore,
    meeting_id: i64,
    generator: &mut G,
    on_progress: &mut dyn FnMut(NotesProgress),
) -> Result<NotesOutcome> {
    let started = Instant::now();
    let segments = load_segments(store, meeting_id)?;
    let notepad: Vec<String> = store
        .notepad(meeting_id)?
        .into_iter()
        .map(|(_, text)| text)
        .collect();

    // Token estimates of the prompt exactly as it renders, against the pieces budget.
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

    // An oversized segment gets a piece of its own; that piece is never sent.
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
        warn!("meeting {meeting_id}: segments {oversized_segments:?} are too long for the notes model and were left out");
    }
    debug!(
        "meeting {meeting_id}: notes in {} call(s), one-shot: {}",
        pieces.len(),
        is_one_shot(&pieces)
    );

    let mut mapped = Vec::with_capacity(pieces.len());
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
            calls_total: pieces.len(),
            elapsed_secs: started.elapsed().as_secs(),
        });
    }

    // The owner rides in the merge key, so one action with two owners stays two lines.
    let mut notes: Vec<StoredNote> = merge_pieces(mapped)
        .into_iter()
        .map(|((kind, owner), text, citations)| StoredNote {
            kind: kind_name(kind).to_string(),
            text,
            owner,
            citations,
        })
        .collect();

    // ponytail: normalised substring match; a paraphrased point comes back as an extra user line.
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

// ponytail: local reader; switch to the store's segment API once the chunker PR lands.
fn load_segments(store: &MeetingsStore, meeting_id: i64) -> Result<Vec<PromptSegment>> {
    let conn = Connection::open(store.db_path())?;
    conn.busy_timeout(Duration::from_secs(1))?;
    let mut stmt = conn.prepare(
        "SELECT id, channel, text FROM meeting_segments
         WHERE meeting_id = ?1 ORDER BY start_ms, id",
    )?;
    let rows = stmt.query_map(params![meeting_id], |row| {
        Ok(PromptSegment {
            id: row.get(0)?,
            channel: row.get(1)?,
            text: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The on-device model, opened with the notes context `NOTES_N_CTX`.
#[cfg(target_os = "macos")]
#[allow(dead_code)] // The Stop flow constructs it; tests use a fake.
pub struct LocalLlmNotes {
    pub model_path: std::path::PathBuf,
}

#[cfg(target_os = "macos")]
impl NotesGenerator for LocalLlmNotes {
    fn generate(&mut self, prompt: &str) -> Result<String> {
        use crate::meeting_notes_pieces::{NOTES_MAX_TOKENS, NOTES_N_CTX};
        tauri::async_runtime::block_on(crate::local_llm::generate_text_in_ctx(
            NOTES_N_CTX as u32,
            &self.model_path,
            "You write meeting notes.",
            prompt,
            NOTES_MAX_TOKENS as i32,
            None,
            None,
        ))
        .map_err(anyhow::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records every prompt; each reply cites the first segment of its prompt.
    struct Fake {
        prompts: Vec<String>,
        reply: fn(i64) -> Result<String, String>,
    }

    impl NotesGenerator for Fake {
        fn generate(&mut self, prompt: &str) -> Result<String> {
            self.prompts.push(prompt.to_string());
            let first = prompt
                .lines()
                .find_map(|l| l.strip_prefix("[S")?.split(']').next()?.parse().ok())
                .expect("prompt has a segment");
            (self.reply)(first).map_err(|e| anyhow!(e))
        }
    }

    fn fake(reply: fn(i64) -> Result<String, String>) -> Fake {
        Fake {
            prompts: Vec::new(),
            reply,
        }
    }

    fn meeting(texts: &[String]) -> (tempfile::TempDir, MeetingsStore, i64, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let id = store.start_meeting("Planning", "model").unwrap();
        let conn = Connection::open(store.db_path()).unwrap();
        let ids = texts
            .iter()
            .enumerate()
            .map(|(i, text)| {
                conn.execute(
                    "INSERT INTO meeting_segments (meeting_id, start_ms, end_ms, channel, text)
                     VALUES (?1, ?2, ?2, 'me', ?3)",
                    params![id, i as i64 * 1000, text],
                )
                .unwrap();
                conn.last_insert_rowid()
            })
            .collect();
        (dir, store, id, ids)
    }

    fn texts(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    /// A segment of about `tokens` estimated tokens, starting with `tag`.
    fn sized(tag: &str, tokens: usize) -> String {
        format!("{tag} {}", "x".repeat(tokens * 4))
    }

    fn run(
        store: &MeetingsStore,
        id: i64,
        gen: &mut Fake,
    ) -> (Result<NotesOutcome>, Vec<NotesProgress>) {
        let mut progress = Vec::new();
        let out = generate_meeting_notes(store, id, gen, &mut |p| progress.push(p));
        (out, progress)
    }

    fn note(kind: &str, text: &str, owner: Option<&str>, citations: Vec<i64>) -> StoredNote {
        StoredNote {
            kind: kind.to_string(),
            text: text.to_string(),
            owner: owner.map(str::to_string),
            citations,
        }
    }

    #[test]
    fn one_shot_writes_and_saves_cited_notes() {
        let (_dir, store, id, ids) = meeting(&texts(&[
            "Priya will send the budget",
            "We decided to ship on Monday",
        ]));
        let mut gen = fake(|s| {
            Ok(format!(
                "DECISION: Ship on Monday [S{s}]\nACTION: Priya | Send the budget [S{s}]"
            ))
        });
        let (out, progress) = run(&store, id, &mut gen);
        let out = out.unwrap();

        assert_eq!(gen.prompts.len(), 1);
        assert!(gen.prompts[0].contains("We decided to ship on Monday"));
        let expected = vec![
            note("decision", "Ship on Monday", None, vec![ids[0]]),
            note("action", "Send the budget", Some("Priya"), vec![ids[0]]),
        ];
        assert_eq!(out.notes, expected);
        assert_eq!(store.notes(id).unwrap(), expected);
        assert_eq!(progress.len(), 1);
        assert_eq!((progress[0].calls_done, progress[0].calls_total), (1, 1));
        assert!(progress[0].label().starts_with("Finishing notes: "));
        assert!(progress[0].label().ends_with(" s"));
    }

    #[test]
    fn long_meeting_maps_pieces_then_merges() {
        let t = piece_budget(0) / 4;
        let segs: Vec<String> = (0..6).map(|i| sized(&format!("seg{i}"), t)).collect();
        let (_dir, store, id, ids) = meeting(&segs);
        let mut gen = fake(|s| {
            Ok(format!(
                "DECISION: Ship the beta [S{s}]\nSUMMARY: Part {s} [S{s}]"
            ))
        });
        let (out, progress) = run(&store, id, &mut gen);
        let out = out.unwrap();

        let calls = gen.prompts.len();
        assert!(calls > 1, "expected map-reduce, got {calls} call");
        for (i, seg) in segs.iter().enumerate() {
            let seen = gen
                .prompts
                .iter()
                .filter(|p| p.contains(seg.as_str()))
                .count();
            assert_eq!(seen, 1, "segment {i} must be in exactly one piece");
        }
        let firsts: Vec<i64> = out
            .notes
            .iter()
            .filter(|n| n.kind == "summary")
            .map(|n| n.citations[0])
            .collect();
        assert_eq!(firsts.len(), calls);
        assert_eq!(firsts[0], ids[0]);
        let decisions: Vec<&StoredNote> =
            out.notes.iter().filter(|n| n.kind == "decision").collect();
        assert_eq!(
            decisions.len(),
            1,
            "the repeated decision merges into one line"
        );
        assert_eq!(decisions[0].citations, firsts);
        assert_eq!(store.notes(id).unwrap(), out.notes);
        assert_eq!(
            progress.last().map(|p| (p.calls_done, p.calls_total)),
            Some((calls, calls))
        );
    }

    #[test]
    fn dropped_notepad_point_is_restored_as_a_user_line() {
        let (_dir, store, id, ids) = meeting(&texts(&["Book the room for Friday"]));
        store
            .add_notepad_point(id, 10, "Check pricing with legal")
            .unwrap();
        store.add_notepad_point(id, 20, "book the  room").unwrap();
        let mut gen = fake(|s| Ok(format!("ACTION: - | Book the room [S{s}]")));
        let out = run(&store, id, &mut gen).0.unwrap();

        assert!(gen.prompts[0].contains("NOTE: Check pricing with legal"));
        let expected = vec![
            note("action", "Book the room", None, vec![ids[0]]),
            note(NOTEPAD_KIND, "Check pricing with legal", None, vec![]),
        ];
        assert_eq!(out.notes, expected);
        assert_eq!(store.notes(id).unwrap(), expected);
    }

    #[test]
    fn generator_error_is_reported_and_keeps_everything() {
        let (_dir, store, id, _ids) = meeting(&texts(&["Ship on Monday"]));
        let old = vec![note("summary", "Earlier notes", None, vec![1])];
        store.save_notes(id, &old).unwrap();
        let before = load_segments(&store, id).unwrap();
        let mut gen = fake(|_| Err("model offline".to_string()));
        let err = run(&store, id, &mut gen).0.unwrap_err();

        assert!(format!("{err:#}").contains("model offline"), "{err:#}");
        assert_eq!(load_segments(&store, id).unwrap(), before);
        assert_eq!(store.notes(id).unwrap(), old);
    }

    #[test]
    fn oversized_segment_is_reported_and_never_sent() {
        let huge = sized("HUGE", piece_budget(0) * 2);
        let (_dir, store, id, ids) = meeting(&[
            "We decided to ship".to_string(),
            huge,
            "Priya owns the email".to_string(),
        ]);
        let mut gen = fake(|s| Ok(format!("SUMMARY: Point {s} [S{s}]")));
        let out = run(&store, id, &mut gen).0.unwrap();

        assert_eq!(out.oversized_segments, vec![ids[1]]);
        assert!(gen.prompts.iter().all(|p| !p.contains("HUGE")));
        assert_eq!(gen.prompts.len(), 2);
        let cited: Vec<i64> = out.notes.iter().map(|n| n.citations[0]).collect();
        assert_eq!(cited, vec![ids[0], ids[2]]);
    }

    #[test]
    fn no_readable_transcript_keeps_earlier_notes() {
        let (_dir, store, id, _ids) = meeting(&[sized("HUGE", piece_budget(0) * 2)]);
        let old = vec![note("summary", "Earlier notes", None, vec![1])];
        store.save_notes(id, &old).unwrap();
        let mut gen = fake(|s| Ok(format!("SUMMARY: Point {s} [S{s}]")));
        let err = run(&store, id, &mut gen).0.unwrap_err();

        assert!(err.to_string().contains("too long"), "{err}");
        assert!(gen.prompts.is_empty());
        assert_eq!(store.notes(id).unwrap(), old);

        let (_dir, store, id, _ids) = meeting(&[]);
        store.save_notes(id, &old).unwrap();
        assert!(run(&store, id, &mut gen).0.is_err());
        assert_eq!(
            store.notes(id).unwrap(),
            old,
            "an empty transcript keeps earlier notes"
        );
    }
}
