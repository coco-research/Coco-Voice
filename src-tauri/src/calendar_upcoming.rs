//! "Coming up": upcoming meetings read from the Mac calendar.
//!
//! This module is the whole privacy story for the calendar feature:
//!
//! * Reading goes through [`CalendarSource`], so tests drive a fake in memory.
//! * A calendar is only read when the person picked it, and never when it looks
//!   like a work calendar ([`work_reason`]).
//! * Only [`UpcomingEvent`] is kept (title, start, end, calendar name and
//!   attendee display names), in memory, and [`UpcomingStore::close`] drops it
//!   unless notes started for that meeting.
//! * Nothing here logs, stores on disk, syncs, or reaches a model.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Mutex;

/// A calendar as the Mac lists it.
#[derive(Clone, PartialEq, Eq)]
pub struct CalendarInfo {
    pub id: String,
    pub title: String,
    pub account: String,
    pub source: String,
}

/// An event as a [`CalendarSource`] hands it over.
#[derive(Clone)]
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

/// Picker rows. `selected` is accepted so a later change can pre-fill rows;
/// today the picker starts empty and blocked rows are never selectable.
pub fn calendar_choices(calendars: &[CalendarInfo], _selected: &[String]) -> Vec<CalendarChoice> {
    calendars
        .iter()
        .map(|c| CalendarChoice {
            id: c.id.clone(),
            title: c.title.clone(),
            account: c.account.clone(),
            blocked: work_reason(c),
        })
        .collect()
}

/// What may be saved as `selected_calendars`: known calendars that are not work.
pub fn sanitize_selection(ids: &[String], calendars: &[CalendarInfo]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        let ok = calendars
            .iter()
            .any(|c| &c.id == id && work_reason(c).is_none());
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
        if selected.is_empty() {
            self.held_mut().upcoming.clear();
            return Ok(vec![]);
        }
        let calendars = source.calendars()?;
        let allowed: Vec<&CalendarInfo> = calendars
            .iter()
            .filter(|c| selected.contains(&c.id) && work_reason(c).is_none())
            .collect();
        if allowed.is_empty() {
            self.held_mut().upcoming.clear();
            return Ok(vec![]);
        }
        let ids: Vec<String> = allowed.iter().map(|c| c.id.clone()).collect();
        let mut events: Vec<UpcomingEvent> = Vec::new();
        for raw in source.events(&ids, now, now + horizon_secs)? {
            let Some(cal) = allowed.iter().find(|c| c.id == raw.calendar_id) else {
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
                calendar_name: cal.title.clone(),
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

    pub fn cal(id: &str, title: &str, account: &str, source: &str) -> CalendarInfo {
        CalendarInfo {
            id: id.into(),
            title: title.into(),
            account: account.into(),
            source: source.into(),
        }
    }

    pub fn ev(cal_id: &str, title: &str, start: i64, end: i64, who: &[&str]) -> RawEvent {
        RawEvent {
            calendar_id: cal_id.into(),
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
        cal("h", "Home", "iCloud", "iCloud")
    }

    #[test]
    fn work_corporate_and_exchange_calendars_are_flagged() {
        let flagged = [
            cal("1", "Work", "iCloud", "iCloud"),
            cal("2", "Team", "me@acme.com", "Exchange"),
            cal("3", "Calendar", "Corporate Mail", "Other"),
            cal("4", "Calendar", "me@company.example", "Employer"),
            cal("5", "Project", "Office 365", "Other"),
            cal("6", "Workplace events", "Local", "Local"),
        ];
        for c in flagged {
            assert!(work_reason(&c) == Some(BlockedReason::Work), "{}", c.title);
        }
    }

    #[test]
    fn personal_calendars_are_not_flagged() {
        for c in [
            home(),
            cal("2", "Birthdays", "iCloud", "Subscribed"),
            cal("3", "Networking dinners", "Local", "Local"),
            cal("4", "Family", "me@gmail.com", "CalDAV"),
        ] {
            assert!(work_reason(&c).is_none(), "{}", c.title);
        }
    }

    #[test]
    fn picker_rows_start_unselected_and_blocked_rows_say_why() {
        let rows = calendar_choices(&[home(), cal("w", "Work", "x", "Exchange")], &[]);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].blocked.is_none());
        assert!(rows[1].blocked == Some(BlockedReason::Work));
    }

    #[test]
    fn saved_selection_drops_unknown_and_work_calendars() {
        let cals = [home(), cal("w", "Work", "x", "Exchange")];
        let ids: Vec<String> = ["h", "w", "gone"].iter().map(|s| s.to_string()).collect();
        assert_eq!(sanitize_selection(&ids, &cals), vec!["h".to_string()]);
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
    fn a_selected_work_calendar_is_never_read() {
        let src = FakeCalendar::new(
            vec![home(), cal("w", "Work", "x", "Exchange")],
            vec![
                ev("h", "Dentist", 100, 200, &[]),
                ev("w", "Board prep", 100, 200, &["Boss"]),
            ],
        );
        let store = UpcomingStore::default();
        let sel = vec!["h".to_string(), "w".to_string()];
        let out = store.refresh(&src, &sel, 0, 1000).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Dentist");
        assert_eq!(*src.asked.lock().unwrap(), vec!["h".to_string()]);
    }

    #[test]
    fn only_selected_calendars_in_the_window_are_kept_in_start_order() {
        let src = FakeCalendar::new(
            vec![home(), cal("f", "Family", "iCloud", "iCloud")],
            vec![
                ev("h", "Later", 500, 600, &["Ann", "", "Ann", "Bo"]),
                ev("h", "Sooner", 100, 200, &[]),
                ev("h", "Past", 10, 20, &[]),
                ev("h", "Too far", 5000, 6000, &[]),
                ev("f", "Not picked", 100, 200, &[]),
            ],
        );
        let store = UpcomingStore::default();
        let out = store.refresh(&src, &["h".to_string()], 50, 1000).unwrap();
        let titles: Vec<&str> = out.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Sooner", "Later"]);
        assert_eq!(out[1].calendar_name, "Home");
        assert_eq!(out[1].attendee_display_names, ["Ann", "Bo"]);
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
        assert!(store.start_notes(999, "Nope", "Home").is_none());
        store.close();
        let (upcoming, kept) = store.held();
        assert!(upcoming.is_empty());
        assert_eq!(kept.map(|e| e.title), Some("A".to_string()));
        store.end_notes();
        assert!(store.held().1.is_none());
    }

    #[test]
    fn closing_without_notes_leaves_nothing_in_memory() {
        let src = FakeCalendar::new(vec![home()], vec![ev("h", "A", 100, 200, &[])]);
        let store = UpcomingStore::default();
        store.refresh(&src, &["h".to_string()], 0, 1000).unwrap();
        store.close();
        let (upcoming, kept) = store.held();
        assert!(upcoming.is_empty() && kept.is_none());
    }

    #[test]
    fn calendar_data_never_shows_up_in_debug_output_or_logs() {
        let e = UpcomingEvent {
            title: "Secret merger".into(),
            start: 1,
            end: 2,
            calendar_name: "Home".into(),
            attendee_display_names: vec!["Ann".into()],
        };
        // No Debug on the types, so a stray `{:?}` or `error!("{e:?}")` cannot compile.
        // This asserts the module source stays free of log, network and model code.
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
        let _ = e;
    }
}
