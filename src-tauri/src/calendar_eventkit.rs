//! The real Mac calendar, read through EventKit. Read-only: it lists calendars
//! and reads events, and never saves, changes or deletes anything. Tests never
//! build this type; they use the fake source in `calendar_upcoming`.

use crate::calendar_upcoming::{CalendarInfo, CalendarSource, RawEvent};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_foundation::{NSArray, NSDate, NSString};
use std::sync::mpsc;
use std::time::Duration;

#[link(name = "EventKit", kind = "framework")]
extern "C" {}

/// `EKEntityTypeEvent`.
const ENTITY_EVENT: usize = 0;
/// `EKAuthorizationStatusFullAccess`.
const STATUS_FULL_ACCESS: isize = 3;
/// `EKAuthorizationStatusNotDetermined`.
const STATUS_NOT_DETERMINED: isize = 0;
const PERMISSION_WAIT: Duration = Duration::from_secs(120);

/// Creates no calendar object until the first call, so launching the app never
/// asks for calendar access.
pub struct EventKitSource;

impl EventKitSource {
    pub fn new() -> Self {
        Self
    }
}

fn text(obj: Option<Retained<NSString>>) -> String {
    obj.map(|s| s.to_string()).unwrap_or_default()
}

fn new_store() -> Retained<AnyObject> {
    // SAFETY: `EKEventStore new` takes no arguments and returns a retained object.
    unsafe { msg_send![class!(EKEventStore), new] }
}

/// Full access to events, asking once if the person has not answered yet.
fn ensure_access(store: &AnyObject) -> Result<(), String> {
    // SAFETY: class method with one integer argument and an integer result.
    let status: isize =
        unsafe { msg_send![class!(EKEventStore), authorizationStatusForEntityType: ENTITY_EVENT] };
    if status == STATUS_FULL_ACCESS {
        return Ok(());
    }
    if status != STATUS_NOT_DETERMINED {
        return Err("Calendar access is off for Coco in System Settings.".into());
    }
    let (tx, rx) = mpsc::channel::<bool>();
    let block = RcBlock::new(move |granted: Bool, _error: *mut AnyObject| {
        let _ = tx.send(granted.as_bool());
    });
    // SAFETY: the selector takes one block `(BOOL, NSError *)` and returns nothing.
    unsafe {
        let _: () = msg_send![store, requestFullAccessToEventsWithCompletion: &*block];
    }
    match rx.recv_timeout(PERMISSION_WAIT) {
        Ok(true) => Ok(()),
        _ => Err("Calendar access was not granted.".into()),
    }
}

fn source_kind(code: isize) -> &'static str {
    match code {
        0 => "Local",
        1 => "Exchange",
        2 => "iCloud",
        3 => "CalDAV",
        4 => "Subscribed",
        5 => "Birthdays",
        _ => "Other",
    }
}

fn calendars_of(store: &AnyObject) -> Retained<NSArray<AnyObject>> {
    // SAFETY: one integer argument, returns an array of calendars.
    unsafe { msg_send![store, calendarsForEntityType: ENTITY_EVENT] }
}

fn info_of(calendar_obj: &AnyObject) -> CalendarInfo {
    // SAFETY: plain getters on EKCalendar and EKSource; text getters may return nil.
    unsafe {
        let source: Option<Retained<AnyObject>> = msg_send![calendar_obj, source];
        let (account, kind) = match source {
            Some(src) => {
                let name: Option<Retained<NSString>> = msg_send![&*src, title];
                let code: isize = msg_send![&*src, sourceType];
                (text(name), source_kind(code).to_string())
            }
            None => (String::new(), "Other".to_string()),
        };
        let id: Option<Retained<NSString>> = msg_send![calendar_obj, calendarIdentifier];
        let title: Option<Retained<NSString>> = msg_send![calendar_obj, title];
        CalendarInfo {
            id: text(id),
            title: text(title),
            account,
            source: kind,
        }
    }
}

impl CalendarSource for EventKitSource {
    fn calendars(&self) -> Result<Vec<CalendarInfo>, String> {
        let store = new_store();
        ensure_access(&store)?;
        Ok(calendars_of(&store).iter().map(|c| info_of(&c)).collect())
    }

    fn events(&self, calendar_ids: &[String], from: i64, to: i64) -> Result<Vec<RawEvent>, String> {
        let store = new_store();
        ensure_access(&store)?;
        let wanted: Vec<Retained<AnyObject>> = calendars_of(&store)
            .iter()
            .filter(|c| calendar_ids.contains(&info_of(c).id))
            .collect();
        if wanted.is_empty() {
            return Ok(vec![]);
        }
        let list = NSArray::from_retained_slice(&wanted);
        let start = NSDate::dateWithTimeIntervalSince1970(from as f64);
        let end = NSDate::dateWithTimeIntervalSince1970(to as f64);
        // SAFETY: read-only predicate and fetch; only calendars the caller allowed.
        let found: Retained<NSArray<AnyObject>> = unsafe {
            let predicate: Retained<AnyObject> = msg_send![
                &*store,
                predicateForEventsWithStartDate: &*start,
                endDate: &*end,
                calendars: &*list
            ];
            msg_send![&*store, eventsMatchingPredicate: &*predicate]
        };
        let mut out = Vec::new();
        for event in found.iter() {
            // SAFETY: plain getters on EKEvent, EKCalendar and EKParticipant.
            unsafe {
                let all_day: Bool = msg_send![&*event, isAllDay];
                if all_day.as_bool() {
                    continue;
                }
                let calendar_obj: Retained<AnyObject> = msg_send![&*event, calendar];
                let calendar_id: Option<Retained<NSString>> =
                    msg_send![&*calendar_obj, calendarIdentifier];
                let title: Option<Retained<NSString>> = msg_send![&*event, title];
                let s: Retained<NSDate> = msg_send![&*event, startDate];
                let e: Retained<NSDate> = msg_send![&*event, endDate];
                let people: Option<Retained<NSArray<AnyObject>>> = msg_send![&*event, attendees];
                let names = people
                    .map(|list| {
                        list.iter()
                            .map(|p| {
                                let n: Option<Retained<NSString>> = msg_send![&*p, name];
                                text(n)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(RawEvent {
                    calendar_id: text(calendar_id),
                    title: text(title),
                    start: s.timeIntervalSince1970() as i64,
                    end: e.timeIntervalSince1970() as i64,
                    attendee_display_names: names,
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn eventkit_source_has_no_write_calls() {
        let src = include_str!("calendar_eventkit.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        for banned in [
            "saveEvent",
            "removeEvent",
            "saveCalendar",
            "removeCalendar",
            "commit",
            "setTitle",
            "setNotes",
            "reset",
        ] {
            assert!(
                !body.contains(banned),
                "EventKit source must stay read-only: {banned}"
            );
        }
        for banned in ["log::", "println!", "eprintln!", "reqwest", "llm_client"] {
            assert!(
                !body.contains(banned),
                "calendar reads must not use {banned}"
            );
        }
    }
}
