//! First-run onboarding checks (fail-closed permission gates, stale Accessibility
//! entry detection, continue-without-typing, Try-it sample), recording pill outcome
//! states, and Home health notices.
// ponytail: ceiling = pure state machines + settings helpers; upgrade path = native macOS TCC event listener.

use crate::settings::{AppSettings, ClipboardHandling, PasteMethod};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RecordingPillState {
    Listening { repo_chip: Option<String> },
    Streaming { live_words: String },
    Working,
    Sent,
    OnClipboard,
    WordsKept,
}

pub fn resolve_recording_pill_state(
    is_recording: bool,
    is_working: bool,
    live_words: Option<&str>,
    repo_chip: Option<&str>,
    typing_off: bool,
    paste_failed: bool,
) -> RecordingPillState {
    if is_recording {
        if let Some(words) = live_words.map(str::trim).filter(|w| !w.is_empty()) {
            return RecordingPillState::Streaming {
                live_words: words.to_string(),
            };
        }
        return RecordingPillState::Listening {
            repo_chip: repo_chip
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_string),
        };
    }
    if is_working {
        return RecordingPillState::Working;
    }
    if paste_failed {
        return RecordingPillState::WordsKept;
    }
    if typing_off {
        return RecordingPillState::OnClipboard;
    }
    RecordingPillState::Sent
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeHealthNotice {
    ShortcutNotSetUp,
    AccessibilityBrokeAfterUpdate,
    TypingIsOff,
    UpdateReady { version: String },
}

pub fn evaluate_home_health_notice(
    shortcut_configured: bool,
    accessibility_trusted: bool,
    typing_off: bool,
    update_ready_version: Option<&str>,
) -> Option<HomeHealthNotice> {
    if !shortcut_configured {
        return Some(HomeHealthNotice::ShortcutNotSetUp);
    }
    if !accessibility_trusted && !typing_off {
        return Some(HomeHealthNotice::AccessibilityBrokeAfterUpdate);
    }
    if typing_off {
        return Some(HomeHealthNotice::TypingIsOff);
    }
    update_ready_version
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| HomeHealthNotice::UpdateReady {
            version: v.to_string(),
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AccessibilityOnboardingState {
    Granted,
    StaleEntry { steps: [&'static str; 3] },
    NotGranted { error: Option<String> },
}

pub const STALE_ACCESSIBILITY_STEPS: [&str; 3] = [
    "1. In the Accessibility list, select Coco Voice and click minus (−).",
    "2. Click plus (+), choose Applications → Coco Voice, switch it on.",
    "3. Come back here. We check again by ourselves.",
];

/// Fail-closed Accessibility check combined with stale-entry input probe.
/// Grants only when `trusted == Some(true)`, `probe_succeeded == Some(true)`, and
/// `check_error.is_none()`. When `trusted == Some(true)` but `probe_succeeded == Some(false)`,
/// reports `StaleEntry`. Any error or unknown value fails closed (`NotGranted`).
pub fn evaluate_accessibility_status(
    trusted: Option<bool>,
    probe_succeeded: Option<bool>,
    check_error: Option<&str>,
) -> AccessibilityOnboardingState {
    if let Some(err) = check_error.filter(|s| !s.trim().is_empty()) {
        return AccessibilityOnboardingState::NotGranted {
            error: Some(err.to_string()),
        };
    }
    match (trusted, probe_succeeded) {
        (Some(true), Some(true)) => AccessibilityOnboardingState::Granted,
        (Some(true), Some(false)) => AccessibilityOnboardingState::StaleEntry {
            steps: STALE_ACCESSIBILITY_STEPS,
        },
        _ => AccessibilityOnboardingState::NotGranted { error: None },
    }
}

/// Fail-closed microphone permission check: grants only on a definite `Some(true)` with no error.
pub fn evaluate_microphone_permission(granted: Option<bool>, check_error: Option<&str>) -> bool {
    check_error.is_none() && granted == Some(true)
}

/// Switches output to clipboard-only and completes onboarding so the user is never stuck
/// on the Accessibility screen.
pub fn apply_continue_without_typing(settings: &mut AppSettings) {
    settings.paste_method = PasteMethod::None;
    settings.clipboard_handling = ClipboardHandling::CopyToClipboard;
    settings.clipboard_handling_user_set = true;
    settings.onboarding_completed = true;
}

/// Evaluates the onboarding "Try it" self-correction sample phrase.
pub fn evaluate_try_it_sample(raw_spoken: &str, clean_up_enabled: bool) -> String {
    if !clean_up_enabled {
        return raw_spoken.trim().to_string();
    }
    let lower = raw_spoken.to_ascii_lowercase();
    for cue in [
        "scratch that,",
        "scratch that",
        "i mean,",
        "i mean",
        "no wait,",
        "no wait",
    ] {
        if let Some((_, after)) = lower.rsplit_once(cue) {
            let tail_len = after.trim().len();
            if tail_len > 0 && tail_len <= raw_spoken.len() {
                let start = raw_spoken.len() - after.len();
                let cleaned = raw_spoken[start..]
                    .trim()
                    .trim_matches(|c: char| c == ',' || c.is_whitespace());
                let mut chars = cleaned.chars();
                if let Some(first) = chars.next() {
                    return first.to_uppercase().collect::<String>() + chars.as_str();
                }
            }
        }
    }
    raw_spoken.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    #[test]
    fn pill_states_and_home_health_notices_follow_priority_order() {
        assert_eq!(
            resolve_recording_pill_state(true, false, None, Some("coco-voice"), false, false),
            RecordingPillState::Listening {
                repo_chip: Some("coco-voice".to_string())
            }
        );
        assert_eq!(
            resolve_recording_pill_state(true, false, Some("hello world"), None, false, false),
            RecordingPillState::Streaming {
                live_words: "hello world".to_string()
            }
        );
        assert_eq!(
            resolve_recording_pill_state(false, true, None, None, false, false),
            RecordingPillState::Working
        );
        assert_eq!(
            resolve_recording_pill_state(false, false, None, None, false, true),
            RecordingPillState::WordsKept
        );
        assert_eq!(
            resolve_recording_pill_state(false, false, None, None, true, false),
            RecordingPillState::OnClipboard
        );
        assert_eq!(
            resolve_recording_pill_state(false, false, None, None, false, false),
            RecordingPillState::Sent
        );

        assert_eq!(
            evaluate_home_health_notice(false, false, true, Some("0.20.0")),
            Some(HomeHealthNotice::ShortcutNotSetUp)
        );
        assert_eq!(
            evaluate_home_health_notice(true, false, false, Some("0.20.0")),
            Some(HomeHealthNotice::AccessibilityBrokeAfterUpdate)
        );
        assert_eq!(
            evaluate_home_health_notice(true, false, true, Some("0.20.0")),
            Some(HomeHealthNotice::TypingIsOff)
        );
        assert_eq!(
            evaluate_home_health_notice(true, true, false, Some("0.20.0")),
            Some(HomeHealthNotice::UpdateReady {
                version: "0.20.0".to_string()
            })
        );
        assert_eq!(evaluate_home_health_notice(true, true, false, None), None);
    }

    #[test]
    fn first_run_checks_fail_closed_detect_stale_accessibility_and_support_continue_and_try_it() {
        assert!(!evaluate_microphone_permission(None, None));
        assert!(!evaluate_microphone_permission(
            Some(true),
            Some("registry read failed")
        ));
        assert!(evaluate_microphone_permission(Some(true), None));

        assert_eq!(
            evaluate_accessibility_status(None, Some(true), None),
            AccessibilityOnboardingState::NotGranted { error: None }
        );
        assert_eq!(
            evaluate_accessibility_status(Some(true), Some(true), Some("probe ipc error")),
            AccessibilityOnboardingState::NotGranted {
                error: Some("probe ipc error".into())
            }
        );
        assert_eq!(
            evaluate_accessibility_status(Some(true), Some(false), None),
            AccessibilityOnboardingState::StaleEntry {
                steps: STALE_ACCESSIBILITY_STEPS
            }
        );
        assert_eq!(
            evaluate_accessibility_status(Some(true), Some(true), None),
            AccessibilityOnboardingState::Granted
        );

        let mut settings = get_default_settings();
        assert!(!settings.onboarding_completed);
        apply_continue_without_typing(&mut settings);
        assert!(settings.onboarding_completed);
        assert_eq!(settings.paste_method, PasteMethod::None);
        assert_eq!(
            settings.clipboard_handling,
            ClipboardHandling::CopyToClipboard
        );

        assert_eq!(
            evaluate_try_it_sample("Monday, scratch that, Tuesday", true),
            "Tuesday"
        );
    }
}
