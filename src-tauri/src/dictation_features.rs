//! Dictation engine helpers for the macOS Globe/fn key emoji guard, code-editor
//! identifier casing, and local open-repo symbol spelling bias.
// ponytail: ceiling = local filesystem walk + defaults plist helper; upgrade path = tree-sitter symbol index.

use crate::audio_toolkit::text::has_identifier_casing;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use strsim::levenshtein;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FnKeyUsageType {
    DoNothing,
    ChangeInputSource,
    ShowEmojiAndSymbols,
    StartDictation,
    Unknown(i64),
}

impl FnKeyUsageType {
    pub fn from_code(code: i64) -> Self {
        match code {
            0 => Self::DoNothing,
            1 => Self::ChangeInputSource,
            2 => Self::ShowEmojiAndSymbols,
            3 => Self::StartDictation,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlobeKeyGuardStatus {
    NotGlobeShortcut,
    AlreadyDisabled,
    EmojiActionDisabled {
        previous: FnKeyUsageType,
    },
    ConflictWarning {
        previous: FnKeyUsageType,
        error: String,
    },
}

pub fn is_globe_or_fn_binding(binding: &str) -> bool {
    binding
        .split(|c: char| c == '+' || c == '-' || c.is_whitespace())
        .any(|tok| matches!(tok.trim().to_ascii_lowercase().as_str(), "fn" | "globe"))
}

pub fn parse_fn_usage_type(raw: &str) -> FnKeyUsageType {
    raw.trim()
        .parse::<i64>()
        .map(FnKeyUsageType::from_code)
        .unwrap_or(FnKeyUsageType::ShowEmojiAndSymbols)
}

pub fn guard_globe_shortcut_with<R, W>(
    binding: &str,
    read_usage: R,
    disable_fn: W,
) -> GlobeKeyGuardStatus
where
    R: FnOnce() -> FnKeyUsageType,
    W: FnOnce() -> Result<(), String>,
{
    if !is_globe_or_fn_binding(binding) {
        return GlobeKeyGuardStatus::NotGlobeShortcut;
    }
    let current = read_usage();
    if current == FnKeyUsageType::DoNothing {
        return GlobeKeyGuardStatus::AlreadyDisabled;
    }
    match disable_fn() {
        Ok(()) => GlobeKeyGuardStatus::EmojiActionDisabled { previous: current },
        Err(error) => GlobeKeyGuardStatus::ConflictWarning {
            previous: current,
            error,
        },
    }
}

pub fn on_shortcut_registered(binding: &str) {
    if !is_globe_or_fn_binding(binding) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let status = guard_globe_shortcut_with(
            binding,
            || {
                std::process::Command::new("defaults")
                    .args(["read", "com.apple.HIToolbox", "AppleFnUsageType"])
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| parse_fn_usage_type(&s))
                    .unwrap_or(FnKeyUsageType::ShowEmojiAndSymbols)
            },
            || {
                std::process::Command::new("defaults")
                    .args([
                        "write",
                        "com.apple.HIToolbox",
                        "AppleFnUsageType",
                        "-int",
                        "0",
                    ])
                    .status()
                    .map_err(|e| e.to_string())
                    .and_then(|s| {
                        if s.success() {
                            Ok(())
                        } else {
                            Err("defaults write failed".into())
                        }
                    })
            },
        );
        log::info!(
            "Globe/fn shortcut guard status for '{}': {:?}",
            binding,
            status
        );
    }
}

pub fn is_code_editor_app(app_name_or_bundle_id: &str) -> bool {
    let lower = app_name_or_bundle_id.to_ascii_lowercase();
    const EDITORS: &[&str] = &[
        "code",
        "vscode",
        "cursor",
        "zed",
        "xcode",
        "nvim",
        "neovim",
        "vim",
        "jetbrains",
        "intellij",
        "rustrover",
        "pycharm",
        "webstorm",
        "goland",
        "clion",
        "sublime",
        "terminal",
        "iterm",
        "warp",
        "alacritty",
        "kitty",
        "ghostty",
        "wezterm",
    ];
    EDITORS.iter().any(|needle| lower.contains(needle))
}

fn norm_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn capitalize_ascii(w: &str) -> String {
    let mut chars = w.chars();
    chars
        .next()
        .map(|f| f.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

fn join_casing_cue(style: &str, words: &[&str]) -> String {
    let clean: Vec<String> = words
        .iter()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect();
    match style {
        "camel" => clean
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if i == 0 {
                    w.clone()
                } else {
                    capitalize_ascii(w)
                }
            })
            .collect::<Vec<_>>()
            .concat(),
        "pascal" => clean
            .iter()
            .map(|w| capitalize_ascii(w))
            .collect::<Vec<_>>()
            .concat(),
        "snake" => clean.join("_"),
        "screaming" => clean
            .iter()
            .map(|w| w.to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join("_"),
        _ => clean.join(" "),
    }
}

pub fn format_code_editor_identifiers(
    text: &str,
    code_editor_active: bool,
    identifiers: &[String],
) -> String {
    if !code_editor_active {
        return text.to_string();
    }
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut stage1: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let rem = &tokens[i..];
        let cue =
            if rem.len() >= 4 && norm_key(rem[0]) == "screaming" && norm_key(rem[1]) == "snake" {
                Some(("screaming", 2, 3.min(rem.len() - 2)))
            } else if rem.len() >= 3 && norm_key(rem[1]) == "case" {
                match norm_key(rem[0]).as_str() {
                    "camel" => Some(("camel", 2, 3.min(rem.len() - 2))),
                    "pascal" => Some(("pascal", 2, 3.min(rem.len() - 2))),
                    "snake" => Some(("snake", 2, 3.min(rem.len() - 2))),
                    _ => None,
                }
            } else {
                None
            };
        if let Some((style, skip, take_n)) = cue {
            let (_, suffix) = split_punct(rem[skip + take_n - 1]);
            stage1.push(format!(
                "{}{suffix}",
                join_casing_cue(style, &rem[skip..skip + take_n])
            ));
            i += skip + take_n;
        } else {
            stage1.push(tokens[i].to_string());
            i += 1;
        }
    }
    apply_repo_symbols(&stage1.join(" "), identifiers, 0.24)
}

fn split_punct(word: &str) -> (&str, &str) {
    let trimmed = word.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '_');
    let prefix = &word[..word.len() - trimmed.len()];
    let core = trimmed.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
    (prefix, &trimmed[core.len()..])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepoUnavailableReason {
    SshRemote,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepoScanResult {
    Symbols(Vec<String>),
    NoLocalRepo { reason: RepoUnavailableReason },
}

pub fn scan_open_repo_symbols(root: &Path, is_ssh_remote: bool) -> RepoScanResult {
    let root_str = root.to_string_lossy();
    if is_ssh_remote || root_str.starts_with("ssh://") || root_str.contains("ssh-remote") {
        return RepoScanResult::NoLocalRepo {
            reason: RepoUnavailableReason::SshRemote,
        };
    }
    if !root.exists() || !root.is_dir() || !root.join(".git").exists() {
        return RepoScanResult::NoLocalRepo {
            reason: RepoUnavailableReason::NotFound,
        };
    }
    let mut symbols = BTreeSet::new();
    let mut files_budget = 250usize;
    collect_repo_symbols_rec(root, 0, &mut files_budget, &mut symbols);
    RepoScanResult::Symbols(symbols.into_iter().collect())
}

const MAX_REPO_SOURCE_FILE_BYTES: u64 = 262_144;

fn collect_repo_symbols_rec(
    dir: &Path,
    depth: usize,
    files_budget: &mut usize,
    out: &mut BTreeSet<String>,
) {
    if depth > 5 || *files_budget == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *files_budget == 0 {
            break;
        }
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let lower_name = name.to_ascii_lowercase();
        if name.starts_with('.')
            || lower_name.starts_with(".env")
            || lower_name.ends_with(".env")
            || lower_name.ends_with(".pem")
            || lower_name.ends_with(".key")
            || matches!(
                name,
                "target" | "node_modules" | "dist" | "build" | "vendor"
            )
        {
            continue;
        }
        if meta.is_dir() {
            collect_repo_symbols_rec(&path, depth + 1, files_budget, out);
        } else if meta.is_file()
            && meta.len() <= MAX_REPO_SOURCE_FILE_BYTES
            && matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "ts" | "tsx" | "js" | "py" | "go")
            )
        {
            *files_budget -= 1;
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.len() >= 3 && !matches!(stem, "mod" | "lib" | "main" | "index") {
                    out.insert(stem.to_string());
                }
            }
            if let Ok(src) = fs::read_to_string(&path) {
                for tok in src.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if tok.len() >= 4 && has_identifier_casing(tok) {
                        out.insert(tok.to_string());
                    }
                }
            }
        }
    }
}

fn is_span_sep(c: char) -> bool {
    c.is_whitespace() || matches!(c, '—' | '–')
}

fn split_symbol_spans(text: &str) -> (&str, Vec<(&str, &str)>) {
    let trimmed = text.trim_start_matches(is_span_sep);
    let leading = &text[..text.len() - trimmed.len()];
    let mut rest = trimmed;
    let mut spans = Vec::new();
    while !rest.is_empty() {
        let after_word = rest.trim_start_matches(|c| !is_span_sep(c));
        let word = &rest[..rest.len() - after_word.len()];
        let after_sep = after_word.trim_start_matches(is_span_sep);
        let sep = &after_word[..after_word.len() - after_sep.len()];
        spans.push((word, sep));
        rest = after_sep;
    }
    (leading, spans)
}

pub fn apply_repo_symbols(text: &str, symbols: &[String], threshold: f64) -> String {
    if symbols.is_empty() {
        return text.to_string();
    }
    let (leading, spans) = split_symbol_spans(text);
    if spans.is_empty() {
        return text.to_string();
    }
    let sym_keys: Vec<(String, &String)> = symbols
        .iter()
        .map(|s| (norm_key(s), s))
        .filter(|(k, _)| !k.is_empty())
        .collect();
    let mut out = String::with_capacity(text.len());
    out.push_str(leading);
    let mut i = 0;
    while i < spans.len() {
        let mut hit: Option<(usize, &String)> = None;
        for n in (1..=5).rev() {
            if i + n <= spans.len() {
                if !spans[i..i + n - 1]
                    .iter()
                    .all(|(_, sep)| sep.chars().all(char::is_whitespace))
                {
                    continue;
                }
                if norm_key(spans[i].0).is_empty() || norm_key(spans[i + n - 1].0).is_empty() {
                    continue;
                }
                let cand: String = spans[i..i + n].iter().map(|(w, _)| norm_key(w)).collect();
                if let Some((_, sym)) = sym_keys.iter().find(|(k, _)| *k == cand) {
                    hit = Some((n, *sym));
                    break;
                }
            }
        }
        if hit.is_none() {
            for n in (1..=4).rev() {
                if i + n <= spans.len() {
                    if n == 1 && has_identifier_casing(spans[i].0) {
                        continue;
                    }
                    if !spans[i..i + n - 1]
                        .iter()
                        .all(|(_, sep)| sep.chars().all(char::is_whitespace))
                    {
                        continue;
                    }
                    let first_k = norm_key(spans[i].0);
                    let last_k = norm_key(spans[i + n - 1].0);
                    let cand: String = spans[i..i + n].iter().map(|(w, _)| norm_key(w)).collect();
                    if cand.len() < 4 || first_k.len() < 2 || last_k.len() < 2 {
                        continue;
                    }
                    let mut best: Option<(f64, &String)> = None;
                    for (k, sym) in &sym_keys {
                        if !k.starts_with(&first_k[..2])
                            || !k.ends_with(&last_k[last_k.len() - 2..])
                        {
                            continue;
                        }
                        let max_len = cand.len().max(k.len()) as f64;
                        let score = levenshtein(&cand, k) as f64 / max_len;
                        if score <= threshold && best.as_ref().is_none_or(|(b, _)| score < *b) {
                            best = Some((score, *sym));
                        }
                    }
                    if let Some((_, sym)) = best {
                        hit = Some((n, sym));
                        break;
                    }
                }
            }
        }
        if let Some((n, sym)) = hit {
            let (prefix, _) = split_punct(spans[i].0);
            let (_, suffix) = split_punct(spans[i + n - 1].0);
            let trailing_sep = spans[i + n - 1].1;
            out.push_str(&format!("{prefix}{sym}{suffix}{trailing_sep}"));
            i += n;
        } else {
            out.push_str(spans[i].0);
            out.push_str(spans[i].1);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn globe_key_guard_disables_emoji_picker_and_warns_on_write_failure() {
        assert!(
            is_globe_or_fn_binding("fn")
                && is_globe_or_fn_binding("Globe")
                && is_globe_or_fn_binding("fn+space")
        );
        assert!(!is_globe_or_fn_binding("option+space"));
        let mut wrote = false;
        let disabled = guard_globe_shortcut_with(
            "fn",
            || parse_fn_usage_type("2\n"),
            || {
                wrote = true;
                Ok(())
            },
        );
        assert!(wrote);
        assert_eq!(
            disabled,
            GlobeKeyGuardStatus::EmojiActionDisabled {
                previous: FnKeyUsageType::ShowEmojiAndSymbols
            }
        );
        let warned = guard_globe_shortcut_with(
            "globe",
            || FnKeyUsageType::ShowEmojiAndSymbols,
            || Err("CFPreferences read-only".into()),
        );
        assert_eq!(
            warned,
            GlobeKeyGuardStatus::ConflictWarning {
                previous: FnKeyUsageType::ShowEmojiAndSymbols,
                error: "CFPreferences read-only".into()
            }
        );
    }

    #[test]
    fn code_editor_mode_preserves_camel_pascal_snake_and_screaming_snake_identifiers() {
        assert!(
            is_code_editor_app("com.microsoft.VSCode")
                && is_code_editor_app("Cursor")
                && !is_code_editor_app("com.apple.mail")
        );
        let idents = vec![
            "getUserById".into(),
            "UserProfileStore".into(),
            "fetch_session_token".into(),
            "MAX_RETRY_COUNT".into(),
        ];
        let raw = "Getuserbyid calls user profile store and fetch session token with max retry count, plus snake case parse token stream.";
        assert_eq!(
            format_code_editor_identifiers(raw, true, &idents),
            "getUserById calls UserProfileStore and fetch_session_token with MAX_RETRY_COUNT, plus parse_token_stream."
        );
    }

    #[test]
    fn repo_symbol_scanner_halves_identifier_errors_under_one_second_and_handles_ssh_fallback() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            scan_open_repo_symbols(dir.path(), true),
            RepoScanResult::NoLocalRepo {
                reason: RepoUnavailableReason::SshRemote
            }
        );
        assert_eq!(
            scan_open_repo_symbols(dir.path(), false),
            RepoScanResult::NoLocalRepo {
                reason: RepoUnavailableReason::NotFound
            }
        );
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/engine.rs"),
            "pub struct TranscriptionCoordinator; pub struct AudioRecordingManager; pub fn get_history_entries() {} \
             pub struct CleanupResolver; pub fn overlay_xy_for_monitor() {} pub const MAX_RETRY_COUNT: usize = 3; \
             pub fn apply_custom_words() {} pub struct ShortcutBinding;",
        )
        .unwrap();
        let huge = format!(
            "pub struct OversizedBundleSymbol;\n{}",
            "// pad\n".repeat(45_000)
        );
        fs::write(dir.path().join("src/huge_bundle.js"), huge).unwrap();
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().expect("outside");
            fs::write(
                outside.path().join("leaked_symlink.rs"),
                "pub struct SymlinkedOutsideSymbol;",
            )
            .unwrap();
            std::os::unix::fs::symlink(
                outside.path().join("leaked_symlink.rs"),
                dir.path().join("src/symlinked.rs"),
            )
            .unwrap();
            let t0 = Instant::now();
            let RepoScanResult::Symbols(symbols) = scan_open_repo_symbols(dir.path(), false) else {
                panic!("expected Symbols")
            };
            assert!(t0.elapsed().as_millis() < 1000, "p50 must be < 1s");
            assert!(!symbols
                .iter()
                .any(|s| s == "SymlinkedOutsideSymbol" || s == "symlinked"));
            assert!(!symbols
                .iter()
                .any(|s| s == "OversizedBundleSymbol" || s == "huge_bundle"));
        }
        let RepoScanResult::Symbols(symbols) = scan_open_repo_symbols(dir.path(), false) else {
            panic!("expected Symbols")
        };
        let fixtures = [
            (
                "Update transcription coordinator now",
                "Update TranscriptionCoordinator now",
            ),
            (
                "Check audio recording manager state",
                "Check AudioRecordingManager state",
            ),
            (
                "Call get history entries first",
                "Call get_history_entries first",
            ),
            (
                "Run cleanup resolver on take",
                "Run CleanupResolver on take",
            ),
            (
                "Use overlay xy for monitor helper",
                "Use overlay_xy_for_monitor helper",
            ),
            (
                "Set max retry count to three",
                "Set MAX_RETRY_COUNT to three",
            ),
            (
                "Pass apply custom words list",
                "Pass apply_custom_words list",
            ),
            (
                "Save shortcut binding in store",
                "Save ShortcutBinding in store",
            ),
        ];
        let baseline_errors = fixtures.iter().filter(|(raw, want)| raw != want).count();
        let biased_errors = fixtures
            .iter()
            .filter(|(raw, want)| &apply_repo_symbols(raw, &symbols, 0.24) != want)
            .count();
        assert_eq!(baseline_errors, 8);
        assert_eq!(biased_errors, 0);
        assert!(biased_errors * 2 <= baseline_errors);
    }

    #[test]
    fn multibyte_punctuation_and_non_ascii_words_preserve_boundaries_without_panicking() {
        let symbols = vec![
            "getUserById".to_string(),
            "TranscriptionCoordinator".to_string(),
        ];
        assert_eq!(
            format_code_editor_identifiers("Call “get user by id”—now…", true, &symbols),
            "Call “getUserById”—now…"
        );
        assert_eq!(
            format_code_editor_identifiers(
                "camel case get user id… — café aé éa 中",
                true,
                &symbols
            ),
            "getUserId… — café aé éa 中"
        );
        assert_eq!(
            apply_repo_symbols("— transcription coordinator —", &symbols, 0.24),
            "— TranscriptionCoordinator —"
        );
    }
}
