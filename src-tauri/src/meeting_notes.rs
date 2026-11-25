//! Pure prompt building and reply parsing for meeting notes.
//! Every kept line cites a transcript segment; nothing here calls a model.

#![cfg_attr(not(test), allow(dead_code))] // The notes runner calls these later.

/// One transcript segment, offered to the model under the tag `[S<id>]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptSegment {
    pub id: i64,
    pub channel: String,
    pub text: String,
}

/// The four kinds of line a notes reply may carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    Summary,
    Decision,
    Action,
    Question,
}

/// One parsed note line. `citations` is never empty and is sorted ascending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteLine {
    pub kind: NoteKind,
    pub text: String,
    pub owner: Option<String>,
    pub citations: Vec<i64>,
}

/// One parsed note line for a custom user template heading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomNoteLine {
    pub kind: String,
    pub text: String,
    pub owner: Option<String>,
    pub citations: Vec<i64>,
}

/// Build the notes prompt for one meeting. No model call happens here.
///
/// The prompt is, in order:
/// 1. Short instructions for the model.
/// 2. One line per segment: `[S<id>] <channel>: <text>`.
/// 3. One line per notepad point: `NOTE: <text>`.
/// 4. The reply format, one item per line:
///    `SUMMARY: <text> [S<id>]`, `DECISION: <text> [S<id>]`,
///    `ACTION: <owner or -> | <text> [S<id>]`, `QUESTION: <text> [S<id>]`.
///
/// The instructions require that every line cites at least one segment,
/// that decisions and owners are never invented, and that every NOTE point
/// is kept.
pub fn build_notes_prompt(segments: &[PromptSegment], notepad: &[String]) -> String {
    let mut prompt = String::new();
    prompt.push_str("Write the notes for this meeting from the transcript below.\n");
    prompt.push_str("Use only the transcript and the notepad; never invent a fact.\n");
    prompt.push_str("Never invent a decision or an owner.\n");
    prompt.push_str("Every line must cite at least one segment as [S<id>].\n");
    prompt.push_str("Keep every NOTE point in the notes.\n");

    for segment in segments {
        prompt.push_str(&format!(
            "[S{}] {}: {}\n",
            segment.id, segment.channel, segment.text
        ));
    }

    if !notepad.is_empty() {
        prompt.push('\n');
        for point in notepad {
            prompt.push_str(&format!("NOTE: {}\n", point));
        }
    }

    prompt.push_str("\nReply with one item per line and nothing else:\n");
    prompt.push_str("SUMMARY: <text> [S<id>]\n");
    prompt.push_str("DECISION: <text> [S<id>]\n");
    prompt.push_str("ACTION: <owner or -> | <text> [S<id>]\n");
    prompt.push_str("QUESTION: <text> [S<id>]\n");
    prompt
}

/// Build the notes prompt when the user picked a custom template with `# Headings`.
///
/// The template's own headings become the required line prefixes so the model
/// writes directly under the user's sections with `[S<id>]` citations.
pub fn build_custom_template_notes_prompt(
    segments: &[PromptSegment],
    notepad: &[String],
    headings: &[String],
) -> String {
    let mut prompt = String::new();
    prompt.push_str("Write the notes for this meeting from the transcript below.\n");
    prompt.push_str("Use only the transcript and the notepad; never invent a fact.\n");
    prompt.push_str("Never invent a decision or an owner.\n");
    prompt.push_str("Every line must cite at least one segment as [S<id>].\n");
    prompt.push_str("Keep every NOTE point in the notes.\n");

    for segment in segments {
        prompt.push_str(&format!(
            "[S{}] {}: {}\n",
            segment.id, segment.channel, segment.text
        ));
    }

    if !notepad.is_empty() {
        prompt.push('\n');
        for point in notepad {
            prompt.push_str(&format!("NOTE: {}\n", point));
        }
    }

    prompt.push_str(
        "\nReply with one item per line using ONLY these section prefixes and nothing else:\n",
    );
    for heading in headings {
        prompt.push_str(&format!("{}: <text> [S<id>]\n", heading.trim()));
    }
    prompt.push_str("Leave out a section prefix if the meeting did not cover it.\n");
    prompt
}

/// Build the merge prompt used when a long meeting was split across `> 1` pieces.
///
/// Reconciles piece notes chronologically so any decision reversed or superseded
/// by a later segment keeps only the final state while preserving valid `[S<id>]`
/// citations.
pub fn build_merge_notes_prompt(
    draft_lines: &[(String, Option<String>, String, Vec<i64>)],
    notepad: &[String],
    custom_headings: &[String],
) -> String {
    let mut prompt = String::new();
    prompt.push_str(
        "Merge these partial meeting notes from earlier and later parts of the same meeting into one clean final set of notes.\n",
    );
    prompt.push_str(
        "Use ONLY facts and [S<id>] citations shown below; never invent a fact, owner, or citation.\n",
    );
    prompt.push_str(
        "If an earlier decision, date, price, or plan was reversed, changed, or superseded in a later higher-numbered [S<id>] segment, KEEP ONLY the final decision and DROP the earlier superseded one.\n",
    );
    prompt.push_str(
        "Remove duplicate items, never split one item across multiple owners, and drop any action that does not name a concrete follow-up commitment.\n",
    );
    prompt.push_str("Keep every NOTE point.\n\n");

    for (kind, owner, text, citations) in draft_lines {
        let cites: String = citations.iter().map(|id| format!("[S{id}]")).collect();
        if custom_headings.is_empty() && kind.eq_ignore_ascii_case("action") {
            let who = owner.as_deref().unwrap_or("-");
            prompt.push_str(&format!("ACTION: {who} | {text} {cites}\n"));
        } else if custom_headings.is_empty() {
            prompt.push_str(&format!("{}: {text} {cites}\n", kind.to_uppercase()));
        } else {
            prompt.push_str(&format!("{kind}: {text} {cites}\n"));
        }
    }

    if !notepad.is_empty() {
        prompt.push('\n');
        for point in notepad {
            prompt.push_str(&format!("NOTE: {point}\n"));
        }
    }

    if custom_headings.is_empty() {
        prompt.push_str("\nReply with one item per line and nothing else:\n");
        prompt.push_str("SUMMARY: <text> [S<id>]\n");
        prompt.push_str("DECISION: <text> [S<id>]\n");
        prompt.push_str("ACTION: <owner or -> | <text> [S<id>]\n");
        prompt.push_str("QUESTION: <text> [S<id>]\n");
    } else {
        prompt.push_str(
            "\nReply with one item per line using ONLY these section prefixes and nothing else:\n",
        );
        for heading in custom_headings {
            prompt.push_str(&format!("{}: <text> [S<id>]\n", heading.trim()));
        }
        prompt.push_str("Leave out a section prefix if the meeting did not cover it.\n");
    }
    prompt
}

/// Parse a reply generated for custom template headings.
///
/// Matches each line against `headings` (case-insensitive, with tolerance for
/// optional markdown list/bold framing and simple singular/plural prefixes) and
/// applies the same strict `[S<id>]` citation validation as [`parse_notes_reply`].
/// Default prefixes (`SUMMARY:`, `DECISION:`, `ACTION:`, `QUESTION:`) are also
/// accepted as a fallback when a caller/mock emits default format.
pub fn parse_custom_template_reply(
    reply: &str,
    segments: &[PromptSegment],
    headings: &[String],
) -> Vec<CustomNoteLine> {
    let mut lines = Vec::new();
    for raw in reply.lines() {
        let cleaned = strip_line_bullet(raw.trim());
        if let Some((heading, rest)) = split_custom_heading_prefix(cleaned, headings) {
            let (text, citations) = split_citations(rest);
            let text = text.trim();
            if text.is_empty() || citations.is_empty() {
                continue;
            }
            let known = citations
                .iter()
                .all(|id| segments.iter().any(|segment| segment.id == *id));
            if !known {
                continue;
            }
            lines.push(CustomNoteLine {
                kind: heading.to_string(),
                text: text.to_string(),
                owner: None,
                citations,
            });
            continue;
        }
        // Fallback for default-format lines (keeps default mocks & mixed replies valid).
        for fallback in parse_notes_reply(cleaned, segments) {
            let kind = match fallback.kind {
                NoteKind::Summary => "summary",
                NoteKind::Decision => "decision",
                NoteKind::Action => "action",
                NoteKind::Question => "question",
            };
            lines.push(CustomNoteLine {
                kind: kind.to_string(),
                text: fallback.text,
                owner: fallback.owner,
                citations: fallback.citations,
            });
        }
    }
    lines
}

fn strip_line_bullet(line: &str) -> &str {
    for prefix in ["- ", "* ", "• "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return rest.trim_start();
        }
    }
    line
}

fn split_custom_heading_prefix<'a>(
    line: &'a str,
    headings: &'a [String],
) -> Option<(&'a str, &'a str)> {
    let (raw_head, rest) = line.split_once(':')?;
    let head = raw_head.trim().trim_matches('*').trim();
    let head_low = head.to_lowercase();
    if head_low.is_empty() {
        return None;
    }
    // Prefer exact case-insensitive match first, then singular/plural match.
    for h in headings {
        let h_trim = h.trim();
        if h_trim.eq_ignore_ascii_case(head) {
            return Some((h_trim, rest.trim()));
        }
    }
    let head_sing = head_low.strip_suffix('s').unwrap_or(&head_low);
    for h in headings {
        let h_trim = h.trim();
        let h_low = h_trim.to_lowercase();
        let h_sing = h_low.strip_suffix('s').unwrap_or(&h_low);
        if !head_sing.is_empty() && head_sing == h_sing {
            return Some((h_trim, rest.trim()));
        }
    }
    None
}

/// Parse a notes reply into the lines that survive the citation checks.
///
/// Prefixes are case-insensitive. Blank lines and lines without a known
/// prefix are skipped. Citations are every `[S<digits>]` token; they are
/// removed from the text, and the text is trimmed. Citations are deduped
/// and sorted ascending.
///
/// A line is dropped when it has no citation, when any citation is not a
/// segment id in `segments`, or when its text is empty.
///
/// An ACTION owner is the part before the first `|`, trimmed; `-` or empty
/// means no owner. A named owner is kept only when it appears
/// (case-insensitive substring) in the text of at least one cited segment;
/// otherwise the owner becomes `None` and the line is kept. Non-ACTION
/// lines always have no owner.
pub fn parse_notes_reply(reply: &str, segments: &[PromptSegment]) -> Vec<NoteLine> {
    let mut lines: Vec<NoteLine> = Vec::new();
    for raw in reply.lines() {
        let Some((kind, rest)) = split_prefix(raw.trim()) else {
            continue;
        };
        let (text, citations) = split_citations(rest);
        let text = text.trim();
        if text.is_empty() || citations.is_empty() {
            continue;
        }
        let known = citations
            .iter()
            .all(|id| segments.iter().any(|segment| segment.id == *id));
        if !known {
            continue;
        }
        let (owner, text) = match kind {
            NoteKind::Action => match text.split_once('|') {
                Some((owner, body)) => (owner.trim(), body.trim()),
                None => ("", text),
            },
            _ => ("", text),
        };
        if text.is_empty() {
            continue;
        }
        lines.push(NoteLine {
            kind,
            text: text.to_string(),
            owner: cited_owner(owner, &citations, segments),
            citations,
        });
    }
    lines
}

/// The reply prefixes, matched case-insensitively at the start of a line.
const PREFIXES: [(&str, NoteKind); 4] = [
    ("SUMMARY:", NoteKind::Summary),
    ("DECISION:", NoteKind::Decision),
    ("ACTION:", NoteKind::Action),
    ("QUESTION:", NoteKind::Question),
];

/// Split a line into its note kind and the text after the prefix.
fn split_prefix(line: &str) -> Option<(NoteKind, &str)> {
    PREFIXES.iter().find_map(|&(prefix, kind)| {
        let head = line.get(..prefix.len())?;
        if !head.eq_ignore_ascii_case(prefix) {
            return None;
        }
        Some((kind, line[prefix.len()..].trim()))
    })
}

/// Strip every `[S<digits>]` token from `text`; the ids come back sorted, deduped.
fn split_citations(text: &str) -> (String, Vec<i64>) {
    let bytes = text.as_bytes();
    let mut kept = String::with_capacity(text.len());
    let mut citations = Vec::new();
    let mut last = 0;
    let mut i = 0;
    // `[` is ASCII, so every byte index found here is a UTF-8 boundary.
    while i < bytes.len() {
        if bytes[i] != b'[' || bytes.get(i + 1) != Some(&b'S') {
            i += 1;
            continue;
        }
        let start = i + 2;
        let mut end = start;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == start || bytes.get(end) != Some(&b']') {
            i += 1;
            continue;
        }
        let Ok(id) = text[start..end].parse::<i64>() else {
            i += 1;
            continue;
        };
        kept.push_str(&text[last..i]);
        citations.push(id);
        last = end + 1;
        i = last;
    }
    kept.push_str(&text[last..]);
    citations.sort_unstable();
    citations.dedup();
    (kept, citations)
}

/// Keep a named owner only when a cited segment mentions it; `-` or empty is none.
fn cited_owner(owner: &str, citations: &[i64], segments: &[PromptSegment]) -> Option<String> {
    if owner.is_empty() || owner == "-" {
        return None;
    }
    let needle = owner.to_lowercase();
    let mentioned = citations.iter().any(|id| {
        segments
            .iter()
            .filter(|segment| segment.id == *id)
            .any(|segment| segment.text.to_lowercase().contains(&needle))
    });
    if !mentioned {
        return None;
    }
    Some(owner.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs() -> Vec<PromptSegment> {
        vec![
            PromptSegment {
                id: 1,
                channel: "mic".to_string(),
                text: "Priya will send the budget by Friday".to_string(),
            },
            PromptSegment {
                id: 2,
                channel: "system".to_string(),
                text: "We decided to ship the beta on Monday".to_string(),
            },
            PromptSegment {
                id: 3,
                channel: "mic".to_string(),
                text: "Who owns the launch email?".to_string(),
            },
        ]
    }

    fn line(kind: NoteKind, text: &str, owner: Option<&str>, citations: &[i64]) -> NoteLine {
        NoteLine {
            kind,
            text: text.to_string(),
            owner: owner.map(str::to_string),
            citations: citations.to_vec(),
        }
    }

    #[test]
    fn prompt_lists_segments_and_notepad() {
        let notepad = vec!["check pricing".to_string()];
        let prompt = build_notes_prompt(&segs(), &notepad);
        assert!(
            prompt.contains("[S1] mic: Priya will send the budget by Friday"),
            "prompt should render segment 1: {prompt}"
        );
        assert!(
            prompt.contains("[S2] system: We decided to ship the beta on Monday"),
            "prompt should render segment 2: {prompt}"
        );
        assert!(
            prompt.contains("NOTE: check pricing"),
            "prompt should carry the notepad: {prompt}"
        );
        assert!(
            prompt.contains("DECISION:"),
            "prompt should state the reply format: {prompt}"
        );
    }

    #[test]
    fn clean_reply_parses() {
        let reply = "SUMMARY: Beta ships Monday [S2]\n\
                     DECISION: Ship the beta on Monday [S2]\n\
                     ACTION: Priya | Send the budget by Friday [S1]\n\
                     question: Who owns the launch email? [S3][S3]";
        let lines = parse_notes_reply(reply, &segs());
        let expected = vec![
            line(NoteKind::Summary, "Beta ships Monday", None, &[2]),
            line(NoteKind::Decision, "Ship the beta on Monday", None, &[2]),
            line(
                NoteKind::Action,
                "Send the budget by Friday",
                Some("Priya"),
                &[1],
            ),
            line(NoteKind::Question, "Who owns the launch email?", None, &[3]),
        ];
        assert_eq!(lines, expected);
    }

    #[test]
    fn invented_citation_drops_the_line() {
        let reply = "DECISION: Raise prices [S99]\nDECISION: Ship the beta on Monday [S2]";
        let lines = parse_notes_reply(reply, &segs());
        assert_eq!(
            lines,
            vec![line(
                NoteKind::Decision,
                "Ship the beta on Monday",
                None,
                &[2]
            )]
        );
    }

    #[test]
    fn uncited_line_is_dropped() {
        let reply = "DECISION: Ship it\nSUMMARY: Beta ships [S2]";
        let lines = parse_notes_reply(reply, &segs());
        assert_eq!(
            lines,
            vec![line(NoteKind::Summary, "Beta ships", None, &[2])]
        );
    }

    #[test]
    fn owner_not_in_cited_text_is_dropped() {
        let unknown = parse_notes_reply("ACTION: Marco | Send the budget [S1]", &segs());
        assert_eq!(
            unknown,
            vec![line(NoteKind::Action, "Send the budget", None, &[1])]
        );

        let known = parse_notes_reply("ACTION: priya | Send the budget [S1]", &segs());
        assert_eq!(
            known,
            vec![line(
                NoteKind::Action,
                "Send the budget",
                Some("priya"),
                &[1]
            )]
        );

        let none = parse_notes_reply("ACTION: - | Draft the email [S3]", &segs());
        assert_eq!(
            none,
            vec![line(NoteKind::Action, "Draft the email", None, &[3])]
        );
    }

    #[test]
    fn noise_lines_are_skipped() {
        let reply = "Here are your notes:\n\n- random\nSUMMARY: Beta ships [S2]";
        let lines = parse_notes_reply(reply, &segs());
        assert_eq!(
            lines,
            vec![line(NoteKind::Summary, "Beta ships", None, &[2])]
        );
    }

    #[test]
    fn custom_template_prompt_and_parser_round_trip() {
        let headings = vec![
            "Blockers".to_string(),
            "Owners and dates".to_string(),
            "Open risks".to_string(),
        ];
        let prompt = build_custom_template_notes_prompt(&segs(), &[], &headings);
        assert!(prompt.contains("Blockers: <text> [S<id>]"));
        assert!(prompt.contains("Owners and dates: <text> [S<id>]"));
        assert!(prompt.contains("Open risks: <text> [S<id>]"));
        assert!(!prompt.contains("SUMMARY: <text>"));

        let reply = "\
            - **Blockers**: Waiting on CI lock [S2]\n\
            Owners and dates: Priya will send budget Friday [S1]\n\
            Open risk: Unowned launch email [S3]\n\
            Blockers: Uncited line dropped\n\
            Open risks: Invented cite [S99]\n";
        let parsed = parse_custom_template_reply(reply, &segs(), &headings);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].kind, "Blockers");
        assert_eq!(parsed[0].citations, vec![2]);
        assert_eq!(parsed[1].kind, "Owners and dates");
        assert_eq!(parsed[1].citations, vec![1]);
        assert_eq!(parsed[2].kind, "Open risks");
        assert_eq!(parsed[2].citations, vec![3]);
    }

    #[test]
    fn merge_prompt_carries_citations_and_reversal_rule() {
        let draft = vec![
            (
                "decision".to_string(),
                None,
                "Launch June 1".to_string(),
                vec![2],
            ),
            (
                "action".to_string(),
                Some("Priya".to_string()),
                "Send budget".to_string(),
                vec![1],
            ),
        ];
        let prompt = build_merge_notes_prompt(&draft, &["check pricing".to_string()], &[]);
        assert!(prompt.contains("DECISION: Launch June 1 [S2]"));
        assert!(prompt.contains("ACTION: Priya | Send budget [S1]"));
        assert!(prompt.contains("NOTE: check pricing"));
        assert!(prompt.contains("KEEP ONLY the final decision"));
    }
}
