//! Headless on-device cleanup eval (`--cleanup-eval`).
//!
//! Scoring is pure and tested without a model. On macOS each take is cleaned
//! once through [`crate::local_llm::generate_text`] with the system prompt and
//! user message the app builds for a fresh cleanup ([`cleanup_messages`]).
//! Other platforms print a message and exit 1. The process exits before Tauri
//! builds a window or a tray.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::Path;

use serde::Deserialize;

/// Built-in prompt used when `--eval-prompt` is omitted.
const DEFAULT_EVAL_PROMPT_ID: &str = "default_improve_transcriptions";

/// Generation budget per take. Keep it equal to the app's local cleanup budget
/// (512) and share one constant once the app is wired to `local_llm`.
#[cfg(target_os = "macos")]
const EVAL_MAX_TOKENS: i32 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
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

impl TryFrom<String> for EvalKind {
    type Error = String;

    fn try_from(kind: String) -> Result<Self, String> {
        [Self::Retract, Self::Keep]
            .into_iter()
            .find(|candidate| candidate.as_str() == kind)
            .ok_or_else(|| format!("kind must be \"retract\" or \"keep\", got {kind:?}"))
    }
}

#[derive(Debug, Deserialize)]
struct EvalItem {
    id: String,
    kind: EvalKind,
    input: String,
    #[serde(default)]
    keep: Vec<String>,
    #[serde(default, rename = "drop")]
    drop_terms: Vec<String>,
}

#[derive(Debug)]
struct TakeScore {
    meaning_ok: bool,
    retraction_ok: bool,
    reason: String,
}

struct ReportRow {
    id: String,
    kind: EvalKind,
    output: String,
    score: TakeScore,
}

struct Summary {
    takes: usize,
    meaning_passed: usize,
    retracts: usize,
    retraction_passed: usize,
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

/// `true` when the drop phrase is a case-insensitive substring.
///
/// Drop phrases are literals. A `|` inside one is not an alternative split.
fn drop_present(term: &str, output: &str) -> bool {
    let needle = term.trim().to_lowercase();
    !needle.is_empty() && output.to_lowercase().contains(&needle)
}

/// Score one cleaned take.
///
/// `Err` is a model refusal. Empty, refusal, and over-long output (more than
/// twice the input, in characters) fail both checks for this take even when the
/// substrings would have passed.
fn score_take(item: &EvalItem, generated: Result<&str, &str>) -> TakeScore {
    let hard_fail = |reason: String| TakeScore {
        meaning_ok: false,
        retraction_ok: false,
        reason,
    };
    let output = match generated {
        Ok(output) => output,
        Err(err) => return hard_fail(format!("refusal: {err}")),
    };
    if output.trim().is_empty() {
        return hard_fail("empty output".to_string());
    }
    if output.chars().count() > item.input.chars().count().saturating_mul(2) {
        return hard_fail("output longer than 2x input".to_string());
    }

    let missing: Vec<&str> = item
        .keep
        .iter()
        .map(String::as_str)
        .filter(|entry| !alternative_present(entry, output))
        .collect();
    let present: Vec<&str> = item
        .drop_terms
        .iter()
        .map(String::as_str)
        .filter(|term| drop_present(term, output))
        .collect();
    let mut reasons = Vec::new();
    if !missing.is_empty() {
        reasons.push(format!("missing keep: {}", missing.join("; ")));
    }
    if !present.is_empty() {
        reasons.push(format!("drop still present: {}", present.join("; ")));
    }
    TakeScore {
        meaning_ok: missing.is_empty(),
        retraction_ok: present.is_empty(),
        reason: reasons.join("; "),
    }
}

/// Minimum retract takes that must clear the drop list: `ceil(0.8 * n)`.
///
/// 22 retracts require 18. 0 retracts require 0.
fn retraction_bar(retract_count: usize) -> usize {
    (retract_count * 4).div_ceil(5)
}

fn summarize(rows: &[ReportRow]) -> Summary {
    let retracts = || rows.iter().filter(|row| row.kind == EvalKind::Retract);
    Summary {
        takes: rows.len(),
        meaning_passed: rows.iter().filter(|row| row.score.meaning_ok).count(),
        retracts: retracts().count(),
        retraction_passed: retracts().filter(|row| row.score.retraction_ok).count(),
    }
}

/// Score the text the app would paste. Invisible characters are stripped first,
/// the same set `actions::strip_invisible_chars` removes.
fn judge(item: &EvalItem, generated: Result<&str, &str>) -> ReportRow {
    let output = generated
        .map(|raw| raw.replace(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'], ""))
        .unwrap_or_default();
    ReportRow {
        id: item.id.clone(),
        kind: item.kind,
        score: score_take(item, generated.map(|_| output.as_str())),
        output,
    }
}

/// Parse a JSONL eval set. A row is rejected in this order: bad JSON or kind,
/// duplicate id, empty keep list (keep takes), empty drop list (retract takes).
fn parse_eval_set(text: &str) -> Result<Vec<EvalItem>, String> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (idx, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let line_no = idx + 1;
        let item: EvalItem =
            serde_json::from_str(line).map_err(|err| format!("line {line_no}: {err}"))?;
        if !seen.insert(item.id.clone()) {
            return Err(format!("line {line_no}: duplicate id {:?}", item.id));
        }
        let (needed, name) = match item.kind {
            EvalKind::Keep => (&item.keep, "keep"),
            EvalKind::Retract => (&item.drop_terms, "drop"),
        };
        if needed.is_empty() {
            let id = &item.id;
            return Err(format!(
                "line {line_no}: {name} list is empty for id {id:?}"
            ));
        }
        items.push(item);
    }
    Ok(items)
}

fn md_cell(value: &str) -> String {
    value.replace(['\n', '\r', '\t'], " ").replace('|', "\\|")
}

fn verdict(ok: bool) -> &'static str {
    if ok {
        "PASS"
    } else {
        "FAIL"
    }
}

fn render_report(prompt_id: &str, model: &str, rows: &[ReportRow], summary: &Summary) -> String {
    let yes_no = |ok: bool| if ok { "yes" } else { "no" };
    let mut out = format!(
        "# Cleanup eval\n\nPrompt `{prompt_id}`. Model `{model}`.\n\n\
         | id | kind | meaning ok | retraction ok | output | reason |\n\
         | --- | --- | --- | --- | --- | --- |\n"
    );
    for row in rows {
        out += &format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            md_cell(&row.id),
            row.kind.as_str(),
            yes_no(row.score.meaning_ok),
            yes_no(row.score.retraction_ok),
            md_cell(&row.output),
            md_cell(&row.score.reason),
        );
    }
    out += &format!(
        "\n## Totals\n\n\
         - Meaning kept: {}/{} {} (bar: all takes)\n\
         - Retraction removed: {}/{} {} (bar: at least {} of {})\n\n\
         **{}**\n",
        summary.meaning_passed,
        summary.takes,
        verdict(summary.meaning_ok()),
        summary.retraction_passed,
        summary.retracts,
        verdict(summary.retraction_ok()),
        retraction_bar(summary.retracts),
        summary.retracts,
        verdict(summary.passed()),
    );
    out
}

fn prompt_template(id: &str) -> Result<String, String> {
    let prompts = crate::settings::get_default_settings().post_process_prompts;
    let ids: Vec<&str> = prompts.iter().map(|prompt| prompt.id.as_str()).collect();
    prompts
        .iter()
        .find(|prompt| prompt.id == id)
        .map(|prompt| prompt.prompt.clone())
        .ok_or_else(|| {
            format!(
                "unknown eval prompt '{id}'. Built-in ids: {}",
                ids.join(", ")
            )
        })
}

/// The `(system prompt, user content)` pair for a fresh cleanup: the template
/// with `${output}` removed, the take as the user message. Mirrors
/// `actions::build_post_process_messages` with no prior output.
fn cleanup_messages(template: &str, input: &str) -> (String, String) {
    (
        template.replace("${output}", "").trim().to_string(),
        input.to_string(),
    )
}

/// Run the eval on `set_path`. Exit code: 0 when both bars pass, 1 otherwise.
pub(crate) fn run_eval(set_path: &Path, args: &crate::cli::CliArgs) -> i32 {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (set_path, args);
        eprintln!("error: --cleanup-eval is only available on macOS");
        1
    }
    #[cfg(target_os = "macos")]
    match run_eval_macos(set_path, args) {
        Ok(passed) => i32::from(!passed),
        Err(err) => {
            eprintln!("error: {err}");
            1
        }
    }
}

#[cfg(target_os = "macos")]
fn run_eval_macos(set_path: &Path, args: &crate::cli::CliArgs) -> Result<bool, String> {
    let prompt_id = args
        .eval_prompt
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(DEFAULT_EVAL_PROMPT_ID);
    let template = prompt_template(prompt_id)?;
    let model_path = args
        .eval_model
        .as_deref()
        .ok_or("--eval-model is required with --cleanup-eval")?;
    if !model_path.is_file() {
        return Err(format!("eval model not found: {}", model_path.display()));
    }
    // A bare file name has the empty parent, which `create_dir_all` accepts.
    if let Some(dir) = args.eval_out.as_deref().and_then(Path::parent) {
        std::fs::create_dir_all(dir)
            .map_err(|err| format!("failed to create directory {}: {err}", dir.display()))?;
    }

    let text = std::fs::read_to_string(set_path)
        .map_err(|err| format!("cannot read {}: {err}", set_path.display()))?;
    let items = parse_eval_set(&text).map_err(|err| format!("{}: {err}", set_path.display()))?;
    if items.is_empty() {
        return Err(format!("eval set is empty: {}", set_path.display()));
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("failed to start eval runtime: {err}"))?;
    let mut rows = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        eprintln!("cleanup-eval: {}/{} {}", index + 1, items.len(), item.id);
        let (system_prompt, user_content) = cleanup_messages(&template, &item.input);
        // No token stream and no cancel. A refusal scores as a failed take.
        let generated = rt
            .block_on(crate::local_llm::generate_text(
                model_path,
                &system_prompt,
                &user_content,
                EVAL_MAX_TOKENS,
                None,
                None,
            ))
            .map_err(|err| err.to_string());
        rows.push(judge(item, generated.as_deref().map_err(String::as_str)));
    }
    // `process::exit` runs ggml's static destructors, and ggml-metal asserts if a
    // model is still resident. Nothing between the first take and here returns early.
    crate::local_llm::release_model();

    let summary = summarize(&rows);
    eprintln!(
        "cleanup-eval: {} (meaning {}/{}, retraction {}/{})",
        verdict(summary.passed()),
        summary.meaning_passed,
        summary.takes,
        summary.retraction_passed,
        summary.retracts,
    );
    let model = model_path.display().to_string();
    write_report(
        args.eval_out.as_deref(),
        &render_report(prompt_id, &model, &rows, &summary),
    )?;
    Ok(summary.passed())
}

#[cfg(target_os = "macos")]
fn write_report(path: Option<&Path>, report: &str) -> Result<(), String> {
    let Some(path) = path else {
        print!("{report}");
        return Ok(());
    };
    std::fs::write(path, report).map_err(|err| {
        // The model run already happened. Put the report on stdout so it is not
        // lost, and still fail because the requested file was not written.
        print!("{report}");
        format!("cannot write {}: {err}", path.display())
    })?;
    eprintln!("cleanup-eval: wrote {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_json(id: &str, kind: &str, input: &str, keep: &[&str], drop: &[&str]) -> String {
        serde_json::json!({"id": id, "kind": kind, "input": input, "keep": keep, "drop": drop})
            .to_string()
    }

    fn item(kind: EvalKind, input: &str, keep: &[&str], drop: &[&str]) -> EvalItem {
        serde_json::from_str(&row_json("id", kind.as_str(), input, keep, drop)).unwrap()
    }

    fn scored(kind: EvalKind, meaning_ok: bool, retraction_ok: bool) -> ReportRow {
        ReportRow {
            id: String::new(),
            kind,
            output: String::new(),
            score: TakeScore {
                meaning_ok,
                retraction_ok,
                reason: String::new(),
            },
        }
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
        let input = "Send it to Priya, no, Marcus.";
        let item = item(EvalKind::Retract, input, &["Marcus"], &["Priya"]);
        let kept = score_take(&item, Ok("Send the report to marcus."));
        assert!(kept.meaning_ok);
        assert!(kept.retraction_ok);
        assert_eq!(kept.reason, "");

        let dropped = score_take(&item, Ok("Send it to PRIYA and marcus."));
        assert!(dropped.meaning_ok);
        assert!(!dropped.retraction_ok);
        assert_eq!(dropped.reason, "drop still present: Priya");

        let both = score_take(&item, Ok("Send it to Priya."));
        assert_eq!(
            both.reason,
            "missing keep: Marcus; drop still present: Priya"
        );
    }

    #[test]
    fn drop_entries_are_literals_not_alternatives() {
        assert!(!drop_present("a|b", "a and b"));
        assert!(drop_present("a|b", "keep a|b here"));
        // A blank phrase never matches.
        assert!(!drop_present("  ", "anything"));
        // Any one drop phrase still present fails the take.
        let item = item(EvalKind::Retract, "priya left now", &[], &["Priya", "12"]);
        assert!(!score_take(&item, Ok("priya left")).retraction_ok);
        assert!(score_take(&item, Ok("marcus left")).retraction_ok);
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
        // Independent of the formula: the smallest bar k with 5k >= 4n.
        for n in 0..=40 {
            let smallest = (0..).find(|k| k * 5 >= n * 4).unwrap();
            assert_eq!(retraction_bar(n), smallest, "n={n}");
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

        rows[0].score.retraction_ok = false;
        let low = summarize(&rows);
        assert_eq!(low.retraction_passed, 17);
        assert!(!low.retraction_ok());
        assert!(!low.passed());
    }

    #[test]
    fn meaning_bar_requires_every_take_and_keep_rows_skip_the_retraction_bar() {
        let mut rows: Vec<ReportRow> = (0..5)
            .map(|_| scored(EvalKind::Retract, true, true))
            .chain((0..10).map(|_| scored(EvalKind::Keep, true, false)))
            .collect();
        let summary = summarize(&rows);
        assert_eq!(summary.retracts, 5);
        assert_eq!(summary.retraction_passed, 5);
        assert!(summary.retraction_ok());
        assert!(summary.passed());

        rows.push(scored(EvalKind::Keep, false, true));
        let missed = summarize(&rows);
        assert!(!missed.meaning_ok());
        assert!(missed.retraction_ok());
        assert_eq!(missed.retraction_passed, 5); // the keep row's flag is not counted
        assert!(!missed.passed());
    }

    #[test]
    fn invisible_chars_are_stripped_before_scoring() {
        let item = item(EvalKind::Keep, "hello", &["hello"], &[]);
        let row = judge(&item, Ok("h\u{200B}e\u{200C}l\u{200D}l\u{FEFF}o"));
        assert_eq!(row.output, "hello");
        assert!(row.score.meaning_ok);
        assert!(row.score.reason.is_empty());
    }

    #[test]
    fn parses_jsonl_kinds_and_keep_alternatives() {
        let text = concat!(
            "{\"id\":\"r01\",\"kind\":\"retract\",\"input\":\"hi\",\"keep\":[\"fifteen|15\"],\"drop\":[\"Priya\"]}\n",
            "\n",
            "{\"id\":\"k01\",\"kind\":\"keep\",\"input\":\"hello\",\"keep\":[\"hello\"],\"drop\":[]}\n",
            // A retract take needs no keep list.
            "{\"id\":\"r02\",\"kind\":\"retract\",\"input\":\"hi\",\"drop\":[\"x\"]}\n",
        );
        let items = parse_eval_set(text).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].kind, EvalKind::Retract);
        assert_eq!(items[0].keep, vec!["fifteen|15"]);
        assert_eq!(items[0].drop_terms, vec!["Priya"]);
        assert_eq!(items[1].kind, EvalKind::Keep);
        assert_eq!(items[1].id, "k01");
        assert!(items[2].keep.is_empty());
    }

    #[test]
    fn rejects_unknown_kind() {
        // The keep list is empty too: kind is checked before it.
        let err = parse_eval_set(&row_json("x", "nope", "hello", &[], &[])).unwrap_err();
        assert!(err.starts_with("line 1: "), "{err}");
        assert!(err.contains("kind"), "{err}");
        assert!(err.contains("\"nope\""), "{err}");
    }

    #[test]
    fn report_has_the_table_totals_and_verdict() {
        let mut row = scored(EvalKind::Retract, false, false);
        row.id = "r01".to_string();
        row.output = "line\nwith | pipe".to_string();
        row.score.reason = "empty output".to_string();
        let rows = vec![row];
        let summary = summarize(&rows);
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
        let (system, user) = cleanup_messages(&template, "Send it to Marcus.");
        assert_eq!(user, "Send it to Marcus.");
        assert!(!system.contains("${output}"));
        assert!(system.contains("<transcript>"));
        assert!(system.contains("Return only the cleaned text"));

        let err = prompt_template("nope").unwrap_err();
        assert!(
            err.contains("'nope'") && err.contains(DEFAULT_EVAL_PROMPT_ID),
            "{err}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn eval_generation_budget_stays_at_the_apps_512() {
        // Pins the number until the eval and the app share one constant.
        assert_eq!(EVAL_MAX_TOKENS, 512);
    }

    #[test]
    fn missing_keep_records_the_entry_including_alternatives() {
        let input = "We need fifteen chairs.";
        let item = item(EvalKind::Keep, input, &["fifteen|15", "chairs"], &[]);
        let score = score_take(&item, Ok("We need seats."));
        assert!(!score.meaning_ok);
        assert!(score.retraction_ok);
        assert_eq!(score.reason, "missing keep: fifteen|15; chairs");
    }

    #[test]
    fn rejects_duplicate_id() {
        let text = format!(
            "{}\n{}\n",
            row_json("k01", "keep", "hello", &["hello"], &[]),
            row_json("k01", "keep", "world", &["world"], &[])
        );
        let err = parse_eval_set(&text).unwrap_err();
        assert!(err.contains("duplicate id"), "{err}");
        assert!(err.contains("\"k01\""), "{err}");
    }

    #[test]
    fn rejects_empty_keep_list() {
        let err = parse_eval_set(&row_json("k01", "keep", "hello", &[], &[])).unwrap_err();
        assert!(err.contains("keep list is empty"), "{err}");
        assert!(err.contains("\"k01\""), "{err}");
    }

    #[test]
    fn rejects_retract_row_with_no_drop_list() {
        let err =
            parse_eval_set(&row_json("r01", "retract", "hello", &["hello"], &[])).unwrap_err();
        assert!(err.contains("drop list is empty"), "{err}");
        assert!(err.contains("\"r01\""), "{err}");

        let omitted =
            "{\"id\":\"r02\",\"kind\":\"retract\",\"input\":\"hello\",\"keep\":[\"hello\"]}\n";
        let err_omitted = parse_eval_set(omitted).unwrap_err();
        assert!(err_omitted.contains("drop list is empty"), "{err_omitted}");
        assert!(err_omitted.contains("\"r02\""), "{err_omitted}");
    }
}
