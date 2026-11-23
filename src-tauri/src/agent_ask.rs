//! Local stdio MCP `ask_user` server and pill queue ("Agents can ask you").
//! Off by default; when enabled, an agent asks a question, a second question
//! queues cleanly behind it, and the user's spoken answer returns to the agent.
// ponytail: ceiling = line-delimited stdio JSON-RPC + FIFO voice queue; upgrade path = live IPC bridge to overlay window.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

const APP_IDENTIFIER: &str = "com.cocoresearch.cocovoice";

fn active_bundle_identifier() -> String {
    if let Ok(id) = std::env::var("COCO_VOICE_APP_IDENTIFIER") {
        let t = id.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(contents) = exe.parent().and_then(|p| p.parent()) {
            let plist = contents.join("Info.plist");
            if let Ok(raw) = std::fs::read_to_string(&plist) {
                if let Some(idx) = raw.find("<key>CFBundleIdentifier</key>") {
                    let after = &raw[idx + "<key>CFBundleIdentifier</key>".len()..];
                    if let (Some(s), Some(e)) = (after.find("<string>"), after.find("</string>")) {
                        let val = after[s + 8..e].trim();
                        if !val.is_empty() {
                            return val.to_string();
                        }
                    }
                }
            }
        }
    }
    APP_IDENTIFIER.to_string()
}

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
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
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join("Library/Application Support")
            .join(active_bundle_identifier())
    })
}

pub fn run_stdio(override_dir: Option<&Path>) -> i32 {
    let Some(data_dir) = resolve_data_dir(override_dir) else {
        eprintln!("coco-voice ask_user mcp: could not resolve local data directory");
        return 2;
    };
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    match serve_stdio_production(stdin.lock(), &mut stdout, &data_dir) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("coco-voice ask_user mcp: {err}");
            1
        }
    }
}

/// Production stdio server: never reads answers from any file on disk.
/// Sends each question to the running Coco Voice app over its local Unix domain
/// socket (`agent-ask.sock`) and fails closed when Coco Voice isn't running.
pub fn serve_stdio_production<R: BufRead, W: Write>(r: R, w: &mut W, dir: &Path) -> io::Result<()> {
    serve_stdio_core(r, w, dir, |agent, q| {
        crate::agent_ask_bridge::send_question_over_socket(dir, agent, q)
    })
}

#[cfg(test)]
pub fn serve_stdio_with_voice<R, W, F>(r: R, w: &mut W, dir: &Path, voice_fn: F) -> io::Result<()>
where
    R: BufRead,
    W: Write,
    F: FnMut(&str, &str) -> Result<String, &'static str>,
{
    serve_stdio_core(r, w, dir, voice_fn)
}

fn serve_stdio_core<R, W, F, E>(r: R, w: &mut W, dir: &Path, mut voice_fn: F) -> io::Result<()>
where
    R: BufRead,
    W: Write,
    E: AsRef<str>,
    F: FnMut(&str, &str) -> Result<String, E>,
{
    let mut client = String::from("Local Agent");
    let mut queue = AgentAskQueue::new("fn");
    for line in r.lines() {
        let line = line?;
        if !line.trim().is_empty() {
            if let Some(reply) =
                handle_json_line(line.trim(), dir, &mut client, &mut queue, &mut voice_fn)
            {
                writeln!(w, "{reply}")?;
                w.flush()?;
            }
        }
    }
    Ok(())
}

pub fn handle_json_line<F, E>(
    line: &str,
    data_dir: &Path,
    client: &mut String,
    queue: &mut AgentAskQueue,
    voice_fn: &mut F,
) -> Option<String>
where
    E: AsRef<str>,
    F: FnMut(&str, &str) -> Result<String, E>,
{
    let Ok(req) = serde_json::from_str::<Value>(line) else {
        return Some(rpc_err(Value::Null, -32700, "parse_error").to_string());
    };
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let id = req.get("id").cloned()?;
    if method.starts_with("notifications/") {
        return None;
    }
    let params = req.get("params").unwrap_or(&Value::Null);
    let reply = match method {
        "initialize" => {
            if let Some(name) = params.pointer("/clientInfo/name").and_then(Value::as_str) {
                let clean: String = name
                    .chars()
                    .filter(|c| c.is_ascii_graphic() || *c == ' ')
                    .take(64)
                    .collect();
                if !clean.trim().is_empty() {
                    *client = clean.trim().to_string();
                }
            }
            rpc_ok(
                id,
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "coco-voice", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
        }
        "ping" => rpc_ok(id, json!({})),
        "tools/list" => rpc_ok(id, json!({ "tools": [ask_user_tool_spec()] })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").unwrap_or(&Value::Null);
            rpc_ok(
                id,
                call_ask_user_tool(data_dir, client, queue, name, args, voice_fn),
            )
        }
        _ => rpc_err(id, -32601, "method_not_found"),
    };
    Some(reply.to_string())
}

fn ask_user_tool_spec() -> Value {
    json!({
        "name": "ask_user",
        "description": "Ask the user a question on the recording pill and return their spoken answer.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "question": { "type": "string" },
                "agent_name": { "type": "string" },
                "queued_follow_up": { "type": "string" }
            },
            "required": ["question"]
        }
    })
}

fn call_ask_user_tool<F, E>(
    data_dir: &Path,
    client: &str,
    queue: &mut AgentAskQueue,
    name: &str,
    args: &Value,
    voice_fn: &mut F,
) -> Value
where
    E: AsRef<str>,
    F: FnMut(&str, &str) -> Result<String, E>,
{
    let policy = AgentsCanAskPolicy::load(data_dir);
    if !policy.enabled {
        return tool_err("refused: agents_can_ask_disabled");
    }
    if name != "ask_user" {
        return tool_err("refused: unknown_tool");
    }
    queue.shortcut_hint = policy.shortcut_hint;
    let agent = args
        .get("agent_name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(client);
    let question = args.get("question").and_then(Value::as_str).unwrap_or("");
    if let Err(code) = queue.enqueue(agent, question) {
        return tool_err(code);
    }
    if let Some(follow_up) = args
        .get("queued_follow_up")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        let _ = queue.enqueue(agent, follow_up);
    }
    let asking_pill = queue.pill_state();
    let spoken = match voice_fn(agent, question.trim()) {
        Ok(ans) => ans,
        Err(code) => {
            let _ = queue.pending.pop_front();
            return tool_err(code.as_ref());
        }
    };
    match queue.answer_current(&spoken) {
        Ok((answered, sent_pill)) => {
            let payload = json!({
                "answer": spoken.trim(),
                "agent": answered.agent_name,
                "question": answered.question,
                "asking_pill": asking_pill,
                "sent_pill": sent_pill,
                "next_pill": queue.pill_state(),
            });
            json!({ "content": [{ "type": "text", "text": payload.to_string() }], "isError": false })
        }
        Err(code) => tool_err(code),
    }
}

fn tool_err(code: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": code }], "isError": true })
}

fn rpc_ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn queue_tracks_active_question_waiting_badge_and_sanitization() {
        let mut queue = AgentAskQueue::new("fn");
        assert_eq!(queue.pill_state(), AgentAskPillState::Hidden);

        queue.enqueue("Local Editor", "Which branch?").unwrap();
        queue.enqueue("Local Editor", "Run tests now?").unwrap();
        assert_eq!(queue.waiting_count(), 1);

        let (answered, sent) = queue.answer_current("feat/voice-ask-user").unwrap();
        assert_eq!(answered.question, "Which branch?");
        assert!(matches!(
            sent,
            AgentAskPillState::AnswerSent {
                waiting_count: 1,
                ..
            }
        ));

        let noisy_q = format!("Line1\n\x07Line2 {}", "x".repeat(250));
        let sanitized = sanitize_question(&noisy_q);
        assert!(sanitized.chars().count() <= MAX_QUESTION_CHARS);
        assert!(!sanitized.contains('\n') && !sanitized.contains('\x07'));
    }

    #[test]
    fn mcp_ask_user_enforces_off_by_default_voice_answer_and_second_question_queue() {
        let (prod, _) = include_str!("agent_ask.rs")
            .split_once("#[cfg(test)]")
            .unwrap();
        assert!(
            !prod.contains("TcpListener")
                && !prod.contains("UdpSocket")
                && !prod.contains("reqwest")
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        let mut client = String::from("Local Agent");
        let mut queue = AgentAskQueue::new("fn");

        let req_off = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "ask_user", "arguments": { "question": "Which branch?" } }
        })
        .to_string();
        let reply_off: Value = serde_json::from_str(
            &handle_json_line(&req_off, p, &mut client, &mut queue, &mut |_, _| {
                Ok::<String, &'static str>("main".into())
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(reply_off["result"]["isError"], json!(true));
        assert_eq!(
            reply_off["result"]["content"][0]["text"],
            json!("refused: agents_can_ask_disabled")
        );

        fs::write(
            p.join("settings_store.json"),
            r#"{"settings":{"agents_can_ask_enabled":true}}"#,
        )
        .unwrap();
        let stream = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"Local Editor"}}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ask_user","arguments":{"question":"Which branch?","queued_follow_up":"Run tests now?"}}}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        let mut out = Vec::new();
        serve_stdio_with_voice(Cursor::new(stream), &mut out, p, |_agent, q| {
            assert_eq!(q, "Which branch?");
            Ok("feat/voice-ask-user".to_string())
        })
        .unwrap();
        let replies: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[1]["result"]["tools"][0]["name"], json!("ask_user"));
        assert_eq!(replies[2]["result"]["isError"], json!(false));
        let body: Value =
            serde_json::from_str(replies[2]["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(body["answer"], json!("feat/voice-ask-user"));
        assert_eq!(
            body["asking_pill"]["Asking"]["label"],
            json!("? Local Editor asks: \"Which branch?\" · hold fn to answer")
        );
        assert_eq!(
            body["asking_pill"]["Asking"]["waiting_badge"],
            json!("? 1 more question waiting")
        );
        assert_eq!(
            body["sent_pill"]["AnswerSent"]["label"],
            json!("✓ Answer sent to Local Editor")
        );
        assert_eq!(
            body["next_pill"]["Asking"]["label"],
            json!("? Local Editor asks: \"Run tests now?\" · hold fn to answer")
        );

        // Production path never reads voice_answers.json from disk and fails closed without a live bridge.
        fs::write(p.join("voice_answers.json"), r#"["yes"]"#).unwrap();
        let mut prod_out = Vec::new();
        let spoof_req = json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "tools/call",
            "params": { "name": "ask_user", "arguments": { "question": "May I deploy?" } }
        })
        .to_string();
        serve_stdio_production(Cursor::new(spoof_req), &mut prod_out, p).unwrap();
        let prod_reply: Value =
            serde_json::from_str(String::from_utf8(prod_out).unwrap().trim()).unwrap();
        assert_eq!(prod_reply["result"]["isError"], json!(true));
        #[cfg(unix)]
        assert_eq!(
            prod_reply["result"]["content"][0]["text"],
            json!(crate::agent_ask_bridge::ERR_NOT_RUNNING)
        );
        #[cfg(not(unix))]
        assert_eq!(
            prod_reply["result"]["content"][0]["text"],
            json!(crate::agent_ask_bridge::ERR_WINDOWS_UNSUPPORTED)
        );
    }
}
