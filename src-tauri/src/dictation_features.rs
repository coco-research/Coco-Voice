//! Dictation engine helpers for the macOS Globe/fn key emoji guard, code-editor
//! identifier casing, and local open-repo symbol spelling bias.
// ponytail: ceiling = local filesystem walk + defaults plist helper; upgrade path = tree-sitter symbol index.

use serde::{Deserialize, Serialize};
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
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
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
    let p = word
        .chars()
        .take_while(|c| !c.is_alphanumeric() && *c != '_')
        .count();
    let s = word
        .char_indices()
        .rev()
        .take_while(|(_, c)| !c.is_alphanumeric() && *c != '_')
        .count();
    (
        if p > 0 { &word[..p] } else { "" },
        if s > 0 { &word[word.len() - s..] } else { "" },
    )
}

pub fn apply_repo_symbols(text: &str, symbols: &[String], threshold: f64) -> String {
    if symbols.is_empty() {
        return text.to_string();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let sym_keys: Vec<(String, &String)> = symbols
        .iter()
        .map(|s| (norm_key(s), s))
        .filter(|(k, _)| !k.is_empty())
        .collect();
    let mut joined = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let mut hit: Option<(usize, &String)> = None;
        for n in (1..=5).rev() {
            if i + n <= words.len() {
                let cand: String = words[i..i + n].iter().map(|w| norm_key(w)).collect();
                if let Some((_, sym)) = sym_keys.iter().find(|(k, _)| *k == cand) {
                    hit = Some((n, *sym));
                    break;
                }
            }
        }
        if hit.is_none() {
            for n in (1..=4).rev() {
                if i + n <= words.len() {
                    let first_k = norm_key(words[i]);
                    let last_k = norm_key(words[i + n - 1]);
                    let cand: String = words[i..i + n].iter().map(|w| norm_key(w)).collect();
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
            let (prefix, _) = split_punct(words[i]);
            let (_, suffix) = split_punct(words[i + n - 1]);
            joined.push(format!("{prefix}{sym}{suffix}"));
            i += n;
        } else {
            joined.push(words[i].to_string());
            i += 1;
        }
    }
    joined.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
