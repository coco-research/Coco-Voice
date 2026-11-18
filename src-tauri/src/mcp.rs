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

