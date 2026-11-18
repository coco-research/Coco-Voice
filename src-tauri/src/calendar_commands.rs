//! Commands for the calendar picker and the Coming up list. They only forward
//! to `calendar_upcoming`; nothing here logs or keeps calendar data.

#[cfg(not(target_os = "macos"))]
use crate::calendar_upcoming::RawEvent;
use crate::calendar_upcoming::{
    calendar_choices, sanitize_selection, CalendarChoice, CalendarInfo, CalendarSource,
    UpcomingEvent, UpcomingStore,
};
use crate::settings;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};

/// How far ahead the list looks.
const HORIZON_SECS: i64 = 7 * 24 * 60 * 60;

pub struct CalendarState {
    source: Box<dyn CalendarSource>,
    store: UpcomingStore,
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
        #[cfg(target_os = "macos")]
        let source: Box<dyn CalendarSource> =
            Box::new(crate::calendar_eventkit::EventKitSource::new());
        #[cfg(not(target_os = "macos"))]
        let source: Box<dyn CalendarSource> = Box::new(NoCalendar);
        Self::with_source(source)
    }

    pub fn with_source(source: Box<dyn CalendarSource>) -> Self {
        Self {
            source,
            store: UpcomingStore::default(),
        }
    }

    fn choices(&self) -> Result<Vec<CalendarChoice>, String> {
        let calendars: Vec<CalendarInfo> = self.source.calendars()?;
        Ok(calendar_choices(&calendars, &[]))
    }

    fn keep_only_allowed(&self, ids: &[String]) -> Result<Vec<String>, String> {
        Ok(sanitize_selection(ids, &self.source.calendars()?))
    }

    fn upcoming(&self, selected: &[String]) -> Result<Vec<UpcomingEvent>, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.store
            .refresh(self.source.as_ref(), selected, now, HORIZON_SECS)
    }
}

// Async so the calendar permission prompt can show while a command waits.
#[tauri::command]
#[specta::specta]
pub async fn list_calendar_choices(
    state: State<'_, CalendarState>,
) -> Result<Vec<CalendarChoice>, String> {
    state.choices()
}

#[tauri::command]
#[specta::specta]
pub async fn change_selected_calendars_setting(
    app: AppHandle,
    state: State<'_, CalendarState>,
    ids: Vec<String>,
) -> Result<Vec<String>, String> {
    let kept = state.keep_only_allowed(&ids)?;
    let saved = kept.clone();
    settings::update_settings(&app, move |s| s.selected_calendars = saved);
    Ok(kept)
}

#[tauri::command]
#[specta::specta]
pub async fn list_upcoming_meetings(
    app: AppHandle,
    state: State<'_, CalendarState>,
) -> Result<Vec<UpcomingEvent>, String> {
    let selected = settings::get_settings(&app).selected_calendars;
    state.upcoming(&selected)
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
    use crate::calendar_upcoming::fake::{cal, ev, FakeCalendar};

    fn state() -> CalendarState {
        CalendarState::with_source(Box::new(FakeCalendar::new(
            vec![
                cal("h", "Home", "iCloud", "iCloud"),
                cal("w", "Work", "x", "Exchange"),
            ],
            vec![ev("h", "Dentist", i64::MAX / 4, i64::MAX / 4 + 10, &[])],
        )))
    }

    #[test]
    fn picker_rows_come_from_the_source_and_none_are_chosen() {
        let rows = state().choices().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[1].blocked.is_some());
    }

    #[test]
    fn saving_a_selection_drops_the_work_calendar() {
        let kept = state()
            .keep_only_allowed(&["h".to_string(), "w".to_string()])
            .unwrap();
        assert_eq!(kept, vec!["h".to_string()]);
    }

    #[test]
    fn nothing_selected_lists_nothing() {
        assert!(state().upcoming(&[]).unwrap().is_empty());
    }
}
