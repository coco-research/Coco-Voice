//! Dev-only headless runner for the meetings engine (never shipped in app bundles).
// ponytail: ceiling = single-process CLI runner over local WAV/transcript fixtures; upgrade path = live IPC harness.

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
    no_consent: bool,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    Notes,
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
    let raw = read_wav_samples(wav)?;
    let mut padded: Vec<f32> = raw.into_iter().skip(skip).collect();
    for (i, sample) in padded.iter_mut().enumerate() {
        let ms = start_ms + (i as i64 * 1_000) / 16_000;
        if pause.is_some_and(|(p, r)| ms >= p && ms < r) {
            *sample = 0.0;
        }
    }
    padded.resize(padded.len() + 16_000, 0.0);

    let dir = root.join("meetings").join(id.to_string());
    std::fs::create_dir_all(&dir)?;
    let name = if ch_key == CHANNEL_ME {
        "mic.wav"
    } else {
        "system.wav"
    };
    let mut writer = AppendWav::create(&dir.join(name))?;
    let pcm: Vec<i16> = padded
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    store.append_pcm(id, &mut writer, &pcm)?;
    writer.flush()?;

    let raw_txt = std::fs::read_to_string(wav.with_extension("txt")).unwrap_or_default();
    let lines: Vec<&str> = raw_txt
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let silero = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/models/silero_vad_v4.onnx");
    let mut chunker = ChannelChunker::silero(ch_key, &silero)?;
    let mut chunks = chunker.push(&padded);
    chunks.extend(chunker.finish());
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
fn run_engine(cli: Cli) -> Result<serde_json::Value> {
    use coco_voice_lib::managers::meeting_chunker::{channel_label, CHANNEL_ME, CHANNEL_OTHERS};
    use coco_voice_lib::managers::meetings::{MeetingChannel, MeetingsStore};
    use coco_voice_lib::meeting_controller::{MeetingController, MeetingState, NoopRecorders};
    use coco_voice_lib::meeting_notes_runner::{generate_meeting_notes, NotesGenerator};
    use serde_json::json;

    let tmp = tempfile::tempdir()?;
    let root = cli.data_dir.as_deref().unwrap_or_else(|| tmp.path());
    let store = MeetingsStore::open(root)?;
    let pause = match (&cli.pause_at, &cli.resume_at) {
        (Some(p), Some(r)) => Some((parse_duration_ms(p)?, parse_duration_ms(r)?)),
        (Some(p), None) => Some((parse_duration_ms(p)?, i64::MAX)),
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

    let id = if cli.me.is_some() || cli.others.is_some() || cli.transcript.is_some() {
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
        None => {
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
        Some(Cmd::Notes) => {
            struct Gen;
            impl NotesGenerator for Gen {
                fn generate(&mut self, p: &str) -> Result<String> {
                    let s = p
                        .lines()
                        .find_map(|l| l.strip_prefix("[S")?.split(']').next()?.parse::<i64>().ok())
                        .ok_or_else(|| anyhow!("empty"))?;
                    Ok(format!("SUMMARY: Transcript captured [S{s}]"))
                }
            }
            let out = generate_meeting_notes(&store, id, &mut Gen, &mut |_| {})?;
            store.mark_notes_ready(id)?;
            let lines: Vec<_> = out.notes.iter().map(|n| json!({"kind": n.kind, "text": n.text, "owner": n.owner, "citations": n.citations})).collect();
            Ok(
                json!({"meeting_id": id, "lines": lines, "oversized_segments": out.oversized_segments}),
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
            let mut n = 0usize;
            for entry in std::fs::read_dir(&path)? {
                let p = entry?.path();
                if p.extension().is_some_and(|e| e == "md" || e == "txt") {
                    let t = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Imported");
                    if store.search_meetings(t)?.iter().all(|m| m.title != t) {
                        let mid = store.start_meeting(t, "imported")?;
                        store.insert_segment(
                            mid,
                            0,
                            0,
                            MeetingChannel::Others,
                            &std::fs::read_to_string(&p)?,
                        )?;
                        store.finalise(mid, false)?;
                        n += 1;
                    }
                }
            }
            Ok(json!({"imported": n}))
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
    }
}
