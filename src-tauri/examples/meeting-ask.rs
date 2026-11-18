//! Dev-only headless runner (`meeting-ask`) for local meeting index and cited Q&A checks.

use anyhow::{anyhow, bail, Result};
use coco_voice_lib::meeting_ask::{AskAnswer, AskScope, MeetingAskIndex, SpeechToText};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::{env, fs};

#[derive(Deserialize)]
struct SeedMeeting {
    id: i64,
    title: String,
    #[serde(default)]
    folder: Option<String>,
    segments: Vec<SeedSegment>,
}

#[derive(Deserialize)]
struct SeedSegment {
    id: i64,
    start_ms: i64,
    end_ms: i64,
    channel: String,
    text: String,
}

struct SidecarTextStt(String);
impl SpeechToText for SidecarTextStt {
    fn transcribe_pcm(&self, samples: &[f32]) -> Result<String> {
        if samples.is_empty() {
            bail!("empty PCM samples");
        }
        Ok(self.0.clone())
    }
}

fn seed_from_json(dir: &Path, json_path: &Path) -> Result<()> {
    let meetings: Vec<SeedMeeting> = serde_json::from_str(&fs::read_to_string(json_path)?)?;
    fs::create_dir_all(dir.join("meetings"))?;
    let c = Connection::open(dir.join("meetings/meetings.db"))?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS meetings (id INTEGER PRIMARY KEY, started_at INTEGER NOT NULL, ended_at INTEGER, title TEXT NOT NULL, status TEXT NOT NULL, model_id TEXT NOT NULL, saved INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE IF NOT EXISTS meeting_segments (id INTEGER PRIMARY KEY, meeting_id INTEGER NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, channel TEXT NOT NULL, text TEXT NOT NULL);",
    )?;
    let idx = MeetingAskIndex::open(dir)?;
    for (i, m) in meetings.into_iter().enumerate() {
        let t0 = 1_700_000_000 + (i as i64) * 3600;
        c.execute(
            "INSERT OR REPLACE INTO meetings VALUES (?1, ?2, ?3, ?4, 'notes_ready', 'local', 0)",
            params![m.id, t0, t0 + 1800, m.title],
        )?;
        for s in m.segments {
            c.execute(
                "INSERT OR REPLACE INTO meeting_segments VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![s.id, m.id, s.start_ms, s.end_ms, s.channel, s.text],
            )?;
        }
        idx.set_folder(m.id, m.folder.as_deref())?;
    }
    idx.sync()?;
    Ok(())
}

fn arg_at<'a>(args: &'a [String], i: &mut usize) -> Result<&'a str> {
    *i += 1;
    args.get(*i)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("missing flag value"))
}

fn run_cli(args: &[String]) -> Result<AskAnswer> {
    let (mut dir, mut seed, mut wav, mut wav_txt) = (None, None, None, None);
    let (mut scope, mut words, mut i) = (AskScope::AllMeetings, Vec::new(), 0usize);
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("usage: meeting-ask --data-dir <DIR> [--seed <JSON>] ask [--meeting <ID>|--all|--folder <NAME>|--set <IDS>] [--question-wav <WAV> --wav-text <TEXT>] [QUESTION...]");
                std::process::exit(0);
            }
            "--data-dir" => dir = Some(PathBuf::from(arg_at(args, &mut i)?)),
            "--seed" => seed = Some(PathBuf::from(arg_at(args, &mut i)?)),
            "ask" | "--all" => {}
            "--meeting" => scope = AskScope::OneMeeting(arg_at(args, &mut i)?.parse()?),
            "--folder" => scope = AskScope::Folder(arg_at(args, &mut i)?.to_string()),
            "--set" => {
                let ids: Result<Vec<i64>, _> = arg_at(args, &mut i)?
                    .split(',')
                    .map(|s| s.trim().parse())
                    .collect();
                scope = AskScope::PickedSet(ids?);
            }
            "--question-wav" => wav = Some(PathBuf::from(arg_at(args, &mut i)?)),
            "--wav-text" => wav_txt = Some(arg_at(args, &mut i)?.to_string()),
            other => words.push(other.to_string()),
        }
        i += 1;
    }
    let dir = dir.ok_or_else(|| anyhow!("--data-dir is required"))?;
    if let Some(s) = &seed {
        seed_from_json(&dir, s)?;
    }
    let idx = MeetingAskIndex::open(&dir)?;
    match wav {
        Some(w) => {
            let txt = wav_txt
                .or_else(|| fs::read_to_string(w.with_extension("txt")).ok())
                .unwrap_or_else(|| words.join(" "));
            idx.ask_by_voice(&scope, &w, &SidecarTextStt(txt), None)
        }
        None => idx.ask(&scope, &words.join(" "), None),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let dir = args
        .iter()
        .position(|a| a == "--data-dir")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    let ans = run_cli(&args)?;
    let ok = dir.as_deref().is_some_and(|d| {
        !ans.citations.is_empty() && ans.citations.iter().all(|c| c.resolves_in(d))
    });
    println!(
        "{}",
        json!({ "question": ans.question, "answer": ans.answer, "citations": ans.citations, "all_citations_resolved": ok })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_runs_meeting_and_all_queries_with_resolving_citations() {
        let dir = tempfile::tempdir().unwrap();
        let seed = dir.path().join("seed.json");
        fs::write(&seed, serde_json::to_string(&json!([
            {"id": 1, "title": "Sync A", "folder": "Product", "segments": [{"id": 10, "start_ms": 60000, "end_ms": 65000, "channel": "me", "text": "Launch moves to Friday"}]},
            {"id": 2, "title": "Sync B", "folder": "Infra", "segments": [{"id": 20, "start_ms": 120000, "end_ms": 125000, "channel": "others", "text": "Friday checklist is ready"}]}
        ])).unwrap()).unwrap();
        let (d, s) = (
            dir.path().to_str().unwrap().to_string(),
            seed.to_str().unwrap().to_string(),
        );
        let all = run_cli(&[
            "--data-dir".into(),
            d.clone(),
            "--seed".into(),
            s,
            "ask".into(),
            "--all".into(),
            "Friday launch".into(),
        ])
        .unwrap();
        assert!(
            all.citations.len() == 2 && all.citations.iter().all(|c| c.resolves_in(dir.path()))
        );
        let one = run_cli(&[
            "--data-dir".into(),
            d,
            "ask".into(),
            "--meeting".into(),
            "1".into(),
            "Friday launch".into(),
        ])
        .unwrap();
        assert!(one.citations.len() == 1 && one.citations[0].meeting_id == 1);
    }
}
