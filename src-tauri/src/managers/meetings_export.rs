//! Markdown for a finished meeting. Pure: no file IO, no database, no Tauri.

// The meetings list and the notes folder call this later.
#![cfg_attr(not(test), allow(dead_code))]

use crate::managers::meetings::{Meeting, StoredNote};

/// One transcript line as it is read from `meeting_segments`.
pub struct ExportSegment {
    pub id: i64,
    pub start_ms: i64,
    pub channel: String,
    pub text: String,
}

/// A whole meeting as Markdown: title, time, the notes by kind, transcript.
pub fn meeting_markdown(
    meeting: &Meeting,
    notes: &[StoredNote],
    segments: &[ExportSegment],
) -> String {
    let mut blocks = vec![title_line(meeting), time_line(meeting)];

    let mut summary = Vec::new();
    let mut decisions = Vec::new();
    let mut actions = Vec::new();
    let mut questions = Vec::new();
    let mut other = Vec::new();
    for note in notes {
        let line = one_line(&note.text);
        match note.kind.as_str() {
            "summary" => summary.push(line),
            "decision" => decisions.push(format!("- {line}")),
            "action" => actions.push(match note.owner.as_deref() {
                Some(owner) if !owner.trim().is_empty() => {
                    format!("- {line} (owner: {})", one_line(owner.trim()))
                }
                _ => format!("- {line}"),
            }),
            "question" => questions.push(format!("- {line}")),
            _ => other.push(format!("- {line}")),
        }
    }
    section(&mut blocks, "## Summary", &summary);
    section(&mut blocks, "## Decisions", &decisions);
    section(&mut blocks, "## Action items", &actions);
    section(&mut blocks, "## Questions", &questions);
    section(&mut blocks, "## Other notes", &other);

    let mut ordered: Vec<&ExportSegment> = segments.iter().collect();
    // Ties (Me and Others starting together) keep segment-id order.
    ordered.sort_by_key(|segment| (segment.start_ms, segment.id));
    let transcript: Vec<String> = ordered
        .into_iter()
        .map(|segment| {
            let who = match segment.channel.as_str() {
                "mic" => "Me".to_string(),
                "system" => "Others".to_string(),
                other => other.to_string(),
            };
            format!(
                "[{}] {who}: {}",
                clock(segment.start_ms),
                one_line(&segment.text)
            )
        })
        .collect();
    section(&mut blocks, "## Transcript", &transcript);

    let mut out = blocks.join("\n\n");
    out.push('\n');
    out
}

fn title_line(meeting: &Meeting) -> String {
    let title = one_line(&meeting.title);
    if title.trim().is_empty() {
        "# Meeting".to_string()
    } else {
        format!("# {}", title.trim())
    }
}

fn time_line(meeting: &Meeting) -> String {
    let Some(start) = utc(meeting.started_at) else {
        return "Unknown time".to_string();
    };
    let mut line = format!("{} UTC", start.format("%Y-%m-%d %H:%M"));
    if let Some(end) = meeting.ended_at.and_then(utc) {
        line.push_str(&format!(" to {} UTC", end.format("%H:%M")));
    }
    line
}

/// Unix seconds (what the meetings table stores) to a UTC instant.
/// `None` for a value chrono can't represent, so a bad row isn't shown as 1970.
fn utc(secs: i64) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)
}

/// `mm:ss`, or `h:mm:ss` from one hour up.
fn clock(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// A newline inside a line becomes a space so one note stays one line.
fn one_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

fn section(blocks: &mut Vec<String>, heading: &str, lines: &[String]) {
    if !lines.is_empty() {
        blocks.push(format!("{heading}\n{}", lines.join("\n")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: i64 = 1_700_000_000;
    const HALF_HOUR: i64 = 30 * 60;

    fn meeting(title: &str) -> Meeting {
        Meeting {
            id: 1,
            started_at: START,
            ended_at: Some(START + HALF_HOUR),
            title: title.to_string(),
            status: "transcript_ready".to_string(),
            model_id: "model".to_string(),
            saved: 0,
        }
    }

    fn note(kind: &str, text: &str, owner: Option<&str>) -> StoredNote {
        StoredNote {
            kind: kind.to_string(),
            text: text.to_string(),
            owner: owner.map(str::to_string),
            citations: Vec::new(),
        }
    }

    fn segment(id: i64, start_ms: i64, channel: &str, text: &str) -> ExportSegment {
        ExportSegment {
            id,
            start_ms,
            channel: channel.to_string(),
            text: text.to_string(),
        }
    }

    fn weekly_sync() -> (Meeting, Vec<StoredNote>, Vec<ExportSegment>) {
        let notes = vec![
            note("summary", "Look at the draft before Friday.", None),
            note("decision", "Ship on Friday", None),
            note("action", "Send the deck", Some("Priya")),
        ];
        // Out of order on purpose: the writer has to sort by start_ms.
        let segments = vec![
            segment(2, 65_000, "system", "Thanks"),
            segment(1, 5_000, "mic", "I will send the deck"),
        ];
        (meeting("Weekly sync"), notes, segments)
    }

    #[test]
    fn weekly_sync_lines() {
        let (meeting, notes, segments) = weekly_sync();
        let out = meeting_markdown(&meeting, &notes, &segments);
        assert!(out.contains("Look at the draft before Friday."), "{out}");
        assert!(out.contains("- Send the deck (owner: Priya)"), "{out}");
        assert!(out.contains("[00:05] Me: I will send the deck"), "{out}");
        assert!(out.contains("[01:05] Others: Thanks"), "{out}");
    }

    #[test]
    fn weekly_sync_snapshot() {
        let (meeting, notes, segments) = weekly_sync();
        let expected = [
            "# Weekly sync",
            "",
            "2023-11-14 22:13 UTC to 22:43 UTC",
            "",
            "## Summary",
            "Look at the draft before Friday.",
            "",
            "## Decisions",
            "- Ship on Friday",
            "",
            "## Action items",
            "- Send the deck (owner: Priya)",
            "",
            "## Transcript",
            "[00:05] Me: I will send the deck",
            "[01:05] Others: Thanks",
            "",
        ]
        .join("\n");
        assert_eq!(meeting_markdown(&meeting, &notes, &segments), expected);
    }

    #[test]
    fn other_notes_catch_unknown_kinds_and_empty_sections_are_absent() {
        let out = meeting_markdown(&meeting(""), &[note("risk", "Vendor delay", None)], &[]);
        assert!(out.starts_with("# Meeting\n"), "{out}");
        assert!(out.contains("## Other notes\n- Vendor delay"), "{out}");
        assert!(!out.contains("## Summary"), "{out}");
        assert!(!out.contains("## Decisions"), "{out}");
        assert!(!out.contains("## Action items"), "{out}");
        assert!(!out.contains("## Questions"), "{out}");
        assert!(!out.contains("## Transcript"), "{out}");
    }

    #[test]
    fn transcript_uses_hours_past_an_hour() {
        let segments = [segment(1, 3_725_000, "mic", "late line")];
        let out = meeting_markdown(&meeting("Long call"), &[], &segments);
        assert!(out.contains("[1:02:05] Me: late line"), "{out}");
    }

    #[test]
    fn newlines_inside_a_note_become_spaces() {
        let out = meeting_markdown(
            &meeting("Weekly sync"),
            &[note("decision", "Ship on Friday\nnot Monday", None)],
            &[],
        );
        assert!(out.contains("- Ship on Friday not Monday"), "{out}");
    }

    #[test]
    fn owner_stays_on_one_line_and_a_bad_time_is_not_1970() {
        let mut m = meeting("Sync");
        m.started_at = i64::MAX;
        let notes = [note("action", "Send it", Some(" Pri\nya "))];
        let out = meeting_markdown(&m, &notes, &[]);
        assert!(out.contains("- Send it (owner: Pri ya)"), "{out}");
        assert!(out.contains("Unknown time"), "{out}");
        assert!(!out.contains("1970"), "{out}");
    }
}
