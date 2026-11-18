//! Post-Stop notes quality metadata, left-out spans, templates, custom-word
//! glossary hints, and live catch-up summaries.

#![cfg(feature = "meetings")]

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::managers::meetings::{MeetingSegment, MeetingsStore};
use crate::meeting_notes::{build_notes_prompt, parse_notes_reply, NoteKind, PromptSegment};
use crate::meeting_notes_runner::{generate_meeting_notes, NotesGenerator};

pub const NOT_COVERED_LINE: &str = "Not covered in this meeting.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NoteSource {
    pub segment_id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub channel: String,
    pub clock: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LeftOutSpan {
    pub segment_ids: Vec<i64>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub clock_range: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScoredNoteLine {
    pub kind: String,
    pub text: String,
    pub owner: Option<String>,
    pub citations: Vec<i64>,
    pub source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TemplateSectionOut {
    pub heading: String,
    pub lines: Vec<String>,
    pub not_covered: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NotesQualityReport {
    pub meeting_id: i64,
    pub approach: String,
    pub time_to_notes_ms: u64,
    pub after_stop_first_token_ms: u64,
    pub after_stop_final_ms: u64,
    pub lines: Vec<ScoredNoteLine>,
    pub sources: Vec<NoteSource>,
    pub left_out: Vec<LeftOutSpan>,
    pub templated_sections: Vec<TemplateSectionOut>,
    pub oversized_segments: Vec<i64>,
}

pub fn format_clock_mmss(ms: i64) -> String {
    let secs = ms.max(0) / 1_000;
    let (h, m, s) = (secs / 3_600, (secs % 3_600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

pub fn parse_template_headings(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|l| l.trim().strip_prefix('#'))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|h| !h.is_empty())
        .collect()
}

struct GuidedGen<'a, G> {
    inner: &'a mut G,
    template_body: Option<&'a str>,
    custom_words: &'a [String],
}

impl<G: NotesGenerator> NotesGenerator for GuidedGen<'_, G> {
    fn generate(&mut self, prompt: &str) -> Result<String> {
        let mut augmented = prompt.to_string();
        if !self.custom_words.is_empty() {
            augmented.push_str("\nPreferred spelling of meeting terms and names: ");
            augmented.push_str(&self.custom_words.join(", "));
            augmented.push('\n');
        }
        if let Some(tpl) = self.template_body {
            augmented.push_str(
                "\nUser template instructions (keep strict [S<id>] citations; never invent decisions or owners):\n",
            );
            augmented.push_str(tpl.trim());
            augmented.push('\n');
        }
        self.inner.generate(&augmented)
    }
}

fn mark_notes_ready(store: &MeetingsStore, meeting_id: i64) -> Result<()> {
    let conn = Connection::open(store.db_path())?;
    conn.busy_timeout(Duration::from_secs(1))?;
    conn.execute(
        "UPDATE meetings SET status = 'notes_ready' WHERE id = ?1",
        params![meeting_id],
    )?;
    Ok(())
}

pub fn run_notes_with_quality<G: NotesGenerator>(
    store: &MeetingsStore,
    meeting_id: i64,
    template_body: Option<&str>,
    generator: &mut G,
) -> Result<NotesQualityReport> {
    run_notes_with_quality_and_words(store, meeting_id, template_body, &[], generator)
}

pub fn run_notes_with_quality_and_words<G: NotesGenerator>(
    store: &MeetingsStore,
    meeting_id: i64,
    template_body: Option<&str>,
    custom_words: &[String],
    generator: &mut G,
) -> Result<NotesQualityReport> {
    let started = Instant::now();
    let mut first_ms = None;
    let mut on_prog = |_| {
        first_ms.get_or_insert_with(|| started.elapsed().as_millis() as u64);
    };
    let mut guided = GuidedGen {
        inner: generator,
        template_body,
        custom_words,
    };
    let outcome = generate_meeting_notes(store, meeting_id, &mut guided, &mut on_prog)?;
    let final_ms = started.elapsed().as_millis() as u64;
    mark_notes_ready(store, meeting_id)?;

    let segs = store.segments(meeting_id)?;
    let by_id: HashMap<i64, &MeetingSegment> = segs.iter().map(|s| (s.id, s)).collect();
    let mut cited_ids = BTreeSet::new();
    let mut lines = Vec::with_capacity(outcome.notes.len());
    for n in &outcome.notes {
        for &cid in &n.citations {
            if !by_id.contains_key(&cid) {
                bail!("unresolved citation S{cid}");
            }
            cited_ids.insert(cid);
        }
        let source = n
            .citations
            .first()
            .and_then(|id| by_id.get(id))
            .map(|s| format_clock_mmss(s.start_ms));
        lines.push(ScoredNoteLine {
            kind: n.kind.clone(),
            text: n.text.clone(),
            owner: n.owner.clone(),
            citations: n.citations.clone(),
            source,
        });
    }

    let sources: Vec<NoteSource> = cited_ids
        .iter()
        .filter_map(|id| by_id.get(id))
        .map(|s| NoteSource {
            segment_id: s.id,
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            channel: s.channel.clone(),
            clock: format_clock_mmss(s.start_ms),
        })
        .collect();

    let oversized: BTreeSet<i64> = outcome.oversized_segments.iter().copied().collect();
    let mut left_out = Vec::new();
    for s in segs.iter().filter(|s| !cited_ids.contains(&s.id)) {
        let reason = if oversized.contains(&s.id) {
            "oversized"
        } else if is_small_talk(&s.text) {
            "small_talk"
        } else {
            "not_covered"
        };
        let clock_range = format!(
            "{}-{}",
            format_clock_mmss(s.start_ms),
            format_clock_mmss(s.end_ms)
        );
        left_out.push(LeftOutSpan {
            segment_ids: vec![s.id],
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            clock_range,
            reason: reason.to_string(),
        });
    }

    let templated_sections = template_body
        .map(parse_template_headings)
        .unwrap_or_default()
        .into_iter()
        .map(|heading| {
            let hl = heading.to_lowercase();
            let matched: Vec<String> = lines
                .iter()
                .filter(|l| heading_matches(&hl, &l.kind, &l.text))
                .map(|l| l.text.clone())
                .collect();
            let not_covered = matched.is_empty();
            let sec_lines = if not_covered {
                vec![NOT_COVERED_LINE.to_string()]
            } else {
                matched
            };
            TemplateSectionOut {
                heading,
                lines: sec_lines,
                not_covered,
            }
        })
        .collect();

    let approach = if outcome.calls_total <= 1 {
        "one-shot"
    } else {
        "pieces"
    };

    Ok(NotesQualityReport {
        meeting_id,
        approach: approach.to_string(),
        time_to_notes_ms: final_ms,
        after_stop_first_token_ms: first_ms.unwrap_or(final_ms),
        after_stop_final_ms: final_ms,
        lines,
        sources,
        left_out,
        templated_sections,
        oversized_segments: outcome.oversized_segments,
    })
}

fn heading_matches(h_lower: &str, kind: &str, text: &str) -> bool {
    if (h_lower.contains("decision") && kind == "decision")
        || ((h_lower.contains("action") || h_lower.contains("next") || h_lower.contains("todo"))
            && kind == "action")
        || (h_lower.contains("question") && kind == "question")
        || (h_lower.contains("summary") && kind == "summary")
    {
        return true;
    }
    let text_low = text.to_lowercase();
    h_lower
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .any(|w| text_low.contains(w))
}

pub fn is_small_talk(text: &str) -> bool {
    let low = text.to_lowercase();
    "unrelated|espresso|coffee|cat |weather|football|five-a-side|freezing|weekend"
        .split('|')
        .any(|k| low.contains(k))
}

pub fn catch_up_window<G: NotesGenerator>(
    store: &MeetingsStore,
    meeting_id: i64,
    at_ms: i64,
    window_ms: i64,
    generator: &mut G,
) -> Result<Vec<ScoredNoteLine>> {
    let from_ms = at_ms.saturating_sub(window_ms.max(0));
    let in_win: Vec<MeetingSegment> = store
        .segments(meeting_id)?
        .into_iter()
        .filter(|s| s.start_ms >= from_ms && s.start_ms <= at_ms)
        .collect();
    if in_win.is_empty() {
        return Ok(Vec::new());
    }
    let prompt_segs: Vec<PromptSegment> = in_win
        .iter()
        .map(|s| PromptSegment {
            id: s.id,
            channel: s.channel.clone(),
            text: s.text.clone(),
        })
        .collect();
    let by_id: HashMap<i64, &MeetingSegment> = in_win.iter().map(|s| (s.id, s)).collect();
    let reply = generator.generate(&build_notes_prompt(&prompt_segs, &[]))?;
    Ok(parse_notes_reply(&reply, &prompt_segs)
        .into_iter()
        .map(|l| ScoredNoteLine {
            kind: match l.kind {
                NoteKind::Summary => "summary",
                NoteKind::Decision => "decision",
                NoteKind::Action => "action",
                NoteKind::Question => "question",
            }
            .to_string(),
            text: l.text,
            owner: l.owner,
            source: l
                .citations
                .first()
                .and_then(|id| by_id.get(id))
                .map(|s| format_clock_mmss(s.start_ms)),
            citations: l.citations,
        })
        .collect())
}
