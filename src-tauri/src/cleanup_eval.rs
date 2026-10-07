//! Headless on-device cleanup eval (`--cleanup-eval`).
//!
//! Scoring is pure and tested without a model. On macOS each take is cleaned
//! once through the same local-provider path the app uses
//! ([`crate::actions::build_post_process_messages`] with no prior output, then
//! [`crate::local_llm::generate_text`] at [`crate::actions::LOCAL_LLM_MAX_TOKENS`]).
//! Other platforms print a message and exit 1. The process exits before Tauri
//! builds a window or a tray.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(target_os = "macos")]
use std::path::Path;

use serde::Deserialize;

/// Built-in prompt used when `--eval-prompt` is omitted.
const DEFAULT_EVAL_PROMPT_ID: &str = "default_improve_transcriptions";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvalKind {
    Retract,
    Keep,
}

impl EvalKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Retract => "retract",
            Self::Keep => "keep",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvalItem {
    id: String,
    kind: EvalKind,
    input: String,
    keep: Vec<String>,
    drop_terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TakeScore {
    meaning_ok: bool,
    retraction_ok: bool,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Summary {
    meaning_passed: usize,
    takes: usize,
    retraction_passed: usize,
    retracts: usize,
}

impl Summary {
    fn meaning_ok(&self) -> bool {
        self.meaning_passed == self.takes
    }

    fn retraction_ok(&self) -> bool {
        self.retraction_passed >= retraction_bar(self.retracts)
    }

    fn passed(&self) -> bool {
        self.meaning_ok() && self.retraction_ok()
    }
}

struct ReportRow {
    id: String,
    kind: EvalKind,
    output: String,
    score: TakeScore,
}

#[derive(Deserialize)]
struct RawItem {
    id: String,
    kind: String,
    input: String,
    #[serde(default)]
    keep: Vec<String>,
    #[serde(default, rename = "drop")]
    drop_terms: Vec<String>,
}

/// `true` when any `|`-separated alternative is a case-insensitive substring.
///
/// A blank alternative does not match. An entry with no `|` must appear whole.
fn alternative_present(entry: &str, output: &str) -> bool {
    let hay = output.to_lowercase();
    entry.split('|').any(|alt| {
        let alt = alt.trim().to_lowercase();
        !alt.is_empty() && hay.contains(&alt)
    })
}

/// `true` when no drop phrase is a case-insensitive substring.
///
/// Drop phrases are literals. A `|` inside one is not an alternative split.
fn retraction_removed(drop_terms: &[String], output: &str) -> bool {
    let hay = output.to_lowercase();
    drop_terms.iter().all(|term| {
        let needle = term.trim().to_lowercase();
        needle.is_empty() || !hay.contains(&needle)
    })
}

/// Empty (after trim) or longer than twice the input, in characters.
fn hard_fail_reason(input: &str, output: &str) -> Option<&'static str> {
    if output.trim().is_empty() {
        Some("empty output")
    } else if output.chars().count() > input.chars().count().saturating_mul(2) {
        Some("output longer than 2x input")
    } else {
        None
    }
}

/// Score one cleaned take.
///
/// `Err` is a model refusal. Empty, refusal, and over-long output fail both
/// checks for this take even when the substrings would have passed.
fn score_take(item: &EvalItem, generated: Result<&str, &str>) -> TakeScore {
    let output = match generated {
        Err(err) => {
            return TakeScore {
                meaning_ok: false,
                retraction_ok: false,
                reason: format!("refusal: {err}"),
            };
        }
        Ok(output) => output,
    };
    if let Some(reason) = hard_fail_reason(&item.input, output) {
        return TakeScore {
            meaning_ok: false,
            retraction_ok: false,
            reason: reason.to_string(),
        };
    }

    let meaning_ok = item
        .keep
        .iter()
        .all(|entry| alternative_present(entry, output));
    let retraction_ok = retraction_removed(&item.drop_terms, output);
    let mut reasons = Vec::new();
    if !meaning_ok {
        let missing = item
            .keep
            .iter()
            .filter(|entry| !alternative_present(entry, output))
            .map(|entry| entry.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        reasons.push(format!("missing keep: {missing}"));
    }
    if !retraction_ok {
        let hay = output.to_lowercase();
        let present = item
            .drop_terms
            .iter()
            .filter(|term| {
                let needle = term.trim().to_lowercase();
                !needle.is_empty() && hay.contains(&needle)
            })
            .map(|term| term.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        reasons.push(format!("drop still present: {present}"));
    }
    TakeScore {
        meaning_ok,
        retraction_ok,
        reason: reasons.join("; "),
    }
}

/// Minimum retract takes that must clear the drop list: `ceil(0.8 * n)`.
///
/// 22 retracts require 18. 0 retracts require 0.
fn retraction_bar(retract_count: usize) -> usize {
    (retract_count * 4 + 4) / 5
}

fn summarize(rows: &[(EvalKind, TakeScore)]) -> Summary {
    Summary {
        takes: rows.len(),
        meaning_passed: rows.iter().filter(|(_, score)| score.meaning_ok).count(),
        retracts: rows
            .iter()
            .filter(|(kind, _)| *kind == EvalKind::Retract)
            .count(),
        retraction_passed: rows
            .iter()
            .filter(|(kind, score)| *kind == EvalKind::Retract && score.retraction_ok)
            .count(),
    }
}

/// Score the text the app would paste. Invisible characters are stripped first,
/// matching [`crate::actions::strip_invisible_chars`].
fn judge(item: &EvalItem, generated: Result<&str, &str>) -> (String, TakeScore) {
    match generated {
        Err(err) => (String::new(), score_take(item, Err(err))),
        Ok(raw) => {
            let text = crate::actions::strip_invisible_chars(raw);
            let score = score_take(item, Ok(&text));
            (text, score)
        }
    }
}

fn parse_eval_set(text: &str) -> Result<Vec<EvalItem>, String> {
    let mut items = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for (idx, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let line_no = idx + 1;
        let raw: RawItem =
            serde_json::from_str(line).map_err(|err| format!("line {line_no}: {err}"))?;
        if !seen_ids.insert(raw.id.clone()) {
            return Err(format!("line {line_no}: duplicate id {:?}", raw.id));
        }
        if raw.keep.is_empty() {
            return Err(format!(
                "line {line_no}: keep list is empty for id {:?}",
                raw.id
            ));
        }
        let kind = match raw.kind.as_str() {
            "retract" => EvalKind::Retract,
            "keep" => EvalKind::Keep,
            other => {
                return Err(format!(
                    "line {line_no}: kind must be \"retract\" or \"keep\", got {other:?}"
                ));
            }
        };
        items.push(EvalItem {
            id: raw.id,
            kind,
            input: raw.input,
            keep: raw.keep,
            drop_terms: raw.drop_terms,
        });
    }
    Ok(items)
}

fn md_cell(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\n' | '\r' | '\t' => out.push(' '),
            '|' => out.push_str("\\|"),
            _ => out.push(ch),
        }
    }
    out
}

fn yes_no(ok: bool) -> &'static str {
    if ok {
        "yes"
    } else {
        "no"
    }
}

fn verdict(ok: bool) -> &'static str {
    if ok {
        "PASS"
    } else {
        "FAIL"
    }
}

fn render_report(prompt_id: &str, model: &str, rows: &[ReportRow], summary: &Summary) -> String {
    let mut out = String::new();
    out.push_str("# Cleanup eval\n\n");
    out.push_str(&format!("Prompt `{prompt_id}`. Model `{model}`.\n\n"));
    out.push_str("| id | kind | meaning ok | retraction ok | output | reason |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            md_cell(&row.id),
            row.kind.as_str(),
            yes_no(row.score.meaning_ok),
            yes_no(row.score.retraction_ok),
            md_cell(&row.output),
            md_cell(&row.score.reason),
        ));
    }
    out.push('\n');
    out.push_str("## Totals\n\n");
    out.push_str(&format!(
        "- Meaning kept: {}/{} {} (bar: all takes)\n",
        summary.meaning_passed,
        summary.takes,
        verdict(summary.meaning_ok()),
    ));
    out.push_str(&format!(
        "- Retraction removed: {}/{} {} (bar: at least {} of {})\n\n",
        summary.retraction_passed,
        summary.retracts,
        verdict(summary.retraction_ok()),
        retraction_bar(summary.retracts),
        summary.retracts,
    ));
    out.push_str(&format!("**{}**\n", verdict(summary.passed())));
    out
}

fn prompt_template(id: &str) -> Result<String, String> {
    let prompts = crate::settings::default_post_process_prompts();
    prompts
        .iter()
        .find(|prompt| prompt.id == id)
        .map(|prompt| prompt.prompt.clone())
        .ok_or_else(|| {
            let ids = prompts
                .iter()
                .map(|prompt| prompt.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("unknown eval prompt '{id}'. Built-in ids: {ids}")
        })
}

/// Run the eval. Exit code: 0 when both bars pass, 1 otherwise.
pub(crate) fn run_eval(args: &crate::cli::CliArgs) -> i32 {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = args;
        eprintln!("error: --cleanup-eval is only available on macOS");
        1
    }
    #[cfg(target_os = "macos")]
    match run_eval_macos(args) {
        Ok(passed) => {
            if passed {
                0
            } else {
                1
            }
        }
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    }
}

#[cfg(target_os = "macos")]
struct ModelRelease;

#[cfg(target_os = "macos")]
impl Drop for ModelRelease {
    fn drop(&mut self) {
        crate::local_llm::release_model();
    }
}

#[cfg(target_os = "macos")]
fn run_eval_macos(args: &crate::cli::CliArgs) -> Result<bool, String> {
    let Some(set_path) = args.cleanup_eval.as_ref() else {
        return Err("--cleanup-eval path is missing".to_string());
    };
    let prompt_id = args
        .eval_prompt
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(DEFAULT_EVAL_PROMPT_ID);
    let template = prompt_template(prompt_id)?;
    let model_path = args
        .eval_model
        .as_ref()
        .ok_or_else(|| "--eval-model is required with --cleanup-eval".to_string())?;
    if !model_path.is_file() {
        return Err(format!("eval model not found: {}", model_path.display()));
    }

    if let Some(out_path) = args.eval_out.as_ref() {
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    format!("failed to create directory {}: {err}", parent.display())
                })?;
            }
        }
    }

    let text = std::fs::read_to_string(set_path)
        .map_err(|err| format!("cannot read {}: {err}", set_path.display()))?;
    let items = parse_eval_set(&text).map_err(|err| format!("{}: {err}", set_path.display()))?;
    if items.is_empty() {
        return Err(format!("eval set is empty: {}", set_path.display()));
    }

    // Drop the GGUF before we return to `process::exit`.
    let _release = ModelRelease;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("failed to start eval runtime: {err}"))?;

    let mut rows = Vec::with_capacity(items.len());
    let mut scored = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        eprintln!("cleanup-eval: {}/{} {}", index + 1, items.len(), item.id);
        // Fresh cleanup, same pair the local provider builds: system prompt with
        // `${output}` removed, transcript as the user message. No cancel.
        let (system_prompt, user_content) =
            crate::actions::build_post_process_messages(&template, &item.input, None);
        let generated = rt.block_on(crate::local_llm::generate_text(
            model_path,
            &system_prompt,
            &user_content,
            crate::actions::LOCAL_LLM_MAX_TOKENS,
            None,
            None,
        ));
        let judged = judge(item, generated.as_deref().map_err(|e| e.as_str()));
        scored.push((item.kind, judged.1.clone()));
        rows.push(ReportRow {
            id: item.id.clone(),
            kind: item.kind,
            output: judged.0,
            score: judged.1,
        });
    }

    let summary = summarize(&scored);
    eprintln!(
        "cleanup-eval: {} (meaning {}/{}, retraction {}/{})",
        verdict(summary.passed()),
        summary.meaning_passed,
        summary.takes,
        summary.retraction_passed,
        summary.retracts,
    );
    let report = render_report(
        prompt_id,
        &model_path.display().to_string(),
        &rows,
        &summary,
    );
    write_report(args.eval_out.as_deref(), &report)?;
    Ok(summary.passed())
}

#[cfg(target_os = "macos")]
fn write_report(path: Option<&Path>, report: &str) -> Result<(), String> {
    match path {
        None => {
            print!("{report}");
            Ok(())
        }
        Some(path) => match std::fs::write(path, report) {
            Ok(()) => {
                eprintln!("cleanup-eval: wrote {}", path.display());
                Ok(())
            }
            Err(err) => {
                // The model run already happened. Put the report on stdout so
                // it is not lost, and still fail because the requested file
                // was not written.
                print!("{report}");
                Err(format!("cannot write {}: {err}", path.display()))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: EvalKind, input: &str, keep: &[&str], drop_terms: &[&str]) -> EvalItem {
        EvalItem {
            id: "id".to_string(),
            kind,
            input: input.to_string(),
            keep: keep.iter().map(|s| (*s).to_string()).collect(),
            drop_terms: drop_terms.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn scored(kind: EvalKind, meaning_ok: bool, retraction_ok: bool) -> (EvalKind, TakeScore) {
        (
            kind,
            TakeScore {
                meaning_ok,
                retraction_ok,
                reason: String::new(),
            },
        )
    }

    #[test]
    fn keep_entry_matches_any_alternative() {
        assert!(alternative_present("fifteen|15", "We need 15 chairs"));
        assert!(alternative_present("fifteen|15", "We need Fifteen chairs"));
        assert!(!alternative_present("fifteen|15", "We need four chairs"));
        assert!(alternative_present("Marcus", "send it to marcus"));
        // A blank alternative must not match every string.
        assert!(!alternative_present("|", "anything"));
        assert!(!alternative_present("", "anything"));
    }

    #[test]
    fn matching_is_case_insensitive() {
        let item = item(EvalKind::Retract, "hi", &["Marcus"], &["Priya"]);
        let kept = score_take(&item, Ok("Send the report to marcus."));
        assert!(kept.meaning_ok);
        assert!(kept.retraction_ok);
        assert_eq!(kept.reason, "");

        let dropped = score_take(&item, Ok("Send it to PRIYA and marcus."));
        assert!(dropped.meaning_ok);
        assert!(!dropped.retraction_ok);
        assert_eq!(dropped.reason, "drop still present: Priya");
    }

    #[test]
    fn drop_entries_are_literals_not_alternatives() {
        assert!(retraction_removed(&[String::from("a|b")], "a and b"));
        assert!(!retraction_removed(&[String::from("a|b")], "keep a|b here"));
        assert!(!retraction_removed(
            &[String::from("Priya"), String::from("12")],
            "priya left"
        ));
        assert!(retraction_removed(
            &[String::from("Priya"), String::from("12")],
            "marcus left"
        ));
    }

    #[test]
    fn empty_refusal_and_overlong_output_fail_both_checks() {
        let item = item(EvalKind::Retract, "ab", &["ab"], &["zz"]);
        for output in ["", "   ", "\n"] {
            let score = score_take(&item, Ok(output));
            assert!(!score.meaning_ok && !score.retraction_ok);
            assert_eq!(score.reason, "empty output");
        }

        let refused = score_take(&item, Err("no eos"));
        assert!(!refused.meaning_ok && !refused.retraction_ok);
        assert_eq!(refused.reason, "refusal: no eos");

        // Exactly twice the input is scored on its text. One more char fails
        // both checks even though "ab" is present and "zz" is not.
        let exact = score_take(&item, Ok("abcd"));
        assert!(exact.meaning_ok && exact.retraction_ok, "{exact:?}");
        let over = score_take(&item, Ok("abcde"));
        assert!(!over.meaning_ok && !over.retraction_ok);
        assert_eq!(over.reason, "output longer than 2x input");
    }

    #[test]
    fn overlong_uses_char_length_not_bytes() {
        // 4 chars, 8 bytes. 9 ASCII chars is over 2× chars (8) and under 2× bytes (16).
        let item = item(EvalKind::Keep, "éééé", &[], &[]);
        let nine = "123456789";
        assert!(nine.len() < "éééé".len() * 2);
        assert!(nine.chars().count() > "éééé".chars().count() * 2);
        let score = score_take(&item, Ok(nine));
        assert_eq!(score.reason, "output longer than 2x input");
        assert!(!score.meaning_ok && !score.retraction_ok);
    }

    #[test]
    fn retraction_bar_is_ceil_of_four_fifths() {
        // Independent of `(4n + 4) / 5`: remainder form of ceil(8n/10).
        fn ceil_eight_tenths(n: usize) -> usize {
            let tenths = n * 8;
            tenths / 10 + usize::from(tenths % 10 != 0)
        }
        for n in 0..=40 {
            assert_eq!(retraction_bar(n), ceil_eight_tenths(n), "n={n}");
        }
        assert_eq!(retraction_bar(22), 18);
        assert_eq!(retraction_bar(0), 0);
        assert_eq!(retraction_bar(1), 1); // trunc(0.8) would be 0
        assert_eq!(retraction_bar(4), 4); // round-to-nearest(3.2) would be 3
        assert_eq!(retraction_bar(5), 4);
    }

    #[test]
    fn retraction_bar_passes_at_eighteen_of_twenty_two_and_fails_at_seventeen() {
        let mut rows = Vec::new();
        for i in 0..22 {
            rows.push(scored(EvalKind::Retract, true, i < 18));
        }
        let high = summarize(&rows);
        assert_eq!(high.retraction_passed, 18);
        assert!(high.retraction_ok());
        assert!(high.passed());

        rows[0].1.retraction_ok = false;
        let low = summarize(&rows);
        assert_eq!(low.retraction_passed, 17);
        assert!(!low.retraction_ok());
        assert!(!low.passed());
    }

    #[test]
    fn meaning_bar_requires_every_take_and_keep_rows_skip_the_retraction_bar() {
        let mut rows = vec![scored(EvalKind::Retract, true, true); 5];
        rows.extend(vec![scored(EvalKind::Keep, true, false); 10]);
        let summary = summarize(&rows);
        assert_eq!(summary.retracts, 5);
        assert_eq!(summary.retraction_passed, 5);
        assert!(summary.retraction_ok());
        assert!(summary.passed());

        rows.push(scored(EvalKind::Keep, false, true));
        let missed = summarize(&rows);
        assert!(!missed.meaning_ok());
        assert!(missed.retraction_ok());
        assert!(!missed.passed());
    }

    #[test]
    fn invisible_chars_are_stripped_before_scoring() {
        let item = item(EvalKind::Keep, "hello", &["hello"], &[]);
        let (text, score) = judge(&item, Ok("hel\u{200B}lo"));
        assert_eq!(text, "hello");
        assert!(score.meaning_ok);
        assert!(score.reason.is_empty());
    }

    #[test]
    fn parses_jsonl_kinds_and_keep_alternatives() {
        let text = concat!(
            "{\"id\":\"r01\",\"kind\":\"retract\",\"input\":\"hi\",\"keep\":[\"fifteen|15\"],\"drop\":[\"Priya\"]}\n",
            "\n",
            "{\"id\":\"k01\",\"kind\":\"keep\",\"input\":\"hello\",\"keep\":[\"hello\"],\"drop\":[]}\n",
        );
        let items = parse_eval_set(text).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, EvalKind::Retract);
        assert_eq!(items[0].keep, vec!["fifteen|15"]);
        assert_eq!(items[0].drop_terms, vec!["Priya"]);
        assert_eq!(items[1].kind, EvalKind::Keep);
        assert_eq!(items[1].id, "k01");
    }

    #[test]
    fn rejects_unknown_kind() {
        let err = parse_eval_set(
            "{\"id\":\"x\",\"kind\":\"nope\",\"input\":\"a\",\"keep\":[],\"drop\":[]}",
        )
        .unwrap_err();
        assert!(err.contains("kind"), "{err}");
    }

    #[test]
    fn report_has_the_table_totals_and_verdict() {
        let score = TakeScore {
            meaning_ok: false,
            retraction_ok: false,
            reason: "empty output".to_string(),
        };
        let rows_scored = vec![(EvalKind::Retract, score.clone())];
        let summary = summarize(&rows_scored);
        let rows = vec![ReportRow {
            id: "r01".to_string(),
            kind: EvalKind::Retract,
            output: "line\nwith | pipe".to_string(),
            score,
        }];
        let report = render_report("default_improve_transcriptions", "m.gguf", &rows, &summary);
        assert!(report.contains("| id | kind | meaning ok | retraction ok | output | reason |"));
        let row_line = report.lines().find(|line| line.contains("r01")).unwrap();
        assert!(row_line.contains("line with"));
        assert!(row_line.contains("\\|"));
        assert!(!row_line.contains('\n'));
        assert!(report.contains("Meaning kept: 0/1 FAIL"));
        assert!(report.contains("at least 1 of 1"));
        assert!(report.contains("**FAIL**"));
        assert!(!report.contains("**PASS**"));
    }

    #[test]
    fn default_eval_prompt_is_a_builtin_and_uses_the_local_provider_path() {
        let template = prompt_template(DEFAULT_EVAL_PROMPT_ID).unwrap();
        let (system, user) =
            crate::actions::build_post_process_messages(&template, "Send it to Marcus.", None);
        assert_eq!(user, "Send it to Marcus.");
        assert!(!system.contains("${output}"));
        assert!(system.contains("<transcript>"));
        assert!(system.contains("Return only the cleaned text"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn eval_generation_budget_matches_the_app() {
        assert_eq!(crate::actions::LOCAL_LLM_MAX_TOKENS, 512);
    }

    #[test]
    fn missing_keep_records_the_entry_including_alternatives() {
        let item = item(EvalKind::Keep, "hello", &["fifteen|15", "chairs"], &[]);
        let score = score_take(&item, Ok("hello there"));
        assert!(!score.meaning_ok);
        assert!(score.retraction_ok);
        assert_eq!(score.reason, "missing keep: fifteen|15; chairs");
    }

    #[test]
    fn rejects_duplicate_id() {
        let text = concat!(
            "{\"id\":\"k01\",\"kind\":\"keep\",\"input\":\"hello\",\"keep\":[\"hello\"],\"drop\":[]}\n",
            "{\"id\":\"k01\",\"kind\":\"keep\",\"input\":\"world\",\"keep\":[\"world\"],\"drop\":[]}\n",
        );
        let err = parse_eval_set(text).unwrap_err();
        assert!(err.contains("duplicate id"), "{err}");
        assert!(err.contains("\"k01\""), "{err}");
    }

    #[test]
    fn rejects_empty_keep_list() {
        let text =
            "{\"id\":\"k01\",\"kind\":\"keep\",\"input\":\"hello\",\"keep\":[],\"drop\":[]}\n";
        let err = parse_eval_set(text).unwrap_err();
        assert!(err.contains("keep list is empty"), "{err}");
        assert!(err.contains("\"k01\""), "{err}");
    }
}
