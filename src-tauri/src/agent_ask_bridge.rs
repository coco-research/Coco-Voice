//! Local Unix domain socket bridge between `--mcp-ask-user` clients and the
//! Coco Voice app overlay.
//!
//! Security properties:
//! - Unix domain socket only (`<app data>/agent-ask.sock`); zero TCP listeners.
//! - Parent directory enforced `0700`; socket file enforced `0600`.
//! - Symlinks at the socket path are refused on both bind and connect via
//!   `fs::symlink_metadata` (never followed or unlinked).
//! - Every accepted peer connection is authenticated against the current
//!   effective UID (`getpeereid` on macOS, `SO_PEERCRED` on Linux).
//! - Questions are sanitized to plain text and truncated to at most 300 chars.
//! - Answers live only in memory and are delivered solely to the asking
//!   connection's reply channel (never pasted, never written to disk).

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SOCKET_FILE_NAME: &str = "agent-ask.sock";
pub const MAX_QUESTION_CHARS: usize = 300;
pub const MAX_AGENT_NAME_CHARS: usize = 64;
pub const MAX_LINE_BYTES: usize = 16 * 1024;
pub const QUESTION_TIMEOUT_MS: u64 = 5 * 60 * 1000;

pub const ERR_QUESTIONS_OFF: &str = "The person has turned questions off.";
pub const ERR_DECLINED: &str = "The person declined to answer.";
pub const ERR_TIMEOUT: &str = "No answer within 5 minutes.";
pub const ERR_NOT_RUNNING: &str = "Coco Voice isn't running.";
pub const ERR_APP_QUIT: &str = "Coco Voice quit.";
pub const ERR_WINDOWS_UNSUPPORTED: &str = "Not available on Windows yet";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentAskRequest {
    pub agent: String,
    pub question: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentAskResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentAskOverlayEvent {
    pub agent_name: String,
    pub question: String,
    pub queued_count: usize,
    pub shortcut_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentAskAnsweredEvent {
    pub agent_name: String,
}

pub fn socket_path_in(data_dir: &Path) -> PathBuf {
    data_dir.join(SOCKET_FILE_NAME)
}

/// Sanitize an agent question to single-line plain text and cap it at 300 chars.
pub fn sanitize_and_truncate_question(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len().min(MAX_QUESTION_CHARS + 4));
    let mut prev_space = true;
    for ch in raw.chars() {
        if ch.is_control() {
            if (ch == '\n' || ch == '\r' || ch == '\t') && !prev_space {
                cleaned.push(' ');
                prev_space = true;
            }
            continue;
        }
        if ch.is_whitespace() {
            if !prev_space {
                cleaned.push(' ');
                prev_space = true;
            }
        } else {
            cleaned.push(ch);
            prev_space = false;
        }
    }
    let trimmed = cleaned.trim();
    let count = trimmed.chars().count();
    if count <= MAX_QUESTION_CHARS {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(MAX_QUESTION_CHARS).collect();
    out.push('…');
    out
}

pub fn sanitize_agent_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_AGENT_NAME_CHARS)
        .collect();
    if cleaned.is_empty() {
        "Local Agent".to_string()
    } else {
        cleaned
    }
}

/// Upgrade helper (Plan E Part 6): agrees with the UI when legacy
/// `agent_ask_enabled` is true and `agents_can_ask_enabled` was not yet set.
pub fn effective_agents_can_ask(agents_can_ask_enabled: bool, legacy_agent_ask: bool) -> bool {
    agents_can_ask_enabled || legacy_agent_ask
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct PendingAsk {
    agent_name: String,
    question: String,
    enqueued_at_ms: u64,
    reply_tx: mpsc::SyncSender<Result<String, String>>,
}

/// Pure in-memory FIFO queue holding pending agent questions and their
/// single-use reply channels. Nothing is ever written to disk.
pub struct AgentAskBridgeState {
    enabled: bool,
    meeting_active: bool,
    shortcut_key: String,
    active: Option<PendingAsk>,
    queue: VecDeque<PendingAsk>,
}

impl AgentAskBridgeState {
    pub fn new(enabled: bool, shortcut_key: impl Into<String>) -> Self {
        Self {
            enabled,
            meeting_active: false,
            shortcut_key: shortcut_key.into(),
            active: None,
            queue: VecDeque::new(),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.fail_all(ERR_QUESTIONS_OFF);
        }
    }

    pub fn set_shortcut_key(&mut self, key: impl Into<String>) {
        self.shortcut_key = key.into();
    }

    pub fn set_meeting_active(&mut self, active: bool) -> Option<AgentAskOverlayEvent> {
        self.meeting_active = active;
        if !active && self.active.is_none() {
            self.promote_next_locked();
        }
        self.current_overlay_event()
    }

    pub fn has_active_question(&self) -> bool {
        !self.meeting_active && self.active.is_some()
    }

    pub fn waiting_count(&self) -> usize {
        self.queue.len() + usize::from(self.meeting_active && self.active.is_some())
    }

    pub fn current_overlay_event(&self) -> Option<AgentAskOverlayEvent> {
        if self.meeting_active {
            return None;
        }
        self.active.as_ref().map(|item| AgentAskOverlayEvent {
            agent_name: item.agent_name.clone(),
            question: item.question.clone(),
            queued_count: self.queue.len(),
            shortcut_key: self.shortcut_key.clone(),
        })
    }

    /// Enqueue a new agent question, returning a single-use receiver for that
    /// connection only plus the overlay event to display (if not paused by a
    /// meeting).
    pub fn enqueue_at(
        &mut self,
        agent: &str,
        question: &str,
        now_ms: u64,
    ) -> Result<
        (
            mpsc::Receiver<Result<String, String>>,
            Option<AgentAskOverlayEvent>,
        ),
        &'static str,
    > {
        if !self.enabled {
            return Err(ERR_QUESTIONS_OFF);
        }
        let agent_name = sanitize_agent_name(agent);
        let question_text = sanitize_and_truncate_question(question);
        if question_text.is_empty() {
            return Err("Question was empty.");
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let item = PendingAsk {
            agent_name,
            question: question_text,
            enqueued_at_ms: now_ms,
            reply_tx: tx,
        };
        if self.active.is_none() && !self.meeting_active {
            self.active = Some(item);
        } else {
            self.queue.push_back(item);
        }
        Ok((rx, self.current_overlay_event()))
    }

    /// Route the next dictation take's text to the active asking agent ONLY.
    /// Returns `(answered_agent_name, next_overlay_event)` when a question was
    /// active, or `None` when dictation should follow the normal paste path.
    pub fn answer_active(
        &mut self,
        answer: String,
    ) -> Option<(String, Option<AgentAskOverlayEvent>)> {
        if self.meeting_active {
            return None;
        }
        let active = self.active.take()?;
        let agent_name = active.agent_name.clone();
        let _ = active.reply_tx.send(Ok(answer));
        self.promote_next_locked();
        Some((agent_name, self.current_overlay_event()))
    }

    /// Called on Escape while the card is shown.
    pub fn decline_active(&mut self) -> Option<AgentAskOverlayEvent> {
        if let Some(active) = self.active.take() {
            let _ = active.reply_tx.send(Err(ERR_DECLINED.to_string()));
        }
        self.promote_next_locked();
        self.current_overlay_event()
    }

    /// Expire any active or queued question older than 5 minutes (fake-clock testable).
    pub fn expire_stale_at(&mut self, now_ms: u64) -> (usize, Option<AgentAskOverlayEvent>) {
        let mut expired = 0usize;
        if let Some(active) = self.active.as_ref() {
            if now_ms.saturating_sub(active.enqueued_at_ms) >= QUESTION_TIMEOUT_MS {
                if let Some(old) = self.active.take() {
                    let _ = old.reply_tx.send(Err(ERR_TIMEOUT.to_string()));
                    expired += 1;
                }
            }
        }
        let mut kept = VecDeque::with_capacity(self.queue.len());
        while let Some(item) = self.queue.pop_front() {
            if now_ms.saturating_sub(item.enqueued_at_ms) >= QUESTION_TIMEOUT_MS {
                let _ = item.reply_tx.send(Err(ERR_TIMEOUT.to_string()));
                expired += 1;
            } else {
                kept.push_back(item);
            }
        }
        self.queue = kept;
        if !self.meeting_active && self.active.is_none() {
            self.promote_next_locked();
        }
        (expired, self.current_overlay_event())
    }

    /// Notify every waiting connection on app shutdown.
    pub fn shutdown_all(&mut self) {
        self.fail_all(ERR_APP_QUIT);
    }

    fn fail_all(&mut self, reason: &str) {
        if let Some(active) = self.active.take() {
            let _ = active.reply_tx.send(Err(reason.to_string()));
        }
        while let Some(item) = self.queue.pop_front() {
            let _ = item.reply_tx.send(Err(reason.to_string()));
        }
    }

    fn promote_next_locked(&mut self) {
        if self.active.is_none() && !self.meeting_active {
            self.active = self.queue.pop_front();
        }
    }
}

/// Refuse any symlink at `path` using `symlink_metadata` (never follows links).
pub fn reject_if_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refused: agent-ask socket path is a symlink",
                ));
            }
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
fn verify_peer_uid(stream: &std::os::unix::net::UnixStream) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    let fd = stream.as_raw_fd();
    let expected_uid = unsafe { libc::geteuid() };

    #[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
    {
        let mut peer_uid: libc::uid_t = 0;
        let mut peer_gid: libc::gid_t = 0;
        let rc = unsafe { libc::getpeereid(fd, &mut peer_uid, &mut peer_gid) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        if peer_uid != expected_uid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refused: peer uid mismatch",
            ));
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast::<libc::c_void>(),
                &mut len,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        if cred.uid != expected_uid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refused: peer uid mismatch",
            ));
        }
        Ok(())
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "linux"
    )))]
    {
        let _ = (fd, expected_uid);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "peer credential check unavailable on this platform",
        ))
    }
}

#[cfg(unix)]
fn bind_secure_unix_socket(
    data_dir: &Path,
) -> io::Result<(std::os::unix::net::UnixListener, PathBuf)> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};

    if !data_dir.exists() {
        fs::create_dir_all(data_dir)?;
    }
    reject_if_symlink(data_dir)?;
    fs::set_permissions(data_dir, fs::Permissions::from_mode(0o700))?;

    let sock_path = socket_path_in(data_dir);
    reject_if_symlink(&sock_path)?;

    if let Ok(meta) = fs::symlink_metadata(&sock_path) {
        if meta.file_type().is_socket() {
            fs::remove_file(&sock_path)?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refused: non-socket file exists at agent-ask socket path",
            ));
        }
    }

    let listener = std::os::unix::net::UnixListener::bind(&sock_path)?;
    fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok((listener, sock_path))
}

#[cfg(unix)]
fn read_bounded_line(stream: &std::os::unix::net::UnixStream) -> io::Result<String> {
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match io::Read::read(&mut reader, &mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                if buf.len() >= MAX_LINE_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "request line exceeds limit",
                    ));
                }
                buf.push(byte[0]);
            }
            Err(err) => return Err(err),
        }
    }
    String::from_utf8(buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Running local Unix socket server owned by the app process.
#[cfg(unix)]
pub struct AgentAskServer {
    sock_path: PathBuf,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<AgentAskBridgeState>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl AgentAskServer {
    pub fn start(
        data_dir: &Path,
        state: Arc<Mutex<AgentAskBridgeState>>,
        on_overlay_event: Arc<dyn Fn(AgentAskOverlayEvent) + Send + Sync>,
    ) -> io::Result<Self> {
        let (listener, sock_path) = bind_secure_unix_socket(data_dir)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_loop = Arc::clone(&stop);
        let state_loop = Arc::clone(&state);

        let handle = std::thread::spawn(move || {
            while !stop_loop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _addr)) => {
                        let st = Arc::clone(&state_loop);
                        let cb = Arc::clone(&on_overlay_event);
                        std::thread::spawn(move || {
                            handle_unix_client(stream, st, cb);
                        });
                    }
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                        let maybe_event = {
                            let mut guard = state_loop.lock().unwrap_or_else(|e| e.into_inner());
                            let (expired, next) = guard.expire_stale_at(now_epoch_ms());
                            if expired > 0 {
                                next
                            } else {
                                None
                            }
                        };
                        if let Some(ev) = maybe_event {
                            on_overlay_event(ev);
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        });

        Ok(Self {
            sock_path,
            stop,
            state,
            handle: Some(handle),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.sock_path
    }

    pub fn state(&self) -> &Arc<Mutex<AgentAskBridgeState>> {
        &self.state
    }
}

#[cfg(unix)]
impl Drop for AgentAskServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.state.lock() {
            guard.shutdown_all();
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        if reject_if_symlink(&self.sock_path).is_ok() {
            let _ = fs::remove_file(&self.sock_path);
        }
    }
}

#[cfg(unix)]
fn handle_unix_client(
    mut stream: std::os::unix::net::UnixStream,
    state: Arc<Mutex<AgentAskBridgeState>>,
    on_overlay_event: Arc<dyn Fn(AgentAskOverlayEvent) + Send + Sync>,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    if verify_peer_uid(&stream).is_err() {
        return;
    }

    let line = match read_bounded_line(&stream) {
        Ok(l) => l,
        Err(_) => return,
    };
    let req: AgentAskRequest = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(_) => {
            let _ = write_response(
                &mut stream,
                &AgentAskResponse {
                    answer: None,
                    error: Some("Invalid request.".to_string()),
                },
            );
            return;
        }
    };

    let enqueued = {
        let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
        guard.enqueue_at(&req.agent, &req.question, now_epoch_ms())
    };

    let (rx, maybe_event) = match enqueued {
        Ok(pair) => pair,
        Err(reason) => {
            let _ = write_response(
                &mut stream,
                &AgentAskResponse {
                    answer: None,
                    error: Some(reason.to_string()),
                },
            );
            return;
        }
    };

    if let Some(ev) = maybe_event {
        on_overlay_event(ev);
    }

    let reply = match rx.recv_timeout(Duration::from_millis(QUESTION_TIMEOUT_MS)) {
        Ok(Ok(answer)) => AgentAskResponse {
            answer: Some(answer),
            error: None,
        },
        Ok(Err(err)) => AgentAskResponse {
            answer: None,
            error: Some(err),
        },
        Err(_) => AgentAskResponse {
            answer: None,
            error: Some(ERR_TIMEOUT.to_string()),
        },
    };
    let _ = write_response(&mut stream, &reply);
}

#[cfg(unix)]
fn write_response(
    stream: &mut std::os::unix::net::UnixStream,
    resp: &AgentAskResponse,
) -> io::Result<()> {
    let encoded = serde_json::to_string(resp)
        .unwrap_or_else(|_| "{\"error\":\"serialization_failed\"}".to_string());
    stream.write_all(encoded.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

/// MCP-side client: sends one `{agent, question}` line over `<data_dir>/agent-ask.sock`
/// and awaits the single answer line. Never touches TCP.
#[cfg(unix)]
pub fn send_question_over_socket(
    data_dir: &Path,
    agent: &str,
    question: &str,
) -> Result<String, String> {
    let sock_path = socket_path_in(data_dir);
    reject_if_symlink(&sock_path).map_err(|e| e.to_string())?;
    if !sock_path.exists() {
        return Err(ERR_NOT_RUNNING.to_string());
    }
    let mut stream = std::os::unix::net::UnixStream::connect(&sock_path)
        .map_err(|_| ERR_NOT_RUNNING.to_string())?;
    let req = AgentAskRequest {
        agent: sanitize_agent_name(agent),
        question: sanitize_and_truncate_question(question),
    };
    let mut encoded = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    encoded.push('\n');
    stream
        .write_all(encoded.as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|_| ERR_NOT_RUNNING.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|_| ERR_APP_QUIT.to_string())?;
    if line.trim().is_empty() {
        return Err(ERR_APP_QUIT.to_string());
    }
    let parsed: AgentAskResponse = serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
    match (parsed.answer, parsed.error) {
        (Some(answer), _) => Ok(answer),
        (None, Some(err)) => Err(err),
        (None, None) => Err(ERR_APP_QUIT.to_string()),
    }
}

#[cfg(not(unix))]
pub fn send_question_over_socket(
    _data_dir: &Path,
    _agent: &str,
    _question: &str,
) -> Result<String, String> {
    Err(ERR_WINDOWS_UNSUPPORTED.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_is_plain_text_and_truncated_at_300_chars() {
        let with_ctrl = "Hello\n\tworld\x07  from   agent";
        assert_eq!(
            sanitize_and_truncate_question(with_ctrl),
            "Hello world from agent"
        );

        let long = "a".repeat(340);
        let truncated = sanitize_and_truncate_question(&long);
        assert_eq!(truncated.chars().count(), MAX_QUESTION_CHARS + 1);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn esc_declines_active_question_and_timeout_at_five_minutes_returns_no_answer() {
        let mut st = AgentAskBridgeState::new(true, "fn");
        let (rx1, ev1) = st
            .enqueue_at("Claude Code", "Which branch?", 1_000)
            .unwrap();
        assert_eq!(ev1.unwrap().agent_name, "Claude Code");

        let (rx2, _) = st.enqueue_at("Codex", "Run tests?", 1_500).unwrap();
        let next_ev = st.decline_active().expect("second question promoted");
        assert_eq!(next_ev.agent_name, "Codex");
        assert_eq!(rx1.recv().unwrap(), Err(ERR_DECLINED.to_string()));

        // Fake clock advances 5 minutes past enqueued_at_ms (1_500 + 300_000 = 301_500).
        let (expired, after_ev) = st.expire_stale_at(301_500);
        assert_eq!(expired, 1);
        assert!(after_ev.is_none());
        assert_eq!(rx2.recv().unwrap(), Err(ERR_TIMEOUT.to_string()));
    }

    #[test]
    fn shutdown_notifies_waiting_agents_with_coco_voice_quit() {
        let mut st = AgentAskBridgeState::new(true, "fn");
        let (rx1, _) = st.enqueue_at("Agent A", "Q1?", 10).unwrap();
        let (rx2, _) = st.enqueue_at("Agent B", "Q2?", 20).unwrap();
        st.shutdown_all();
        assert_eq!(rx1.recv().unwrap(), Err(ERR_APP_QUIT.to_string()));
        assert_eq!(rx2.recv().unwrap(), Err(ERR_APP_QUIT.to_string()));
    }

    #[test]
    fn meeting_active_defers_card_until_meeting_stops() {
        let mut st = AgentAskBridgeState::new(true, "fn");
        st.set_meeting_active(true);
        let (_rx, ev) = st.enqueue_at("Claude Code", "Ship?", 100).unwrap();
        assert!(ev.is_none());
        assert_eq!(st.waiting_count(), 1);
        assert!(!st.has_active_question());

        let shown = st
            .set_meeting_active(false)
            .expect("shown after meeting stop");
        assert_eq!(shown.agent_name, "Claude Code");
        assert!(st.has_active_question());
    }

    #[test]
    fn missing_socket_returns_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let err = send_question_over_socket(dir.path(), "Claude", "Ready?").unwrap_err();
        assert_eq!(err, ERR_NOT_RUNNING);
    }

    #[cfg(unix)]
    #[test]
    fn socket_created_with_0600_mode_refuses_symlinks_and_removes_on_drop() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let dir = tempfile::tempdir().unwrap();
        let sock_dir = dir.path().join("app-data");
        let state = Arc::new(Mutex::new(AgentAskBridgeState::new(true, "fn")));

        // 1. Refuse a pre-existing symlink at the socket path.
        fs::create_dir_all(&sock_dir).unwrap();
        let decoy = dir.path().join("decoy");
        fs::write(&decoy, "x").unwrap();
        let sock_path = socket_path_in(&sock_dir);
        symlink(&decoy, &sock_path).unwrap();
        let bind_err = AgentAskServer::start(&sock_dir, Arc::clone(&state), Arc::new(|_| {}))
            .err()
            .expect("symlink must be refused");
        assert_eq!(bind_err.kind(), io::ErrorKind::PermissionDenied);
        assert!(decoy.exists());
        fs::remove_file(&sock_path).unwrap();

        // 2. Normal bind creates 0700 parent dir and 0600 socket, removed on drop.
        let server = AgentAskServer::start(&sock_dir, Arc::clone(&state), Arc::new(|_| {}))
            .expect("bind socket");
        let dir_mode = fs::metadata(&sock_dir).unwrap().permissions().mode() & 0o777;
        let sock_mode = fs::symlink_metadata(server.socket_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(sock_mode, 0o600);
        let path_copy = server.socket_path().to_path_buf();
        drop(server);
        assert!(!path_copy.exists());
    }

    #[cfg(unix)]
    #[test]
    fn two_agents_answered_in_fifo_order_each_on_own_connection() {
        let dir = tempfile::tempdir().unwrap();
        let sock_dir = dir.path().join("app-data");
        let state = Arc::new(Mutex::new(AgentAskBridgeState::new(true, "fn")));
        let seen = Arc::new(Mutex::new(Vec::<AgentAskOverlayEvent>::new()));
        let seen_cb = Arc::clone(&seen);
        let _server = AgentAskServer::start(
            &sock_dir,
            Arc::clone(&state),
            Arc::new(move |ev| {
                seen_cb.lock().unwrap().push(ev);
            }),
        )
        .expect("start server");

        let d1 = sock_dir.clone();
        let t1 = std::thread::spawn(move || send_question_over_socket(&d1, "Agent One", "First?"));
        // Wait until first question is active.
        for _ in 0..80 {
            if state.lock().unwrap().has_active_question() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let d2 = sock_dir.clone();
        let t2 = std::thread::spawn(move || send_question_over_socket(&d2, "Agent Two", "Second?"));
        for _ in 0..80 {
            if state.lock().unwrap().waiting_count() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let (a1, next_ev) = state
            .lock()
            .unwrap()
            .answer_active("answer-one".to_string())
            .expect("answer first");
        assert_eq!(a1, "Agent One");
        assert_eq!(next_ev.expect("second promoted").agent_name, "Agent Two");

        let (a2, done_ev) = state
            .lock()
            .unwrap()
            .answer_active("answer-two".to_string())
            .expect("answer second");
        assert_eq!(a2, "Agent Two");
        assert!(done_ev.is_none());

        assert_eq!(t1.join().unwrap().unwrap(), "answer-one");
        assert_eq!(t2.join().unwrap().unwrap(), "answer-two");
    }

    #[test]
    fn no_tcp_listener_in_bridge_source() {
        let src = include_str!("agent_ask_bridge.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(!prod.contains(concat!("Tcp", "Listener")));
        assert!(!prod.contains(concat!("Tcp", "Stream")));
    }

    #[test]
    fn d3_disabled_bridge_accepts_question_after_set_enabled_true() {
        let mut st = AgentAskBridgeState::new(false, "fn");
        assert_eq!(
            st.enqueue_at("Claude Code", "Which branch?", 100)
                .unwrap_err(),
            ERR_QUESTIONS_OFF
        );
        assert!(!st.has_active_question());

        st.set_enabled(true);
        let (rx, ev) = st
            .enqueue_at("Claude Code", "Which branch?", 200)
            .expect("bridge enabled dynamically must accept questions");
        let shown = ev.expect("active question event emitted");
        assert_eq!(shown.agent_name, "Claude Code");
        assert_eq!(shown.question, "Which branch?");
        assert!(st.has_active_question());

        let (answered_agent, next_ev) = st
            .answer_active("main".to_string())
            .expect("answer routed to active caller");
        assert_eq!(answered_agent, "Claude Code");
        assert!(next_ev.is_none());
        assert_eq!(rx.recv().unwrap(), Ok("main".to_string()));
    }

    #[test]
    fn d4_turning_off_ends_active_and_queued_questions_with_err_questions_off_and_clears_pill() {
        let mut st = AgentAskBridgeState::new(true, "fn");
        let (rx1, ev1) = st
            .enqueue_at("Claude Code", "First waiting?", 100)
            .expect("first active");
        assert_eq!(ev1.expect("first pill event").agent_name, "Claude Code");

        let (rx2, ev2) = st
            .enqueue_at("Codex", "Second queued?", 150)
            .expect("second queued");
        assert_eq!(ev2.expect("pill shows queue count").queued_count, 1);
        assert!(st.has_active_question());
        assert_eq!(st.waiting_count(), 1);
        assert!(st.current_overlay_event().is_some());

        st.set_enabled(false);

        let err1 = rx1.recv().unwrap().unwrap_err();
        let err2 = rx2.recv().unwrap().unwrap_err();
        assert_eq!(err1, ERR_QUESTIONS_OFF);
        assert_eq!(err2, ERR_QUESTIONS_OFF);
        assert!(err1.contains("turned questions off"));
        assert!(err2.contains("turned questions off"));
        assert!(!st.has_active_question());
        assert_eq!(st.waiting_count(), 0);
        assert!(
            st.current_overlay_event().is_none(),
            "pill overlay event must clear when questions are turned off"
        );
    }
}
