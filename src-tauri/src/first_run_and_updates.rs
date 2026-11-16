//! First-run onboarding checks (`V-R2`: fail-closed permission gates #248, stale
//! Accessibility entry detection #255, continue-without-typing, Try-it sample) and
//! post-update notices (`V-R5`: What's new once per upgrade + Went back on rollback).
// ponytail: ceiling = pure state machines + settings helpers; upgrade path = native macOS TCC event listener.

use crate::settings::{AppSettings, ClipboardHandling, PasteMethod};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Fail-closed Accessibility check (#248) combined with stale-entry input probe (#255).
/// Grants ONLY when `trusted == Some(true)`, `probe_succeeded == Some(true)`, and
/// `check_error.is_none()`. When `trusted == Some(true)` but `probe_succeeded == Some(false)`,
/// reports `StaleEntry` (B22). Any error or unknown value fails closed (`NotGranted`).
pub fn evaluate_accessibility_status(
    trusted: Option<bool>,
    probe_succeeded: Option<bool>,
    check_error: Option<&str>,
) -> AccessibilityOnboardingState {
    if let Some(err) = check_error.filter(|s| !s.trim().is_empty()) {
        return AccessibilityOnboardingState::NotGranted { error: Some(err.to_string()) };
    }
    match (trusted, probe_succeeded) {
        (Some(true), Some(true)) => AccessibilityOnboardingState::Granted,
        (Some(true), Some(false)) => AccessibilityOnboardingState::StaleEntry { steps: STALE_ACCESSIBILITY_STEPS },
        _ => AccessibilityOnboardingState::NotGranted { error: None },
    }
}

/// Fail-closed microphone permission check (#248): grants only on a definite `Some(true)` with no error.
pub fn evaluate_microphone_permission(granted: Option<bool>, check_error: Option<&str>) -> bool {
    check_error.is_none() && granted == Some(true)
}

/// "Continue without typing" (B19, B22): switches output to clipboard-only and completes onboarding
/// so the user is never stuck on the Accessibility screen.
pub fn apply_continue_without_typing(settings: &mut AppSettings) {
    settings.paste_method = PasteMethod::None;
    settings.clipboard_handling = ClipboardHandling::CopyToClipboard;
    settings.clipboard_handling_user_set = true;
    settings.onboarding_completed = true;
}

/// Evaluates the First Run 4 "Try it" phrase (`"Monday, scratch that, Tuesday"`, B26).
pub fn evaluate_try_it_sample(raw_spoken: &str, clean_up_enabled: bool) -> String {
    if !clean_up_enabled {
        return raw_spoken.trim().to_string();
    }
    let lower = raw_spoken.to_ascii_lowercase();
    for cue in ["scratch that,", "scratch that", "i mean,", "i mean", "no wait,", "no wait"] {
        if let Some((_, after)) = lower.rsplit_once(cue) {
            let tail_len = after.trim().len();
            if tail_len > 0 && tail_len <= raw_spoken.len() {
                let start = raw_spoken.len() - after.len();
                let cleaned = raw_spoken[start..].trim().trim_matches(|c: char| c == ',' || c.is_whitespace());
                let mut chars = cleaned.chars();
                if let Some(first) = chars.next() {
                    return first.to_uppercase().collect::<String>() + chars.as_str();
                }
            }
        }
    }
    raw_spoken.trim().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateNoticeState {
    None,
    WhatsNew {
        version: String,
        previous_version: String,
    },
    WentBack {
        rolled_back_from: String,
        current_version: String,
        message: String,
    },
}

pub fn parse_semver_triplet(version: &str) -> Option<(u64, u64, u64)> {
    let clean = version.trim().trim_start_matches('v');
    if clean.is_empty() {
        return None;
    }
    let core = clean.split(['-', '+']).next().unwrap_or(clean);
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch))
}

pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    Some(parse_semver_triplet(a)?.cmp(&parse_semver_triplet(b)?))
}

/// Evaluates whether to show "What's new" once (B27) or the "Went back" rollback notice (B28),
/// and advances `settings.whats_new_last_seen_version` to `current_version` so neither notice repeats.
pub fn evaluate_update_notice_and_advance(
    settings: &mut AppSettings,
    current_version: &str,
) -> UpdateNoticeState {
    let current = current_version.trim();
    if current.is_empty() {
        return UpdateNoticeState::None;
    }
    let stored = settings.whats_new_last_seen_version.trim().to_string();
    if stored == current {
        return UpdateNoticeState::None;
    }
    if stored.is_empty() {
        settings.whats_new_last_seen_version = current.to_string();
        if settings.show_whats_new_on_update {
            return UpdateNoticeState::WhatsNew {
                version: current.to_string(),
                previous_version: String::new(),
            };
        }
        return UpdateNoticeState::None;
    }
    match compare_versions(&stored, current) {
        Some(Ordering::Greater) => {
            settings.whats_new_last_seen_version = current.to_string();
            UpdateNoticeState::WentBack {
                rolled_back_from: stored.clone(),
                current_version: current.to_string(),
                message: format!(
                    "{stored} did not start, so Coco Voice went back to {current}. Your settings and history are unchanged."
                ),
            }
        }
        Some(Ordering::Less) => {
            settings.whats_new_last_seen_version = current.to_string();
            if settings.show_whats_new_on_update {
                UpdateNoticeState::WhatsNew {
                    version: current.to_string(),
                    previous_version: stored,
                }
            } else {
                UpdateNoticeState::None
            }
        }
        _ => UpdateNoticeState::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    #[test]
    fn first_run_checks_fail_closed_detect_stale_accessibility_and_support_continue_and_try_it() {
        // #248 fail-closed: unknown, error, or false never grants.
        assert!(!evaluate_microphone_permission(None, None));
        assert!(!evaluate_microphone_permission(Some(true), Some("registry read failed")));
        assert!(evaluate_microphone_permission(Some(true), None));

        assert_eq!(
            evaluate_accessibility_status(None, Some(true), None),
            AccessibilityOnboardingState::NotGranted { error: None }
        );
        assert_eq!(
            evaluate_accessibility_status(Some(true), Some(true), Some("probe ipc error")),
            AccessibilityOnboardingState::NotGranted { error: Some("probe ipc error".into()) }
        );

        // #255 stale Accessibility entry (B22): macOS says trusted, but input simulator probe fails.
        assert_eq!(
            evaluate_accessibility_status(Some(true), Some(false), None),
            AccessibilityOnboardingState::StaleEntry { steps: STALE_ACCESSIBILITY_STEPS }
        );
        assert_eq!(
            evaluate_accessibility_status(Some(true), Some(true), None),
            AccessibilityOnboardingState::Granted
        );

        // Continue without typing (B19, B22): switches to clipboard-only and completes onboarding.
        let mut settings = get_default_settings();
        assert!(!settings.onboarding_completed);
        apply_continue_without_typing(&mut settings);
        assert!(settings.onboarding_completed);
        assert_eq!(settings.paste_method, PasteMethod::None);
        assert_eq!(settings.clipboard_handling, ClipboardHandling::CopyToClipboard);

        // Try it (B26): clean-up drops "Monday, scratch that" and leaves "Tuesday".
        assert_eq!(evaluate_try_it_sample("Monday, scratch that, Tuesday", true), "Tuesday");
    }

    #[test]
    fn update_notice_shows_whats_new_once_and_detects_version_rollback() {
        let mut settings = get_default_settings();
        settings.show_whats_new_on_update = true;
        settings.whats_new_last_seen_version = "0.10.1".to_string();

        // Upgrade 0.10.1 -> 0.11.0 shows What's new once (B27), then None on second check.
        assert_eq!(
            evaluate_update_notice_and_advance(&mut settings, "0.11.0"),
            UpdateNoticeState::WhatsNew {
                version: "0.11.0".to_string(),
                previous_version: "0.10.1".to_string(),
            }
        );
        assert_eq!(settings.whats_new_last_seen_version, "0.11.0");
        assert_eq!(evaluate_update_notice_and_advance(&mut settings, "0.11.0"), UpdateNoticeState::None);

        // Rollback 0.11.0 -> 0.10.1 shows WentBack once (B28) and records 0.10.1.
        assert_eq!(
            evaluate_update_notice_and_advance(&mut settings, "0.10.1"),
            UpdateNoticeState::WentBack {
                rolled_back_from: "0.11.0".to_string(),
                current_version: "0.10.1".to_string(),
                message: "0.11.0 did not start, so Coco Voice went back to 0.10.1. Your settings and history are unchanged.".to_string(),
            }
        );
        assert_eq!(settings.whats_new_last_seen_version, "0.10.1");
        assert_eq!(evaluate_update_notice_and_advance(&mut settings, "0.10.1"), UpdateNoticeState::None);
    }
}
