//! Opt-in local stdio MCP JSON-RPC 2.0 server (`coco-voice mcp`).
//! Exposes text-only meeting and dictation tools, never audio, with zero network sockets.

// ponytail: ceiling = line-delimited stdio JSON-RPC (<=64 KiB); upgrade path = Content-Length framing when a client exceeds 64 KiB.
use crate::mcp_store::*;
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

const APP_IDENTIFIER: &str = "com.cocoresearch.cocovoice";
const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
pub const MAX_LINE_BYTES: usize = 65_536;
const MAX_TEXT_BYTES: usize = 65_536;

pub fn resolve_data_dir(override_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = override_dir {
        return Some(dir.to_path_buf());
    }
    if let Some(d) = std::env::var("COCO_VOICE_DATA_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return Some(PathBuf::from(d.trim()));
    }
    if let Some(dir) = crate::portable::data_dir() {
        return Some(dir.clone());
    }
    #[cfg(target_os = "macos")]
    {
        let sub = format!("Library/Application Support/{APP_IDENTIFIER}");
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(sub))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(APP_IDENTIFIER))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|base| base.join(APP_IDENTIFIER))
    }
}

pub fn run_stdio(override_dir: Option<&Path>) -> i32 {
    let Some(data_dir) = resolve_data_dir(override_dir) else {
        eprintln!("coco-voice mcp: could not resolve local data directory");
        return 2;
    };
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    match serve_stdio(stdin.lock(), &mut stdout, &data_dir) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("coco-voice mcp: {err}");
            1
        }
    }
}

pub fn serve_stdio<R: BufRead, W: Write>(mut r: R, w: &mut W, dir: &Path) -> io::Result<()> {
    let (mut client, mut buf) = (String::from("local-agent"), Vec::new());
    loop {
        buf.clear();
        let n = (&mut r)
            .take((MAX_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        if n > MAX_LINE_BYTES {
            if buf.last() != Some(&b'\n') {
                drain_to_newline(&mut r)?;
            }
            writeln!(w, "{}", rpc_err(Value::Null, -32600, "request too large"))?;
            w.flush()?;
            continue;
        }
        let Ok(raw) = std::str::from_utf8(&buf) else {
            writeln!(w, "{}", rpc_err(Value::Null, -32700, "parse_error"))?;
            w.flush()?;
            continue;
        };
        if let Some(reply) = (!raw.trim().is_empty())
            .then(|| handle_json_line(raw.trim(), dir, &mut client))
            .flatten()
        {
            writeln!(w, "{reply}")?;
            w.flush()?;
        }
    }
    Ok(())
}

fn drain_to_newline<R: BufRead>(r: &mut R) -> io::Result<()> {
    loop {
        let chunk = r.fill_buf()?;
        if chunk.is_empty() {
            return Ok(());
        }
        if let Some(i) = chunk.iter().position(|&b| b == b'\n') {
            r.consume(i + 1);
            return Ok(());
        }
        let len = chunk.len();
        r.consume(len);
    }
}

pub fn handle_json_line(line: &str, data_dir: &Path, client: &mut String) -> Option<String> {
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
            let info = json!({ "name": "coco-voice", "version": env!("CARGO_PKG_VERSION") });
            rpc_ok(
                id,
                json!({ "protocolVersion": MCP_PROTOCOL_VERSION, "capabilities": { "tools": {} }, "serverInfo": info }),
            )
        }
        "ping" => rpc_ok(id, json!({})),
        "tools/list" => rpc_ok(id, json!({ "tools": tool_specs() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").unwrap_or(&Value::Null);
            rpc_ok(id, call_tool(data_dir, client, name, args))
        }
        _ => rpc_err(id, -32601, "method_not_found"),
    };
    Some(reply.to_string())
}

fn spec(name: &str, desc: &str, arg: Option<(&str, &str, bool)>) -> Value {
    let (props, req) = match arg {
        Some((k, ty, true)) => (json!({ k: { "type": ty } }), json!([k])),
        Some((k, ty, false)) => (json!({ k: { "type": ty } }), json!([])),
        None => (json!({}), json!([])),
    };
    json!({ "name": name, "description": desc, "inputSchema": { "type": "object", "properties": props, "required": req } })
}

fn tool_specs() -> Vec<Value> {
    vec![
        spec(
            "live_meeting_transcript",
            "Read live segments.",
            Some(("after_segment_id", "integer", false)),
        ),
        spec(
            "list_meetings",
            "List completed meetings.",
            Some(("query", "string", false)),
        ),
        spec(
            "get_meeting",
            "Read meeting notes and transcript.",
            Some(("meeting_id", "integer", true)),
        ),
        spec("dictation_history", "Read dictation text entries.", None),
        spec("custom_words", "Read custom words.", None),
    ]
}

fn call_tool(data_dir: &Path, client: &str, name: &str, args: &Value) -> Value {
    let policy = AgentAccessPolicy::load(data_dir);
    if !policy.enabled {
        return tool_err("refused: agent_access_disabled");
    }
    let int_arg = |k: &str| args.get(k).and_then(Value::as_i64);
    let str_arg = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("");
    let res = match name {
        "live_meeting_transcript" if !policy.live => Err("refused: live_access_disabled"),
        "live_meeting_transcript" => {
            read_live_meeting(data_dir, int_arg("after_segment_id").unwrap_or(0))
        }
        "list_meetings" | "get_meeting" | "dictation_history" | "custom_words" if !policy.past => {
            Err("refused: past_access_disabled")
        }
        "list_meetings" => read_past_list(data_dir, str_arg("query")),
        "get_meeting" => int_arg("meeting_id")
            .ok_or("refused: invalid_meeting_id")
            .and_then(|id| read_past_meeting(data_dir, id)),
        "dictation_history" => read_dictation_history(data_dir),
        "custom_words" => Ok((
            "custom_words".into(),
            json!({ "custom_words": policy.custom_words }),
        )),
        _ => Err("refused: unknown_tool"),
    };
    match res {
        Ok((target, payload)) => {
            let mut text = payload.to_string();
            if text.len() > MAX_TEXT_BYTES {
                let mut end = MAX_TEXT_BYTES;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
            }
            let _ = record_read(data_dir, client, name, &target);
            json!({ "content": [{ "type": "text", "text": text }], "isError": false })
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
    use crate::mcp_store::recent_reads;
    use rusqlite::Connection;
    use std::fs;
    use std::io::Cursor;

    fn seed_stores(dir: &Path) {
        fs::create_dir_all(dir.join("meetings/1")).unwrap();
        fs::write(dir.join("meetings/1/mic.wav"), b"RIFF-audio-secret").unwrap();
        let mc = Connection::open(dir.join("meetings/meetings.db")).unwrap();
        mc.execute_batch(
            "CREATE TABLE meetings (id INTEGER PRIMARY KEY, started_at INTEGER NOT NULL, ended_at INTEGER, title TEXT NOT NULL, status TEXT NOT NULL, model_id TEXT NOT NULL, saved INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE meeting_segments (id INTEGER PRIMARY KEY, meeting_id INTEGER NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, channel TEXT NOT NULL, text TEXT NOT NULL);
             CREATE TABLE meeting_notes (id INTEGER PRIMARY KEY, meeting_id INTEGER NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL, owner TEXT, citations TEXT NOT NULL, position INTEGER NOT NULL);
             INSERT INTO meetings VALUES (1, 1700000100, 1700001900, 'Product sync', 'notes_ready', 'whisper-small', 0);
             INSERT INTO meeting_segments VALUES (10, 1, 0, 4000, 'me', 'Ship the homepage on Thursday');
             INSERT INTO meeting_notes VALUES (1, 1, 'decision', 'Ship the homepage on Thursday', 'Me', '[10]', 0);
             INSERT INTO meetings VALUES (2, 1700003000, NULL, 'Live design review', 'recording', 'whisper-small', 0);
             INSERT INTO meeting_segments VALUES (20, 2, 0, 3000, 'others', 'First live line');",
        )
        .unwrap();
        let hc = Connection::open(dir.join("history.db")).unwrap();
        hc.execute_batch(
            "CREATE TABLE transcription_history (id INTEGER PRIMARY KEY AUTOINCREMENT, file_name TEXT NOT NULL, timestamp INTEGER NOT NULL, saved BOOLEAN NOT NULL DEFAULT 0, title TEXT NOT NULL, transcription_text TEXT NOT NULL, post_processed_text TEXT);
             INSERT INTO transcription_history VALUES (1, 'take-secret.wav', 1700002500, 0, 'Quick take', 'raw words', 'Clean dictated words');",
        )
        .unwrap();
    }

    fn call(dir: &Path, id: i64, method: &str, params: Value) -> Value {
        let mut client = String::from("Local Editor");
        let req =
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        serde_json::from_str(&handle_json_line(&req, dir, &mut client).unwrap()).unwrap()
    }

    fn parse_lines(out: Vec<u8>) -> Vec<Value> {
        let s = String::from_utf8(out).unwrap();
        s.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn mcp_client_enforces_off_by_default_no_audio_live_deltas_past_notes_and_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        seed_stores(p);
        let tc = |t: &str, a: Value| call(p, 9, "tools/call", json!({"name": t, "arguments": a}));
        let is_err = |t: &str, a: Value| tc(t, a)["result"]["isError"] == json!(true);
        let listed = call(p, 1, "tools/list", json!({}));
        let names: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names.join(","),
            "live_meeting_transcript,list_meetings,get_meeting,dictation_history,custom_words"
        );
        assert!(names
            .iter()
            .all(|n| !n.contains("audio") && !n.contains("wav")));
        let (prod, _) = include_str!("mcp.rs").split_once("#[cfg(test)]").unwrap();
        assert!(["TcpListener", "UdpSocket", "reqwest"]
            .iter()
            .all(|k| !prod.contains(k)));
        for tool in &names {
            assert!(is_err(tool, json!({"meeting_id": 1})), "{tool}");
        }
        assert!(recent_reads(p).is_empty());

        let sf = p.join("settings_store.json");
        fs::write(
            &sf,
            r#"{"settings":{"agent_access_enabled":true,"custom_words":["CocoVoice"]}}"#,
        )
        .unwrap();
        let stream = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"Local Editor"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"live_meeting_transcript","arguments":{}}}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_meeting","arguments":{"meeting_id":1}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"dictation_history","arguments":{}}}),
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"custom_words","arguments":{}}}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        let mut out = Vec::new();
        serve_stdio(Cursor::new(stream), &mut out, p).unwrap();
        let replies = parse_lines(out);
        assert_eq!(replies.len(), 5);
        let txt = |v: &Value| {
            v["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .to_string()
        };

        let live1: Value = serde_json::from_str(&txt(&replies[1])).unwrap();
        let after = live1["next_after_segment_id"].as_i64().unwrap();
        let ins =
            "INSERT INTO meeting_segments VALUES (21, 2, 3000, 6000, 'me', 'Second live line')";
        Connection::open(p.join("meetings/meetings.db"))
            .unwrap()
            .execute(ins, [])
            .unwrap();
        let delta = tc(
            "live_meeting_transcript",
            json!({"after_segment_id": after}),
        );
        let live2: Value = serde_json::from_str(&txt(&delta)).unwrap();
        assert_eq!(live2["segments"].as_array().unwrap().len(), 1);
        assert_eq!(live2["segments"][0]["text"], json!("Second live line"));

        let past_txt = txt(&replies[2]);
        assert!(!past_txt.contains("mic.wav") && !past_txt.contains("RIFF"));
        let past: Value = serde_json::from_str(&past_txt).unwrap();
        assert_eq!(
            past["notes"][0]["text"],
            json!("Ship the homepage on Thursday")
        );
        let dict_txt = txt(&replies[3]);
        assert!(dict_txt.contains("Clean dictated words") && !dict_txt.contains("take-secret.wav"));
        assert!(txt(&replies[4]).contains("CocoVoice"));
        assert!(is_err("get_meeting", json!({"meeting_id": 2})));

        fs::write(&sf, r#"{"settings":{"agent_access_enabled":false}}"#).unwrap();
        assert!(is_err("list_meetings", json!({})));
    }

    #[test]
    fn oversized_stdio_line_returns_minus_32600_drains_and_serves_next_request() {
        let dir = tempfile::tempdir().unwrap();
        let big = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\",\"pad\":\"{}\"}}\n{{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"ping\"}}\n",
            "x".repeat(70_000)
        );
        let mut out = Vec::new();
        serve_stdio(Cursor::new(big), &mut out, dir.path()).unwrap();
        let rows = parse_lines(out);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["error"]["code"], json!(-32600));
        assert_eq!(
            (&rows[1]["id"], &rows[1]["result"]),
            (&json!(8), &json!({}))
        );
        assert_eq!(MAX_TEXT_BYTES, 65_536);
    }
}
