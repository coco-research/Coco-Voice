//! Local-only opt-in focused text-box formatting (`V-D10`: `D3`, `D16`, `D30`, `D49`, `D50`).
//!
//! Disabled by default (`read_focused_text_enabled = false`). When disabled, no
//! Accessibility value attribute is ever queried. When enabled by the user:
//! - Refuses `AXSecureTextField` / password fields and `NeverDictate` blocked apps.
//! - Bounds any focused-value slice to `MAX_FOCUSED_CHARS` (`4096`) around the caret.
//! - Never logs, persists (`history.db`), or forwards focused-box text to any
//!   cloud or LLM prompt — formatting runs 100% deterministically in local Rust.
// ponytail: ceiling = 4096-char AXValue window + local token scan on macOS; upgrade path = AXTextMarkerRange slice around insertion point when WebKit/Electron expose markers.

use crate::settings::AppSettings;
use std::sync::atomic::{AtomicUsize, Ordering};
use unicode_segmentation::UnicodeSegmentation;

pub const MAX_FOCUSED_CHARS: usize = 4096;
const MAX_WINDOW_TITLE_CHARS: usize = 256;

/// Counts Accessibility read attempts in tests so `read_focused_text_enabled == false`
/// and `NeverDictate` blocked apps are proven to make zero AX value reads.
static AX_READ_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub fn ax_read_attempts() -> usize {
    AX_READ_ATTEMPTS.load(Ordering::SeqCst)
}

#[cfg(test)]
pub fn reset_ax_read_attempts() {
    AX_READ_ATTEMPTS.store(0, Ordering::SeqCst);
}

/// Ephemeral in-memory snapshot of the focused input box before the caret.
/// Intentionally omits `Debug`, `Display`, `Serialize`, and `Clone` of raw box
/// contents into any persistent store or log.
pub struct FocusedBoxSnapshot {
    pub before_cursor: String,
    pub window_tokens: Vec<String>,
}

/// Deterministic spoken edit applied against text already before the caret (`D16`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpokenBoxEdit {
    /// Number of graphemes immediately before the caret to backspace.
    pub replace_char_count: usize,
    /// Replacement text (from the start of the matched target through the prior caret).
    pub replacement_text: String,
}

/// Returns `true` when the Accessibility role or subrole marks a password/secure field.
pub fn is_secure_field_role(role: Option<&str>, subrole: Option<&str>) -> bool {
    for raw in [role, subrole].into_iter().flatten() {
        let lower = raw.to_ascii_lowercase();
        if lower.contains("securetextfield") || lower.contains("password") {
            return true;
        }
    }
    false
}

/// Bounds `full_value` to at most `MAX_FOCUSED_CHARS` Unicode scalars immediately
/// preceding `cursor_utf16_offset` (or the tail of `full_value` when no caret
/// range is reported by the target app).
pub fn bounded_before_cursor(full_value: &str, cursor_utf16_offset: Option<usize>) -> String {
    let prefix = match cursor_utf16_offset {
        Some(target_utf16) => {
            let mut seen_utf16 = 0usize;
            let mut byte_end = full_value.len();
            for (idx, ch) in full_value.char_indices() {
                if seen_utf16 >= target_utf16 {
                    byte_end = idx;
                    break;
                }
                seen_utf16 += ch.len_utf16();
            }
            &full_value[..byte_end]
        }
        None => full_value,
    };

    let char_count = prefix.chars().count();
    if char_count <= MAX_FOCUSED_CHARS {
        return prefix.to_string();
    }
    let skip = char_count - MAX_FOCUSED_CHARS;
    prefix.chars().skip(skip).collect()
}

/// Extracts mixed-case / identifier vocabulary tokens from a visible window title (`D50`).
pub fn extract_window_vocab_tokens(title: &str) -> Vec<String> {
    let bounded: String = title.chars().take(MAX_WINDOW_TITLE_CHARS).collect();
    let mut out = Vec::new();
    for raw in bounded.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        let tok = raw.trim_matches(|c: char| c == '_' || c == '-');
        if tok.chars().count() < 3 {
            continue;
        }
        let has_upper = tok.chars().any(|c| c.is_uppercase());
        let has_sep = tok.contains('_') || tok.contains('-');
        let internal_upper = tok.chars().skip(1).any(|c| c.is_uppercase());
        let all_upper = tok.chars().all(|c| !c.is_alphabetic() || c.is_uppercase());
        if (has_upper && (internal_upper || all_upper || tok.chars().count() >= 4)) || has_sep {
            if !out
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(tok))
            {
                out.push(tok.to_string());
            }
        }
    }
    out
}

/// Preserves exact identifier / proper-noun casing from `window_tokens` in `take` (`D50`).
pub fn apply_window_vocab_hint(window_tokens: &[String], take: &str) -> String {
    if window_tokens.is_empty() || take.is_empty() {
        return take.to_string();
    }
    let mut result = String::with_capacity(take.len());
    let mut idx = 0;
    let bytes = take.as_bytes();
    while idx < bytes.len() {
        let ch = take[idx..].chars().next().unwrap();
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            let start = idx;
            idx += ch.len_utf8();
            while idx < bytes.len() {
                let next_ch = take[idx..].chars().next().unwrap();
                if next_ch.is_alphanumeric() || next_ch == '_' || next_ch == '-' {
                    idx += next_ch.len_utf8();
                } else {
                    break;
                }
            }
            let word = &take[start..idx];
            if let Some(matched) = window_tokens.iter().find(|t| t.eq_ignore_ascii_case(word)) {
                result.push_str(matched);
            } else {
                result.push_str(word);
            }
        } else {
            result.push(ch);
            idx += ch.len_utf8();
        }
    }
    result
}

fn parse_spoken_replace_command(take: &str) -> Option<(&str, &str)> {
    let trimmed = take.trim().trim_end_matches(['.', '!']);
    let lower = trimmed.to_lowercase();
    for (prefix, sep) in [("replace ", " with "), ("change ", " to ")] {
        if lower.starts_with(prefix) {
            let rest_orig = &trimmed[prefix.len()..];
            let rest_lower = &lower[prefix.len()..];
            if let Some(sep_idx) = rest_lower.find(sep) {
                let target = rest_orig[..sep_idx]
                    .trim()
                    .trim_matches(['"', '\'', '“', '”']);
                let replacement = rest_orig[sep_idx + sep.len()..]
                    .trim()
                    .trim_matches(['"', '\'', '“', '”']);
                if !target.is_empty() {
                    return Some((target, replacement));
                }
            }
        }
    }
    None
}

fn find_last_case_insensitive_range(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let needle_chars: Vec<char> = needle.chars().collect();
    let hay_indices: Vec<(usize, char)> = haystack.char_indices().collect();
    if needle_chars.len() > hay_indices.len() {
        return None;
    }
    let mut best: Option<(usize, usize)> = None;
    for start_pos in 0..=(hay_indices.len() - needle_chars.len()) {
        let matches = needle_chars
            .iter()
            .enumerate()
            .all(|(offset, nc)| hay_indices[start_pos + offset].1.eq_ignore_ascii_case(nc));
        if matches {
            let byte_start = hay_indices[start_pos].0;
            let end_pos = start_pos + needle_chars.len();
            let byte_end = if end_pos < hay_indices.len() {
                hay_indices[end_pos].0
            } else {
                haystack.len()
            };
            best = Some((byte_start, byte_end));
        }
    }
    best
}

/// Applies deterministic spoken edits (`"replace <old> with <new>"` / `"change <old> to <new>"`)
/// against text already before the caret (`D16`).
pub fn apply_spoken_box_edit(before_cursor: &str, take: &str) -> Option<SpokenBoxEdit> {
    if before_cursor.is_empty() {
        return None;
    }
    let bounded = bounded_before_cursor(before_cursor, None);
    let (target, replacement) = parse_spoken_replace_command(take)?;
    let (match_start, match_end) = find_last_case_insensitive_range(&bounded, target)?;
    let suffix_from_match = &bounded[match_start..];
    let graphemes_to_replace = suffix_from_match.graphemes(true).count();
    let clamped = crate::utils::backspaces_for_replace(graphemes_to_replace);
    if clamped == 0 {
        return None;
    }
    let mut replacement_text =
        String::with_capacity(replacement.len() + (bounded.len() - match_end));
    replacement_text.push_str(replacement);
    replacement_text.push_str(&bounded[match_end..]);
    Some(SpokenBoxEdit {
        replace_char_count: clamped,
        replacement_text,
    })
}

fn should_preserve_initial_uppercase(first_word: &str, protected_words: &[String]) -> bool {
    let clean = first_word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’');
    if clean.is_empty() {
        return true;
    }
    if clean == "I" || clean.starts_with("I'") || clean.starts_with("I’") {
        return true;
    }
    let alpha_chars: Vec<char> = clean.chars().filter(|c| c.is_alphabetic()).collect();
    if alpha_chars.len() >= 2 && alpha_chars.iter().all(|c| c.is_uppercase()) {
        return true;
    }
    if clean.chars().skip(1).any(|c| c.is_uppercase()) {
        return true;
    }
    protected_words
        .iter()
        .any(|w| w.trim().eq_ignore_ascii_case(clean))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CasingContext {
    SentenceStart,
    MidSentence,
}

fn detect_casing_context(before_cursor: &str) -> CasingContext {
    if before_cursor.is_empty() {
        return CasingContext::SentenceStart;
    }
    if before_cursor.ends_with('\n') || before_cursor.ends_with('\r') {
        return CasingContext::SentenceStart;
    }
    let trimmed = before_cursor.trim_end_matches([' ', '\t', '"', '\'', '”', '’', ')']);
    if trimmed.is_empty() || trimmed.ends_with('\n') || trimmed.ends_with('\r') {
        return CasingContext::SentenceStart;
    }
    if let Some(last) = trimmed.chars().last() {
        if matches!(last, '.' | '?' | '!') {
            return CasingContext::SentenceStart;
        }
    }
    CasingContext::MidSentence
}

fn needs_leading_space(before_cursor: &str, take: &str) -> bool {
    let Some(prev_char) = before_cursor.chars().last() else {
        return false;
    };
    if prev_char.is_whitespace() || matches!(prev_char, '(' | '[' | '{' | '"' | '“' | '/' | '@') {
        return false;
    }
    let Some(first_take) = take.chars().next() else {
        return false;
    };
    !matches!(
        first_take,
        ',' | '.' | '?' | '!' | ';' | ':' | ')' | ']' | '}'
    )
}

/// Formats `take` to fit `before_cursor` spacing and sentence capitalization (`D3`, `D30`, `D49`).
pub fn format_take_for_focused_box(
    before_cursor: &str,
    take: &str,
    protected_words: &[String],
) -> String {
    let stripped = take.trim_start_matches(' ');
    if stripped.is_empty() {
        return String::new();
    }

    let context = detect_casing_context(before_cursor);
    let first_word = stripped.split_whitespace().next().unwrap_or("");
    let mut adjusted = String::with_capacity(stripped.len() + 1);
    if needs_leading_space(before_cursor, stripped) {
        adjusted.push(' ');
    }

    let mut chars = stripped.chars();
    if let Some(first_ch) = chars.next() {
        match context {
            CasingContext::SentenceStart => {
                if first_ch.is_lowercase()
                    && !first_word.chars().skip(1).any(|c| c.is_uppercase())
                    && !protected_words
                        .iter()
                        .any(|w| w.trim().eq_ignore_ascii_case(first_word))
                {
                    for upper in first_ch.to_uppercase() {
                        adjusted.push(upper);
                    }
                } else {
                    adjusted.push(first_ch);
                }
            }
            CasingContext::MidSentence => {
                if first_ch.is_uppercase()
                    && !should_preserve_initial_uppercase(first_word, protected_words)
                {
                    for lower in first_ch.to_lowercase() {
                        adjusted.push(lower);
                    }
                } else {
                    adjusted.push(first_ch);
                }
            }
        }
        adjusted.push_str(chars.as_str());
    }

    adjusted
}

/// Applies local-only focused-box formatting (`V-D10`) to a finalized dictation take,
/// returning `(formatted_text, extra_replace_char_count)`.
pub fn apply_focused_snapshot_to_take(
    snapshot: &FocusedBoxSnapshot,
    take: &str,
    window_context_enabled: bool,
    protected_words: &[String],
) -> (String, usize) {
    let vocab_adjusted = if window_context_enabled && !snapshot.window_tokens.is_empty() {
        apply_window_vocab_hint(&snapshot.window_tokens, take)
    } else {
        take.to_string()
    };

    if let Some(edit) = apply_spoken_box_edit(&snapshot.before_cursor, &vocab_adjusted) {
        return (edit.replacement_text, edit.replace_char_count);
    }

    let mut protected = protected_words.to_vec();
    if window_context_enabled {
        for tok in &snapshot.window_tokens {
            if !protected.iter().any(|w| w.eq_ignore_ascii_case(tok)) {
                protected.push(tok.clone());
            }
        }
    }

    let formatted =
        format_take_for_focused_box(&snapshot.before_cursor, &vocab_adjusted, &protected);
    (formatted, 0)
}

/// Reads the focused input box on macOS ONLY when `settings.read_focused_text_enabled`
/// is `true` and the active app is not blocked by `NeverDictate`.
pub fn read_focused_box_snapshot(settings: &AppSettings) -> Option<FocusedBoxSnapshot> {
    if !settings.read_focused_text_enabled {
        return None;
    }
    if crate::app_profiles::check_active_app_start(settings).is_blocked() {
        return None;
    }
    AX_READ_ATTEMPTS.fetch_add(1, Ordering::SeqCst);

    #[cfg(target_os = "macos")]
    {
        macos::capture_snapshot(settings.read_focused_window_context_enabled)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{
        bounded_before_cursor, extract_window_vocab_tokens, is_secure_field_role,
        FocusedBoxSnapshot,
    };
    use std::ffi::{c_char, c_void, CStr, CString};

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CfRange {
        location: isize,
        length: isize,
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFStringGetLength(the_string: *const c_void) -> isize;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
        fn CFStringGetCString(
            the_string: *const c_void,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXUIElementCreateSystemWide() -> *mut c_void;
        fn AXUIElementSetMessagingTimeout(element: *mut c_void, timeout_in_seconds: f32) -> i32;
        fn AXUIElementCopyAttributeValue(
            element: *mut c_void,
            attribute: *const c_void,
            value: *mut *const c_void,
        ) -> i32;
        fn AXValueGetValue(value: *const c_void, the_type: u32, value_ptr: *mut c_void) -> u8;
    }

    const UTF8: u32 = 0x0800_0100;
    const AX_SUCCESS: i32 = 0;
    const AX_VALUE_CF_RANGE_TYPE: u32 = 4;

    struct CfRef(*const c_void);

    impl Drop for CfRef {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn make_cf_string(s: &str) -> Option<CfRef> {
        let c = CString::new(s).ok()?;
        let ptr = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) };
        if ptr.is_null() {
            None
        } else {
            Some(CfRef(ptr))
        }
    }

    unsafe fn cf_to_string(cf: *const c_void) -> Option<String> {
        if cf.is_null() {
            return None;
        }
        if unsafe { CFGetTypeID(cf) != CFStringGetTypeID() } {
            return None;
        }
        let len = unsafe { CFStringGetLength(cf) };
        if len < 0 {
            return None;
        }
        let cap = unsafe { CFStringGetMaximumSizeForEncoding(len, UTF8) }
            .saturating_add(1)
            .clamp(1, 262_144);
        let mut buf = vec![0 as c_char; cap as usize];
        let ok = unsafe { CFStringGetCString(cf, buf.as_mut_ptr(), cap, UTF8) };
        if ok == 0 {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(buf.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    unsafe fn copy_attr(element: *mut c_void, attr_name: &str) -> Option<CfRef> {
        let attr = make_cf_string(attr_name)?;
        let mut out: *const c_void = std::ptr::null();
        let status = unsafe { AXUIElementCopyAttributeValue(element, attr.0, &mut out) };
        if status == AX_SUCCESS && !out.is_null() {
            Some(CfRef(out))
        } else {
            None
        }
    }

    pub fn capture_snapshot(include_window_context: bool) -> Option<FocusedBoxSnapshot> {
        if unsafe { AXIsProcessTrusted() == 0 } {
            return None;
        }
        let system_wide_ptr = unsafe { AXUIElementCreateSystemWide() };
        if system_wide_ptr.is_null() {
            return None;
        }
        let _system_wide = CfRef(system_wide_ptr as *const c_void);
        unsafe {
            let _ = AXUIElementSetMessagingTimeout(system_wide_ptr, 0.2);
        }

        let focused = unsafe { copy_attr(system_wide_ptr, "AXFocusedUIElement") }?;
        let focused_elem = focused.0 as *mut c_void;
        unsafe {
            let _ = AXUIElementSetMessagingTimeout(focused_elem, 0.2);
        }

        let role = unsafe { copy_attr(focused_elem, "AXRole").and_then(|v| cf_to_string(v.0)) };
        let subrole =
            unsafe { copy_attr(focused_elem, "AXSubrole").and_then(|v| cf_to_string(v.0)) };
        if is_secure_field_role(role.as_deref(), subrole.as_deref()) {
            return None;
        }

        let value_cf = unsafe { copy_attr(focused_elem, "AXValue") }?;
        let full_value = unsafe { cf_to_string(value_cf.0) }?;

        let cursor_utf16 = unsafe {
            copy_attr(focused_elem, "AXSelectedTextRange").and_then(|range_cf| {
                let mut range = CfRange::default();
                let ok = AXValueGetValue(
                    range_cf.0,
                    AX_VALUE_CF_RANGE_TYPE,
                    (&mut range as *mut CfRange).cast::<c_void>(),
                );
                if ok != 0 && range.location >= 0 {
                    Some(range.location as usize)
                } else {
                    None
                }
            })
        };

        let before_cursor = bounded_before_cursor(&full_value, cursor_utf16);

        let window_tokens = if include_window_context {
            unsafe {
                copy_attr(system_wide_ptr, "AXFocusedWindow")
                    .or_else(|| copy_attr(focused_elem, "AXWindow"))
                    .and_then(|win| copy_attr(win.0 as *mut c_void, "AXTitle"))
                    .and_then(|title_cf| cf_to_string(title_cf.0))
                    .map(|t| extract_window_vocab_tokens(&t))
                    .unwrap_or_default()
            }
        } else {
            Vec::new()
        };

        Some(FocusedBoxSnapshot {
            before_cursor,
            window_tokens,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    #[test]
    fn disabled_by_default_makes_zero_ax_reads() {
        reset_ax_read_attempts();
        let defaults = get_default_settings();
        assert!(!defaults.read_focused_text_enabled);
        assert!(!defaults.read_focused_window_context_enabled);
        assert!(read_focused_box_snapshot(&defaults).is_none());
        assert_eq!(ax_read_attempts(), 0);
    }

    #[test]
    fn refuses_secure_password_roles() {
        assert!(is_secure_field_role(Some("AXSecureTextField"), None));
        assert!(is_secure_field_role(
            Some("AXTextField"),
            Some("AXSecureTextField")
        ));
        assert!(is_secure_field_role(Some("AXPasswordField"), None));
        assert!(!is_secure_field_role(Some("AXTextArea"), None));
    }

    #[test]
    fn bounds_before_cursor_to_max_focused_chars_and_utf16_offset() {
        let long = "a".repeat(MAX_FOCUSED_CHARS + 500);
        let bounded = bounded_before_cursor(&long, None);
        assert_eq!(bounded.chars().count(), MAX_FOCUSED_CHARS);

        // Emoji '😀' takes 2 UTF-16 code units; "Hi 😀 world" cursor at 5 ("Hi 😀")
        let with_emoji = "Hi 😀 world";
        assert_eq!(bounded_before_cursor(with_emoji, Some(5)), "Hi 😀");
    }

    #[test]
    fn formats_spacing_and_casing_for_focused_box_d3_d30_d49() {
        // Empty field -> capitalize, no leading space
        assert_eq!(
            format_take_for_focused_box("", "hello there.", &[]),
            "Hello there."
        );
        // After sentence period with no trailing space -> add leading space + keep capitalized
        assert_eq!(
            format_take_for_focused_box("First sentence.", "Next sentence.", &[]),
            " Next sentence."
        );
        // After comma + space -> no double space, lowercase continuation
        assert_eq!(
            format_take_for_focused_box("Thanks, ", "Let me check that.", &[]),
            "let me check that."
        );
        // Mid-sentence without trailing space -> add leading space + lowercase unless "I" / acronym / custom word
        assert_eq!(
            format_take_for_focused_box("We decided that", "This stays local.", &[]),
            " this stays local."
        );
        assert_eq!(
            format_take_for_focused_box("Thanks, ", "I'll send it.", &[]),
            "I'll send it."
        );
        assert_eq!(
            format_take_for_focused_box("Check the", "API response", &[]),
            " API response"
        );
        assert_eq!(
            format_take_for_focused_box("Ask", "Parakeet now", &["Parakeet".to_string()]),
            " Parakeet now"
        );
    }

    #[test]
    fn applies_spoken_box_edits_locally_d16() {
        let edit = apply_spoken_box_edit(
            "Ship on Tuesday afternoon",
            "replace Tuesday with Thursday.",
        )
        .expect("matched spoken replace");
        assert_eq!(edit.replacement_text, "Thursday afternoon");
        assert_eq!(edit.replace_char_count, "Tuesday afternoon".chars().count());

        let change = apply_spoken_box_edit("Status: draft", "change draft to ready")
            .expect("matched spoken change");
        assert_eq!(change.replacement_text, "ready");
        assert_eq!(change.replace_char_count, 5);
    }

    #[test]
    fn applies_visible_window_vocab_hints_locally_d50() {
        let tokens = extract_window_vocab_tokens("AXUIElement.rs — CocoVoice — PullRequest");
        assert!(tokens.contains(&"AXUIElement".to_string()));
        assert!(tokens.contains(&"CocoVoice".to_string()));
        let hinted = apply_window_vocab_hint(&tokens, "check axuielement in cocovoice");
        assert_eq!(hinted, "check AXUIElement in CocoVoice");
    }
}
