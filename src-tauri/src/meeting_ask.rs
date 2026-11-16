//! Grounded local Q&A over [`MeetingAskIndex`] with mandatory resolving citations and spoken WAV input.
//! Never calls any network host; every kept claim resolves to a stored `(meeting_id, segment_id)`.

#![cfg(feature = "meetings")]

// ponytail: ceiling = local prompt + verified [M<id>:S<id>] citations; upgrade path = multi-hop rerank.

pub use crate::meeting_ask_index::{format_mm_ss, AskCitation, AskScope, MeetingAskIndex};
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskAnswer {
    pub question: String,
    pub answer: String,
    pub citations: Vec<AskCitation>,
}

pub trait LocalAskModel {
    fn generate(&self, prompt: &str) -> Result<String>;
}

pub trait SpeechToText {
    fn transcribe_pcm(&self, samples_16khz: &[f32]) -> Result<String>;
}

impl MeetingAskIndex {
    pub fn ask(
        &self,
        scope: &AskScope,
        question: &str,
        model: Option<&dyn LocalAskModel>,
    ) -> Result<AskAnswer> {
        let q = question.trim();
        if q.is_empty() {
            bail!("question cannot be empty");
        }
        self.sync()?;
        let hits = self.retrieve(scope, q)?;
        if hits.is_empty() {
            return Ok(AskAnswer {
                question: q.to_string(),
                answer: "No matching moment found in the selected meetings.".to_string(),
                citations: Vec::new(),
            });
        }
        let single = matches!(scope, AskScope::OneMeeting(_));
        if let Some(m) = model {
            let prompt = build_ask_prompt(q, &hits);
            if let Ok(reply) = m.generate(&prompt) {
                if let Some((ans, cites)) =
                    verify_model_reply(&reply, &hits, &self.data_dir, single)
                {
                    return Ok(AskAnswer {
                        question: q.to_string(),
                        answer: ans,
                        citations: cites,
                    });
                }
            }
        }
        let mut parts = Vec::new();
        let mut citations = Vec::new();
        for c in hits.into_iter().take(3) {
            if !c.resolves_in(&self.data_dir) {
                continue;
            }
            let tag = if single {
                format!("[{}]", c.timestamp)
            } else {
                format!("[{} · {}]", c.meeting_title, c.timestamp)
            };
            parts.push(format!("{} {tag}", c.quote.trim()));
            citations.push(c);
        }
        Ok(AskAnswer {
            question: q.to_string(),
            answer: parts.join(" "),
            citations,
        })
    }

    pub fn ask_by_voice(
        &self,
        scope: &AskScope,
        wav_path: &Path,
        stt: &dyn SpeechToText,
        model: Option<&dyn LocalAskModel>,
    ) -> Result<AskAnswer> {
        let samples = crate::audio_toolkit::read_wav_samples(wav_path)
            .map_err(|e| anyhow!("failed to read question WAV {}: {e}", wav_path.display()))?;
        if samples.is_empty() {
            bail!("question WAV contains no audio samples");
        }
        let spoken = stt.transcribe_pcm(&samples)?;
        self.ask(scope, &spoken, model)
    }
}

pub fn build_ask_prompt(question: &str, hits: &[AskCitation]) -> String {
    let mut out = String::from(
        "Answer the question using only the moments below. Cite every claim as [M<mid>:S<sid>].\n",
    );
    for c in hits {
        out.push_str(&format!(
            "[M{}:S{}] ({} @ {}) {}: {}\n",
            c.meeting_id, c.segment_id, c.meeting_title, c.timestamp, c.channel, c.quote
        ));
    }
    out.push_str(&format!("\nQuestion: {question}\nAnswer:"));
    out
}

fn verify_model_reply(
    reply: &str,
    hits: &[AskCitation],
    data_dir: &Path,
    single: bool,
) -> Option<(String, Vec<AskCitation>)> {
    let by_key: HashMap<(i64, i64), &AskCitation> = hits
        .iter()
        .map(|c| ((c.meeting_id, c.segment_id), c))
        .collect();
    let mut cites = Vec::new();
    let mut rendered = String::new();
    let mut rest = reply.trim();
    while let Some(open) = rest.find("[M") {
        rendered.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find(']') else {
            rest = after;
            continue;
        };
        if let Some((ms, ss)) = after[..close].split_once(":S") {
            if let (Ok(mid), Ok(sid)) = (ms.parse::<i64>(), ss.parse::<i64>()) {
                if let Some(c) = by_key.get(&(mid, sid)) {
                    if c.resolves_in(data_dir) {
                        let disp = if single {
                            format!("[{}]", c.timestamp)
                        } else {
                            format!("[{} · {}]", c.meeting_title, c.timestamp)
                        };
                        rendered.push_str(&disp);
                        if !cites
                            .iter()
                            .any(|x: &AskCitation| x.meeting_id == mid && x.segment_id == sid)
                        {
                            cites.push((*c).clone());
                        }
                    }
                }
            }
        }
        rest = &after[close + 1..];
    }
    rendered.push_str(rest);
    let clean = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    (!cites.is_empty() && !clean.is_empty()).then_some((clean, cites))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_toolkit::audio::AppendWav;
    use crate::meeting_ask_index::tests::seed_three_meetings;

    struct FakeModel(&'static str);
    impl LocalAskModel for FakeModel {
        fn generate(&self, _prompt: &str) -> Result<String> {
            Ok(self.0.to_string())
        }
    }

    struct FakeStt(&'static str);
    impl SpeechToText for FakeStt {
        fn transcribe_pcm(&self, samples: &[f32]) -> Result<String> {
            assert!(!samples.is_empty());
            Ok(self.0.to_string())
        }
    }

    #[test]
    fn ask_one_and_all_resolve_citations_drop_hallucinations_and_match_spoken_wav() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        seed_three_meetings(p);
        let idx = MeetingAskIndex::open(p).unwrap();
        let ask = |s: AskScope, q: &str, m: Option<&dyn LocalAskModel>| idx.ask(&s, q, m).unwrap();

        let one = ask(
            AskScope::OneMeeting(1),
            "What did we decide about the homepage?",
            None,
        );
        assert!(one.citations.len() == 1 && one.citations[0].meeting_id == 1);
        assert!(one.answer.contains("[12:04]") && one.citations[0].resolves_in(p));

        let q_all = "What is the Thursday deadline, and did design critique move it?";
        let all = ask(AskScope::AllMeetings, q_all, None);
        assert!(all.citations.len() == 2 && all.citations.iter().all(|c| c.resolves_in(p)));
        assert!(all.answer.contains("[Weekly product sync · 12:04]"));
        assert!(all.answer.contains("[Design critique · 16:20]"));

        let bad = FakeModel("Invented claim [M999:S999] [M2:S202]");
        let guarded = ask(AskScope::OneMeeting(1), "homepage deadline", Some(&bad));
        assert!(guarded
            .citations
            .iter()
            .all(|c| c.meeting_id == 1 && c.resolves_in(p)));

        let good = FakeModel("Homepage ships Thursday [M1:S101].");
        let cited = ask(AskScope::OneMeeting(1), "homepage deadline", Some(&good));
        assert_eq!(
            (&*cited.answer, cited.citations[0].segment_id),
            ("Homepage ships Thursday [12:04].", 101)
        );

        let wav_path = p.join("question.wav");
        let mut wav = AppendWav::create(&wav_path).unwrap();
        wav.append(&[600i16; 16_000]).unwrap();
        wav.flush().unwrap();
        let spoken = idx
            .ask_by_voice(&AskScope::AllMeetings, &wav_path, &FakeStt(q_all), None)
            .unwrap();
        assert_eq!(spoken, all);
    }
}
