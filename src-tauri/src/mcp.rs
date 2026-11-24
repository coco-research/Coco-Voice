//! Opt-in local stdio MCP JSON-RPC 2.0 server (`coco-voice mcp`).
//! Exposes text-only meeting tools, never audio, with zero network sockets.

// ponytail: ceiling = line-delimited stdio JSON-RPC (<=64 KiB); upgrade path = Content-Length framing when a client exceeds 64 KiB.
use crate::mcp_store::*;
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
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
        let sub = format!("Library/Application Support/{}", active_bundle_identifier());
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

pub const MEETINGS_NOT_IN_BUILD_MSG: &str = "Meetings aren't in this build.";

/// Entry point for `coco-voice mcp`.
/// Initializes portable mode (stderr-only diagnostics) before serving stdio JSON-RPC
/// so portable installs resolve the right data dir without polluting stdout.
pub fn run_cli() -> i32 {
    #[cfg(feature = "meetings")]
    {
        crate::portable::init();
        run_stdio(None)
    }
    #[cfg(not(feature = "meetings"))]
    {
        eprintln!("{MEETINGS_NOT_IN_BUILD_MSG}");
        2
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
    ]
}

fn call_tool(data_dir: &Path, client: &str, name: &str, args: &Value) -> Value {
    if matches!(name, "dictation_history" | "custom_words") {
        return tool_err("refused: not offered");
    }
    let policy = AgentAccessPolicy::load(data_dir);
    if !policy.enabled {
        return tool_err("refused: agent_access_disabled");
    }
    let int_arg = |k: &str| args.get(k).and_then(Value::as_i64);
    let str_arg = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("");
    let res: Result<(String, &'static str, Value), &'static str> = match name {
        "live_meeting_transcript" if !policy.live => Err("refused: live_access_disabled"),
        "live_meeting_transcript" => {
            read_live_meeting(data_dir, int_arg("after_segment_id").unwrap_or(0))
                .map(|(target, payload)| (target, "live", payload))
        }
        "list_meetings" | "get_meeting" if !policy.past => Err("refused: past_access_disabled"),
        "list_meetings" => read_past_list(data_dir, str_arg("query"))
            .map(|(target, payload)| (target, "list", payload)),
        "get_meeting" => int_arg("meeting_id")
            .ok_or("refused: invalid_meeting_id")
            .and_then(|id| read_past_meeting(data_dir, id))
            .map(|(target, payload)| {
                let has_notes = payload["notes"].as_array().is_some_and(|a| !a.is_empty())
                    || payload["notepad"].as_array().is_some_and(|a| !a.is_empty());
                let kind = if has_notes { "notes" } else { "transcript" };
                (target, kind, payload)
            }),
        _ => Err("refused: unknown_tool"),
    };
    match res {
        Ok((target, kind, payload)) => {
            if let Err(err) = record_read_with_kind(data_dir, client, name, &target, Some(kind)) {
                log::warn!("mcp: failed to record audit entry for '{name}': {err}");
                return tool_err("refused: audit_log_failed");
            }
            let text = serialize_bounded_payload(payload);
            json!({ "content": [{ "type": "text", "text": text }], "isError": false })
        }
        Err(code) => tool_err(code),
    }
}

fn utf8_prefix(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Ensures the serialized tool payload never exceeds `MAX_TEXT_BYTES` while remaining
/// syntactically valid JSON (`"truncated": true` when items or text had to be trimmed).
fn serialize_bounded_payload(payload: Value) -> String {
    let raw = payload.to_string();
    if raw.len() <= MAX_TEXT_BYTES {
        return raw;
    }
    let Value::Object(mut map) = payload else {
        return json!({ "truncated": true }).to_string();
    };
    map.insert("truncated".to_string(), json!(true));
    const ARRAY_KEYS: &[&str] = &[
        "segments",
        "meetings",
        "entries",
        "notes",
        "notepad",
        "custom_words",
    ];
    loop {
        let encoded = Value::Object(map.clone()).to_string();
        if encoded.len() <= MAX_TEXT_BYTES {
            return encoded;
        }
        let best_key = ARRAY_KEYS
            .iter()
            .copied()
            .filter_map(|k| {
                let len = map.get(k)?.as_array()?.len();
                (len > 0).then_some((k, len))
            })
            .max_by_key(|(_, len)| *len);
        let Some((key, len)) = best_key else {
            break;
        };
        if len == 1 {
            if let Some(item_obj) = map
                .get_mut(key)
                .and_then(Value::as_array_mut)
                .and_then(|arr| arr.first_mut())
                .and_then(Value::as_object_mut)
            {
                if let Some(t) = item_obj.get("text").and_then(Value::as_str) {
                    let overhead = encoded.len().saturating_sub(t.len()) + 64;
                    if overhead < MAX_TEXT_BYTES && t.len() > MAX_TEXT_BYTES - overhead {
                        let clipped = format!("{}…", utf8_prefix(t, MAX_TEXT_BYTES - overhead));
                        if clipped.len() < t.len() {
                            item_obj.insert("text".to_string(), json!(clipped));
                            continue;
                        }
                    }
                }
            }
        }
        if let Some(arr) = map.get_mut(key).and_then(Value::as_array_mut) {
            let new_len = if len > 8 && encoded.len() > MAX_TEXT_BYTES * 2 {
                len / 2
            } else {
                len - 1
            };
            arr.truncate(new_len);
        }
    }
    let fallback = json!({ "truncated": true }).to_string();
    debug_assert!(fallback.len() <= MAX_TEXT_BYTES);
    fallback
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
            "live_meeting_transcript,list_meetings,get_meeting"
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
            json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"list_meetings","arguments":{}}}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        let mut out = Vec::new();
        serve_stdio(Cursor::new(stream), &mut out, p).unwrap();
        let replies = parse_lines(out);
        assert_eq!(replies.len(), 6);
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
        assert_eq!(replies[3]["result"]["isError"], json!(true));
        let dict_txt = txt(&replies[3]);
        assert!(dict_txt.contains("not offered"));
        assert!(!dict_txt.contains("Clean dictated words"));
        assert_eq!(replies[4]["result"]["isError"], json!(true));
        let words_txt = txt(&replies[4]);
        assert!(words_txt.contains("not offered"));
        assert!(!words_txt.contains("CocoVoice"));
        assert_eq!(replies[5]["result"]["isError"], json!(false));
        assert!(txt(&replies[5]).contains("Product sync"));
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

    #[test]
    fn tool_specs_expose_real_argument_property_names() {
        let specs = tool_specs();
        let by_name = |n: &str| {
            specs
                .iter()
                .find(|s| s["name"] == json!(n))
                .expect("tool spec")
        };
        let live_props = by_name("live_meeting_transcript")["inputSchema"]["properties"]
            .as_object()
            .unwrap();
        assert_eq!(live_props["after_segment_id"]["type"], json!("integer"));
        assert!(!live_props.contains_key("k"));

        let list_props = by_name("list_meetings")["inputSchema"]["properties"]
            .as_object()
            .unwrap();
        assert_eq!(list_props["query"]["type"], json!("string"));
        assert!(!list_props.contains_key("k"));

        let get_spec = by_name("get_meeting");
        let get_props = get_spec["inputSchema"]["properties"].as_object().unwrap();
        assert_eq!(get_props["meeting_id"]["type"], json!("integer"));
        assert_eq!(get_spec["inputSchema"]["required"], json!(["meeting_id"]));
        assert!(!get_props.contains_key("k"));
    }

    #[test]
    fn oversized_tool_payload_truncates_at_item_boundaries_and_stays_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        seed_stores(p);
        fs::write(
            p.join("settings_store.json"),
            r#"{"settings":{"agent_access_enabled":true}}"#,
        )
        .unwrap();

        let mc = Connection::open(p.join("meetings/meetings.db")).unwrap();
        let line_500b = format!("Transcript segment body — café {}", "w".repeat(480));
        for seg_id in 100..300 {
            mc.execute(
                "INSERT INTO meeting_segments VALUES (?1, 1, ?2, ?3, 'others', ?4)",
                rusqlite::params![seg_id, seg_id * 1000, seg_id * 1000 + 900, &line_500b],
            )
            .unwrap();
        }
        let res = call(
            p,
            11,
            "tools/call",
            json!({"name": "get_meeting", "arguments": {"meeting_id": 1}}),
        );
        assert_eq!(res["result"]["isError"], json!(false));
        let raw_text = res["result"]["content"][0]["text"].as_str().unwrap();
        assert!(raw_text.len() <= MAX_TEXT_BYTES);
        let parsed: Value =
            serde_json::from_str(raw_text).expect("truncated tool output must be valid JSON");
        assert_eq!(parsed["truncated"], json!(true));
        let segs = parsed["segments"].as_array().unwrap();
        assert!(!segs.is_empty() && segs.len() < 200);

        // Single huge segment (> 64 KiB) also clips cleanly inside valid JSON.
        mc.execute("DELETE FROM meeting_segments WHERE meeting_id = 1", [])
            .unwrap();
        let single_huge = format!("中{}", "a".repeat(80_000));
        mc.execute(
            "INSERT INTO meeting_segments VALUES (1, 1, 0, 1000, 'me', ?1)",
            rusqlite::params![&single_huge],
        )
        .unwrap();
        let res_single = call(
            p,
            12,
            "tools/call",
            json!({"name": "get_meeting", "arguments": {"meeting_id": 1}}),
        );
        let single_text = res_single["result"]["content"][0]["text"].as_str().unwrap();
        assert!(single_text.len() <= MAX_TEXT_BYTES);
        let parsed_single: Value = serde_json::from_str(single_text)
            .expect("single oversized segment output must remain valid JSON");
        assert_eq!(parsed_single["truncated"], json!(true));
        assert_eq!(parsed_single["segments"].as_array().unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn audit_log_failure_refuses_tool_read_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        seed_stores(p);
        fs::write(
            p.join("settings_store.json"),
            r#"{"settings":{"agent_access_enabled":true,"custom_words":["SecretTerm"]}}"#,
        )
        .unwrap();
        let victim = p.join("victim.txt");
        fs::write(&victim, "untouched").unwrap();
        std::os::unix::fs::symlink(&victim, p.join("agent_reads.jsonl")).unwrap();

        let res = call(
            p,
            15,
            "tools/call",
            json!({"name": "get_meeting", "arguments": {"meeting_id": 1}}),
        );
        assert_eq!(res["result"]["isError"], json!(true));
        let err_text = res["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(err_text, "refused: audit_log_failed");
        assert!(!err_text.contains("Ship the homepage"));
        assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched");
    }

    #[test]
    fn get_meeting_records_notes_vs_transcript_audit_kind_and_missing_meetings_message() {
        assert_eq!(MEETINGS_NOT_IN_BUILD_MSG, "Meetings aren't in this build.");
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        seed_stores(p);
        Connection::open(p.join("meetings/meetings.db"))
            .unwrap()
            .execute_batch(
                "INSERT INTO meetings VALUES (3, 1700004000, 1700004600, 'Transcript only call', 'stopped', 'whisper-small', 0);
                 INSERT INTO meeting_segments VALUES (30, 3, 0, 2000, 'me', 'Raw transcript line');",
            )
            .unwrap();
        fs::write(
            p.join("settings_store.json"),
            r#"{"settings":{"agent_access_enabled":true}}"#,
        )
        .unwrap();

        let notes_res = call(
            p,
            21,
            "tools/call",
            json!({"name": "get_meeting", "arguments": {"meeting_id": 1}}),
        );
        assert_eq!(notes_res["result"]["isError"], json!(false));
        let tr_res = call(
            p,
            22,
            "tools/call",
            json!({"name": "get_meeting", "arguments": {"meeting_id": 3}}),
        );
        assert_eq!(tr_res["result"]["isError"], json!(false));

        let rows = recent_reads(p);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].target, "Transcript only call");
        assert_eq!(rows[0].kind.as_deref(), Some("transcript"));
        assert_eq!(rows[1].target, "Product sync");
        assert_eq!(rows[1].kind.as_deref(), Some("notes"));
    }
}
