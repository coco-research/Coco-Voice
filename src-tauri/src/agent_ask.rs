//! Local stdio MCP `ask_user` pill queue and policy ("Agents can ask you").
//! Off by default; when enabled, an agent asks a question, a second question
//! queues cleanly behind it, and the user's spoken answer returns to the agent.
// ponytail: ceiling = line-delimited stdio JSON-RPC + FIFO voice queue; upgrade path = live IPC bridge to overlay window.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

const APP_IDENTIFIER: &str = "com.cocoresearch.cocovoice";
pub(crate) const MAX_QUESTION_CHARS: usize = 200;

pub fn sanitize_question(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(MAX_QUESTION_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentsCanAskPolicy {
    pub enabled: bool,
    pub shortcut_hint: String,
}

impl AgentsCanAskPolicy {
    pub fn load(data_dir: &Path) -> Self {
        let parsed = fs::read_to_string(data_dir.join("settings_store.json"))
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
        let settings = parsed
            .as_ref()
            .and_then(|v| v.get("settings"))
            .or(parsed.as_ref());
        let enabled = settings
            .and_then(|s| s.get("agents_can_ask_enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let shortcut_hint = settings
            .and_then(|s| s.pointer("/bindings/transcribe/current_binding"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("fn")
            .to_string();
        Self {
            enabled,
            shortcut_hint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentQuestion {
    pub id: u64,
    pub agent_name: String,
    pub question: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentAskPillState {
    Hidden,
    Asking {
        agent_name: String,
        question: String,
        waiting_count: usize,
        label: String,
        waiting_badge: Option<String>,
    },
    AnswerSent {
        agent_name: String,
        answer: String,
        waiting_count: usize,
        label: String,
        waiting_badge: Option<String>,
    },
}

#[derive(Debug, Default)]
pub struct AgentAskQueue {
    next_id: u64,
    pub(crate) pending: VecDeque<AgentQuestion>,
    last_sent: Option<(String, String)>,
    pub(crate) shortcut_hint: String,
}

impl AgentAskQueue {
    pub fn new(shortcut_hint: impl Into<String>) -> Self {
        Self {
            next_id: 1,
            pending: VecDeque::new(),
            last_sent: None,
            shortcut_hint: shortcut_hint.into(),
        }
    }

    pub fn enqueue(&mut self, agent_name: &str, question: &str) -> Result<u64, &'static str> {
        let clean_q = sanitize_question(question);
        if clean_q.is_empty() {
            return Err("refused: empty_question");
        }
        let clean_agent = if agent_name.trim().is_empty() {
            "Agent"
        } else {
            agent_name.trim()
        };
        let id = self.next_id;
        self.next_id += 1;
        self.pending.push_back(AgentQuestion {
            id,
            agent_name: clean_agent.to_string(),
            question: clean_q,
        });
        Ok(id)
    }

    pub fn waiting_count(&self) -> usize {
        self.pending.len().saturating_sub(1)
    }

    pub fn waiting_badge(waiting: usize) -> Option<String> {
        match waiting {
            0 => None,
            1 => Some("? 1 more question waiting".to_string()),
            n => Some(format!("? {n} more questions waiting")),
        }
    }

    pub fn pill_state(&self) -> AgentAskPillState {
        let waiting = self.waiting_count();
        let badge = Self::waiting_badge(waiting);
        if let Some(front) = self.pending.front() {
            let key = if self.shortcut_hint.is_empty() {
                "fn"
            } else {
                &self.shortcut_hint
            };
            return AgentAskPillState::Asking {
                agent_name: front.agent_name.clone(),
                question: front.question.clone(),
                waiting_count: waiting,
                label: format!(
                    "? {} asks: \"{}\" · hold {} to answer",
                    front.agent_name, front.question, key
                ),
                waiting_badge: badge,
            };
        }
        if let Some((agent_name, answer)) = &self.last_sent {
            return AgentAskPillState::AnswerSent {
                agent_name: agent_name.clone(),
                answer: answer.clone(),
                waiting_count: 0,
                label: format!("✓ Answer sent to {agent_name}"),
                waiting_badge: None,
            };
        }
        AgentAskPillState::Hidden
    }

    pub fn answer_current(
        &mut self,
        spoken_answer: &str,
    ) -> Result<(AgentQuestion, AgentAskPillState), &'static str> {
        let clean = spoken_answer.trim();
        if clean.is_empty() {
            return Err("refused: empty_voice_answer");
        }
        let answered = self
            .pending
            .pop_front()
            .ok_or("refused: no_active_question")?;
        let waiting_after = self.pending.len();
        let sent_pill = AgentAskPillState::AnswerSent {
            agent_name: answered.agent_name.clone(),
            answer: clean.to_string(),
            waiting_count: waiting_after,
            label: format!("✓ Answer sent to {}", answered.agent_name),
            waiting_badge: Self::waiting_badge(waiting_after),
        };
        self.last_sent = Some((answered.agent_name.clone(), clean.to_string()));
        Ok((answered, sent_pill))
    }
}

pub fn resolve_data_dir(override_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = override_dir {
        return Some(dir.to_path_buf());
    }
    if let Ok(env_dir) = std::env::var("COCO_VOICE_DATA_DIR") {
        if !env_dir.trim().is_empty() {
            return Some(PathBuf::from(env_dir.trim()));
        }
    }
    if let Some(dir) = crate::portable::data_dir() {
        return Some(dir.clone());
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/Application Support").join(APP_IDENTIFIER))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_tracks_active_question_waiting_badge_and_sanitization() {
        let mut queue = AgentAskQueue::new("fn");
        assert_eq!(queue.pill_state(), AgentAskPillState::Hidden);

        queue.enqueue("Local Editor", "Which branch?").unwrap();
        queue.enqueue("Local Editor", "Run tests now?").unwrap();
        assert_eq!(queue.waiting_count(), 1);

        let (answered, sent) = queue.answer_current("feat/voice-ask-user").unwrap();
        assert_eq!(answered.question, "Which branch?");
        assert!(matches!(sent, AgentAskPillState::AnswerSent { waiting_count: 1, .. }));

        let noisy_q = format!("Line1\n\x07Line2 {}", "x".repeat(250));
        let sanitized = sanitize_question(&noisy_q);
        assert!(sanitized.chars().count() <= MAX_QUESTION_CHARS);
        assert!(!sanitized.contains('\n') && !sanitized.contains('\x07'));
    }
}
