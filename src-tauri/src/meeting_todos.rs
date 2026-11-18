#![allow(dead_code, unused_imports)]
//! Cross-meeting spoken to-dos extracted from the user's meeting track.
//!
//! Parses first-person spoken commitments and completion phrases from the
//! user's audio channel, formats transcript moment citations, and keeps
//! `{app_data_dir}/meetings/todos.db` synchronized with `meetings.db`.

#![cfg(feature = "meetings")]

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;

static MIGRATIONS: &[M] = &[M::up(
    "CREATE TABLE meeting_todos (
        id INTEGER PRIMARY KEY, dedup_key TEXT NOT NULL UNIQUE, text TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'open', origin_meeting_id INTEGER NOT NULL,
        origin_meeting_title TEXT NOT NULL, origin_started_at INTEGER NOT NULL,
        origin_segment_id INTEGER NOT NULL, origin_start_ms INTEGER NOT NULL,
        origin_end_ms INTEGER NOT NULL, completed_meeting_id INTEGER,
        completed_segment_id INTEGER, completed_start_ms INTEGER
    );",
)];

const COMMITMENT_PREFIXES: &[&str] = &[
    "remind me to ",
    "my action item is to ",
    "action item: ",
    "to-do: ",
    "todo: ",
    "i'm going to ",
    "i am going to ",
    "i need to ",
    "i have to ",
    "i will ",
    "i'll ",
    "let me ",
];

const COMPLETION_PREFIXES: &[&str] = &[
    "i already ",
    "i have ",
    "i've ",
    "i sent ",
    "i wrote ",
    "i booked ",
    "i listed ",
    "i finished ",
    "i completed ",
    "i rotated ",
    "i updated ",
    "i shared ",
    "i reviewed ",
    "completed: ",
    "done: ",
];

const STOPWORDS: &str = "a|an|the|to|for|with|on|in|at|by|of|and|or|my|our|this|that|it|up|out|today|tomorrow|week|monday|tuesday|wednesday|thursday|friday";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Open,
    Done,
}

impl TodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Done => "done",
        }
    }

    pub fn from_db(raw: &str) -> Self {
        if raw.eq_ignore_ascii_case("done") {
            Self::Done
        } else {
            Self::Open
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TodoMoment {
    pub meeting_id: i64,
    pub meeting_title: String,
    pub meeting_started_at: i64,
    pub segment_id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub timestamp: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MeetingTodo {
    pub id: i64,
    pub text: String,
    pub status: TodoStatus,
    pub origin: TodoMoment,
    pub completed_meeting_id: Option<i64>,
    pub completed_segment_id: Option<i64>,
    pub completed_start_ms: Option<i64>,
}

pub fn format_timestamp_ms(ms: i64) -> String {
    let total_secs = (ms.max(0)) / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

pub fn is_user_channel(channel: &str) -> bool {
    let c = channel.trim();
    c.eq_ignore_ascii_case("me") || c.eq_ignore_ascii_case("mic")
}

fn content_tokens(text: &str) -> Vec<String> {
    let stops: HashSet<&str> = STOPWORDS.split('|').collect();
    text.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| w.len() >= 2 && !stops.contains(w.as_str()))
        .collect()
}

pub fn extract_commitments(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for raw_clause in text.split(['.', '!', '?', ';', '\n']) {
        let mut clause = raw_clause.trim();
        for lead in [
            "okay,", "okay ", "alright,", "alright ", "yes,", "yeah,", "sure,", "so ",
        ] {
            if clause.len() > lead.len() && clause[..lead.len()].eq_ignore_ascii_case(lead) {
                clause = clause[lead.len()..].trim_start();
            }
        }
        let lower = clause.to_ascii_lowercase();
        if lower.starts_with("i'll be ")
            && !["i'll be sending ", "i'll be writing ", "i'll be updating "]
                .iter()
                .any(|p| lower.starts_with(p))
        {
            continue;
        }
        let Some(prefix) = COMMITMENT_PREFIXES.iter().find(|p| lower.starts_with(**p)) else {
            continue;
        };
        let body = clause[prefix.len()..]
            .trim()
            .trim_end_matches([',', ':', ';', '.'])
            .trim();
        if body.split_whitespace().count() < 2 || body.chars().count() > 160 {
            continue;
        }
        let tokens = content_tokens(body);
        if tokens.is_empty() {
            continue;
        }
        let mut chars = body.chars();
        let display = match chars.next() {
            Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
            None => continue,
        };
        found.push((tokens.join(" "), display));
    }
    found
}

pub fn extract_completion_token_sets(text: &str) -> Vec<HashSet<String>> {
    let mut sets = Vec::new();
    for raw_clause in text.split(['.', '!', '?', ';', '\n']) {
        let clause = raw_clause.trim();
        let lower = clause.to_ascii_lowercase();
        if COMPLETION_PREFIXES.iter().any(|p| lower.starts_with(p)) {
            let toks: HashSet<String> = content_tokens(clause).into_iter().collect();
            if toks.len() >= 2 {
                sets.push(toks);
            }
        }
    }
    sets
}

pub fn completion_matches(dedup_key: &str, completion_tokens: &HashSet<String>) -> bool {
    let mut iter = dedup_key.split_whitespace();
    let _verb = iter.next();
    let targets: Vec<&str> = iter.collect();
    targets.len() >= 2 && targets.iter().all(|t| completion_tokens.contains(*t))
}

type SegRow = (i64, String, i64, i64, i64, i64, String, String);
