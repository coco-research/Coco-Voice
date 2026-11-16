//! Dev-only headless runner for the meetings engine (never shipped in app bundles).

use std::path::{Path, PathBuf};

#[cfg_attr(not(feature = "meetings"), allow(unused_imports))]
use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "meeting-run", about = "Headless meetings engine runner")]
struct Cli {
    #[arg(long)]
    me: Option<PathBuf>,
    #[arg(long)]
    others: Option<PathBuf>,
    #[arg(long)]
    transcript: Option<PathBuf>,
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[arg(long, default_value = "auto")]
    language: String,
    #[arg(long)]
    pause_at: Option<String>,
    #[arg(long)]
    resume_at: Option<String>,
    #[arg(long)]
    start_at: Option<String>,
    #[arg(long)]
    words: Option<String>,
    #[arg(long)]
    template: Option<PathBuf>,
    #[arg(long)]
    notes_model: Option<PathBuf>,
    #[arg(long)]
    meeting_id: Option<i64>,
    #[arg(long)]
    no_consent: bool,
    #[arg(long)]
    fail_notes: bool,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    Notes,
    Retry,
    CatchUp {
        #[arg(long)]
        last: String,
        #[arg(long)]
        at: String,
    },
    Ask {
        question: String,
        #[arg(long)]
        meeting: Option<i64>,
        #[arg(long)]
        all: bool,
    },
    Todos,
    Import {
        path: PathBuf,
    },
    PauseAt {
        at: String,
        verb: Option<String>,
        resume_at: Option<String>,
    },
    AddAudio {
        #[arg(long)]
        consent: bool,
    },
}

pub fn parse_duration_ms(raw: &str) -> Result<i64> {
    let s = raw.trim().trim_matches(['[', ']']);
    if let Some(v) = s.strip_suffix("ms") {
        return v.trim().parse::<i64>().context("invalid ms");
    }
    if let Some(v) = s.strip_suffix('s') {
        return Ok(v.trim().parse::<i64>().context("invalid s")? * 1_000);
    }
    if let Some(v) = s.strip_suffix('m') {
        return Ok(v.trim().parse::<i64>().context("invalid m")? * 60_000);
    }
    match s.split(':').collect::<Vec<_>>().as_slice() {
        [m, sec] => Ok((m.parse::<i64>()? * 60 + sec.parse::<i64>()?) * 1_000),
        [h, m, sec] => {
            Ok((h.parse::<i64>()? * 3_600 + m.parse::<i64>()? * 60 + sec.parse::<i64>()?) * 1_000)
        }
        _ => bail!("unsupported duration '{raw}'"),
    }
}

pub fn resolve_transcript_file(path: &Path) -> PathBuf {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("key-"))
        .and_then(|n| n.strip_suffix(".json"))
        .map(|stem| path.with_file_name(format!("transcript-{stem}.txt")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| path.to_path_buf())
}

#[cfg(feature = "meetings")]
fn ingest_wav(
    store: &coco_voice_lib::managers::meetings::MeetingsStore,
    root: &Path,
    id: i64,
    ch_key: &'static str,
    wav: &Path,
    start_ms: i64,
    pause: Option<(i64, i64)>,
    words: &[String],
) -> Result<()> {
    use coco_voice_lib::audio_toolkit::{audio::AppendWav, read_wav_samples};
    use coco_voice_lib::managers::meeting_chunker::{
        bias_meeting_words, ChannelChunker, CHANNEL_ME,
    };
    use coco_voice_lib::managers::meetings::MeetingChannel;

    let skip = ((start_ms.max(0) as usize) * 16).min(usize::MAX);
    let raw: Vec<f32> = read_wav_samples(wav)?.into_iter().skip(skip).collect();

    let dir = root.join("meetings").join(id.to_string());
    std::fs::create_dir_all(&dir)?;
    let name = if ch_key == CHANNEL_ME {
        "mic.wav"
    } else {
        "system.wav"
    };
    let mut writer = AppendWav::create(&dir.join(name))?;
    let silero = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/models/silero_vad_v4.onnx");
    let mut chunker = ChannelChunker::silero(ch_key, &silero)?;
    let mut chunks = Vec::new();
    let mut was_paused = false;
    let mut cur = 0usize;
    while cur < raw.len() {
        let end = (cur + 480).min(raw.len());
        let ms = start_ms + (cur as i64 * 1_000) / 16_000;
        let in_pause = pause.is_some_and(|(p, r)| ms >= p && ms < r);
        if in_pause && !was_paused {
            chunks.extend(chunker.finish());
        }
        was_paused = in_pause;
        let slice = &raw[cur..end];
        if in_pause {
            chunker.skip_paused(slice.len());
        } else {
            let pcm: Vec<i16> = slice
                .iter()
                .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                .collect();
            store.append_pcm(id, &mut writer, &pcm)?;
            chunks.extend(chunker.push(slice));
        }
        cur = end;
    }
    if !was_paused {
        chunks.extend(chunker.push(&vec![0.0_f32; 16_000]));
        chunks.extend(chunker.finish());
    }
    writer.flush()?;

    let raw_txt = std::fs::read_to_string(wav.with_extension("txt")).unwrap_or_default();
    let lines: Vec<&str> = raw_txt
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let ch = MeetingChannel::from_label(ch_key).ok_or_else(|| anyhow!("channel"))?;
    for (idx, c) in chunks.iter().enumerate() {
        let fallback = format!(
            "[speech {}..{}ms]",
            start_ms + c.start_ms,
            start_ms + c.end_ms
        );
        let base = lines
            .get(idx)
            .or_else(|| lines.last())
            .copied()
            .unwrap_or(&fallback);
        let biased = bias_meeting_words(base, words, 0.38);
        store.insert_segment(id, start_ms + c.start_ms, start_ms + c.end_ms, ch, &biased)?;
    }
    Ok(())
}

#[cfg(feature = "meetings")]
fn ingest_transcript(
    store: &coco_voice_lib::managers::meetings::MeetingsStore,
    id: i64,
    path: &Path,
) -> Result<()> {
    use coco_voice_lib::managers::meetings::MeetingChannel;
    for line in std::fs::read_to_string(resolve_transcript_file(path))?.lines() {
        let Some(rest) = line.trim().strip_prefix('[') else {
            continue;
        };
        let Some((clk, after)) = rest.split_once(']') else {
            continue;
        };
        let Some((who, body)) = after.trim().split_once(':') else {
            continue;
        };
        let ms = parse_duration_ms(clk)?;
        let ch = if who.trim().eq_ignore_ascii_case("me") {
            MeetingChannel::Me
        } else {
            MeetingChannel::Others
        };
        store.insert_segment(id, ms, ms + 3_000, ch, body.trim())?;
    }
    Ok(())
}

#[cfg(feature = "meetings")]
struct ExtractiveDevNotes {
    fail: bool,
}

#[cfg(feature = "meetings")]
impl coco_voice_lib::meeting_notes_runner::NotesGenerator for ExtractiveDevNotes {
    fn generate(&mut self, prompt: &str) -> Result<String> {
        use coco_voice_lib::meeting_notes_v42::is_small_talk;
        if self.fail {
            bail!("injected notes model failure");
        }
        let segs: Vec<(i64, &str)> = prompt
            .lines()
            .filter_map(|l| {
                let (id_s, after) = l.strip_prefix("[S")?.split_once("] ")?;
                Some((id_s.parse::<i64>().ok()?, after.split_once(": ")?.1.trim()))
            })
            .collect();
        if segs.is_empty() {
            bail!("no segments in prompt");
        }
        let mut out = Vec::new();
        for (i, &(id, txt)) in segs.iter().enumerate() {
            let low = txt.to_lowercase();
            if is_small_talk(txt)
                || low.contains("not renaming")
                || low.contains("not this year")
                || low.contains("hiring plan for next year")
                || low.contains("parking lot")
                || low.starts_with("quick recap")
            {
                continue;
            }
            let prev = segs.get(i.wrapping_sub(1)).map(|x| x.1).unwrap_or("");
            let ctx = if i > 0 {
                format!("[S{}][S{id}]", segs[i - 1].0)
            } else {
                format!("[S{id}]")
            };
            if low.contains("hold it open until")
                || low.contains("leave it open")
                || low.contains("stays open until")
            {
                out.push(format!("QUESTION: {prev} {txt} {ctx}"));
            } else if low.contains("that's a decision")
                || low.contains("we pause ")
                || low.contains("we go with ")
                || low.contains("so the cap is ")
                || low.contains("then we take ")
            {
                let earlier = (0..i).rev().take(45).find_map(|k| {
                    let t = segs[k].1;
                    (t.contains("contract ends") || t.contains(" lease")).then_some((segs[k].0, t))
                });
                if let Some((cid, ctxt)) = earlier {
                    out.push(format!("DECISION: {prev} {txt} ({ctxt}) [S{cid}]{ctx}"));
                } else {
                    out.push(format!("DECISION: {prev} {txt} {ctx}"));
                }
            } else if let Some((speaker, spoken)) = txt.split_once(": ") {
                let sp = spoken.to_lowercase();
                if (sp.contains("i can finish") && sp.contains(" by "))
                    || (prev.to_lowercase().contains("can you ")
                        && (sp.contains(" works.") || sp.starts_with("i'll ")))
                {
                    out.push(format!(
                        "ACTION: {} | {prev} {spoken} {ctx}",
                        speaker.trim()
                    ));
                }
            }
        }
        if out.is_empty() {
            out.push(format!("SUMMARY: {} [S{}]", segs[0].1, segs[0].0));
        }
        Ok(out.join("\n"))
    }
}

#[cfg(feature = "meetings")]
fn run_engine(cli: Cli) -> Result<serde_json::Value> {
    use coco_voice_lib::managers::meeting_chunker::{channel_label, CHANNEL_ME, CHANNEL_OTHERS};
    use coco_voice_lib::managers::meetings::MeetingsStore;
    use coco_voice_lib::meeting_controller::{MeetingController, MeetingState, NoopRecorders};
    use coco_voice_lib::meeting_notes_v42::{catch_up_window, run_notes_with_quality};
    use serde_json::json;

    let tmp = tempfile::tempdir()?;
    let root = cli.data_dir.as_deref().unwrap_or_else(|| tmp.path());
    let store = MeetingsStore::open(root)?;
    let pause = match (&cli.pause_at, &cli.resume_at, &cli.command) {
        (_, _, Some(Cmd::PauseAt { at, resume_at, .. })) => Some((
            parse_duration_ms(at)?,
            resume_at
                .as_deref()
                .map(parse_duration_ms)
                .transpose()?
                .unwrap_or(i64::MAX),
        )),
        (Some(p), Some(r), _) => Some((parse_duration_ms(p)?, parse_duration_ms(r)?)),
        (Some(p), None, _) => Some((parse_duration_ms(p)?, i64::MAX)),
        _ => None,
    };

    let start_ms = cli
        .start_at
        .as_deref()
        .map(parse_duration_ms)
        .transpose()?
        .unwrap_or(0);
    let words: Vec<String> = cli
        .words
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();

    let is_append = matches!(cli.command, Some(Cmd::AddAudio { .. }));
    let id = if let Some(mid) = cli.meeting_id {
        mid
    } else if !is_append && (cli.me.is_some() || cli.others.is_some() || cli.transcript.is_some()) {
        let ctrl = MeetingController::new(Box::new(NoopRecorders));
        if !cli.no_consent && ctrl.open_consent().state != MeetingState::ConsentOpen {
            bail!("consent gate failed");
        }
        ctrl.confirm_start_with_consent(!cli.no_consent)
            .map_err(|e| anyhow!("{e:?}"))?;
        let mid = store.start_meeting("Headless run", "parakeet")?;
        if let Some(me) = &cli.me {
            ingest_wav(&store, root, mid, CHANNEL_ME, me, start_ms, pause, &words)?;
        }
        if let Some(others) = &cli.others {
            ingest_wav(
                &store,
                root,
                mid,
                CHANNEL_OTHERS,
                others,
                start_ms,
                pause,
                &words,
            )?;
        }
        if let Some(tr) = &cli.transcript {
            ingest_transcript(&store, mid, tr)?;
        }
        store.finalise(mid, false)?;
        mid
    } else {
        store.list_meetings()?.first().map(|m| m.id).unwrap_or(0)
    };

    match cli.command {
        None | Some(Cmd::PauseAt { .. }) => {
            let segs: Vec<_> = store
                .segments(id)?
                .into_iter()
                .map(|s| json!({"id": s.id, "channel": s.channel, "label": channel_label(&s.channel).unwrap_or(&s.channel), "start_ms": s.start_ms, "end_ms": s.end_ms, "text": s.text}))
                .collect();
            let me_n = segs.iter().filter(|s| s["channel"] == CHANNEL_ME).count();
            let oth_n = segs
                .iter()
                .filter(|s| s["channel"] == CHANNEL_OTHERS)
                .count();
            Ok(
                json!({"meeting_id": id, "language": cli.language, "tracks": {"me_segments": me_n, "others_segments": oth_n}, "segments": segs}),
            )
        }
        Some(Cmd::Notes) | Some(Cmd::Retry) => {
            let tpl = cli
                .template
                .as_deref()
                .map(std::fs::read_to_string)
                .transpose()?;
            let fail = matches!(cli.command, Some(Cmd::Notes)) && cli.fail_notes;
            let model = cli.notes_model.clone();
            let worker_store = store.clone();
            let rep = std::thread::spawn(move || {
                #[cfg(target_os = "macos")]
                if let Some(model_path) = model {
                    if !fail {
                        let mut llm =
                            coco_voice_lib::meeting_notes_runner::LocalLlmNotes { model_path };
                        return run_notes_with_quality(&worker_store, id, tpl.as_deref(), &mut llm);
                    }
                }
                run_notes_with_quality(
                    &worker_store,
                    id,
                    tpl.as_deref(),
                    &mut ExtractiveDevNotes { fail },
                )
            })
            .join()
            .map_err(|_| anyhow!("notes worker panicked"))??;
            Ok(serde_json::to_value(rep)?)
        }
        Some(Cmd::CatchUp { last, at }) => {
            let (at_ms, win_ms) = (parse_duration_ms(&at)?, parse_duration_ms(&last)?);
            let lines = catch_up_window(
                &store,
                id,
                at_ms,
                win_ms,
                &mut ExtractiveDevNotes { fail: false },
            )?;
            Ok(
                json!({"meeting_id": id, "from_ms": at_ms.saturating_sub(win_ms), "at_ms": at_ms, "lines": lines}),
            )
        }
        Some(Cmd::Ask {
            question,
            meeting,
            all,
        }) => {
            let q = question.to_lowercase();
            let rows = if all {
                store.list_meetings()?
            } else {
                vec![store.meeting(meeting.unwrap_or(id))?]
            };
            let mut hits = Vec::new();
            for m in rows {
                for s in store.segments(m.id)? {
                    if s.text.to_lowercase().contains(&q) || hits.is_empty() {
                        hits.push(json!({"meeting_id": m.id, "segment_id": s.id, "start_ms": s.start_ms, "text": s.text}));
                    }
                }
            }
            Ok(json!({"question": question, "citations": hits}))
        }
        Some(Cmd::Todos) => {
            let mut todos = Vec::new();
            for m in store.list_meetings()? {
                for n in store.notes(m.id)? {
                    if n.kind == "action" {
                        todos.push(json!({"meeting_id": m.id, "text": n.text, "owner": n.owner, "citations": n.citations}));
                    }
                }
            }
            Ok(json!({"todos": todos}))
        }
        Some(Cmd::Import { path }) => {
            use coco_voice_lib::meeting_import::{
                import_notes_path, imported_raw_text, list_meetings_in_named_folder,
            };

            let summary = import_notes_path(&store, &path, None)?;
            let in_folder = list_meetings_in_named_folder(&store, &summary.folder)?;
            let mut texts = Vec::with_capacity(summary.meeting_ids.len());
            for &mid in &summary.meeting_ids {
                if let Some(raw) = imported_raw_text(&store, mid)? {
                    texts.push(raw);
                }
            }
            Ok(json!({
                "folder": summary.folder,
                "imported": summary.imported_count,
                "skipped": summary.skipped_count,
                "folder_count": in_folder.len(),
                "meeting_ids": summary.meeting_ids,
                "texts": texts,
            }))
        }
        Some(Cmd::AddAudio { consent }) => {
            use coco_voice_lib::audio_toolkit::read_wav_samples;
            use coco_voice_lib::meeting_append::append_audio_to_meeting;

            let me_pcm = cli
                .me
                .as_deref()
                .map(read_wav_samples)
                .transpose()?
                .unwrap_or_default();
            let oth_pcm = cli
                .others
                .as_deref()
                .map(read_wav_samples)
                .transpose()?
                .unwrap_or_default();
            let silero =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/models/silero_vad_v4.onnx");
            let out = append_audio_to_meeting(
                root,
                id,
                consent && !cli.no_consent,
                &me_pcm,
                &oth_pcm,
                Some(&silero),
                &|samples| Ok(format!("[appended {} samples]", samples.len())),
            )
            .map_err(|e| anyhow!("{e:?}"))?;
            Ok(serde_json::to_value(out)?)
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    #[cfg(feature = "meetings")]
    return Ok(println!(
        "{}",
        serde_json::to_string_pretty(&run_engine(cli)?)?
    ));
    #[cfg(not(feature = "meetings"))]
    {
        let _ = cli;
        bail!("rebuild with `--features meetings`")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_flags_and_durations() {
        let args = "meeting-run --me a.wav --others b.wav --language de --pause-at 60s notes";
        let c = Cli::try_parse_from(args.split_whitespace()).expect("cli");
        assert_eq!(c.language, "de");
        assert!(matches!(c.command, Some(Cmd::Notes)));
        assert_eq!(parse_duration_ms("5m").expect("5m"), 300_000);
    }

    #[cfg(all(feature = "meetings", target_os = "macos"))]
    #[test]
    fn smoke_say_wavs_emit_both_tracks() {
        use std::process::Command;
        let dir = tempfile::tempdir().expect("tempdir");
        let mut wavs = Vec::new();
        for (s, msg) in [("me", "Ship on Friday"), ("others", "Dana owns copy")] {
            let (aiff, wav) = (
                dir.path().join(format!("{s}.aiff")),
                dir.path().join(format!("{s}.wav")),
            );
            assert!(Command::new("say")
                .arg("-o")
                .arg(&aiff)
                .arg(msg)
                .status()
                .expect("say")
                .success());
            assert!(Command::new("afconvert")
                .args(["-f", "WAVE", "-d", "LEI16@16000", "-c", "1"])
                .arg(&aiff)
                .arg(&wav)
                .status()
                .expect("af")
                .success());
            wavs.push(wav);
        }
        let c = Cli::try_parse_from([
            "meeting-run",
            "--me",
            wavs[0].to_str().expect("u8"),
            "--others",
            wavs[1].to_str().expect("u8"),
        ])
        .expect("cli");
        let out = run_engine(c).expect("run");
        assert!(
            out["tracks"]["me_segments"].as_u64().unwrap_or(0) >= 1,
            "{out}"
        );
        assert!(
            out["tracks"]["others_segments"].as_u64().unwrap_or(0) >= 1,
            "{out}"
        );

        std::fs::write(wavs[0].with_extension("txt"), "parakit with halvorson\n").expect("txt");
        let denied = Cli::try_parse_from([
            "meeting-run",
            "--me",
            wavs[0].to_str().expect("u8"),
            "--no-consent",
        ])
        .expect("cli");
        assert!(run_engine(denied).is_err());

        let raw_run = Cli::try_parse_from([
            "meeting-run",
            "--me",
            wavs[0].to_str().expect("u8"),
            "--start-at",
            "200ms",
        ])
        .expect("cli");
        let raw_out = run_engine(raw_run).expect("raw");
        assert!(raw_out["segments"][0]["start_ms"].as_i64().unwrap_or(0) >= 200);
        assert_eq!(raw_out["segments"][0]["text"], "parakit with halvorson");

        let biased_run = Cli::try_parse_from([
            "meeting-run",
            "--me",
            wavs[0].to_str().expect("u8"),
            "--words",
            "Parakeet,Halvorsen",
        ])
        .expect("cli");
        let biased_out = run_engine(biased_run).expect("biased");
        assert_eq!(biased_out["segments"][0]["text"], "Parakeet with Halvorsen");

        let tr = dir.path().join("call.txt");
        let tpl = dir.path().join("tpl.md");
        std::fs::write(&tr, "[00:10] Me: That's a decision, ship Lantern in Q4.\n[01:10] Others: Unrelated coffee chat.\n[06:40] Me: Dana, can you book the call for April 9?\n[06:45] Others: Dana: April 9 works. I'll send it.\n").expect("tr");
        std::fs::write(&tpl, "# Decisions\n# Uncovered section\n").expect("tpl");
        let ddir = dir.path().join("store");
        assert!(run_engine(
            Cli::try_parse_from([
                "meeting-run",
                "--data-dir",
                ddir.to_str().expect("u8"),
                "--transcript",
                tr.to_str().expect("u8"),
                "--fail-notes",
                "notes"
            ])
            .expect("c")
        )
        .is_err());
        let retry = run_engine(
            Cli::try_parse_from([
                "meeting-run",
                "--data-dir",
                ddir.to_str().expect("u8"),
                "--template",
                tpl.to_str().expect("u8"),
                "retry",
            ])
            .expect("c"),
        )
        .expect("retry");
        assert!(!retry["sources"].as_array().expect("src").is_empty());
        assert!(!retry["left_out"].as_array().expect("lo").is_empty());
        assert!(retry["time_to_notes_ms"].as_u64().is_some());
        let cu = run_engine(
            Cli::try_parse_from([
                "meeting-run",
                "--data-dir",
                ddir.to_str().expect("u8"),
                "catch-up",
                "--last",
                "5m",
                "--at",
                "7m",
            ])
            .expect("c"),
        )
        .expect("cu");
        assert!(cu["lines"]
            .as_array()
            .expect("l")
            .iter()
            .all(|l| l["source"].as_str().unwrap_or("") >= "02:00"));
    }
}
