//! Local-only repository symbol detection and caching (`P-f`: `AC-P22..P24`).
//!
//! Security & privacy invariants:
//! - Only `file:///` and `file://localhost/` URLs are accepted; `ssh://`,
//!   `http://`, `https://`, `vscode-remote://`, remote hosts, and NUL bytes are
//!   rejected immediately.
//! - Both the decoded path and `$HOME` are canonicalized before verifying
//!   `canon_target.starts_with(&canon_home)`.
//! - Walks up at most 12 directory levels looking for `.git` via
//!   `fs::symlink_metadata` and never returns `$HOME` itself or `/`.
//! - Symbols live strictly in memory (10-minute TTL), are never written to disk
//!   or logged, and are applied only locally (never forwarded to cloud LLMs).

use crate::app_profiles::{classify_app_kind, ActiveAppInfo, AppKind};
use crate::dictation_features::{apply_repo_symbols, scan_open_repo_symbols, RepoScanResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MAX_WALK_UP_LEVELS: usize = 12;
pub const REPO_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

const EXTRA_TERMINAL_IDENTIFIERS: &[&str] = &[
    "com.apple.terminal",
    "terminal",
    "com.googlecode.iterm2",
    "iterm2",
    "dev.warp.warp-stable",
    "warp",
    "com.mitchellh.ghostty",
    "ghostty",
    "com.todesktop.230313mzl4w4u92",
    "cursor",
];

/// Returns `true` when `info` is a code editor or terminal where open-repo
/// symbol matching is appropriate (`AC-P22`).
pub fn is_repo_capable_app(info: &ActiveAppInfo) -> bool {
    if classify_app_kind(info) == AppKind::Code {
        return true;
    }
    let fields = [
        info.bundle_id.as_deref().unwrap_or(""),
        info.app_name.as_str(),
        info.process_stem.as_deref().unwrap_or(""),
    ];
    EXTRA_TERMINAL_IDENTIFIERS
        .iter()
        .any(|id| fields.iter().any(|f| f.trim().eq_ignore_ascii_case(id)))
}

fn percent_decode_path(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            0 => return None,
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                let val = u8::from_str_radix(hex, 16).ok()?;
                if val == 0 {
                    return None;
                }
                out.push(val);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Resolves the nearest local git repository root from a macOS `AXDocument` URL
/// (`AC-P22`). Strictly confined inside canonicalized `home`, walking up at most
/// 12 levels, checking `.git` via `symlink_metadata`, and never returning `home`
/// itself or `/`.
pub fn repo_root_from_document(doc_url: &str, home: &Path) -> Option<PathBuf> {
    let trimmed = doc_url.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }

    // Accept ONLY `file:///` or `file://localhost/` (case-insensitive scheme/host).
    let raw_path = if let Some(rest) = trimmed.strip_prefix("file:///") {
        format!("/{rest}")
    } else if let Some(rest) = trimmed
        .strip_prefix("file://localhost/")
        .or_else(|| trimmed.strip_prefix("FILE://localhost/"))
        .or_else(|| trimmed.strip_prefix("file://LOCALHOST/"))
    {
        format!("/{rest}")
    } else {
        return None;
    };

    // Strip any trailing query or fragment before percent-decoding.
    let path_only = raw_path.split(['?', '#']).next().unwrap_or("");
    let decoded = percent_decode_path(path_only)?;
    if decoded.contains('\0') || decoded.is_empty() {
        return None;
    }

    let canon_home = fs::canonicalize(home).ok()?;
    let canon_target = fs::canonicalize(Path::new(&decoded)).ok()?;
    if !canon_target.starts_with(&canon_home) || canon_target == canon_home {
        return None;
    }

    let mut cursor = if canon_target.is_dir() {
        canon_target
    } else {
        canon_target.parent()?.to_path_buf()
    };

    for _ in 0..=MAX_WALK_UP_LEVELS {
        if cursor == canon_home || cursor.parent().is_none() || !cursor.starts_with(&canon_home) {
            return None;
        }
        let git_entry = cursor.join(".git");
        if let Ok(meta) = fs::symlink_metadata(&git_entry) {
            if !meta.file_type().is_symlink() && (meta.is_dir() || meta.is_file()) {
                return Some(cursor);
            }
        }
        cursor = cursor.parent()?.to_path_buf();
    }
    None
}

/// In-memory 10-minute cache of local repo symbols (`AC-P23`).
/// Never written to disk, never logged, never shared with cloud providers.
#[derive(Default)]
pub struct RepoSymbolsCache {
    cached_root: Option<PathBuf>,
    symbols: Vec<String>,
    scanned_at: Option<Instant>,
}

impl RepoSymbolsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn symbols_for_root_at(&mut self, root: &Path, now: Instant) -> &[String] {
        let fresh = self.cached_root.as_deref().is_some_and(|r| r == root)
            && self
                .scanned_at
                .is_some_and(|ts| now.saturating_duration_since(ts) < REPO_CACHE_TTL);

        if !fresh {
            self.symbols = match scan_open_repo_symbols(root, false) {
                RepoScanResult::Symbols(syms) => syms,
                RepoScanResult::NoLocalRepo { .. } => Vec::new(),
            };
            self.cached_root = Some(root.to_path_buf());
            self.scanned_at = Some(now);
        }
        &self.symbols
    }

    pub fn apply_to_take(
        &mut self,
        text: &str,
        root: &Path,
        threshold: f64,
        now: Instant,
    ) -> String {
        let syms = self.symbols_for_root_at(root, now).to_vec();
        apply_repo_symbols(text, &syms, threshold)
    }

    pub fn clear(&mut self) {
        self.cached_root = None;
        self.symbols.clear();
        self.scanned_at = None;
    }
}

/// Reads the focused window's `AXDocument` URL string on macOS using raw
/// ApplicationServices/CoreFoundation FFI (no `objc2` dependency).
pub fn read_front_window_document_url() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos_doc::read_ax_document()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
mod macos_doc {
    use std::ffi::{c_char, c_void, CStr, CString};

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFGetTypeID(cf: *const c_void) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFURLGetTypeID() -> usize;
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
        if cf.is_null() {
            return None;
        }
        let tid = unsafe { CFGetTypeID(cf) };
        let str_ref = if tid == unsafe { CFStringGetTypeID() } {
            cf
        } else if tid == unsafe { CFURLGetTypeID() } {
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
        unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
            .to_str()
            .ok()
            .map(str::to_string)
    }

    pub fn read_ax_document() -> Option<String> {
        if unsafe { AXIsProcessTrusted() } == 0 {
            return None;
        }
        let sys_ptr = unsafe { AXUIElementCreateSystemWide() };
        if sys_ptr.is_null() {
            return None;
        }
        let sys = CfRef(sys_ptr as *const c_void);
        unsafe { AXUIElementSetMessagingTimeout(sys.0 as *mut c_void, 0.2) };

        let front_app = unsafe { copy_attr(sys.0 as *mut c_void, "AXFocusedApplication") }?;
        let window = unsafe { copy_attr(front_app.0 as *mut c_void, "AXFocusedWindow") }?;
        let doc = unsafe { copy_attr(window.0 as *mut c_void, "AXDocument") }?;
        unsafe { cf_to_string(doc.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_root_from_document_validates_file_urls_home_confinement_and_git_walk() {
        let outer = tempfile::tempdir().unwrap();
        let home = outer.path().join("home");
        let repo = home.join("work").join("my project");
        let nested = repo.join("src").join("nested");
        let file = nested.join("main.rs");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&nested).unwrap();
        fs::write(&file, "fn main() {}\n").unwrap();

        let encoded = format!(
            "file://{}/work/my%20project/src/nested/main.rs",
            home.to_str().unwrap()
        );
        let found = repo_root_from_document(&encoded, &home).expect("repo root found");
        assert_eq!(found, fs::canonicalize(&repo).unwrap());

        let localhost_url = format!(
            "file://localhost{}/work/my%20project/src/nested/main.rs",
            home.to_str().unwrap()
        );
        assert_eq!(
            repo_root_from_document(&localhost_url, &home),
            Some(fs::canonicalize(&repo).unwrap())
        );

        // Reject remote/non-file schemes, remote hosts, NUL bytes, and escapes above $HOME.
        assert_eq!(
            repo_root_from_document("ssh://host/repo/main.rs", &home),
            None
        );
        assert_eq!(
            repo_root_from_document("https://example.com/main.rs", &home),
            None
        );
        assert_eq!(
            repo_root_from_document("vscode-remote://ssh-remote+box/repo", &home),
            None
        );
        assert_eq!(
            repo_root_from_document("file://remotehost/share/main.rs", &home),
            None
        );
        assert_eq!(
            repo_root_from_document("file:///tmp/bad%00path", &home),
            None
        );

        // Even if $HOME itself has a .git dir, repo_root_from_document must never return $HOME.
        let outside_repo = outer.path().join("outside-repo");
        fs::create_dir_all(outside_repo.join(".git")).unwrap();
        let outside_file = outside_repo.join("lib.rs");
        fs::write(&outside_file, "").unwrap();
        let outside_url = format!("file://{}", outside_file.to_str().unwrap());
        assert_eq!(repo_root_from_document(&outside_url, &home), None);

        // Symlink inside $HOME pointing outside $HOME is rejected after canonicalization.
        #[cfg(unix)]
        {
            let link_in_home = home.join("escape-link");
            std::os::unix::fs::symlink(&outside_repo, &link_in_home).unwrap();
            let escaped_url = format!("file://{}/lib.rs", link_in_home.to_str().unwrap());
            assert_eq!(repo_root_from_document(&escaped_url, &home), None);
        }
    }

    #[test]
    fn repo_scan_excludes_env_pem_key_files_and_caches_within_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("demo-repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".env"), "SECRET_TOKEN_ALPHA=1\n").unwrap();
        fs::write(repo.join(".env.local"), "SECRET_TOKEN_BETA=2\n").unwrap();
        fs::write(repo.join("cert.pem"), "PemMarkerIdentifier\n").unwrap();
        fs::write(repo.join("priv.key"), "KeyMarkerIdentifier\n").unwrap();
        fs::write(
            repo.join("engine.rs"),
            "pub fn processTranscriptionOutput() {}\nstruct MeetingNotesPanel;\n",
        )
        .unwrap();

        let mut cache = RepoSymbolsCache::new();
        let t0 = Instant::now();
        let syms = cache.symbols_for_root_at(&repo, t0).to_vec();
        assert!(syms.contains(&"processTranscriptionOutput".to_string()));
        assert!(syms.contains(&"MeetingNotesPanel".to_string()));
        assert!(!syms.iter().any(|s| s.contains("SECRET_TOKEN")));
        assert!(!syms.iter().any(|s| s.contains("PemMarker")));
        assert!(!syms.iter().any(|s| s.contains("KeyMarker")));

        // Apply benchmark (`AC-P24`): improves identifier accuracy and stays < 50 ms per take.
        let bench_start = Instant::now();
        let out = cache.apply_to_take(
            "check process transcription output in meeting notes panel",
            &repo,
            0.25,
            t0 + Duration::from_secs(5),
        );
        assert!(bench_start.elapsed() < Duration::from_millis(50));
        assert!(out.contains("processTranscriptionOutput"));
        assert!(out.contains("MeetingNotesPanel"));
    }
}
