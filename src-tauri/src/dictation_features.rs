//! Dictation engine helpers for the macOS Globe/fn key emoji guard, code-editor
//! identifier casing, and local open-repo symbol spelling bias.
// ponytail: ceiling = local filesystem walk + defaults plist helper; upgrade path = tree-sitter symbol index.

use serde::{Deserialize, Serialize};

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
}
