//! "Coming up": upcoming meetings read from the Mac calendar.
//!
//! This module is the whole privacy story for the calendar feature:
//!
//! * Reading goes through [`CalendarSource`], so tests drive a fake in memory.
//! * In isolated test-calendar mode, only explicit test calendars (`[Test]` or
//!   `Coco Voice Test`) are ever listed or read; personal and real calendars are
//!   never surfaced or queried.
//! * A calendar is only read when the person picked it, and never when it looks
//!   like a work calendar ([`work_reason`]) or was marked as work.
//! * Only [`UpcomingEvent`] is kept (title, start, end, calendar name and
//!   attendee display names), in memory, and [`UpcomingStore::close`] drops it
//!   unless notes started for that meeting.
//! * Nothing here logs, stores on disk, syncs, or reaches a model.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Mutex;

/// Maximum rows shown in Coming up (I16 / D-X4).
pub const MAX_UPCOMING_ROWS: usize = 10;

/// Allowed title prefixes in isolated test-calendar mode so real personal or
/// employer calendars on a developer Mac are never listed or read.
pub const TEST_CALENDAR_PREFIXES: [&str; 2] = ["coco voice test", "[test]"];

/// A calendar as the Mac lists it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarInfo {
    pub id: String,
    pub title: String,
    pub account: String,
    pub source: String,
}

/// An event as a [`CalendarSource`] hands it over.
#[derive(Clone, Serialize, Deserialize)]
pub struct RawEvent {
    pub calendar_id: String,
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub attendee_display_names: Vec<String>,
}

/// Read-only view of a calendar store.
pub trait CalendarSource: Send + Sync {
    fn calendars(&self) -> Result<Vec<CalendarInfo>, String>;
    /// Events that overlap `[from, to)` (unix seconds) in the given calendars only.
    fn events(&self, calendar_ids: &[String], from: i64, to: i64) -> Result<Vec<RawEvent>, String>;
}

/// Optional in-memory JSON fixture (`COCO_VOICE_TEST_CALENDAR_JSON`) for isolated
/// test-copy verification without touching macOS Calendar or EventKit.
#[derive(Default, Deserialize)]
struct FixturePayload {
    #[serde(default)]
    calendars: Vec<CalendarInfo>,
    #[serde(default)]
    events: Vec<RawEvent>,
}

pub struct FixtureCalendarSource {
    calendars: Vec<CalendarInfo>,
    events: Vec<RawEvent>,
}

impl FixtureCalendarSource {
    pub fn from_json(raw: &str) -> Result<Self, String> {
        let parsed: FixturePayload =
            serde_json::from_str(raw).map_err(|_| String::from("Invalid test calendar JSON."))?;
        let calendars: Vec<CalendarInfo> = parsed
            .calendars
            .into_iter()
            .filter(is_allowed_test_calendar)
            .collect();
        let allowed_ids: Vec<String> = calendars.iter().map(|c| c.id.clone()).collect();
        let events = parsed
            .events
            .into_iter()
            .filter(|e| allowed_ids.contains(&e.calendar_id))
            .collect();
        Ok(Self { calendars, events })
    }
}

impl CalendarSource for FixtureCalendarSource {
    fn calendars(&self) -> Result<Vec<CalendarInfo>, String> {
        Ok(self.calendars.clone())
    }

    fn events(&self, calendar_ids: &[String], from: i64, to: i64) -> Result<Vec<RawEvent>, String> {
        Ok(self
            .events
            .iter()
            .filter(|e| calendar_ids.contains(&e.calendar_id) && e.end > from && e.start < to)
            .cloned()
            .collect())
    }
}

/// Returns `true` only when a calendar is explicitly named as a test calendar.
pub fn is_allowed_test_calendar(calendar: &CalendarInfo) -> bool {
    let lower = calendar.title.trim().to_lowercase();
    TEST_CALENDAR_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

/// Why a calendar row cannot be picked.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockedReason {
    Work,
}

/// One row in the picker.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CalendarChoice {
    pub id: String,
    pub title: String,
    pub account: String,
    pub blocked: Option<BlockedReason>,
}

/// The only fields kept about an upcoming meeting.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct UpcomingEvent {
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub calendar_name: String,
    pub attendee_display_names: Vec<String>,
}

/// Words that mark a calendar as a work calendar. Matched against the start of
/// each word in the title, account and source, so "Networking" does not match
/// but "Workplace" does. Over-blocking is the safe direction.
const WORK_WORDS: [&str; 10] = [
    "work",
    "corp",
    "employ",
    "exchange",
    "enterprise",
    "compan",
    "business",
    "office365",
    "o365",
    "365",
];

/// A calendar that looks like it belongs to a job is never read.
pub fn work_reason(calendar: &CalendarInfo) -> Option<BlockedReason> {
    let text = format!(
        "{} {} {}",
        calendar.title, calendar.account, calendar.source
    )
    .to_lowercase();
    let hit = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .any(|w| WORK_WORDS.iter().any(|k| w.starts_with(k)));
    hit.then_some(BlockedReason::Work)
}

fn effective_block(calendar: &CalendarInfo, user_work_ids: &[String]) -> Option<BlockedReason> {
    if user_work_ids.contains(&calendar.id) {
        Some(BlockedReason::Work)
    } else {
        work_reason(calendar)
    }
}

/// Picker rows. Today the picker starts empty and blocked rows are never selectable.
pub fn calendar_choices(calendars: &[CalendarInfo], _selected: &[String]) -> Vec<CalendarChoice> {
    calendar_choices_with_work(calendars, &[], false)
}

pub fn calendar_choices_with_work(
    calendars: &[CalendarInfo],
    user_work_ids: &[String],
    test_only: bool,
) -> Vec<CalendarChoice> {
    calendars
        .iter()
        .filter(|c| !test_only || is_allowed_test_calendar(c))
        .map(|c| CalendarChoice {
            id: c.id.clone(),
            title: c.title.clone(),
            account: c.account.clone(),
            blocked: effective_block(c, user_work_ids),
        })
        .collect()
}

/// What may be saved as `selected_calendars`: known calendars that are not work.
pub fn sanitize_selection(ids: &[String], calendars: &[CalendarInfo]) -> Vec<String> {
    sanitize_selection_with_work(ids, calendars, &[], false)
}

pub fn sanitize_selection_with_work(
    ids: &[String],
    calendars: &[CalendarInfo],
    user_work_ids: &[String],
    test_only: bool,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        let ok = calendars.iter().any(|c| {
            &c.id == id
                && (!test_only || is_allowed_test_calendar(c))
                && effective_block(c, user_work_ids).is_none()
        });
        if ok && !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

#[derive(Default)]
struct Held {
    upcoming: Vec<UpcomingEvent>,
    started: Option<UpcomingEvent>,
}

/// In-memory only. No file, no database, no log.
#[derive(Default)]
pub struct UpcomingStore {
    inner: Mutex<Held>,
}

impl UpcomingStore {
    fn held_mut(&self) -> std::sync::MutexGuard<'_, Held> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reads the picked, non-work calendars for events from `now` to `now + horizon_secs`.
    /// With nothing picked, the source is not asked for anything.
    pub fn refresh(
        &self,
        source: &dyn CalendarSource,
        selected: &[String],
        now: i64,
        horizon_secs: i64,
    ) -> Result<Vec<UpcomingEvent>, String> {
        self.refresh_with_policy(source, selected, &[], false, now, horizon_secs)
    }

    pub fn refresh_with_policy(
        &self,
        source: &dyn CalendarSource,
        selected: &[String],
        user_work_ids: &[String],
        test_only: bool,
        now: i64,
        horizon_secs: i64,
    ) -> Result<Vec<UpcomingEvent>, String> {
        if selected.is_empty() {
            self.held_mut().upcoming.clear();
            return Ok(vec![]);
        }
        let calendars = source.calendars()?;
        let allowed: Vec<&CalendarInfo> = calendars
            .iter()
            .filter(|c| {
                selected.contains(&c.id)
                    && (!test_only || is_allowed_test_calendar(c))
                    && effective_block(c, user_work_ids).is_none()
            })
            .collect();
        if allowed.is_empty() {
            self.held_mut().upcoming.clear();
            return Ok(vec![]);
        }
        let ids: Vec<String> = allowed.iter().map(|c| c.id.clone()).collect();
        let mut events: Vec<UpcomingEvent> = Vec::new();
        for raw in source.events(&ids, now, now + horizon_secs)? {
            let Some(calendar_entry) = allowed.iter().find(|c| c.id == raw.calendar_id) else {
                continue;
            };
            if raw.end <= now || raw.start >= now + horizon_secs {
                continue;
            }
            let mut names: Vec<String> = Vec::new();
            for n in raw.attendee_display_names {
                let n = n.trim().to_string();
                if !n.is_empty() && !names.contains(&n) {
                    names.push(n);
                }
            }
            events.push(UpcomingEvent {
                title: raw.title,
                start: raw.start,
                end: raw.end,
                calendar_name: calendar_entry.title.clone(),
                attendee_display_names: names,
            });
        }
        events.sort_by_key(|e| e.start);
        self.held_mut().upcoming = events.clone();
        Ok(events)
    }

    /// Notes are starting for this meeting: it stays in memory past `close`.
    pub fn start_notes(
        &self,
        start: i64,
        title: &str,
        calendar_name: &str,
    ) -> Option<UpcomingEvent> {
        let mut held = self.held_mut();
        let found = held
            .upcoming
            .iter()
            .find(|e| e.start == start && e.title == title && e.calendar_name == calendar_name)
            .cloned()?;
        held.started = Some(found.clone());
        Some(found)
    }

    /// Meetings closed: drop the list, keep only the meeting notes started for.
    pub fn close(&self) {
        self.held_mut().upcoming.clear();
    }

    /// Take and consume the event kept for notes when recording starts (`C3`).
    pub fn take_started_notes(&self) -> Option<UpcomingEvent> {
        self.held_mut().started.take()
    }

    /// Notes for the kept meeting finished: nothing is left.
    pub fn end_notes(&self) {
        self.held_mut().started = None;
    }

    #[cfg(test)]
    pub fn held(&self) -> (Vec<UpcomingEvent>, Option<UpcomingEvent>) {
        let h = self.held_mut();
        (h.upcoming.clone(), h.started.clone())
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::Mutex;

    /// In-memory calendars for tests. Records every id it was asked to read.
    pub struct FakeCalendar {
        pub calendars: Vec<CalendarInfo>,
        pub events: Vec<RawEvent>,
        pub asked: Mutex<Vec<String>>,
        pub calls: Mutex<u32>,
    }

    pub fn make_calendar(id: &str, title: &str, account: &str, source: &str) -> CalendarInfo {
        CalendarInfo {
            id: id.into(),
            title: title.into(),
            account: account.into(),
            source: source.into(),
        }
    }

    pub fn ev(calendar_id: &str, title: &str, start: i64, end: i64, who: &[&str]) -> RawEvent {
        RawEvent {
            calendar_id: calendar_id.into(),
            title: title.into(),
            start,
            end,
            attendee_display_names: who.iter().map(|s| s.to_string()).collect(),
        }
    }

    impl FakeCalendar {
        pub fn new(calendars: Vec<CalendarInfo>, events: Vec<RawEvent>) -> Self {
            Self {
                calendars,
                events,
                asked: Mutex::new(vec![]),
                calls: Mutex::new(0),
            }
        }
    }

    impl CalendarSource for FakeCalendar {
        fn calendars(&self) -> Result<Vec<CalendarInfo>, String> {
            Ok(self.calendars.clone())
        }
        fn events(&self, ids: &[String], from: i64, to: i64) -> Result<Vec<RawEvent>, String> {
            *self.calls.lock().unwrap() += 1;
            self.asked.lock().unwrap().extend(ids.iter().cloned());
            Ok(self
                .events
                .iter()
                .filter(|e| ids.contains(&e.calendar_id) && e.end > from && e.start < to)
                .cloned()
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    fn home() -> CalendarInfo {
        make_calendar("h", "Home", "iCloud", "iCloud")
    }

    #[test]
    fn work_corporate_and_exchange_calendars_are_flagged() {
        let flagged = [
            make_calendar("1", "Work", "iCloud", "iCloud"),
            make_calendar("2", "Team", "user@example.test", "Exchange"),
            make_calendar("3", "Calendar", "Corporate Mail", "Other"),
            make_calendar("4", "Calendar", "user@enterprise.test", "Employer"),
            make_calendar("5", "Project", "Office 365", "Other"),
            make_calendar("6", "Workplace events", "Local", "Local"),
        ];
        for c in flagged {
            assert!(work_reason(&c) == Some(BlockedReason::Work), "{}", c.title);
        }
    }

    #[test]
    fn test_only_policy_blocks_all_real_personal_and_work_calendars() {
        let src = FakeCalendar::new(
            vec![
                home(),
                make_calendar("p", "Personal", "iCloud", "iCloud"),
                make_calendar("w", "Work", "x", "Exchange"),
                make_calendar("t1", "[Test] Product syncs", "Local", "Local"),
                make_calendar("t2", "Coco Voice Test Work", "Local", "Local"),
            ],
            vec![
                ev("h", "Private appointment", 100, 200, &[]),
                ev("p", "Personal dinner", 100, 200, &[]),
                ev("t1", "Design review", 100, 200, &["Alex"]),
                ev("t2", "Blocked work test", 100, 200, &[]),
            ],
        );
        let choices = calendar_choices_with_work(&src.calendars, &[], true);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].id, "t1");
        assert!(choices[0].blocked.is_none());
        assert_eq!(choices[1].id, "t2");
        assert!(choices[1].blocked == Some(BlockedReason::Work));

        let store = UpcomingStore::default();
        let attempted = vec![
            "h".to_string(),
            "p".to_string(),
            "w".to_string(),
            "t1".to_string(),
            "t2".to_string(),
        ];
        let kept = sanitize_selection_with_work(&attempted, &src.calendars, &[], true);
        assert_eq!(kept, vec!["t1".to_string()]);
        let out = store
            .refresh_with_policy(&src, &attempted, &[], true, 0, 1000)
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Design review");
        assert_eq!(*src.asked.lock().unwrap(), vec!["t1".to_string()]);
    }

    #[test]
    fn nothing_selected_never_touches_the_source() {
        let src = FakeCalendar::new(vec![home()], vec![ev("h", "Standup", 100, 200, &[])]);
        let store = UpcomingStore::default();
        let out = store.refresh(&src, &[], 0, 1000).unwrap();
        assert!(out.is_empty());
        assert_eq!(*src.calls.lock().unwrap(), 0);
    }

    #[test]
    fn closing_drops_the_list_but_keeps_the_meeting_notes_started_for() {
        let src = FakeCalendar::new(
            vec![home()],
            vec![
                ev("h", "A", 100, 200, &["Ann"]),
                ev("h", "B", 300, 400, &[]),
            ],
        );
        let store = UpcomingStore::default();
        store.refresh(&src, &["h".to_string()], 0, 1000).unwrap();
        let started = store.start_notes(100, "A", "Home").expect("known meeting");
        assert_eq!(started.title, "A");
        store.close();
        let (upcoming, kept) = store.held();
        assert!(upcoming.is_empty());
        assert_eq!(kept.map(|e| e.title), Some("A".to_string()));
        store.end_notes();
        assert!(store.held().1.is_none());
    }

    #[test]
    fn calendar_data_never_shows_up_in_debug_output_or_logs() {
        let src = include_str!("calendar_upcoming.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        for banned in [
            "log::",
            "println!",
            "eprintln!",
            "tracing",
            "reqwest",
            "llm_client",
            "std::fs",
            "dbg!",
        ] {
            assert!(
                !body.contains(banned),
                "calendar module must not use {banned}"
            );
        }
        assert!(!body.contains("derive(Debug") && !body.contains(", Debug"));
    }
}
