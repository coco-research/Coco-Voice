//! Shared frontmost-application lookup and per-app dictation profile rules.
//!
//! Resolves the active application's display name, bundle identifier, and
//! executable stem in-process without reading any screen text or window title,
//! and maps application kinds and profile tones to capitalization and
//! punctuation surface styles.

use crate::settings::{AppSettings, CorrectionPair};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;

const CHAT_APPS: &str = "com.tinyspeck.slackmacgap|slack|com.apple.mobilesms|messages|com.hnc.discord|discord|ru.keepcoder.telegram|telegram|net.whatsapp.whatsapp|whatsapp|org.whispersystems.signal-desktop|signal";
const EMAIL_APPS: &str = "com.apple.mail|mail|com.microsoft.outlook|microsoft outlook|com.superhuman.electron|superhuman|org.mozilla.thunderbird|thunderbird";
const CODE_APPS: &str = "com.todesktop.230313mzl4w4u92|cursor|com.microsoft.vscode|code|visual studio code|com.apple.dt.xcode|xcode|dev.zed.zed|zed|com.googlecode.iterm2|iterm2|com.apple.terminal|terminal";
const DOC_APPS: &str =
    "notion.id|notion|com.apple.notes|notes|com.apple.iwork.pages|pages|md.obsidian|obsidian";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppTone {
    #[default]
    Default,
    Auto,
    Casual,
    Direct,
    Formal,
    Code,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapitalizationStyle {
    #[default]
    Default,
    Sentence,
    Lowercase,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum PunctuationStyle {
    #[default]
    Default,
    Full,
    DropTrailingPeriod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceStyle {
    pub capitalization: CapitalizationStyle,
    pub punctuation: PunctuationStyle,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    Chat,
    Email,
    Code,
    Document,
    Other,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type, Default)]
pub struct ActiveAppInfo {
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub process_stem: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CleanupLevel {
    #[default]
    Default,
    On,
    Light,
    Off,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type, Default)]
pub struct BlockedSite {
    pub name: String,
    pub browser_identifier: String,
    pub domain: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct AppDisplayName {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type, Default)]
pub struct AppProfile {
    pub id: String,
    pub name: String,
    pub app_identifier: String,
    #[serde(default)]
    pub blocked: bool,
    #[serde(default)]
    pub tone: AppTone,
    #[serde(default)]
    pub capitalization: CapitalizationStyle,
    #[serde(default)]
    pub punctuation: PunctuationStyle,
    #[serde(default)]
    pub cleanup: CleanupLevel,
    #[serde(default)]
    pub repo_words: Option<bool>,
    #[serde(default)]
    pub post_process_enabled: Option<bool>,
    #[serde(default)]
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub corrections: Vec<CorrectionPair>,
}

impl AppProfile {
    /// Resolves effective clean-up level, falling back to legacy
    /// `post_process_enabled` when `cleanup == CleanupLevel::Default`.
    pub fn effective_cleanup_level(&self) -> CleanupLevel {
        if self.cleanup != CleanupLevel::Default {
            return self.cleanup;
        }
        match self.post_process_enabled {
            Some(true) => CleanupLevel::On,
            Some(false) => CleanupLevel::Off,
            None => CleanupLevel::Default,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartDecision {
    Allow,
    BlockedApp(String),
}

impl StartDecision {
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::BlockedApp(_))
    }
}

pub(crate) struct ResolvedAppProfile {
    pub settings: AppSettings,
    pub surface_style: SurfaceStyle,
    pub post_process_override: Option<bool>,
    pub cleanup_level: CleanupLevel,
    pub remove_fillers: bool,
    pub rewrite_self_corrections: bool,
    pub apply_surface_style: bool,
    pub repo_words_enabled: bool,
}

pub fn is_self_identifier(value: &str) -> bool {
    let t = value.trim();
    t.eq_ignore_ascii_case("Coco Voice")
        || t.eq_ignore_ascii_case("coco-voice")
        || t.eq_ignore_ascii_case("com.cocoresearch.cocovoice")
}

pub fn is_self_app(info: &ActiveAppInfo) -> bool {
    is_self_identifier(&info.app_name)
        || info.bundle_id.as_deref().is_some_and(is_self_identifier)
        || info.process_stem.as_deref().is_some_and(is_self_identifier)
}

pub fn filter_active_app(mut info: ActiveAppInfo) -> Option<ActiveAppInfo> {
    info.app_name = info.app_name.trim().to_string();
    info.bundle_id = info
        .bundle_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    info.process_stem = info
        .process_stem
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if info.app_name.is_empty() {
        info.app_name = info.process_stem.clone().unwrap_or_default();
    }
    if (info.app_name.is_empty() && info.bundle_id.is_none() && info.process_stem.is_none())
        || is_self_app(&info)
    {
        return None;
    }
    Some(info)
}

// ponytail: ceiling = macOS NSWorkspace frontmostApplication (0 subprocesses, 0 screen/window-title reads); upgrade path = foreground process query on Windows/Linux.
pub fn read_active_app() -> Option<ActiveAppInfo> {
    #[cfg(target_os = "macos")]
    {
        use std::path::Path;
        use tauri_nspanel::objc2_app_kit::NSWorkspace;

        let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let app_name = front
            .localizedName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let bundle_id = front.bundleIdentifier().map(|s| s.to_string());
        let process_stem = front.executableURL().and_then(|u| u.path()).and_then(|p| {
            let s = p.to_string();
            Path::new(&s)
                .file_stem()
                .and_then(|st| st.to_str())
                .map(str::to_string)
        });
        filter_active_app(ActiveAppInfo {
            app_name,
            bundle_id,
            process_stem,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn app_profile_matches(saved: &str, info: &ActiveAppInfo) -> bool {
    let p = saved.trim();
    if p.is_empty() || is_self_app(info) || is_self_identifier(p) {
        return false;
    }
    let eq = |c: &str| c.trim().eq_ignore_ascii_case(p);
    info.bundle_id.as_deref().is_some_and(eq)
        || (!info.app_name.is_empty() && eq(&info.app_name))
        || info.process_stem.as_deref().is_some_and(eq)
}

pub const MAX_PAGE_HOST_AX_DEPTH: usize = 8;
pub const MAX_PAGE_HOST_AX_NODES: usize = 200;

/// Thin trait for reading the active browser page host without ever logging or
/// persisting any URL (`AC-P14`, `AC-P15`).
pub trait PageHostReader {
    fn read_page_host(&self, browser: &ActiveAppInfo) -> Option<String>;
}

/// Parses and normalizes ONLY the host component from a URL string.
/// Strips scheme, userinfo, port, path, query, and fragment; lowercases and
/// strips trailing dot. Returns `None` on malformed or empty hosts.
pub fn parse_host_from_url(raw_url: &str) -> Option<String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }
    let after_scheme = match trimmed.find("://") {
        Some(idx) => {
            let scheme = &trimmed[..idx];
            if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
                return None;
            }
            &trimmed[idx + 3..]
        }
        None => {
            // Reject non-HTTP schemes without `://` such as `javascript:alert(1)`,
            // `data:...`, `about:blank`, or `mailto:...`, while keeping `host:port`.
            let before_delim = trimmed.split(['/', '?', '#']).next().unwrap_or("");
            if let Some((_, after_colon)) = before_delim.split_once(':') {
                if after_colon.is_empty() || !after_colon.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
            }
            trimmed
        }
    };
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim();
    if authority.is_empty() {
        return None;
    }
    // Strip userinfo (`user:pass@host`).
    let host_port = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        rest.split(']').next()?
    } else {
        host_port.split(':').next()?
    };
    normalize_site_domain(host)
}

/// Validates and lower-cases a domain rule (`bank.example`).
pub fn normalize_site_domain(raw: &str) -> Option<String> {
    let lower = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if lower.is_empty() || lower.len() > 253 {
        return None;
    }
    if lower
        .bytes()
        .any(|b| !(b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
    {
        return None;
    }
    if lower.starts_with('.') || lower.contains("..") {
        return None;
    }
    Some(lower)
}

/// Exact host or subdomain match (`bank.example` matches `bank.example` and
/// `www.bank.example`, never `notbank.example`).
pub fn site_domain_matches(page_host: &str, rule_domain: &str) -> bool {
    let Some(host) = normalize_site_domain(page_host) else {
        return false;
    };
    let Some(rule) = normalize_site_domain(rule_domain) else {
        return false;
    };
    host == rule || host.ends_with(&format!(".{rule}"))
}

pub fn normalize_blocked_sites(sites: Vec<BlockedSite>) -> Vec<BlockedSite> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for s in sites {
        if out.len() >= 64 {
            break;
        }
        let browser = clamp_str(&s.browser_identifier, 260);
        let Some(domain) = normalize_site_domain(&s.domain) else {
            continue;
        };
        if browser.is_empty() || is_self_identifier(&browser) {
            continue;
        }
        let name = clamp_str(&s.name, 80);
        let key = format!("{}|{}", browser.to_ascii_lowercase(), domain);
        if !seen.insert(key) {
            continue;
        }
        out.push(BlockedSite {
            name: if name.is_empty() {
                domain.clone()
            } else {
                name
            },
            browser_identifier: browser,
            domain,
        });
    }
    out
}

/// Pure bounded AX tree walk used by unit tests (and mirrored by the macOS AX
/// implementation): stops at `MAX_PAGE_HOST_AX_DEPTH` (8), `MAX_PAGE_HOST_AX_NODES`
/// (200), and guards against cycles via visited node IDs.
pub fn find_web_area_host_in_tree<Id, FChildren, FRole, FUrl>(
    root: Id,
    mut children_of: FChildren,
    mut role_of: FRole,
    mut url_of: FUrl,
) -> Option<String>
where
    Id: Copy + Eq + std::hash::Hash,
    FChildren: FnMut(Id) -> Vec<Id>,
    FRole: FnMut(Id) -> Option<String>,
    FUrl: FnMut(Id) -> Option<String>,
{
    let mut visited = HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((root, 0usize));
    visited.insert(root);
    let mut visited_count = 0usize;

    while let Some((node, depth)) = queue.pop_front() {
        if visited_count >= MAX_PAGE_HOST_AX_NODES {
            break;
        }
        visited_count += 1;

        if role_of(node).as_deref() == Some("AXWebArea") {
            if let Some(raw_url) = url_of(node) {
                if let Some(host) = parse_host_from_url(&raw_url) {
                    return Some(host);
                }
            }
        }
        if depth >= MAX_PAGE_HOST_AX_DEPTH {
            continue;
        }
        for child in children_of(node) {
            if visited.insert(child) {
                queue.push_back((child, depth + 1));
            }
        }
    }
    None
}

pub struct MacOsPageHostReader;

impl PageHostReader for MacOsPageHostReader {
    fn read_page_host(&self, _browser: &ActiveAppInfo) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            macos_ax::read_front_browser_web_area_host()
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }
}

#[cfg(target_os = "macos")]
mod macos_ax {
    use super::{parse_host_from_url, MAX_PAGE_HOST_AX_DEPTH, MAX_PAGE_HOST_AX_NODES};
    use std::collections::{HashSet, VecDeque};
    use std::ffi::{c_char, c_void, CStr, CString};

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFURLGetTypeID() -> usize;
        fn CFArrayGetTypeID() -> usize;
        fn CFArrayGetCount(the_array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(the_array: *const c_void, idx: isize) -> *const c_void;
        fn CFURLGetString(an_url: *const c_void) -> *const c_void;
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
        let type_id = unsafe { CFGetTypeID(cf) };
        let str_ref = if type_id == unsafe { CFStringGetTypeID() } {
            cf
        } else if type_id == unsafe { CFURLGetTypeID() } {
            unsafe { CFURLGetString(cf) }
        } else {
            return None;
        };
        if str_ref.is_null() {
            return None;
        }
        let len = unsafe { CFStringGetLength(str_ref) };
        if len < 0 {
            return None;
        }
        let max_bytes = unsafe { CFStringGetMaximumSizeForEncoding(len, UTF8) } + 1;
        if max_bytes <= 0 || max_bytes > 16_384 {
            return None;
        }
        let mut buf = vec![0u8; max_bytes as usize];
        let ok = unsafe {
            CFStringGetCString(str_ref, buf.as_mut_ptr() as *mut c_char, max_bytes, UTF8)
        };
        if ok == 0 {
            return None;
        }
        let cstr = unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) };
        cstr.to_str().ok().map(str::to_string)
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

    /// Bounded walk (`depth <= 8`, `nodes <= 200`, pointer cycle guard) from the
    /// front application's focused window to the first `AXWebArea`, reading only
    /// `AXURL` and parsing out the host. Never logs or stores the URL.
    pub fn read_front_browser_web_area_host() -> Option<String> {
        if unsafe { AXIsProcessTrusted() } == 0 {
            return None;
        }
        let sys_ptr = unsafe { AXUIElementCreateSystemWide() };
        if sys_ptr.is_null() {
            return None;
        }
        let sys = CfRef(sys_ptr as *const c_void);
        unsafe { AXUIElementSetMessagingTimeout(sys.0 as *mut c_void, 0.25) };

        let front_app = unsafe { copy_attr(sys.0 as *mut c_void, "AXFocusedApplication") }?;
        let window = unsafe { copy_attr(front_app.0 as *mut c_void, "AXFocusedWindow") }?;

        let mut visited: HashSet<usize> = HashSet::new();
        let mut queue: VecDeque<(CfRef, usize)> = VecDeque::new();
        visited.insert(window.0 as usize);
        queue.push_back((window, 0));
        let mut nodes_seen = 0usize;

        while let Some((node, depth)) = queue.pop_front() {
            if nodes_seen >= MAX_PAGE_HOST_AX_NODES {
                break;
            }
            nodes_seen += 1;

            if let Some(role_cf) = unsafe { copy_attr(node.0 as *mut c_void, "AXRole") } {
                if unsafe { cf_to_string(role_cf.0) }.as_deref() == Some("AXWebArea") {
                    if let Some(url_cf) = unsafe { copy_attr(node.0 as *mut c_void, "AXURL") } {
                        if let Some(raw_url) = unsafe { cf_to_string(url_cf.0) } {
                            if let Some(host) = parse_host_from_url(&raw_url) {
                                return Some(host);
                            }
                        }
                    }
                }
            }

            if depth >= MAX_PAGE_HOST_AX_DEPTH {
                continue;
            }
            if let Some(children_cf) = unsafe { copy_attr(node.0 as *mut c_void, "AXChildren") } {
                if unsafe { CFGetTypeID(children_cf.0) == CFArrayGetTypeID() } {
                    let count = unsafe { CFArrayGetCount(children_cf.0) };
                    for idx in 0..count {
                        if nodes_seen + queue.len() >= MAX_PAGE_HOST_AX_NODES {
                            break;
                        }
                        let raw_child = unsafe { CFArrayGetValueAtIndex(children_cf.0, idx) };
                        if raw_child.is_null() {
                            continue;
                        }
                        if visited.insert(raw_child as usize) {
                            let retained = unsafe { CFRetain(raw_child) };
                            if !retained.is_null() {
                                queue.push_back((CfRef(retained), depth + 1));
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

/// Single unified dictation gate (`AC-P13`, `AC-P14`).
/// Checks `blocked_apps`, `app_profiles` with `blocked == true`, and
/// `blocked_sites` (only invoking `page_host_reader` when the frontmost app
/// matches a browser in `blocked_sites`; fails closed when the reader returns
/// `None` for that browser).
pub fn dictation_blocked<R: PageHostReader>(
    blocked_apps: &[String],
    app_profiles: &[AppProfile],
    blocked_sites: &[BlockedSite],
    front_app: Option<&ActiveAppInfo>,
    page_host_reader: &R,
) -> bool {
    let Some(info) = front_app.filter(|i| !is_self_app(i)) else {
        return false;
    };
    if blocked_apps.iter().any(|b| app_profile_matches(b, info)) {
        return true;
    }
    if app_profiles
        .iter()
        .any(|p| p.blocked && app_profile_matches(&p.app_identifier, info))
    {
        return true;
    }
    let matching_sites: Vec<&BlockedSite> = blocked_sites
        .iter()
        .filter(|s| app_profile_matches(&s.browser_identifier, info))
        .collect();
    if matching_sites.is_empty() {
        return false;
    }
    match page_host_reader.read_page_host(info) {
        None => true, // Fail closed (I4 / AC-P14) when a rule targets this browser.
        Some(host) => matching_sites
            .iter()
            .any(|s| site_domain_matches(&host, &s.domain)),
    }
}

/// One-time F9 migration: unions legacy `never_dictate_apps` into `blocked_apps`
/// (case-insensitively deduplicated, zero lost entries) and empties the legacy
/// list. Runs strictly AFTER F9 backup and BEFORE `seed_blocked_apps`.
pub fn merge_legacy_never_dictate_into_blocked(
    blocked_apps: &mut Vec<String>,
    never_dictate_apps: &mut Vec<String>,
) {
    if never_dictate_apps.is_empty() {
        return;
    }
    let mut combined = std::mem::take(blocked_apps);
    combined.extend(never_dictate_apps.drain(..));
    *blocked_apps = normalize_blocked_apps(combined);
}

/// Seeds the four approved default profiles once when `voice_pages_seeded` is
/// false and the profile list is empty (`AC-P8`).
pub fn seed_app_profiles(profiles: &mut Vec<AppProfile>, voice_pages_seeded: &mut bool) {
    if *voice_pages_seeded || !profiles.is_empty() {
        *voice_pages_seeded = true;
        return;
    }
    *profiles = vec![
        AppProfile {
            id: "seed-mail".to_string(),
            name: "Mail".to_string(),
            app_identifier: "com.apple.mail".to_string(),
            tone: AppTone::Formal,
            capitalization: CapitalizationStyle::Sentence,
            punctuation: PunctuationStyle::Full,
            cleanup: CleanupLevel::On,
            post_process_enabled: Some(true),
            ..Default::default()
        },
        AppProfile {
            id: "seed-messages".to_string(),
            name: "Messages".to_string(),
            app_identifier: "com.apple.MobileSMS".to_string(),
            tone: AppTone::Casual,
            capitalization: CapitalizationStyle::Lowercase,
            punctuation: PunctuationStyle::DropTrailingPeriod,
            cleanup: CleanupLevel::On,
            post_process_enabled: Some(true),
            ..Default::default()
        },
        AppProfile {
            id: "seed-cursor".to_string(),
            name: "Cursor".to_string(),
            app_identifier: "com.todesktop.230313mzl4w4u92".to_string(),
            tone: AppTone::Code,
            capitalization: CapitalizationStyle::Default,
            punctuation: PunctuationStyle::DropTrailingPeriod,
            cleanup: CleanupLevel::Light,
            repo_words: Some(true),
            post_process_enabled: Some(false),
            ..Default::default()
        },
        AppProfile {
            id: "seed-terminal".to_string(),
            name: "Terminal".to_string(),
            app_identifier: "com.apple.Terminal".to_string(),
            tone: AppTone::Default,
            capitalization: CapitalizationStyle::Default,
            punctuation: PunctuationStyle::Default,
            cleanup: CleanupLevel::Off,
            post_process_enabled: Some(false),
            ..Default::default()
        },
    ];
    *voice_pages_seeded = true;
}

/// Seeds 1Password bundle IDs once on an empty blocked list before
/// `voice_pages_seeded` flips true (`AC-P17`).
pub fn seed_blocked_apps(blocked_apps: &mut Vec<String>, voice_pages_seeded: bool) {
    if voice_pages_seeded || !blocked_apps.is_empty() {
        return;
    }
    *blocked_apps = vec![
        "com.1password.1password".to_string(),
        "com.agilebits.onepassword7".to_string(),
    ];
}

pub fn resolve_app_display_name(id: &str) -> String {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "com.1password.1password" | "com.agilebits.onepassword7" => "1Password".to_string(),
        "com.apple.mail" => "Mail".to_string(),
        "com.apple.mobilesms" => "Messages".to_string(),
        "com.todesktop.230313mzl4w4u92" => "Cursor".to_string(),
        "com.apple.terminal" => "Terminal".to_string(),
        "com.apple.safari" => "Safari".to_string(),
        "com.google.chrome" => "Google Chrome".to_string(),
        _ => {
            #[cfg(target_os = "macos")]
            {
                use tauri_nspanel::objc2_app_kit::NSWorkspace;
                use tauri_nspanel::objc2_foundation::NSString;

                let ns_id = NSString::from_str(trimmed);
                if let Some(url) =
                    NSWorkspace::sharedWorkspace().URLForApplicationWithBundleIdentifier(&ns_id)
                {
                    if let Some(path) = url.path() {
                        let s = path.to_string();
                        if let Some(stem) = std::path::Path::new(&s)
                            .file_stem()
                            .and_then(|st| st.to_str())
                        {
                            if !stem.is_empty() {
                                return stem.to_string();
                            }
                        }
                    }
                }
            }
            trimmed.to_string()
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn app_display_names(ids: Vec<String>) -> Vec<AppDisplayName> {
    ids.into_iter()
        .map(|id| AppDisplayName {
            name: resolve_app_display_name(&id),
            id,
        })
        .collect()
}

pub(crate) fn check_start_allowed(
    settings: &AppSettings,
    app: Option<&ActiveAppInfo>,
) -> StartDecision {
    let Some(info) = app.filter(|i| !is_self_app(i)) else {
        return StartDecision::Allow;
    };
    if let Some(pat) = settings
        .blocked_apps
        .iter()
        .chain(settings.never_dictate_apps.iter())
        .find(|b| app_profile_matches(b, info))
    {
        return StartDecision::BlockedApp(pat.trim().to_string());
    }
    if let Some(p) = settings
        .app_profiles
        .iter()
        .find(|p| p.blocked && app_profile_matches(&p.app_identifier, info))
    {
        return StartDecision::BlockedApp(p.app_identifier.trim().to_string());
    }
    if !settings.blocked_sites.is_empty()
        && dictation_blocked(
            &settings.blocked_apps,
            &settings.app_profiles,
            &settings.blocked_sites,
            Some(info),
            &MacOsPageHostReader,
        )
    {
        return StartDecision::BlockedApp(info.app_name.trim().to_string());
    }
    StartDecision::Allow
}

pub(crate) fn check_active_app_start(settings: &AppSettings) -> StartDecision {
    if settings.blocked_apps.is_empty()
        && settings.never_dictate_apps.is_empty()
        && settings.blocked_sites.is_empty()
        && !settings.app_profiles.iter().any(|p| p.blocked)
    {
        return StartDecision::Allow;
    }
    check_start_allowed(settings, read_active_app().as_ref())
}

fn clamp_str(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

pub(crate) fn normalize_app_profiles(
    profiles: Vec<AppProfile>,
    settings: &AppSettings,
) -> Vec<AppProfile> {
    let prompts: HashSet<&str> = settings
        .post_process_prompts
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    let providers: HashSet<&str> = settings
        .post_process_providers
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    let (mut ids, mut apps, mut out) = (HashSet::new(), HashSet::new(), Vec::new());
    for mut p in profiles {
        if out.len() >= 64 {
            break;
        }
        p.id = clamp_str(&p.id, 128);
        p.name = clamp_str(&p.name, 80);
        p.app_identifier = clamp_str(&p.app_identifier, 260);
        if p.id.is_empty()
            || p.app_identifier.is_empty()
            || is_self_identifier(&p.app_identifier)
            || !ids.insert(p.id.clone())
            || !apps.insert(p.app_identifier.to_ascii_lowercase())
        {
            continue;
        }
        if p.name.is_empty() {
            p.name = p.app_identifier.clone();
        }
        p.prompt_id = p
            .prompt_id
            .map(|s| clamp_str(&s, 128))
            .filter(|s| prompts.contains(s.as_str()));
        p.provider_id = p
            .provider_id
            .map(|s| clamp_str(&s, 128))
            .filter(|s| providers.contains(s.as_str()));
        p.model = p
            .model
            .map(|s| clamp_str(&s, 260))
            .filter(|s| !s.is_empty());
        out.push(p);
    }
    out
}

pub(crate) fn normalize_blocked_apps(apps: Vec<String>) -> Vec<String> {
    let (mut seen, mut out) = (HashSet::new(), Vec::new());
    for raw in apps {
        if out.len() >= 128 {
            break;
        }
        let c = clamp_str(&raw, 260);
        if c.is_empty() || is_self_identifier(&c) || !seen.insert(c.to_ascii_lowercase()) {
            continue;
        }
        out.push(c);
    }
    out
}

pub(crate) fn resolve_for_app(
    settings: &AppSettings,
    app: Option<&ActiveAppInfo>,
) -> ResolvedAppProfile {
    let default_out = || ResolvedAppProfile {
        settings: settings.clone(),
        surface_style: SurfaceStyle::default(),
        post_process_override: None,
        cleanup_level: CleanupLevel::Default,
        remove_fillers: settings.filler_word_removal_enabled,
        rewrite_self_corrections: true,
        apply_surface_style: true,
        repo_words_enabled: settings.repo_words_enabled,
    };
    let Some(info) = app.filter(|i| !is_self_app(i)) else {
        return default_out();
    };
    let Some(profile) = settings
        .app_profiles
        .iter()
        .find(|p| app_profile_matches(&p.app_identifier, info))
    else {
        return default_out();
    };
    let mut effective = settings.clone();
    let cleanup_level = profile.effective_cleanup_level();
    let post_process_override = match cleanup_level {
        CleanupLevel::On => Some(true),
        CleanupLevel::Light | CleanupLevel::Off => Some(false),
        CleanupLevel::Default => None,
    };
    if let Some(enabled) = post_process_override {
        effective.post_process_enabled = enabled;
        if enabled && effective.post_process_selected_prompt_id.is_none() {
            effective.post_process_selected_prompt_id =
                effective.post_process_prompts.first().map(|p| p.id.clone());
        }
    }
    let remove_fillers = match cleanup_level {
        CleanupLevel::Off => false,
        CleanupLevel::On | CleanupLevel::Light | CleanupLevel::Default => {
            effective.filler_word_removal_enabled
        }
    };
    effective.filler_word_removal_enabled = remove_fillers;
    let rewrite_self_corrections = cleanup_level != CleanupLevel::Off;
    let apply_surface_style = cleanup_level != CleanupLevel::Off;
    let repo_words_enabled = profile.repo_words.unwrap_or(effective.repo_words_enabled);
    effective.repo_words_enabled = repo_words_enabled;

    if let Some(pid) = profile
        .prompt_id
        .as_ref()
        .filter(|id| effective.post_process_prompts.iter().any(|p| &p.id == *id))
    {
        effective.post_process_selected_prompt_id = Some(pid.clone());
    }
    if let Some(prov) = profile
        .provider_id
        .as_ref()
        .filter(|id| effective.post_process_provider(id).is_some())
    {
        effective.post_process_provider_id = prov.clone();
    }
    if let Some(m) = profile.model.as_ref().filter(|m| !m.trim().is_empty()) {
        effective
            .post_process_models
            .insert(effective.post_process_provider_id.clone(), m.clone());
    }
    effective
        .corrections
        .extend(profile.corrections.iter().cloned());

    let base = tone_defaults(profile.tone, Some(info));
    ResolvedAppProfile {
        settings: effective,
        surface_style: SurfaceStyle {
            capitalization: if profile.capitalization == CapitalizationStyle::Default {
                base.capitalization
            } else {
                profile.capitalization
            },
            punctuation: if profile.punctuation == PunctuationStyle::Default {
                base.punctuation
            } else {
                profile.punctuation
            },
        },
        post_process_override,
        cleanup_level,
        remove_fillers,
        rewrite_self_corrections,
        apply_surface_style,
        repo_words_enabled,
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_active_app_info() -> Option<ActiveAppInfo> {
    read_active_app()
}

#[tauri::command]
#[specta::specta]
pub fn update_app_profiles(app: tauri::AppHandle, profiles: Vec<AppProfile>) -> Result<(), String> {
    crate::settings::update_settings(&app, |s| {
        s.app_profiles = normalize_app_profiles(profiles, s);
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn update_blocked_apps(app: tauri::AppHandle, apps: Vec<String>) -> Result<(), String> {
    crate::settings::update_settings(&app, |s| {
        s.blocked_apps = normalize_blocked_apps(apps);
    });
    Ok(())
}

pub fn classify_app_kind(info: &ActiveAppInfo) -> AppKind {
    let fields = [
        info.bundle_id.as_deref().unwrap_or(""),
        info.app_name.as_str(),
        info.process_stem.as_deref().unwrap_or(""),
    ];
    let matches_list = |list: &str| {
        list.split('|')
            .any(|item| fields.iter().any(|f| f.trim().eq_ignore_ascii_case(item)))
    };
    if matches_list(CHAT_APPS) {
        AppKind::Chat
    } else if matches_list(EMAIL_APPS) {
        AppKind::Email
    } else if matches_list(CODE_APPS) {
        AppKind::Code
    } else if matches_list(DOC_APPS) {
        AppKind::Document
    } else {
        AppKind::Other
    }
}

pub fn tone_defaults(tone: AppTone, app: Option<&ActiveAppInfo>) -> SurfaceStyle {
    let effective = match tone {
        AppTone::Auto => match app.map(classify_app_kind).unwrap_or(AppKind::Other) {
            AppKind::Chat => AppTone::Casual,
            AppKind::Email => AppTone::Formal,
            AppKind::Code => AppTone::Code,
            AppKind::Document => AppTone::Direct,
            AppKind::Other => AppTone::Default,
        },
        other => other,
    };
    match effective {
        AppTone::Default | AppTone::Auto => SurfaceStyle::default(),
        AppTone::Casual => SurfaceStyle {
            capitalization: CapitalizationStyle::Lowercase,
            punctuation: PunctuationStyle::DropTrailingPeriod,
        },
        AppTone::Direct | AppTone::Formal => SurfaceStyle {
            capitalization: CapitalizationStyle::Sentence,
            punctuation: PunctuationStyle::Full,
        },
        AppTone::Code => SurfaceStyle {
            capitalization: CapitalizationStyle::Default,
            punctuation: PunctuationStyle::DropTrailingPeriod,
        },
    }
}

pub fn apply_surface_style(text: &str, style: SurfaceStyle) -> String {
    if style == SurfaceStyle::default() || text.trim().is_empty() {
        return text.to_string();
    }
    let trimmed = text.trim();
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    let mut body = match style.capitalization {
        CapitalizationStyle::Default => trimmed.to_string(),
        CapitalizationStyle::Sentence => apply_sentence_caps(trimmed),
        CapitalizationStyle::Lowercase => apply_lowercase_starts(trimmed),
    };
    match style.punctuation {
        PunctuationStyle::Default => {}
        PunctuationStyle::DropTrailingPeriod if body.ends_with('.') && !body.ends_with("..") => {
            body.pop();
        }
        PunctuationStyle::DropTrailingPeriod => {}
        PunctuationStyle::Full => {
            if body
                .chars()
                .last()
                .is_some_and(|c| !matches!(c, '.' | '!' | '?' | ':' | ';'))
            {
                body.push('.');
            }
        }
    }
    format!("{lead}{body}{trail}")
}

fn apply_sentence_caps(text: &str) -> String {
    let (mut out, mut cap) = (String::with_capacity(text.len()), true);
    for ch in text.chars() {
        if cap && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            cap = false;
        } else {
            out.push(ch);
            cap = if matches!(ch, '.' | '!' | '?') {
                true
            } else if ch.is_whitespace() || matches!(ch, '"' | '\'' | '(' | '[') {
                cap
            } else {
                false
            };
        }
    }
    out
}

fn apply_lowercase_starts(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut lower) = (String::with_capacity(text.len()), true);
    for i in 0..chars.len() {
        let ch = chars[i];
        if lower && ch.is_alphabetic() {
            let end = chars[i + 1..]
                .iter()
                .position(|c| !(c.is_alphanumeric() || matches!(c, '\'' | '\u{2019}')))
                .map_or(chars.len(), |p| i + 1 + p);
            let word: String = chars[i..end].iter().collect();
            let keep = chars[i + 1..end].iter().any(|c| c.is_uppercase())
                || word == "I"
                || word.starts_with("I'")
                || word.starts_with("I\u{2019}");
            if ch.is_uppercase() && !keep {
                out.extend(ch.to_lowercase());
            } else {
                out.push(ch);
            }
            lower = false;
        } else {
            out.push(ch);
            lower = if matches!(ch, '.' | '!' | '?') {
                true
            } else if ch.is_whitespace() || matches!(ch, '"' | '\'' | '(' | '[') {
                lower
            } else {
                false
            };
        }
    }
    out
}

#[cfg(test)]
mod per_app_tests {
    use super::*;

    fn app(name: &str, bundle: Option<&str>, stem: Option<&str>) -> ActiveAppInfo {
        ActiveAppInfo {
            app_name: name.into(),
            bundle_id: bundle.map(Into::into),
            process_stem: stem.map(Into::into),
        }
    }

    #[test]
    fn frontmost_app_filter_excludes_self_identifiers_and_keeps_external_apps() {
        assert_eq!(
            filter_active_app(app(
                "Coco Voice",
                Some("com.cocoresearch.cocovoice"),
                Some("coco-voice"),
            )),
            None
        );
        assert_eq!(
            filter_active_app(app(
                "Custom Build",
                Some("com.cocoresearch.cocovoice"),
                None
            )),
            None
        );
        assert_eq!(
            filter_active_app(app("Custom Build", None, Some("coco-voice"))),
            None
        );
        let mail = filter_active_app(app(" Mail ", Some("com.apple.mail"), Some("Mail"))).unwrap();
        assert_eq!(mail.app_name, "Mail");
    }

    #[test]
    fn per_app_matching_kind_and_surface_style_preserve_words() {
        let slack = app("Slack", Some("com.tinyspeck.slackmacgap"), Some("Slack"));
        assert!(app_profile_matches("com.tinyspeck.slackmacgap", &slack));
        assert!(app_profile_matches("SLACK", &slack));
        assert!(!app_profile_matches(" ", &slack));

        let chat_style = tone_defaults(AppTone::Auto, Some(&slack));
        assert_eq!(
            apply_surface_style("Sounds good to me.", chat_style),
            "sounds good to me"
        );
        let mail = app("Mail", Some("com.apple.mail"), None);
        assert_eq!(
            apply_surface_style(
                "thanks for the note. see you soon",
                tone_defaults(AppTone::Auto, Some(&mail))
            ),
            "Thanks for the note. See you soon."
        );
        let vscode = app("Code", Some("com.microsoft.VSCode"), Some("Electron"));
        assert_eq!(classify_app_kind(&vscode), AppKind::Code);
        assert_eq!(
            apply_surface_style(
                "cargo test useState.",
                tone_defaults(AppTone::Code, Some(&vscode))
            ),
            "cargo test useState"
        );
    }

    #[test]
    fn per_app_overrides_and_blocked_bundle_noop_before_recording() {
        let mut s = crate::settings::get_default_settings();
        s.blocked_apps = normalize_blocked_apps(vec![
            " com.1password.1password ".into(),
            "com.cocoresearch.cocovoice".into(),
        ]);
        assert_eq!(s.blocked_apps, vec!["com.1password.1password"]);
        s.app_profiles = vec![
            AppProfile {
                id: "mail".into(),
                name: "Mail".into(),
                app_identifier: "com.apple.mail".into(),
                tone: AppTone::Auto,
                post_process_enabled: Some(true),
                corrections: vec![CorrectionPair {
                    from: "coco".into(),
                    to: "Coco Voice".into(),
                }],
                ..Default::default()
            },
            AppProfile {
                id: "bank".into(),
                name: "Bank".into(),
                app_identifier: "com.example.banking".into(),
                blocked: true,
                ..Default::default()
            },
        ];

        let onepass = app("1Password", Some("com.1password.1password"), None);
        assert_eq!(
            check_start_allowed(&s, Some(&onepass)),
            StartDecision::BlockedApp("com.1password.1password".into())
        );
        let bank = app("Banking", Some("com.example.banking"), None);
        assert!(check_start_allowed(&s, Some(&bank)).is_blocked());

        let mail = app("Mail", Some("com.apple.mail"), None);
        assert_eq!(check_start_allowed(&s, Some(&mail)), StartDecision::Allow);
        let resolved = resolve_for_app(&s, Some(&mail));
        assert_eq!(resolved.post_process_override, Some(true));
        assert_eq!(resolved.settings.corrections.len(), 1);
        assert_eq!(
            resolved.surface_style.capitalization,
            CapitalizationStyle::Sentence
        );

        let norm = normalize_app_profiles(
            vec![AppProfile {
                id: "v".into(),
                name: "".into(),
                app_identifier: "com.microsoft.VSCode".into(),
                prompt_id: Some("missing".into()),
                provider_id: Some("missing".into()),
                ..Default::default()
            }],
            &s,
        );
        assert!(norm[0].prompt_id.is_none() && norm[0].provider_id.is_none());
    }

    #[test]
    fn seed_profiles_blocked_apps_and_f9_legacy_merge_preserve_all_entries() {
        let mut profiles = Vec::new();
        let mut blocked = Vec::new();
        let mut legacy_never = vec!["com.custom.vault".to_string(), "1Password".to_string()];
        let mut seeded = false;

        // F9 merge runs BEFORE seed_blocked_apps so legacy entries are never lost.
        merge_legacy_never_dictate_into_blocked(&mut blocked, &mut legacy_never);
        assert!(legacy_never.is_empty());
        assert_eq!(blocked, vec!["com.custom.vault", "1Password"]);

        // Since blocked is non-empty after F9 merge, seed_blocked_apps does not overwrite it.
        seed_blocked_apps(&mut blocked, seeded);
        assert_eq!(blocked, vec!["com.custom.vault", "1Password"]);

        // Fresh empty install seeds 1Password bundle IDs and all 4 profiles once.
        let mut fresh_blocked = Vec::new();
        seed_blocked_apps(&mut fresh_blocked, seeded);
        assert_eq!(
            fresh_blocked,
            vec!["com.1password.1password", "com.agilebits.onepassword7"]
        );
        seed_app_profiles(&mut profiles, &mut seeded);
        assert!(seeded);
        assert_eq!(profiles.len(), 4);
        assert_eq!(profiles[2].app_identifier, "com.todesktop.230313mzl4w4u92");
        assert_eq!(profiles[2].cleanup, CleanupLevel::Light);
        assert_eq!(profiles[2].repo_words, Some(true));
        assert_eq!(profiles[3].cleanup, CleanupLevel::Off);

        // Deleting all profiles does not re-seed once voice_pages_seeded is true.
        profiles.clear();
        seed_app_profiles(&mut profiles, &mut seeded);
        assert!(profiles.is_empty());
    }

    #[test]
    fn site_rules_parse_hosts_match_subdomains_fail_closed_and_guard_ax_walk_bounds() {
        struct FakeReader {
            host: Option<String>,
            calls: std::cell::Cell<usize>,
        }
        impl PageHostReader for FakeReader {
            fn read_page_host(&self, _browser: &ActiveAppInfo) -> Option<String> {
                self.calls.set(self.calls.get() + 1);
                self.host.clone()
            }
        }

        assert_eq!(
            parse_host_from_url(concat!(
                "https://",
                "user:pass@WWW.Bank.Example.:8443/path?x=1#top"
            )),
            Some("www.bank.example".to_string())
        );
        assert_eq!(parse_host_from_url("javascript:alert(1)"), None);
        assert!(site_domain_matches("bank.example", "bank.example"));
        assert!(site_domain_matches("www.bank.example", "bank.example"));
        assert!(!site_domain_matches("notbank.example", "bank.example"));

        let sites = normalize_blocked_sites(vec![BlockedSite {
            name: "Banking".to_string(),
            browser_identifier: "com.apple.Safari".to_string(),
            domain: "Bank.Example.".to_string(),
        }]);
        let safari = app("Safari", Some("com.apple.Safari"), None);
        let chrome = app("Google Chrome", Some("com.google.Chrome"), None);

        // Unlisted browser never triggers a page read.
        let reader_unlisted = FakeReader {
            host: Some("bank.example".to_string()),
            calls: std::cell::Cell::new(0),
        };
        assert!(!dictation_blocked(
            &[],
            &[],
            &sites,
            Some(&chrome),
            &reader_unlisted
        ));
        assert_eq!(reader_unlisted.calls.get(), 0);

        // Listed browser with matching subdomain is blocked.
        let reader_hit = FakeReader {
            host: Some("www.bank.example".to_string()),
            calls: std::cell::Cell::new(0),
        };
        assert!(dictation_blocked(
            &[],
            &[],
            &sites,
            Some(&safari),
            &reader_hit
        ));
        assert_eq!(reader_hit.calls.get(), 1);

        // Listed browser where host cannot be read fails closed (blocked).
        let reader_none = FakeReader {
            host: None,
            calls: std::cell::Cell::new(0),
        };
        assert!(dictation_blocked(
            &[],
            &[],
            &sites,
            Some(&safari),
            &reader_none
        ));

        // Cycle & depth guard on fake AX tree: cyclic nodes 0 <-> 1 terminate cleanly.
        let found = find_web_area_host_in_tree(
            0usize,
            |id| match id {
                0 => vec![1],
                1 => vec![0, 2],
                _ => vec![],
            },
            |id| {
                if id == 2 {
                    Some("AXWebArea".into())
                } else {
                    None
                }
            },
            |id| {
                if id == 2 {
                    Some(concat!("https://", "portal.bank.example/login").into())
                } else {
                    None
                }
            },
        );
        assert_eq!(found.as_deref(), Some("portal.bank.example"));
    }

    #[test]
    fn cleanup_levels_on_light_off_resolve_exact_pipeline_policies() {
        let mut s = crate::settings::get_default_settings();
        s.filler_word_removal_enabled = true;
        s.repo_words_enabled = false;
        let mut seeded = false;
        seed_app_profiles(&mut s.app_profiles, &mut seeded);

        let mail = resolve_for_app(&s, Some(&app("Mail", Some("com.apple.mail"), None)));
        assert_eq!(mail.cleanup_level, CleanupLevel::On);
        assert_eq!(mail.post_process_override, Some(true));
        assert!(mail.remove_fillers && mail.rewrite_self_corrections && mail.apply_surface_style);

        let cursor = resolve_for_app(
            &s,
            Some(&app("Cursor", Some("com.todesktop.230313mzl4w4u92"), None)),
        );
        assert_eq!(cursor.cleanup_level, CleanupLevel::Light);
        assert_eq!(cursor.post_process_override, Some(false));
        assert!(
            cursor.remove_fillers && cursor.rewrite_self_corrections && cursor.apply_surface_style
        );
        assert!(cursor.repo_words_enabled);

        let terminal =
            resolve_for_app(&s, Some(&app("Terminal", Some("com.apple.Terminal"), None)));
        assert_eq!(terminal.cleanup_level, CleanupLevel::Off);
        assert_eq!(terminal.post_process_override, Some(false));
        assert!(!terminal.remove_fillers);
        assert!(!terminal.rewrite_self_corrections);
        assert!(!terminal.apply_surface_style);
    }
}
