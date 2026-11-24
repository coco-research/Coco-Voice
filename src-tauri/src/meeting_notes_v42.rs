//! Post-Stop notes quality metadata, left-out spans, templates, custom-word
//! glossary hints, and live catch-up summaries.

#![cfg(feature = "meetings")]

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::managers::meetings::{MeetingSegment, MeetingsStore};
use crate::meeting_notes::{build_notes_prompt, parse_notes_reply, NoteKind, PromptSegment};
use crate::meeting_notes_runner::{generate_meeting_notes, NotesGenerator};

pub const NOT_COVERED_LINE: &str = "Not covered in this meeting.";
/// Minimum spoken duration in a catch-up window before summarising (`AC-X18`).
pub const MIN_CATCH_UP_SPOKEN_MS: i64 = 60_000;
/// Maximum bullet lines returned by catch-up (`AC-X18`).
pub const MAX_CATCH_UP_LINES: usize = 5;
/// Maximum gap (ms) between two adjacent left-out segments of the same kind to merge (`AC-X26`).
pub const ADJACENT_LEFT_OUT_GAP_MS: i64 = 15_000;

/// Persisted row in `meeting_left_out` (`AC-X26`, `AC-X27`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LeftOutRecord {
    pub kind: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub topic: String,
    pub reason: String,
    pub segment_ids: Vec<i64>,
}

/// Outcome of a live catch-up request over the last 5/10/15 minutes (`AC-X18`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CatchUpOutcome {
    NotEnoughSaid,
    Lines { lines: Vec<ScoredNoteLine> },
}

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
            "not_covered"
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
    let left_out = join_adjacent_left_out_spans(left_out);

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

/// Normalize any legacy left-out category (`"oversized"`, `"uncovered"`) into the
/// two canonical stored kinds (`"small_talk"` or `"not_covered"`, `AC-X26`).
pub fn normalize_left_out_kind(raw: &str) -> &'static str {
    if raw.trim().eq_ignore_ascii_case("small_talk") {
        "small_talk"
    } else {
        "not_covered"
    }
}

/// Join adjacent left-out spans of the same normalized kind within `ADJACENT_LEFT_OUT_GAP_MS`.
pub fn join_adjacent_left_out_spans(mut spans: Vec<LeftOutSpan>) -> Vec<LeftOutSpan> {
    spans.sort_by_key(|s| (s.start_ms, s.end_ms));
    let mut merged: Vec<LeftOutSpan> = Vec::new();
    for mut span in spans {
        span.reason = normalize_left_out_kind(&span.reason).to_string();
        if let Some(prev) = merged.last_mut() {
            if prev.reason == span.reason
                && span.start_ms <= prev.end_ms.saturating_add(ADJACENT_LEFT_OUT_GAP_MS)
            {
                prev.end_ms = prev.end_ms.max(span.end_ms);
                prev.segment_ids.extend(span.segment_ids);
                prev.clock_range = format!(
                    "{}-{}",
                    format_clock_mmss(prev.start_ms),
                    format_clock_mmss(prev.end_ms)
                );
                continue;
            }
        }
        merged.push(span);
    }
    merged
}

fn clamp_words(raw: &str, max_words: usize) -> String {
    raw.split_whitespace()
        .filter(|w| !w.is_empty())
        .take(max_words)
        .collect::<Vec<_>>()
        .join(" ")
}

fn fallback_left_out_topic(kind: &str) -> &'static str {
    if kind == "small_talk" {
        "Small talk"
    } else {
        "Other discussion"
    }
}

fn fallback_left_out_reason(kind: &str, is_oversized: bool) -> &'static str {
    if is_oversized {
        "Too long to make notes from."
    } else if kind == "small_talk" {
        "Small talk."
    } else {
        "No note covers this part."
    }
}

fn format_prompt_speaker(channel: &str) -> &'static str {
    if channel.eq_ignore_ascii_case("me") || channel.eq_ignore_ascii_case("mic") {
        "Me"
    } else {
        "Others"
    }
}

/// Build an on-device prompt for one left-out span containing strictly numbered
/// `[n] <Me|Others>: <text>` excerpts — NEVER meeting titles, attendees, or calendar fields (`AC-X31`).
pub fn build_left_out_reason_prompt(segments: &[&MeetingSegment]) -> String {
    let mut prompt = String::from(
        "Label this short transcript excerpt that was not included in the meeting notes.\n\
         Reply on one line as: TOPIC | REASON\n\
         - TOPIC: 2 to 5 plain words.\n\
         - REASON: at most 10 plain words.\n\n",
    );
    for (idx, seg) in segments.iter().enumerate() {
        prompt.push_str(&format!(
            "[{}] {}: {}\n",
            idx + 1,
            format_prompt_speaker(&seg.channel),
            seg.text.trim()
        ));
    }
    prompt
}

/// Compute persisted `LeftOutRecord`s for every uncited segment span and optionally
/// enrich each span with an on-device 2–5 word topic and <=10 word reason (`AC-X26`, `AC-X27`).
pub fn generate_left_out_reasons<G: NotesGenerator>(
    store: &MeetingsStore,
    meeting_id: i64,
    oversized_ids: &[i64],
    mut generator: Option<&mut G>,
) -> Result<Vec<LeftOutRecord>> {
    let segs = store.segments(meeting_id)?;
    let notes = store.notes(meeting_id)?;
    let mut cited = BTreeSet::new();
    for n in &notes {
        for &cid in &n.citations {
            cited.insert(cid);
        }
    }
    let oversized: BTreeSet<i64> = oversized_ids.iter().copied().collect();
    let by_id: HashMap<i64, &MeetingSegment> = segs.iter().map(|s| (s.id, s)).collect();

    let mut raw_spans = Vec::new();
    for s in segs
        .iter()
        .filter(|s| !s.text.trim().is_empty() && !cited.contains(&s.id))
    {
        let kind = if oversized.contains(&s.id) {
            "not_covered"
        } else if is_small_talk(&s.text) {
            "small_talk"
        } else {
            "not_covered"
        };
        raw_spans.push(LeftOutSpan {
            segment_ids: vec![s.id],
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            clock_range: format!(
                "{}-{}",
                format_clock_mmss(s.start_ms),
                format_clock_mmss(s.end_ms)
            ),
            reason: kind.to_string(),
        });
    }
    let joined = join_adjacent_left_out_spans(raw_spans);
    let mut records = Vec::with_capacity(joined.len());

    for span in joined {
        let kind = normalize_left_out_kind(&span.reason).to_string();
        let span_oversized = span.segment_ids.iter().any(|id| oversized.contains(id));
        let span_segs: Vec<&MeetingSegment> = span
            .segment_ids
            .iter()
            .filter_map(|id| by_id.get(id).copied())
            .collect();

        let (mut topic, mut reason) = (
            fallback_left_out_topic(&kind).to_string(),
            fallback_left_out_reason(&kind, span_oversized).to_string(),
        );

        if !span_oversized && !span_segs.is_empty() {
            if let Some(gen) = generator.as_deref_mut() {
                let prompt = build_left_out_reason_prompt(&span_segs);
                if let Ok(reply) = gen.generate(&prompt) {
                    if let Some(first_line) = reply.lines().map(str::trim).find(|l| !l.is_empty()) {
                        if let Some((raw_t, raw_r)) = first_line.split_once('|') {
                            let clean_t = clamp_words(raw_t.trim_matches(['"', '\'', '-', '*']), 5);
                            let clean_r =
                                clamp_words(raw_r.trim_matches(['"', '\'', '-', '*']), 10);
                            if !clean_t.is_empty() {
                                topic = clean_t;
                            }
                            if !clean_r.is_empty() {
                                reason = clean_r;
                            }
                        }
                    }
                }
            }
        }

        records.push(LeftOutRecord {
            kind,
            start_ms: span.start_ms,
            end_ms: span.end_ms,
            topic,
            reason,
            segment_ids: span.segment_ids,
        });
    }
    Ok(records)
}

/// Replace all persisted `meeting_left_out` rows for `meeting_id` (`AC-X26`).
pub fn save_left_out_spans(
    store: &MeetingsStore,
    meeting_id: i64,
    records: &[LeftOutRecord],
) -> Result<()> {
    let conn = store.connection()?;
    conn.execute(
        "DELETE FROM meeting_left_out WHERE meeting_id = ?1",
        params![meeting_id],
    )?;
    for rec in records {
        let seg_json = serde_json::to_string(&rec.segment_ids)?;
        conn.execute(
            "INSERT INTO meeting_left_out (meeting_id, kind, start_ms, end_ms, topic, reason, segment_ids)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                meeting_id,
                normalize_left_out_kind(&rec.kind),
                rec.start_ms,
                rec.end_ms,
                rec.topic,
                rec.reason,
                seg_json,
            ],
        )?;
    }
    Ok(())
}

/// Load persisted `meeting_left_out` rows for `meeting_id` in chronological order (`AC-X26`).
pub fn load_left_out_spans(store: &MeetingsStore, meeting_id: i64) -> Result<Vec<LeftOutRecord>> {
    let conn = store.connection()?;
    let mut stmt = conn.prepare(
        "SELECT kind, start_ms, end_ms, topic, reason, segment_ids
         FROM meeting_left_out
         WHERE meeting_id = ?1
         ORDER BY start_ms ASC, id ASC",
    )?;
    let rows = stmt
        .query_map(params![meeting_id], |r| {
            let raw_kind: String = r.get(0)?;
            let seg_json: String = r.get(5)?;
            let segment_ids: Vec<i64> = serde_json::from_str(&seg_json).unwrap_or_default();
            Ok(LeftOutRecord {
                kind: normalize_left_out_kind(&raw_kind).to_string(),
                start_ms: r.get(1)?,
                end_ms: r.get(2)?,
                topic: r.get(3)?,
                reason: r.get(4)?,
                segment_ids,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Live catch-up over the last `window_ms` (5/10/15 min) with `< 60 s` guard, max 5 cited lines,
/// numbered-excerpt-only prompt (`AC-X31`), and direct quote fallback when no model is available (`AC-X18`).
pub fn catch_up_window_v2<G: NotesGenerator>(
    store: &MeetingsStore,
    meeting_id: i64,
    at_ms: i64,
    window_ms: i64,
    generator: Option<&mut G>,
) -> Result<CatchUpOutcome> {
    let from_ms = at_ms.saturating_sub(window_ms.max(0));
    let in_win: Vec<MeetingSegment> = store
        .segments(meeting_id)?
        .into_iter()
        .filter(|s| !s.text.trim().is_empty() && s.start_ms >= from_ms && s.start_ms <= at_ms)
        .collect();

    let spoken_ms: i64 = in_win.iter().map(|s| (s.end_ms - s.start_ms).max(0)).sum();
    if in_win.is_empty() || spoken_ms < MIN_CATCH_UP_SPOKEN_MS {
        return Ok(CatchUpOutcome::NotEnoughSaid);
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

    if let Some(gen) = generator {
        let prompt = build_notes_prompt(&prompt_segs, &[]);
        if let Ok(reply) = gen.generate(&prompt) {
            let parsed: Vec<ScoredNoteLine> = parse_notes_reply(&reply, &prompt_segs)
                .into_iter()
                .take(MAX_CATCH_UP_LINES)
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
                .collect();
            if !parsed.is_empty() {
                return Ok(CatchUpOutcome::Lines { lines: parsed });
            }
        }
    }

    // Direct grounded quote fallback when the local model is absent or fails.
    let fallback: Vec<ScoredNoteLine> = in_win
        .iter()
        .rev()
        .take(MAX_CATCH_UP_LINES)
        .rev()
        .map(|s| ScoredNoteLine {
            kind: "summary".to_string(),
            text: s.text.trim().to_string(),
            owner: None,
            citations: vec![s.id],
            source: Some(format_clock_mmss(s.start_ms)),
        })
        .collect();
    Ok(CatchUpOutcome::Lines { lines: fallback })
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    struct FakeGen(bool);

    impl NotesGenerator for FakeGen {
        fn generate(&mut self, prompt: &str) -> Result<String> {
            if self.0 {
                return Err(anyhow!("model offline"));
            }
            let mut lines = Vec::new();
            let has_halvorsen = prompt.contains("Halvorsen");
            for (id_s, after) in prompt
                .lines()
                .filter_map(|l| l.strip_prefix("[S")?.split_once("] "))
            {
                if after.contains("Lantern") {
                    let who = if has_halvorsen { " for Halvorsen" } else { "" };
                    lines.push(format!("DECISION: Ship Lantern{who} [S{id_s}]"));
                    lines.push("DECISION: Invented [S99999]".to_string());
                } else if after.contains("April 9") {
                    lines.push(format!("ACTION: Dana | Book call [S{id_s}]"));
                    lines.push(format!("ACTION: GhostOwner | Book call [S{id_s}]"));
                }
            }
            Ok(lines.join("\n"))
        }
    }

    fn add_seg(s: &MeetingsStore, id: i64, ms: i64, ch: &str, txt: &str) -> i64 {
        s.insert_segment(
            id,
            ms,
            ms + 4_000,
            crate::managers::meetings::MeetingChannel::from_label(ch).unwrap(),
            txt,
        )
        .unwrap()
    }

    #[test]
    fn quality_template_words_retry_and_catchup_window_hold_invariants() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let id = store.start_meeting("Review", "parakeet").unwrap();
        let s1 = add_seg(&store, id, 10_000, "me", "Ship Lantern.");
        let _s2 = add_seg(&store, id, 70_000, "others", "Unrelated coffee.");
        let s3 = add_seg(&store, id, 400_000, "others", "Dana: April 9 works.");

        assert!(run_notes_with_quality(&store, id, None, &mut FakeGen(true)).is_err());
        assert!(store.notes(id).unwrap().is_empty());

        let rep = run_notes_with_quality_and_words(
            &store,
            id,
            Some("# Decisions\n# Risks\n"),
            &["Halvorsen".to_string()],
            &mut FakeGen(false),
        )
        .unwrap();
        assert_eq!(rep.approach, "one-shot");
        assert!(rep.sources.iter().any(|s| s.segment_id == s1));
        assert!(rep.lines.iter().any(|l| l.text.contains("Halvorsen")));
        assert!(rep
            .lines
            .iter()
            .all(|l| l.owner.as_deref() != Some("GhostOwner") && !l.text.contains("Invented")));
        assert!(rep.left_out.iter().any(|l| l.reason == "small_talk"));
        assert!(rep
            .templated_sections
            .iter()
            .any(|t| t.heading == "Risks" && t.not_covered));

        let catch = catch_up_window(&store, id, 420_000, 60_000, &mut FakeGen(false)).unwrap();
        assert!(
            !catch.is_empty()
                && catch
                    .iter()
                    .flat_map(|l| &l.citations)
                    .all(|&cid| cid == s3)
        );
    }

    struct PromptCapturingGen {
        captured: Vec<String>,
        reply: String,
    }

    impl NotesGenerator for PromptCapturingGen {
        fn generate(&mut self, prompt: &str) -> Result<String> {
            self.captured.push(prompt.to_string());
            Ok(self.reply.clone())
        }
    }

    #[test]
    fn left_out_adjacent_merge_persistence_and_zero_title_leak() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let id = store
            .start_meeting("Board Budget Private Title", "parakeet")
            .unwrap();
        let _s1 = store
            .insert_segment(
                id,
                10_000,
                15_000,
                crate::managers::meetings::MeetingChannel::Me,
                "Morning coffee chat about weather.",
            )
            .unwrap();
        let _s2 = store
            .insert_segment(
                id,
                20_000,
                26_000,
                crate::managers::meetings::MeetingChannel::Others,
                "Still talking about coffee and weekend plans.",
            )
            .unwrap();

        let mut gen = PromptCapturingGen {
            captured: Vec::new(),
            reply: "Morning coffee chat | Small talk before the agenda started.".to_string(),
        };
        let records = generate_left_out_reasons(&store, id, &[], Some(&mut gen)).unwrap();
        assert_eq!(records.len(), 1, "adjacent same-kind spans should merge");
        assert_eq!(records[0].kind, "small_talk");
        assert_eq!(records[0].segment_ids.len(), 2);
        assert_eq!(records[0].topic, "Morning coffee chat");
        assert!(
            !gen.captured[0].contains("Board Budget Private Title"),
            "left-out prompt must never include meeting titles or calendar fields"
        );

        save_left_out_spans(&store, id, &records).unwrap();
        let loaded = load_left_out_spans(&store, id).unwrap();
        assert_eq!(loaded, records);
    }

    #[test]
    fn catch_up_v2_enforces_min_spoken_duration_quote_fallback_and_zero_title_leak() {
        let dir = tempfile::tempdir().unwrap();
        let store = MeetingsStore::open(dir.path()).unwrap();
        let id = store
            .start_meeting("Confidential Planning Title", "parakeet")
            .unwrap();
        let _short = store
            .insert_segment(
                id,
                5_000,
                20_000,
                crate::managers::meetings::MeetingChannel::Me,
                "Quick hello before we begin.",
            )
            .unwrap();

        // Only 15 s spoken -> NotEnoughSaid.
        let outcome =
            catch_up_window_v2::<PromptCapturingGen>(&store, id, 60_000, 300_000, None).unwrap();
        assert_eq!(outcome, CatchUpOutcome::NotEnoughSaid);

        // Add 65 s of spoken talk in window -> quote fallback works without a model.
        let _long = store
            .insert_segment(
                id,
                25_000,
                90_000,
                crate::managers::meetings::MeetingChannel::Others,
                "We will ship the desktop update on Thursday.",
            )
            .unwrap();
        let fallback =
            catch_up_window_v2::<PromptCapturingGen>(&store, id, 120_000, 300_000, None).unwrap();
        match fallback {
            CatchUpOutcome::Lines { lines } => {
                assert!(!lines.is_empty() && lines.len() <= MAX_CATCH_UP_LINES);
            }
            CatchUpOutcome::NotEnoughSaid => panic!("expected lines once >= 60 s spoken"),
        }

        // With generator, verify meeting title never leaks into catch-up prompt.
        let mut gen = PromptCapturingGen {
            captured: Vec::new(),
            reply: String::new(),
        };
        let _ = catch_up_window_v2(&store, id, 120_000, 300_000, Some(&mut gen)).unwrap();
        assert!(!gen.captured.is_empty());
        assert!(!gen.captured[0].contains("Confidential Planning Title"));
    }
}
