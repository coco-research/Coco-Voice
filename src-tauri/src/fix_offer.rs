//! Post-paste spelling-fix offer watcher (`P-c`: `AC-P25..P27`).
//!
//! Security & privacy invariants:
//! - Strictly in-memory only: offers and dismissed pairs are NEVER written to
//!   disk, and raw words are NEVER logged even at `debug!`.
//! - Re-checks `offer_remember_fix_enabled`, `never_store_history == false`
//!   (stands down completely while Never Store History is active),
//!   `!is_secure_field_role`, and `!dictation_blocked` on EVERY tick
//!   (`+3s`, `+8s`, `+15s`, `+30s`, and on next shortcut).
//! - Uses a `Send` RAII `RetainedAxElement` (`CFRetain` / `CFRelease`) on macOS
//!   with raw ApplicationServices/CoreFoundation FFI (no `objc2` dependency).

use crate::audio_toolkit::text::suggest_spelling_corrections;
use crate::focused_text::is_secure_field_role;
use crate::settings::CorrectionPair;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub const FIX_OFFER_POLL_DELAYS: &[Duration] = &[
    Duration::from_secs(3),
    Duration::from_secs(8),
    Duration::from_secs(15),
    Duration::from_secs(30),
];
pub const MAX_PENDING_FIX_OFFERS: usize = 3;
pub const FIX_OFFER_TTL_MS: u64 = 24 * 60 * 60 * 1000;
pub const SPAN_SEARCH_MARGIN_CHARS: usize = 200;
pub const MIN_SPAN_SIMILARITY: f64 = 0.5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FixOfferPayload {
    pub id: String,
    pub old: String,
    pub new: String,
    pub app: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixOfferEntry {
    pub payload: FixOfferPayload,
    pub created_at_ms: u64,
}

/// Runtime guard flags re-evaluated before EVERY AX read tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixOfferGate {
    pub offer_remember_fix_enabled: bool,
    pub never_store_history: bool,
    pub dictation_blocked: bool,
}

impl FixOfferGate {
    /// Returns `true` ONLY when watching is allowed (switch on, history not in
    /// Never mode, and app/site not blocked).
    pub fn allows_watch(self) -> bool {
        self.offer_remember_fix_enabled && !self.never_store_history && !self.dictation_blocked
    }
}

/// Snapshot read from a focused input element on each poll tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedFieldReading {
    pub value: String,
    pub role: Option<String>,
    pub subrole: Option<String>,
}

/// Abstraction over the focused AX element so all watch/diff/privacy logic is
/// deterministically unit-tested without live GUI apps.
pub trait FocusedFieldSource: Send {
    fn read_field(&self) -> Option<FocusedFieldReading>;
}

/// Generation counter cancelling any prior paste's watcher immediately when a
/// new paste starts or the app quits.
pub struct WatcherCancellation {
    generation: AtomicU64,
}

impl WatcherCancellation {
    pub const fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
        }
    }

    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn cancel_all(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn is_current(&self, gen: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == gen
    }
}

/// In-memory queue of waiting fix offers (max 3, newest first, 24 h TTL,
/// dismissed pairs suppressed until app restart).
#[derive(Default)]
pub struct FixOfferQueue {
    offers: VecDeque<FixOfferEntry>,
    dismissed_pairs: HashSet<(String, String)>,
    next_id: u64,
}

impl FixOfferQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn prune_expired(&mut self, now_ms: u64) {
        self.offers
            .retain(|entry| now_ms.saturating_sub(entry.created_at_ms) < FIX_OFFER_TTL_MS);
    }

    pub fn current(&mut self, now_ms: u64) -> Option<FixOfferPayload> {
        self.prune_expired(now_ms);
        self.offers.front().map(|e| e.payload.clone())
    }

    pub fn push_offer(
        &mut self,
        old: String,
        new: String,
        app: String,
        now_ms: u64,
    ) -> Option<FixOfferPayload> {
        self.prune_expired(now_ms);
        let key = (old.to_ascii_lowercase(), new.to_ascii_lowercase());
        if self.dismissed_pairs.contains(&key) {
            return None;
        }
        if self.offers.iter().any(|e| {
            e.payload.old.eq_ignore_ascii_case(&old) && e.payload.new.eq_ignore_ascii_case(&new)
        }) {
            return None;
        }
        self.next_id += 1;
        let payload = FixOfferPayload {
            id: format!("fix-{}", self.next_id),
            old,
            new,
            app,
        };
        self.offers.push_front(FixOfferEntry {
            payload: payload.clone(),
            created_at_ms: now_ms,
        });
        while self.offers.len() > MAX_PENDING_FIX_OFFERS {
            self.offers.pop_back();
        }
        Some(payload)
    }

    /// Handles "Not now": removes the offer and suppresses that `(old, new)` pair
    /// until Coco Voice restarts.
    pub fn dismiss(&mut self, id: &str, now_ms: u64) -> Option<FixOfferPayload> {
        self.prune_expired(now_ms);
        if let Some(pos) = self.offers.iter().position(|e| e.payload.id == id) {
            if let Some(removed) = self.offers.remove(pos) {
                self.dismissed_pairs.insert((
                    removed.payload.old.to_ascii_lowercase(),
                    removed.payload.new.to_ascii_lowercase(),
                ));
            }
        }
        self.current(now_ms)
    }

    /// Removes the offer after the user taps "Remember".
    pub fn accept(&mut self, id: &str, now_ms: u64) -> Option<FixOfferPayload> {
        self.prune_expired(now_ms);
        if let Some(pos) = self.offers.iter().position(|e| e.payload.id == id) {
            self.offers.remove(pos);
        }
        self.current(now_ms)
    }

    pub fn len(&self) -> usize {
        self.offers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offers.is_empty()
    }
}

/// Adds both the correction pair (`wrong -> right`) and `right` as a custom word
/// (case-insensitively deduplicated), matching `AC-P27` / `I11`.
pub fn remember_spelling_fix_into_settings(
    custom_words: &mut Vec<String>,
    corrections: &mut Vec<CorrectionPair>,
    wrong: &str,
    right: &str,
) -> bool {
    let w = wrong.trim();
    let r = right.trim();
    if w.is_empty() || r.is_empty() || w.eq_ignore_ascii_case(r) {
        return false;
    }
    let added_pair = if let Some(existing) = corrections
        .iter_mut()
        .find(|p| p.from.eq_ignore_ascii_case(w))
    {
        if existing.to.eq_ignore_ascii_case(r) {
            false
        } else {
            existing.to = r.to_string();
            true
        }
    } else {
        corrections.push(CorrectionPair {
            from: w.to_string(),
            to: r.to_string(),
        });
        true
    };
    let mut added_word = false;
    if !custom_words
        .iter()
        .any(|existing| existing.trim().eq_ignore_ascii_case(r))
    {
        custom_words.push(r.to_string());
        added_word = true;
    }
    added_pair || added_word
}

fn normalized_similarity(a: &str, b: &str) -> f64 {
    let a_chars: Vec<char> = a.to_ascii_lowercase().chars().collect();
    let b_chars: Vec<char> = b.to_ascii_lowercase().chars().collect();
    let max_len = a_chars.len().max(b_chars.len());
    if max_len == 0 {
        return 1.0;
    }
    let dist =
        natural::distance::levenshtein_distance(&a.to_ascii_lowercase(), &b.to_ascii_lowercase());
    1.0 - (dist as f64 / max_len as f64).min(1.0)
}

/// Locates the best-matching window around `start_char_offset` (±200 chars) for
/// `pasted_text` inside `current_value`. Returns `None` once similarity drops
/// below `0.5` (indicating the text was sent or rewritten).
pub fn find_best_pasted_span(
    current_value: &str,
    pasted_text: &str,
    start_char_offset: usize,
) -> Option<(String, f64)> {
    let target = pasted_text.trim();
    if target.is_empty() || current_value.trim().is_empty() {
        return None;
    }
    let chars: Vec<char> = current_value.chars().collect();
    let target_len = target.chars().count();
    let target_words = target.split_whitespace().count();
    let search_start = start_char_offset.saturating_sub(SPAN_SEARCH_MARGIN_CHARS);
    let search_end = (start_char_offset + target_len + SPAN_SEARCH_MARGIN_CHARS).min(chars.len());
    if search_start >= search_end {
        return None;
    }
    let window: String = chars[search_start..search_end].iter().collect();

    // Candidate spans aligned on word boundaries with matching word count (±1).
    let mut word_ranges: Vec<(usize, usize)> = Vec::new();
    let mut in_word = false;
    let mut w_start = 0;
    for (idx, ch) in window.char_indices() {
        if ch.is_whitespace() {
            if in_word {
                word_ranges.push((w_start, idx));
                in_word = false;
            }
        } else if !in_word {
            w_start = idx;
            in_word = true;
        }
    }
    if in_word {
        word_ranges.push((w_start, window.len()));
    }
    if word_ranges.is_empty() {
        return None;
    }

    let min_w = target_words.saturating_sub(1).max(1);
    let max_w = (target_words + 1).min(word_ranges.len());
    let mut best: Option<(String, f64)> = None;

    for start_w in 0..word_ranges.len() {
        for len_w in min_w..=max_w {
            let end_w = start_w + len_w;
            if end_w > word_ranges.len() {
                break;
            }
            let cand = &window[word_ranges[start_w].0..word_ranges[end_w - 1].1];
            let sim = normalized_similarity(target, cand);
            if best.as_ref().is_none_or(|(_, b)| sim > *b) {
                best = Some((cand.to_string(), sim));
            }
        }
    }

    best.filter(|(_, sim)| *sim >= MIN_SPAN_SIMILARITY)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollStepOutcome {
    /// Continue watching on the next tick.
    Continue,
    /// Stop watching (gate closed, secure field, or text sent/rewritten).
    Stop,
    /// A single spelling fix was detected and should be offered.
    Offer { old: String, new: String },
}

/// Executes one poll tick against `source` after re-checking `gate`, secure
/// field role, span similarity, and `suggest_spelling_corrections`.
pub fn evaluate_poll_tick(
    gate: FixOfferGate,
    source: &dyn FocusedFieldSource,
    pasted_text: &str,
    start_char_offset: usize,
    custom_words: &[String],
    corrections: &[CorrectionPair],
) -> PollStepOutcome {
    if !gate.allows_watch() {
        return PollStepOutcome::Stop;
    }
    let Some(reading) = source.read_field() else {
        return PollStepOutcome::Stop;
    };
    if is_secure_field_role(reading.role.as_deref(), reading.subrole.as_deref()) {
        return PollStepOutcome::Stop;
    }
    let Some((edited_span, _sim)) =
        find_best_pasted_span(&reading.value, pasted_text, start_char_offset)
    else {
        return PollStepOutcome::Stop;
    };
    if edited_span.trim() == pasted_text.trim() {
        return PollStepOutcome::Continue;
    }
    let suggestions =
        suggest_spelling_corrections(pasted_text, &edited_span, custom_words, corrections);
    // Only offer single-word/single-phrase clean spelling fixes (ignore broad rewrites).
    if suggestions.len() == 1 {
        let pair = &suggestions[0];
        if !pair.from.eq_ignore_ascii_case(&pair.to) || pair.from != pair.to {
            return PollStepOutcome::Offer {
                old: pair.from.clone(),
                new: pair.to.clone(),
            };
        }
    }
    PollStepOutcome::Continue
}

/// `Send` RAII handle around a retained macOS `AXUIElementRef` (`CFRetain` on
/// construction, `CFRelease` on `Drop`). Zero `objc2` dependency.
#[cfg(target_os = "macos")]
pub struct RetainedAxElement {
    ptr: *mut std::ffi::c_void,
}

#[cfg(target_os = "macos")]
unsafe impl Send for RetainedAxElement {}

#[cfg(target_os = "macos")]
mod macos_ax {
    use super::{FocusedFieldReading, FocusedFieldSource, RetainedAxElement};
    use std::ffi::{c_char, c_void, CStr, CString};

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
        fn CFRetain(cf: *const c_void) -> *const c_void;
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
    }

    const UTF8: u32 = 0x0800_0100;
    const AX_SUCCESS: i32 = 0;

    struct CfRef(*const c_void);

    impl Drop for CfRef {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    impl Drop for RetainedAxElement {
        fn drop(&mut self) {
            if !self.ptr.is_null() {
                unsafe { CFRelease(self.ptr as *const c_void) };
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

    unsafe fn copy_attr(el: *mut c_void, attr_name: &str) -> Option<CfRef> {
        let attr = make_cf_string(attr_name)?;
        let mut out: *const c_void = std::ptr::null();
        let rc = unsafe { AXUIElementCopyAttributeValue(el, attr.0, &mut out) };
        if rc != AX_SUCCESS || out.is_null() {
            None
        } else {
            Some(CfRef(out))
        }
    }

    unsafe fn cf_to_string(cf: *const c_void) -> Option<String> {
        if cf.is_null() || unsafe { CFGetTypeID(cf) != CFStringGetTypeID() } {
            return None;
        }
        let len = unsafe { CFStringGetLength(cf) };
        if len < 0 {
            return None;
        }
        let max_bytes = unsafe { CFStringGetMaximumSizeForEncoding(len, UTF8) } + 1;
        if max_bytes <= 0 || max_bytes > 32_768 {
            return None;
        }
        let mut buf = vec![0u8; max_bytes as usize];
        let ok =
            unsafe { CFStringGetCString(cf, buf.as_mut_ptr() as *mut c_char, max_bytes, UTF8) };
        if ok == 0 {
            return None;
        }
        unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
            .to_str()
            .ok()
            .map(str::to_string)
    }

    impl RetainedAxElement {
        pub fn capture_focused() -> Option<Self> {
            if unsafe { AXIsProcessTrusted() } == 0 {
                return None;
            }
            let sys_ptr = unsafe { AXUIElementCreateSystemWide() };
            if sys_ptr.is_null() {
                return None;
            }
            let sys = CfRef(sys_ptr as *const c_void);
            unsafe { AXUIElementSetMessagingTimeout(sys.0 as *mut c_void, 0.2) };
            let focused = unsafe { copy_attr(sys.0 as *mut c_void, "AXFocusedUIElement") }?;
            let retained = unsafe { CFRetain(focused.0) };
            if retained.is_null() {
                return None;
            }
            Some(Self {
                ptr: retained as *mut c_void,
            })
        }
    }

    impl FocusedFieldSource for RetainedAxElement {
        fn read_field(&self) -> Option<FocusedFieldReading> {
            if self.ptr.is_null() {
                return None;
            }
            let role = unsafe { copy_attr(self.ptr, "AXRole").and_then(|r| cf_to_string(r.0)) };
            let subrole =
                unsafe { copy_attr(self.ptr, "AXSubrole").and_then(|r| cf_to_string(r.0)) };
            if super::is_secure_field_role(role.as_deref(), subrole.as_deref()) {
                return Some(FocusedFieldReading {
                    value: String::new(),
                    role,
                    subrole,
                });
            }
            let value = unsafe { copy_attr(self.ptr, "AXValue").and_then(|v| cf_to_string(v.0)) }?;
            Some(FocusedFieldReading {
                value,
                role,
                subrole,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct FakeField {
        reading: Option<FocusedFieldReading>,
        reads: AtomicUsize,
    }

    impl FakeField {
        fn new(value: &str) -> Self {
            Self {
                reading: Some(FocusedFieldReading {
                    value: value.to_string(),
                    role: Some("AXTextArea".to_string()),
                    subrole: None,
                }),
                reads: AtomicUsize::new(0),
            }
        }

        fn secure(value: &str) -> Self {
            Self {
                reading: Some(FocusedFieldReading {
                    value: value.to_string(),
                    role: Some("AXTextField".to_string()),
                    subrole: Some("AXSecureTextField".to_string()),
                }),
                reads: AtomicUsize::new(0),
            }
        }
    }

    impl FocusedFieldSource for FakeField {
        fn read_field(&self) -> Option<FocusedFieldReading> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.reading.clone()
        }
    }

    #[test]
    fn gate_stands_down_when_switch_off_never_store_on_blocked_or_secure_field() {
        let field = FakeField::new("I use Coco Voice daily");
        let off_gate = FixOfferGate {
            offer_remember_fix_enabled: false,
            never_store_history: false,
            dictation_blocked: false,
        };
        assert_eq!(
            evaluate_poll_tick(off_gate, &field, "I use cocoa voice daily", 0, &[], &[]),
            PollStepOutcome::Stop
        );
        assert_eq!(field.reads.load(Ordering::SeqCst), 0);

        // Stands down while never_store_history is ON (0 reads!).
        let never_gate = FixOfferGate {
            offer_remember_fix_enabled: true,
            never_store_history: true,
            dictation_blocked: false,
        };
        assert_eq!(
            evaluate_poll_tick(never_gate, &field, "I use cocoa voice daily", 0, &[], &[]),
            PollStepOutcome::Stop
        );
        assert_eq!(field.reads.load(Ordering::SeqCst), 0);

        let blocked_gate = FixOfferGate {
            offer_remember_fix_enabled: true,
            never_store_history: false,
            dictation_blocked: true,
        };
        assert_eq!(
            evaluate_poll_tick(blocked_gate, &field, "I use cocoa voice daily", 0, &[], &[]),
            PollStepOutcome::Stop
        );
        assert_eq!(field.reads.load(Ordering::SeqCst), 0);

        // Secure password field stops immediately without offering anything.
        let ok_gate = FixOfferGate {
            offer_remember_fix_enabled: true,
            never_store_history: false,
            dictation_blocked: false,
        };
        let secure = FakeField::secure("secret");
        assert_eq!(
            evaluate_poll_tick(ok_gate, &secure, "sekret", 0, &[], &[]),
            PollStepOutcome::Stop
        );
    }

    #[test]
    fn fixtures_detect_at_least_8_of_10_spelling_fixes_and_0_of_20_non_fixes() {
        let gate = FixOfferGate {
            offer_remember_fix_enabled: true,
            never_store_history: false,
            dictation_blocked: false,
        };

        let real_fixes = [
            (
                "Send this to alxander later",
                "Send this to Alexander later",
            ),
            ("Open parakit settings now", "Open Parakeet settings now"),
            ("Check postgress logs today", "Check Postgres logs today"),
            ("Deploy to vercel tonight", "Deploy to Vercel tonight"),
            ("Ask claud about this", "Ask Claude about this"),
            (
                "Update kubrenetes config now",
                "Update Kubernetes config now",
            ),
            ("Use typescrip for frontend", "Use TypeScript for frontend"),
            ("Build the tauri bundel", "Build the Tauri bundle"),
            ("Ping sofiaa before lunch", "Ping Sofia before lunch"),
            ("Review the graphqul schema", "Review the GraphQL schema"),
        ];
        let mut offered = 0usize;
        for (pasted, edited) in real_fixes {
            let field = FakeField::new(edited);
            if matches!(
                evaluate_poll_tick(gate, &field, pasted, 0, &[], &[]),
                PollStepOutcome::Offer { .. }
            ) {
                offered += 1;
            }
        }
        assert!(
            offered >= 8,
            "expected >= 8/10 spelling fixes offered, got {offered}/10"
        );

        // 20 non-fix edits (unchanged, sent/cleared, completely rewritten, punctuation-only, appended).
        let non_fixes = [
            ("Let us ship tomorrow", "Let us ship tomorrow"),
            ("Let us ship tomorrow", ""),
            (
                "Let us ship tomorrow",
                "Completely unrelated paragraph about dinner",
            ),
            ("Let us ship tomorrow", "Let us ship tomorrow!"),
            ("Let us ship tomorrow", "Let us ship tomorrow."),
            ("Let us ship tomorrow", "Let us ship tomorrow?"),
            ("Meeting at three", "Meeting at three, plus design review"),
            ("Good morning team", "Good morning team and welcome aboard"),
            ("Check the PR", "Check the PR"),
            ("Ready to test", ""),
            ("Hello world", "Goodbye Mars and Saturn forever"),
            ("Ship on Friday", "Ship on Friday."),
            ("Send the draft", "Send the draft"),
            ("See you soon", "Catch you next Thursday evening instead"),
            ("One two three", "Four five six seven eight nine"),
            ("Quick update", "Quick update!"),
            ("Looks good to me", "Looks good to me."),
            ("Thanks everyone", ""),
            ("Draft is ready", "Draft is ready"),
            ("Please review", "Cancel the entire launch right now"),
        ];
        let mut false_offers = 0usize;
        for (pasted, edited) in non_fixes {
            let field = FakeField::new(edited);
            if matches!(
                evaluate_poll_tick(gate, &field, pasted, 0, &[], &[]),
                PollStepOutcome::Offer { .. }
            ) {
                false_offers += 1;
            }
        }
        assert_eq!(false_offers, 0, "expected 0/20 false offers");
    }

    #[test]
    fn queue_caps_at_3_expires_after_24h_suppresses_dismissed_and_remember_dedups() {
        let mut q = FixOfferQueue::new();
        let o1 = q
            .push_offer("a".into(), "A".into(), "Mail".into(), 100)
            .unwrap();
        let _o2 = q
            .push_offer("b".into(), "B".into(), "Mail".into(), 200)
            .unwrap();
        let _o3 = q
            .push_offer("c".into(), "C".into(), "Mail".into(), 300)
            .unwrap();
        let o4 = q
            .push_offer("d".into(), "D".into(), "Mail".into(), 400)
            .unwrap();
        assert_eq!(q.len(), MAX_PENDING_FIX_OFFERS);
        assert_eq!(q.current(400).unwrap().id, o4.id);

        // Dismissing o4 suppresses ("d", "D") until restart.
        q.dismiss(&o4.id, 401);
        assert!(q
            .push_offer("d".into(), "D".into(), "Mail".into(), 402)
            .is_none());
        let _ = o1;

        // After 24h TTL, queue empties.
        assert!(q.current(400 + FIX_OFFER_TTL_MS + 1).is_none());

        // AC-P27: remember adds both pair and custom word with case-insensitive dedup.
        let mut words = vec!["Parakeet".to_string()];
        let mut fixes = Vec::new();
        assert!(remember_spelling_fix_into_settings(
            &mut words,
            &mut fixes,
            "cocoa voice",
            "Coco Voice"
        ));
        assert_eq!(words, vec!["Parakeet", "Coco Voice"]);
        assert_eq!(fixes.len(), 1);
        // Repeating is a no-op.
        assert!(!remember_spelling_fix_into_settings(
            &mut words,
            &mut fixes,
            "COCOA VOICE",
            "coco voice"
        ));
        assert_eq!(words.len(), 2);
        assert_eq!(fixes.len(), 1);
    }
}
