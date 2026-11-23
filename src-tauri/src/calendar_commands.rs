//! Commands for the calendar picker and the Coming up list. They only forward
//! to `calendar_upcoming`; nothing here logs or keeps calendar data.

#[cfg(not(target_os = "macos"))]
use crate::calendar_upcoming::RawEvent;
use crate::calendar_upcoming::{
    calendar_choices_with_work, sanitize_selection_with_work, CalendarChoice, CalendarInfo,
    CalendarSource, FixtureCalendarSource, UpcomingEvent, UpcomingStore, MAX_UPCOMING_ROWS,
};
use crate::settings;
use chrono::{Local, TimeZone};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};

/// Shipped macOS bundle identifier. Any other identifier (and all `cfg(test)` builds)
/// enforces `test_only = true` so personal calendars are never read in development or test builds.
pub const SHIPPED_BUNDLE_ID: &str = "com.cocoresearch.cocovoice";

/// Derive `test_only` strictly from the compiled/runtime bundle identifier (AC-X28, I17).
/// Never honors any environment variable bypass.
pub fn is_test_only_bundle_id(bundle_id: &str) -> bool {
    cfg!(test) || bundle_id.trim() != SHIPPED_BUNDLE_ID
}

/// Seconds from `now_secs` until the local end of tomorrow (23:59:59 local time, I16 / D-X4).
pub fn horizon_until_end_of_tomorrow_secs(now_secs: i64) -> i64 {
    let fallback = 48 * 60 * 60;
    let Some(now_dt) = Local.timestamp_opt(now_secs, 0).single() else {
        return fallback;
    };
    let Some(tomorrow) = now_dt.date_naive().succ_opt() else {
        return fallback;
    };
    let Some(end_naive) = tomorrow.and_hms_opt(23, 59, 59) else {
        return fallback;
    };
    let end_ts = Local
        .from_local_datetime(&end_naive)
        .latest()
        .map(|dt| dt.timestamp())
        .unwrap_or(now_secs + fallback);
    end_ts.saturating_sub(now_secs).max(60)
}

pub struct CalendarState {
    source: Box<dyn CalendarSource>,
    store: UpcomingStore,
    test_only: bool,
}

#[cfg(not(target_os = "macos"))]
struct NoCalendar;

#[cfg(not(target_os = "macos"))]
impl CalendarSource for NoCalendar {
    fn calendars(&self) -> Result<Vec<CalendarInfo>, String> {
        Err("Calendar is only available on macOS.".into())
    }
    fn events(&self, _: &[String], _: i64, _: i64) -> Result<Vec<RawEvent>, String> {
        Err("Calendar is only available on macOS.".into())
    }
}

impl CalendarState {
    pub fn new() -> Self {
        Self::for_bundle_id(option_env!("COCO_VOICE_BUNDLE_ID").unwrap_or(""))
    }

    pub fn for_bundle_id(bundle_id: &str) -> Self {
        let test_only = is_test_only_bundle_id(bundle_id);
        if test_only {
            if let Ok(raw) = std::env::var("COCO_VOICE_TEST_CALENDAR_JSON") {
                if !raw.trim().is_empty() {
                    if let Ok(fixture) = FixtureCalendarSource::from_json(&raw) {
                        return Self::with_policy(Box::new(fixture), true);
                    }
                }
            }
        }
        #[cfg(target_os = "macos")]
        let source: Box<dyn CalendarSource> =
            Box::new(crate::calendar_eventkit::EventKitSource::new(test_only));
        #[cfg(not(target_os = "macos"))]
        let source: Box<dyn CalendarSource> = Box::new(NoCalendar);
        Self::with_policy(source, test_only)
    }

    pub fn take_started_for_notes(&self) -> Option<UpcomingEvent> {
        self.store.take_started_notes()
    }

    pub fn with_source(source: Box<dyn CalendarSource>) -> Self {
        Self::with_policy(source, false)
    }

    pub fn with_policy(source: Box<dyn CalendarSource>, test_only: bool) -> Self {
        Self {
            source,
            store: UpcomingStore::default(),
            test_only,
        }
    }

    fn choices(&self, user_work_ids: &[String]) -> Result<Vec<CalendarChoice>, String> {
        let calendars: Vec<CalendarInfo> = self.source.calendars()?;
        Ok(calendar_choices_with_work(
            &calendars,
            user_work_ids,
            self.test_only,
        ))
    }

    fn keep_only_allowed(
        &self,
        ids: &[String],
        user_work_ids: &[String],
    ) -> Result<Vec<String>, String> {
        Ok(sanitize_selection_with_work(
            ids,
            &self.source.calendars()?,
            user_work_ids,
            self.test_only,
        ))
    }

    fn upcoming(
        &self,
        selected: &[String],
        user_work_ids: &[String],
    ) -> Result<Vec<UpcomingEvent>, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let horizon = horizon_until_end_of_tomorrow_secs(now);
        let mut rows = self.store.refresh_with_policy(
            self.source.as_ref(),
            selected,
            user_work_ids,
            self.test_only,
            now,
            horizon,
        )?;
        rows.truncate(MAX_UPCOMING_ROWS);
        Ok(rows)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn list_calendar_choices(
    app: AppHandle,
    state: State<'_, CalendarState>,
) -> Result<Vec<CalendarChoice>, String> {
    let work_ids = settings::get_settings(&app).blocked_work_calendars;
    state.choices(&work_ids)
}

#[tauri::command]
#[specta::specta]
pub async fn change_selected_calendars_setting(
    app: AppHandle,
    state: State<'_, CalendarState>,
    ids: Vec<String>,
) -> Result<Vec<String>, String> {
    let work_ids = settings::get_settings(&app).blocked_work_calendars;
    let kept = state.keep_only_allowed(&ids, &work_ids)?;
    let saved = kept.clone();
    settings::update_settings(&app, move |s| s.selected_calendars = saved);
    Ok(kept)
}

#[tauri::command]
#[specta::specta]
pub async fn mark_calendar_as_work_setting(
    app: AppHandle,
    id: String,
) -> Result<Vec<String>, String> {
    let mut current = settings::get_settings(&app).blocked_work_calendars;
    if !id.trim().is_empty() && !current.contains(&id) {
        current.push(id.clone());
    }
    let next = current.clone();
    let drop_id = id;
    settings::update_settings(&app, move |s| {
        s.blocked_work_calendars = next;
        s.selected_calendars.retain(|x| x != &drop_id);
    });
    Ok(current)
}

#[tauri::command]
#[specta::specta]
pub async fn list_upcoming_meetings(
    app: AppHandle,
    state: State<'_, CalendarState>,
) -> Result<Vec<UpcomingEvent>, String> {
    let cfg = settings::get_settings(&app);
    state.upcoming(&cfg.selected_calendars, &cfg.blocked_work_calendars)
}

#[tauri::command]
#[specta::specta]
pub fn keep_upcoming_for_notes(
    state: State<'_, CalendarState>,
    start: i64,
    title: String,
    calendar_name: String,
) -> Result<Option<UpcomingEvent>, String> {
    Ok(state.store.start_notes(start, &title, &calendar_name))
}

#[tauri::command]
#[specta::specta]
pub fn close_upcoming_meetings(state: State<'_, CalendarState>) -> Result<(), String> {
    state.store.close();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar_upcoming::fake::{ev, make_calendar, FakeCalendar};

    fn test_only_state() -> CalendarState {
        CalendarState::with_policy(
            Box::new(FakeCalendar::new(
                vec![
                    make_calendar("real", "Personal", "iCloud", "iCloud"),
                    make_calendar("t1", "[Test] Team sync", "Local", "Local"),
                    make_calendar("w", "Work", "x", "Exchange"),
                ],
                vec![
                    ev(
                        "real",
                        "Real personal event",
                        i64::MAX / 4,
                        i64::MAX / 4 + 10,
                        &[],
                    ),
                    ev(
                        "t1",
                        "Test planning",
                        i64::MAX / 4,
                        i64::MAX / 4 + 10,
                        &["Sam"],
                    ),
                ],
            )),
            true,
        )
    }

    #[test]
    fn test_only_state_surfaces_only_test_calendars_and_starts_unselected() {
        let rows = test_only_state().choices(&[]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "t1");
        assert!(test_only_state().upcoming(&[], &[]).unwrap().is_empty());
        let kept = test_only_state()
            .keep_only_allowed(
                &["real".to_string(), "t1".to_string(), "w".to_string()],
                &[],
            )
            .unwrap();
        assert_eq!(kept, vec!["t1".to_string()]);
    }

    #[test]
    fn bundle_id_policy_enforces_test_only_outside_shipped_id_and_work_calendar_is_never_queried() {
        assert!(is_test_only_bundle_id("com.cocoresearch.cocovoice.test"));
        assert!(is_test_only_bundle_id(""));
        assert!(is_test_only_bundle_id("com.cocoresearch.cocovoice")); // cfg(test) always true

        let now = 1_800_000_000;
        let horizon = horizon_until_end_of_tomorrow_secs(now);
        assert!(horizon >= 60 && horizon <= 48 * 60 * 60);

        let fake = FakeCalendar::new(
            vec![
                make_calendar("t1", "[Test] Team", "Local", "Local"),
                make_calendar("work1", "[Test] Work marked", "Local", "Local"),
            ],
            vec![
                ev("t1", "Planning", now + 600, now + 2400, &["Sam"]),
                ev("work1", "Blocked work event", now + 600, now + 2400, &[]),
            ],
        );
        let asked_ref = &fake.asked;
        let store = UpcomingStore::default();
        let rows = store
            .refresh_with_policy(
                &fake,
                &["t1".to_string(), "work1".to_string()],
                &["work1".to_string()],
                true,
                now,
                horizon,
            )
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Planning");
        assert!(!asked_ref.lock().unwrap().contains(&"work1".to_string()));
        store.close();
        assert!(store.held().0.is_empty());
    }
}

#[cfg(test)]
mod ef10_guard_tests {
    use super::*;

    const EF10_FIXTURE_JSON: &str = r#"{
        "calendars": [
            { "id": "t1", "title": "[Test] E-F10-MARKER-CALENDAR", "account": "Local", "source": "Local" },
            { "id": "real", "title": "Personal", "account": "iCloud", "source": "iCloud" }
        ],
        "events": [
            {
                "calendar_id": "t1",
                "title": "E-F10-MARKER-TITLE",
                "start": 4102444800,
                "end": 4102446600,
                "attendee_display_names": ["E-F10-MARKER-ATTENDEE"]
            }
        ]
    }"#;

    const EF10_NOW_SECS: i64 = 4102441200;

    fn prod_source_before_tests(src: &str) -> &str {
        src.split("#[cfg(test)]").next().unwrap_or(src)
    }

    fn extract_crate_modules(prod: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in prod.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            let mut rest = trimmed;
            while let Some(idx) = rest.find("crate::") {
                let after = &rest[idx + "crate::".len()..];
                let ident: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !ident.is_empty() && !out.contains(&ident) {
                    out.push(ident);
                }
                rest = after;
            }
        }
        out
    }

    fn run_ef10_fixture_read() -> Vec<UpcomingEvent> {
        let fixture = FixtureCalendarSource::from_json(EF10_FIXTURE_JSON)
            .expect("E-F10 fixture JSON must parse");
        let store = UpcomingStore::default();
        store
            .refresh_with_policy(
                &fixture,
                &["t1".to_string()],
                &[],
                true,
                EF10_NOW_SECS,
                horizon_until_end_of_tomorrow_secs(EF10_NOW_SECS),
            )
            .expect("E-F10 fixture read must succeed")
    }

    #[test]
    fn ac_x42_calendar_sources_have_no_network_model_log_or_unlisted_crate_imports() {
        let files: [(&str, &str, &[&str]); 3] = [
            (
                "calendar_commands.rs",
                include_str!("calendar_commands.rs"),
                &["calendar_upcoming", "calendar_eventkit", "settings"],
            ),
            (
                "calendar_eventkit.rs",
                include_str!("calendar_eventkit.rs"),
                &["calendar_upcoming"],
            ),
            (
                "calendar_upcoming.rs",
                include_str!("calendar_upcoming.rs"),
                &[],
            ),
        ];

        let banned_network_or_model_or_log = [
            "reqwest",
            "ureq",
            "hyper",
            "std::net",
            "tauri_plugin_http",
            "NSURLSession",
            "TcpStream",
            "UdpSocket",
            "local_llm",
            "apple_intelligence",
            "llm_client",
            "openai",
            "anthropic",
            "post_process",
            "log::",
            "tracing::",
            "println!",
            "eprintln!",
            "dbg!",
        ];

        for (name, full_src, allow_list) in files {
            let body = prod_source_before_tests(full_src);
            for banned in banned_network_or_model_or_log {
                assert!(
                    !body.contains(banned),
                    "AC-X42 failed: {name} production code must not reference `{banned}`"
                );
            }
            for crate_mod in extract_crate_modules(body) {
                assert!(
                    allow_list.contains(&crate_mod.as_str()),
                    "AC-X42 failed: {name} references unlisted `crate::{crate_mod}` (allowed: {allow_list:?})"
                );
            }
        }
    }

    #[test]
    fn ac_x42_sandbox_fixture_read_child() {
        let rows = run_ef10_fixture_read();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "E-F10-MARKER-TITLE");
        assert_eq!(rows[0].calendar_name, "[Test] E-F10-MARKER-CALENDAR");
        assert_eq!(
            rows[0].attendee_display_names,
            vec!["E-F10-MARKER-ATTENDEE".to_string()]
        );
        if std::env::var("EF10_SANDBOX_CHILD").as_deref() == Ok("1") {
            let encoded = serde_json::to_string(&rows).expect("serialize rows");
            println!("EF10_ROWS={encoded}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ac_x42_fixture_read_identical_under_sandbox_exec_deny_network_outbound() {
        let expected = run_ef10_fixture_read();
        assert_eq!(expected.len(), 1);

        let exe = std::env::current_exe().expect("resolve current test binary");
        let output = std::process::Command::new("/usr/bin/sandbox-exec")
            .arg("-p")
            .arg("(version 1)(allow default)(deny network-outbound)")
            .arg(&exe)
            .arg("--exact")
            .arg("calendar_commands::ef10_guard_tests::ac_x42_sandbox_fixture_read_child")
            .arg("--nocapture")
            .env("EF10_SANDBOX_CHILD", "1")
            .output()
            .expect("invoke sandbox-exec for AC-X42");

        assert!(
            output.status.success(),
            "sandbox-exec child failed: status={:?} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        let row_line = stdout
            .lines()
            .find_map(|line| line.strip_prefix("EF10_ROWS="))
            .expect("sandbox-exec child must emit EF10_ROWS=<json>");
        let sandboxed: Vec<UpcomingEvent> =
            serde_json::from_str(row_line).expect("parse sandboxed UpcomingEvent JSON");
        assert!(
            sandboxed == expected,
            "sandbox-exec rows must match normal fixture rows"
        );
    }
}
