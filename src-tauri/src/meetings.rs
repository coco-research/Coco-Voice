//! Start-notes offer. Looks at running apps and emits at most one offer.
//! Never starts capture, never writes a meeting row, and never asks for Accessibility.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::settings;

const POLL_SECS: u64 = 5;
const ZOOM_BUNDLE: &str = "us.zoom.xos";
const TEAMS_BUNDLES: &[&str] = &["com.microsoft.teams2", "com.microsoft.teams"];
/// Plan table order. The first browser whose title matches is the one offer.
const MEET_BROWSERS: &[&str] = &[
    "com.google.Chrome",
    "com.apple.Safari",
    "company.thebrowser.Browser",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "org.mozilla.firefox",
];
/// Zoom's call phrase is the literal "Zoom Meeting". The plan gives Teams the same
/// window-title rule and no literal, so a title containing "Meeting" is the call signal.
const TEAMS_CALL_MARK: &str = "Meeting";

static MEETING_RECORDING: AtomicBool = AtomicBool::new(false);
static OFFER_POLL_RUNNING: AtomicBool = AtomicBool::new(false);

/// Later capture PRs set this for the whole time a meeting is recording.
/// The poll does not look at apps while it is true. Nothing in this module sets it.
#[allow(dead_code)]
pub fn set_meeting_recording(recording: bool) {
    MEETING_RECORDING.store(recording, Ordering::Relaxed);
}

pub fn meeting_is_recording() -> bool {
    MEETING_RECORDING.load(Ordering::Relaxed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RunningApp {
    bundle_id: String,
    window_titles: Vec<String>,
}

struct DetectInput<'a> {
    meetings_enabled: bool,
    os: &'a str,
    os_version: &'a str,
    recording: bool,
    accessibility_granted: bool,
    apps: &'a [RunningApp],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MeetingOffer {
    pub bundle_id: String,
    pub app: String,
    pub in_call: bool,
    /// English sentence locked by the Rust tests. The panel translates `app` + `inCall`.
    pub body: String,
    /// Always false. This module has no capture path.
    pub starts_capture: bool,
}

#[derive(Default)]
struct OfferState {
    suppressed: HashSet<String>,
    /// Stays 0. There is no meetings table in this module.
    meeting_rows: u32,
    /// Stays 0. There is no wav writer in this module.
    audio_files: u32,
}

impl OfferState {
    fn dismiss(&mut self, bundle_id: &str) {
        if !bundle_id.is_empty() {
            self.suppressed.insert(bundle_id.to_string());
        }
    }

    fn decide(&mut self, input: &DetectInput<'_>) -> Option<MeetingOffer> {
        if input.recording
            || !settings::meetings_visible(input.meetings_enabled, input.os, input.os_version)
        {
            return None;
        }
        self.suppressed
            .retain(|id| input.apps.iter().any(|app| app.bundle_id == *id));
        self.native_offer(input).or_else(|| self.meet_offer(input))
    }

    fn native_offer(&self, input: &DetectInput<'_>) -> Option<MeetingOffer> {
        if let Some(app) = find_app(input.apps, ZOOM_BUNDLE) {
            if !self.suppressed.contains(ZOOM_BUNDLE) {
                let in_call = title_has(app, input.accessibility_granted, "Zoom Meeting");
                return Some(native("zoom", ZOOM_BUNDLE, in_call));
            }
        }
        for bundle_id in TEAMS_BUNDLES {
            if self.suppressed.contains(*bundle_id) {
                continue;
            }
            if let Some(app) = find_app(input.apps, bundle_id) {
                let in_call = title_has(app, input.accessibility_granted, TEAMS_CALL_MARK);
                return Some(native("teams", bundle_id, in_call));
            }
        }
        None
    }

    fn meet_offer(&self, input: &DetectInput<'_>) -> Option<MeetingOffer> {
        // Titles are the only signal. Without Accessibility already granted there is no offer,
        // and this path never claims the browser is in a call.
        if !input.accessibility_granted {
            return None;
        }
        for bundle_id in MEET_BROWSERS {
            if self.suppressed.contains(*bundle_id) {
                continue;
            }
            let Some(app) = find_app(input.apps, bundle_id) else {
                continue;
            };
            let matched = app.window_titles.iter().any(|title| meet_title(title));
            if matched {
                return Some(MeetingOffer {
                    bundle_id: (*bundle_id).to_string(),
                    app: "meet".to_string(),
                    in_call: false,
                    body: "Google Meet may be open.".to_string(),
                    starts_capture: false,
                });
            }
        }
        None
    }
}

fn find_app<'a>(apps: &'a [RunningApp], bundle_id: &str) -> Option<&'a RunningApp> {
    apps.iter().find(|app| app.bundle_id == bundle_id)
}

fn title_has(app: &RunningApp, accessibility_granted: bool, mark: &str) -> bool {
    accessibility_granted && app.window_titles.iter().any(|title| title.contains(mark))
}

fn meet_title(title: &str) -> bool {
    title.contains("Meet -") || title.contains("Google Meet")
}

fn native(app: &str, bundle_id: &str, in_call: bool) -> MeetingOffer {
    let body = match (app, in_call) {
        ("zoom", true) => "Zoom is in a call.",
        ("zoom", false) => "Zoom is open.",
        ("teams", true) => "Teams is in a call.",
        _ => "Teams is open.",
    };
    MeetingOffer {
        bundle_id: bundle_id.to_string(),
        app: app.to_string(),
        in_call,
        body: body.to_string(),
        starts_capture: false,
    }
}

#[cfg(target_os = "macos")]
fn wants_titles(bundle_id: &str) -> bool {
    bundle_id == ZOOM_BUNDLE
        || TEAMS_BUNDLES.contains(&bundle_id)
        || MEET_BROWSERS.contains(&bundle_id)
}

pub struct OfferRuntime {
    state: Mutex<OfferState>,
    last: Mutex<Option<MeetingOffer>>,
    /// One publish at a time. A poll that already chose an offer must not emit it after Not now.
    publish_lock: Mutex<()>,
}

impl OfferRuntime {
    fn new() -> Self {
        Self {
            state: Mutex::new(OfferState::default()),
            last: Mutex::new(None),
            publish_lock: Mutex::new(()),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

/// Start the 5-second poll. No-op on the headless path; `setup` calls this only for the GUI.
pub fn install(app: &AppHandle) {
    app.manage(OfferRuntime::new());
    OFFER_POLL_RUNNING.store(true, Ordering::Relaxed);
    let app = app.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("meetings-offer".into())
        .spawn(move || poll_loop(app))
    {
        log::warn!("meetings offer poll did not start: {err}");
    }
}

pub fn stop_offer_poll() {
    OFFER_POLL_RUNNING.store(false, Ordering::Relaxed);
}

fn poll_loop(app: AppHandle) {
    while OFFER_POLL_RUNNING.load(Ordering::Relaxed) {
        publish(&app);
        for _ in 0..POLL_SECS {
            if !OFFER_POLL_RUNNING.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

fn publish(app: &AppHandle) {
    let Some(runtime) = app.try_state::<OfferRuntime>() else {
        return;
    };
    let _one = lock(&runtime.publish_lock);
    publish_locked(app, &runtime);
}

fn publish_locked(app: &AppHandle, runtime: &OfferRuntime) {
    let enabled = settings::get_settings(app).meetings_enabled;
    let recording = meeting_is_recording();
    let os = tauri_plugin_os::platform();
    let version = tauri_plugin_os::version().to_string();
    if recording || !settings::meetings_visible(enabled, os, &version) {
        emit_if_changed(app, runtime, None);
        return;
    }
    let (accessibility_granted, apps) = snapshot_running_apps();
    let offer = lock(&runtime.state).decide(&DetectInput {
        meetings_enabled: enabled,
        os,
        os_version: &version,
        recording,
        accessibility_granted,
        apps: &apps,
    });
    emit_if_changed(app, runtime, offer);
}

fn emit_if_changed(app: &AppHandle, runtime: &OfferRuntime, next: Option<MeetingOffer>) {
    if *lock(&runtime.last) == next {
        return;
    }
    if let Err(err) = app.emit("meetings-offer", &next) {
        log::warn!("Could not emit the meetings offer: {err}");
        return;
    }
    *lock(&runtime.last) = next;
}

#[tauri::command]
#[specta::specta]
pub fn current_meeting_offer(app: AppHandle) -> Result<Option<MeetingOffer>, String> {
    let runtime = app
        .try_state::<OfferRuntime>()
        .ok_or_else(|| "meetings offer is not running".to_string())?;
    let offer = lock(&runtime.last).clone();
    Ok(offer)
}

// `async`: Tauri runs plain sync commands on the main thread, and the snapshot below can wait
// seconds on an unresponsive app's Accessibility reply.
#[tauri::command(async)]
#[specta::specta]
pub fn dismiss_meeting_offer(app: AppHandle, bundle_id: String) -> Result<(), String> {
    let runtime = app
        .try_state::<OfferRuntime>()
        .ok_or_else(|| "meetings offer is not running".to_string())?;
    let _one = lock(&runtime.publish_lock);
    lock(&runtime.state).dismiss(&bundle_id);
    publish_locked(&app, &runtime);
    Ok(())
}

fn snapshot_running_apps() -> (bool, Vec<RunningApp>) {
    #[cfg(target_os = "macos")]
    {
        macos::snapshot()
    }
    #[cfg(not(target_os = "macos"))]
    {
        (false, Vec::new())
    }
}

/// Running apps come from `NSWorkspace`. Window titles are read only when
/// `AXIsProcessTrusted` is already true. That call does not prompt.
/// `AXIsProcessTrustedWithOptions` is never called.
#[cfg(target_os = "macos")]
mod macos {
    use super::{wants_titles, RunningApp};
    use std::ffi::{c_char, c_void, CStr};

    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn objc_msgSend();
        fn objc_autoreleasePoolPush() -> *mut c_void;
        fn objc_autoreleasePoolPop(pool: *mut c_void);
        fn sel_registerName(name: *const c_char) -> *mut c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringGetCString(
            the_string: *const c_void,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
        fn AXUIElementCopyAttributeValue(
            element: *mut c_void,
            attribute: *const c_void,
            value: *mut *const c_void,
        ) -> i32;
        static kAXWindowsAttribute: *const c_void;
        static kAXTitleAttribute: *const c_void;
    }

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {
        #[link_name = "OBJC_CLASS_$_NSWorkspace"]
        static OBJC_CLASS_NSWorkspace: u8;
    }

    const UTF8: u32 = 0x0800_0100;

    struct Pool(*mut c_void);

    impl Drop for Pool {
        fn drop(&mut self) {
            unsafe { objc_autoreleasePoolPop(self.0) }
        }
    }

    fn sel(name: &str) -> *mut c_void {
        let c = std::ffi::CString::new(name).expect("selector");
        unsafe { sel_registerName(c.as_ptr()) }
    }

    /// `objc_msgSend` has to be cast to the real argument types. Apple's header says so.
    unsafe fn msg0(recv: *mut c_void, selector: *mut c_void) -> *mut c_void {
        unsafe {
            type F = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
            let f: F = std::mem::transmute::<*const (), F>(objc_msgSend as *const ());
            f(recv, selector)
        }
    }

    unsafe fn msg_usize(recv: *mut c_void, selector: *mut c_void) -> usize {
        unsafe {
            type F = unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize;
            let f: F = std::mem::transmute::<*const (), F>(objc_msgSend as *const ());
            f(recv, selector)
        }
    }

    unsafe fn msg_i32(recv: *mut c_void, selector: *mut c_void) -> i32 {
        unsafe {
            type F = unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32;
            let f: F = std::mem::transmute::<*const (), F>(objc_msgSend as *const ());
            f(recv, selector)
        }
    }

    unsafe fn msg1(recv: *mut c_void, selector: *mut c_void, index: usize) -> *mut c_void {
        unsafe {
            type F = unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void;
            let f: F = std::mem::transmute::<*const (), F>(objc_msgSend as *const ());
            f(recv, selector, index)
        }
    }

    fn cf_string(value: *const c_void) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let mut buf = [0u8; 2048];
        let ok = unsafe {
            CFStringGetCString(
                value,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as isize,
                UTF8,
            )
        };
        if ok == 0 {
            return None;
        }
        let text = unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) };
        let owned = text.to_string_lossy().into_owned();
        if owned.is_empty() {
            None
        } else {
            Some(owned)
        }
    }

    unsafe fn window_titles(pid: i32) -> Vec<String> {
        // SAFETY: `element`, `windows` and each `title` come from Create/Copy calls, so we own
        // them (+1) and release each exactly once. Array items are borrowed (+0) from
        // `windows`, read only while it is alive, at indexes below its count.
        unsafe {
            let element = AXUIElementCreateApplication(pid);
            if element.is_null() {
                return Vec::new();
            }
            let mut windows: *const c_void = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(element, kAXWindowsAttribute, &mut windows);
            CFRelease(element);
            if err != 0 || windows.is_null() {
                return Vec::new();
            }
            let count = CFArrayGetCount(windows);
            let mut titles = Vec::new();
            for index in 0..count {
                let window = CFArrayGetValueAtIndex(windows, index);
                if window.is_null() {
                    continue;
                }
                let mut title: *const c_void = std::ptr::null();
                let title_err = AXUIElementCopyAttributeValue(
                    window as *mut c_void,
                    kAXTitleAttribute,
                    &mut title,
                );
                if title_err == 0 && !title.is_null() {
                    if let Some(text) = cf_string(title) {
                        titles.push(text);
                    }
                    CFRelease(title);
                }
            }
            CFRelease(windows);
            titles
        }
    }

    pub fn snapshot() -> (bool, Vec<RunningApp>) {
        let _pool = Pool(unsafe { objc_autoreleasePoolPush() });
        // Trusted check only. The prompt variant is intentionally not linked.
        let granted = unsafe { AXIsProcessTrusted() != 0 };
        let class = std::ptr::addr_of!(OBJC_CLASS_NSWorkspace) as *mut c_void;
        let workspace = unsafe { msg0(class, sel("sharedWorkspace")) };
        let running = unsafe { msg0(workspace, sel("runningApplications")) };
        if workspace.is_null() || running.is_null() {
            return (granted, Vec::new());
        }
        let count = unsafe { msg_usize(running, sel("count")) };
        let mut apps: Vec<RunningApp> = Vec::new();
        for index in 0..count {
            let app = unsafe { msg1(running, sel("objectAtIndex:"), index) };
            if app.is_null() {
                continue;
            }
            let bundle = unsafe { msg0(app, sel("bundleIdentifier")) };
            let Some(bundle_id) = cf_string(bundle) else {
                continue;
            };
            let pid = unsafe { msg_i32(app, sel("processIdentifier")) };
            let window_titles = if granted && wants_titles(&bundle_id) {
                unsafe { window_titles(pid) }
            } else {
                Vec::new()
            };
            if let Some(existing) = apps.iter_mut().find(|item| item.bundle_id == bundle_id) {
                existing.window_titles.extend(window_titles);
            } else {
                apps.push(RunningApp {
                    bundle_id,
                    window_titles,
                });
            }
        }
        (granted, apps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(bundle_id: &str, titles: &[&str]) -> RunningApp {
        RunningApp {
            bundle_id: bundle_id.to_string(),
            window_titles: titles.iter().map(|title| (*title).to_string()).collect(),
        }
    }

    fn input<'a>(
        enabled: bool,
        version: &'a str,
        recording: bool,
        accessibility_granted: bool,
        apps: &'a [RunningApp],
    ) -> DetectInput<'a> {
        DetectInput {
            meetings_enabled: enabled,
            os: "macos",
            os_version: version,
            recording,
            accessibility_granted,
            apps,
        }
    }

    #[test]
    fn offer_never_starts_capture() {
        let mut state = OfferState::default();
        let zoom = vec![app(ZOOM_BUNDLE, &["Zoom Meeting"])];
        let offer = state
            .decide(&input(true, "14.4", false, true, &zoom))
            .expect("zoom running is an offer");
        assert_eq!(offer.app, "zoom");
        assert_eq!(offer.bundle_id, ZOOM_BUNDLE);
        assert_eq!(offer.body, "Zoom is in a call.");
        assert!(offer.in_call);
        assert!(!offer.starts_capture);
        assert_eq!(state.meeting_rows, 0);
        assert_eq!(state.audio_files, 0);

        // Accessibility not already granted: do not read the call title, and do not ask.
        let open = state
            .decide(&input(true, "14.4.1", false, false, &zoom))
            .expect("zoom still offers without accessibility");
        assert_eq!(open.body, "Zoom is open.");
        assert!(!open.in_call);
        assert!(!open.starts_capture);

        let meet_too = vec![
            app(ZOOM_BUNDLE, &["Zoom"]),
            app("com.google.Chrome", &["Google Meet"]),
        ];
        let one = state
            .decide(&input(true, "15.0", false, true, &meet_too))
            .expect("one offer");
        assert_eq!(one.app, "zoom");
        assert_eq!(one.body, "Zoom is open.");

        state.dismiss(ZOOM_BUNDLE);
        assert!(
            state
                .decide(&input(true, "14.4", false, true, &zoom))
                .is_none(),
            "not now suppresses that bundle id while the process is still running"
        );
        assert_eq!(state.meeting_rows, 0);
        assert_eq!(state.audio_files, 0);

        let gone: Vec<RunningApp> = Vec::new();
        assert!(state
            .decide(&input(true, "14.4", false, true, &gone))
            .is_none());
        let again = state
            .decide(&input(true, "14.4", false, true, &zoom))
            .expect("suppression lifts once the bundle id leaves the process list");
        assert_eq!(again.bundle_id, ZOOM_BUNDLE);
        assert!(!again.starts_capture);
        assert_eq!(state.meeting_rows, 0);
        assert_eq!(state.audio_files, 0);
    }

    #[test]
    fn meet_offer_never_claims_a_call() {
        let mut state = OfferState::default();
        let chrome = vec![app("com.google.Chrome", &["Weekly - Google Meet"])];
        let offer = state
            .decide(&input(true, "14.4", false, true, &chrome))
            .expect("a meet title is an offer");
        assert_eq!(offer.app, "meet");
        assert_eq!(offer.bundle_id, "com.google.Chrome");
        assert_eq!(offer.body, "Google Meet may be open.");
        assert!(!offer.in_call);
        assert!(!offer.body.contains("in a call"));
        assert!(!offer.starts_capture);

        let tab = vec![app("com.apple.Safari", &["Meet - abc-defg-hij"])];
        let tab_offer = state
            .decide(&input(true, "14.4", false, true, &tab))
            .expect("Meet - is an offer");
        assert_eq!(tab_offer.body, "Google Meet may be open.");
        assert!(!tab_offer.in_call);

        let closed = vec![app("com.google.Chrome", &["Inbox - Gmail"])];
        assert!(state
            .decide(&input(true, "14.4", false, true, &closed))
            .is_none());

        let other_browser = vec![app("com.operasoftware.Opera", &["Google Meet"])];
        assert!(state
            .decide(&input(true, "14.4", false, true, &other_browser))
            .is_none());

        assert!(
            state
                .decide(&input(true, "14.4", false, false, &chrome))
                .is_none(),
            "without accessibility the detector does not read titles and does not ask"
        );
    }

    #[test]
    fn detector_does_nothing_when_disabled_or_os_too_old() {
        let mut state = OfferState::default();
        let zoom = vec![app(ZOOM_BUNDLE, &["Zoom Meeting"])];
        assert!(state
            .decide(&input(false, "14.4", false, true, &zoom))
            .is_none());
        assert!(state
            .decide(&input(true, "14.3", false, true, &zoom))
            .is_none());
        assert!(state
            .decide(&input(true, "14.3.1", false, true, &zoom))
            .is_none());
        assert!(state
            .decide(&input(true, "10.15.7", false, true, &zoom))
            .is_none());
        assert!(state
            .decide(&input(true, "14.4", true, true, &zoom))
            .is_none());
        assert!(state.suppressed.is_empty());
        assert_eq!(state.meeting_rows, 0);
        assert_eq!(state.audio_files, 0);
        assert!(state
            .decide(&input(true, "14.4", false, true, &zoom))
            .is_some());
    }
}
