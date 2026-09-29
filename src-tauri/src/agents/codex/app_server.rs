use serde::Serialize;
use serde_json::{json, Value};
use std::{
  io::{self, Read, Write},
  process::{Child, Command, Stdio},
  sync::{atomic::{AtomicU64, Ordering}, mpsc::{self, Receiver, RecvTimeoutError, SyncSender}},
  thread::{self, JoinHandle},
  time::{Duration, Instant},
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_PROBE_BYTES: usize = 256 * 1024;
const MAX_STDERR_BYTES: usize = 4 * 1024;
const MAX_NOTIFICATIONS: usize = 32;
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexAppServerDiagnosticCode {
  CodexAppServerNotFound,
  CodexAppServerSpawnFailed,
  CodexAppServerHandshakeTimeout,
  CodexAppServerProtocolError,
  CodexAppServerInitializeRejected,
  CodexAppServerClosed,
  CodexAppServerShutdownFailed,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAppServerProbe {
  pub launched: bool,
  pub initialized: bool,
  pub platform_family: Option<&'static str>,
  pub platform_os: Option<&'static str>,
  pub diagnostic_code: Option<CodexAppServerDiagnosticCode>,
}

impl CodexAppServerProbe {
  fn failed(launched: bool, code: CodexAppServerDiagnosticCode) -> Self {
    Self { launched, initialized: false, platform_family: None, platform_os: None, diagnostic_code: Some(code) }
  }
}

enum Frame { Line(Vec<u8>), TooLarge, ReadFailed }

struct CodexAppServerProcess {
  child: Child,
  stdout_reader: Option<JoinHandle<()>>,
  stderr_reader: Option<JoinHandle<Vec<u8>>>,
}

impl CodexAppServerProcess {
  fn spawn() -> io::Result<(Self, Receiver<Frame>)> {
    let mut child = Command::new("codex")
      .args(["app-server", "--stdio"])
      .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let (stdout, stderr) = match (child.stdout.take(), child.stderr.take()) {
      (Some(stdout), Some(stderr)) => (stdout, stderr),
      _ => {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other("app-server pipes unavailable"));
      }
    };
    let (sender, receiver) = mpsc::sync_channel(8);
    let stdout_reader = thread::spawn(move || read_frames(stdout, sender));
    let stderr_reader = thread::spawn(move || drain_stderr(stderr));
    Ok((Self { child, stdout_reader: Some(stdout_reader), stderr_reader: Some(stderr_reader) }, receiver))
  }

  fn write_message(&mut self, value: &Value) -> Result<(), CodexAppServerDiagnosticCode> {
    let stdin = self.child.stdin.as_mut().ok_or(CodexAppServerDiagnosticCode::CodexAppServerClosed)?;
    serde_json::to_writer(&mut *stdin, value).map_err(|_| CodexAppServerDiagnosticCode::CodexAppServerProtocolError)?;
    stdin.write_all(b"\n").and_then(|_| stdin.flush())
      .map_err(|_| CodexAppServerDiagnosticCode::CodexAppServerClosed)
  }

  fn shutdown(&mut self) -> Result<(), CodexAppServerDiagnosticCode> {
    self.child.stdin.take(); // EOF is the app-server stdio transport shutdown.
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    let mut clean = false;
    loop {
      match self.child.try_wait() {
        Ok(Some(_)) => { clean = true; break; }
        Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
        _ => break,
      }
    }
    if !clean {
      let _ = self.child.kill();
      self.child.wait().map_err(|_| CodexAppServerDiagnosticCode::CodexAppServerShutdownFailed)?;
    }
    if self.stdout_reader.take().is_some_and(|reader| reader.join().is_err()) { clean = false; }
    if self.stderr_reader.take().is_some_and(|reader| reader.join().is_err()) { clean = false; }
    if clean { Ok(()) } else { Err(CodexAppServerDiagnosticCode::CodexAppServerShutdownFailed) }
  }
}

impl Drop for CodexAppServerProcess {
  fn drop(&mut self) { let _ = self.shutdown(); }
}

fn read_frames<R: Read>(mut stdout: R, sender: SyncSender<Frame>) {
  let mut pending = Vec::new();
  let mut total = 0usize;
  let mut buffer = [0u8; 4096];
  loop {
    match stdout.read(&mut buffer) {
      Ok(0) => { if !pending.is_empty() { let _ = sender.try_send(Frame::ReadFailed); } break; }
      Ok(size) => {
        total += size;
        if total > MAX_PROBE_BYTES { let _ = sender.try_send(Frame::TooLarge); break; }
        for byte in &buffer[..size] {
          if *byte == b'\n' {
            if sender.try_send(Frame::Line(std::mem::take(&mut pending))).is_err() { return; }
          } else {
            pending.push(*byte);
            if pending.len() > MAX_MESSAGE_BYTES { let _ = sender.try_send(Frame::TooLarge); return; }
          }
        }
      }
      Err(_) => { let _ = sender.try_send(Frame::ReadFailed); break; }
    }
  }
}

fn drain_stderr<R: Read>(mut stderr: R) -> Vec<u8> {
  let mut retained = Vec::new();
  let mut buffer = [0u8; 4096];
  while let Ok(size) = stderr.read(&mut buffer) {
    if size == 0 { break; }
    let available = MAX_STDERR_BYTES.saturating_sub(retained.len());
    retained.extend_from_slice(&buffer[..size.min(available)]);
  }
  retained // Internal only; never logged, serialized, or returned to the frontend.
}

fn initialize_request(id: u64) -> Value {
  json!({"method": "initialize", "id": id, "params": {"clientInfo": {
    "name": "assistente-3d", "title": "Assistente-3D", "version": env!("CARGO_PKG_VERSION")
  }}})
}

fn parse_initialize_line(line: &[u8], expected_id: u64) -> Result<Option<CodexAppServerProbe>, CodexAppServerDiagnosticCode> {
  use CodexAppServerDiagnosticCode::*;
  let value: Value = serde_json::from_slice(line).map_err(|_| CodexAppServerProtocolError)?;
  let object = value.as_object().ok_or(CodexAppServerProtocolError)?;
  if let Some(method) = object.get("method") {
    if object.contains_key("id") || !method.is_string() || object.contains_key("result") || object.contains_key("error") {
      return Err(CodexAppServerProtocolError);
    }
    return Ok(None);
  }
  let id = object.get("id").and_then(Value::as_u64).ok_or(CodexAppServerProtocolError)?;
  if id != expected_id { return Err(CodexAppServerProtocolError); }
  if object.contains_key("error") {
    let error = object.get("error").and_then(Value::as_object).ok_or(CodexAppServerProtocolError)?;
    if !error.get("code").is_some_and(Value::is_number) || !error.get("message").is_some_and(Value::is_string)
      || object.contains_key("result") { return Err(CodexAppServerProtocolError); }
    return Err(CodexAppServerInitializeRejected);
  }
  let result = object.get("result").and_then(Value::as_object).ok_or(CodexAppServerProtocolError)?;
  // Validate required schema fields, but never retain codexHome or userAgent.
  for field in ["codexHome", "userAgent", "platformFamily", "platformOs"] {
    if !result.get(field).is_some_and(Value::is_string) { return Err(CodexAppServerProtocolError); }
  }
  let platform_family = match result["platformFamily"].as_str() { Some("unix") => Some("unix"), Some("windows") => Some("windows"), _ => None };
  let platform_os = match result["platformOs"].as_str() { Some("linux") => Some("linux"), Some("macos") => Some("macos"), Some("windows") => Some("windows"), _ => None };
  Ok(Some(CodexAppServerProbe { launched: true, initialized: true, platform_family, platform_os, diagnostic_code: None }))
}

fn await_initialize(receiver: &Receiver<Frame>, id: u64, deadline: Instant) -> Result<CodexAppServerProbe, CodexAppServerDiagnosticCode> {
  use CodexAppServerDiagnosticCode::*;
  let mut notifications = 0;
  loop {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() { return Err(CodexAppServerHandshakeTimeout); }
    match receiver.recv_timeout(remaining) {
      Ok(Frame::Line(line)) => match parse_initialize_line(&line, id)? {
        Some(probe) => return Ok(probe),
        None => { notifications += 1; if notifications > MAX_NOTIFICATIONS { return Err(CodexAppServerProtocolError); } }
      },
      Ok(Frame::TooLarge | Frame::ReadFailed) => return Err(CodexAppServerProtocolError),
      Err(RecvTimeoutError::Timeout) => return Err(CodexAppServerHandshakeTimeout),
      Err(RecvTimeoutError::Disconnected) => return Err(CodexAppServerClosed),
    }
  }
}

fn run_probe() -> CodexAppServerProbe {
  let (mut process, receiver) = match CodexAppServerProcess::spawn() {
    Ok(parts) => parts,
    Err(error) if error.kind() == io::ErrorKind::NotFound => return CodexAppServerProbe::failed(false, CodexAppServerDiagnosticCode::CodexAppServerNotFound),
    Err(_) => return CodexAppServerProbe::failed(false, CodexAppServerDiagnosticCode::CodexAppServerSpawnFailed),
  };
  let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
  let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
  let result = process.write_message(&initialize_request(id))
    .and_then(|_| await_initialize(&receiver, id, deadline))
    .and_then(|probe| process.write_message(&json!({"method": "initialized"})).map(|_| probe));
  let cleanup = process.shutdown();
  match (result, cleanup) {
    (Ok(probe), Ok(())) => probe,
    (Err(code), _) => CodexAppServerProbe::failed(true, code),
    (Ok(_), Err(code)) => CodexAppServerProbe::failed(true, code),
  }
}

pub async fn probe_codex_app_server() -> Result<CodexAppServerProbe, String> {
  tauri::async_runtime::spawn_blocking(run_probe).await
    .map_err(|_| "codex_app_server_spawn_failed".to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn response(id: u64) -> Vec<u8> {
    format!(r#"{{"id":{id},"result":{{"codexHome":"/private/home/sam/.codex","userAgent":"codex-cli/0.158.0","platformFamily":"unix","platformOs":"linux"}}}}"#).into_bytes()
  }

  #[test] fn initialize_serialization() {
    let request = initialize_request(42);
    assert_eq!(request["method"], "initialize");
    assert_eq!(request["id"], 42);
    assert_eq!(request["params"]["clientInfo"]["name"], "assistente-3d");
    assert_eq!(request["params"]["clientInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(request["params"].get("capabilities").is_none());
  }

  #[test] fn initialize_success_and_sanitization() {
    let probe = parse_initialize_line(&response(7), 7).unwrap().unwrap();
    assert!(probe.launched && probe.initialized);
    let public = serde_json::to_string(&probe).unwrap();
    assert!(public.contains("linux"));
    assert!(!public.contains("codexHome") && !public.contains("/private/") && !public.contains("userAgent"));
  }

  #[test] fn json_rpc_error_is_rejected() {
    assert!(matches!(parse_initialize_line(br#"{"id":7,"error":{"code":-32600,"message":"private path"}}"#, 7), Err(CodexAppServerDiagnosticCode::CodexAppServerInitializeRejected)));
  }

  #[test] fn invalid_json_and_wrong_id_fail_closed() {
    assert!(parse_initialize_line(b"not json", 7).is_err());
    assert!(matches!(parse_initialize_line(&response(8), 7), Err(CodexAppServerDiagnosticCode::CodexAppServerProtocolError)));
    assert!(parse_initialize_line(br#"{"result":{}}"#, 7).is_err());
  }

  #[test] fn interleaved_notification_and_eof() {
    let (sender, receiver) = mpsc::sync_channel(2);
    sender.send(Frame::Line(br#"{"method":"config/warning","params":{}}"#.to_vec())).unwrap();
    sender.send(Frame::Line(response(9))).unwrap();
    assert!(await_initialize(&receiver, 9, Instant::now() + Duration::from_secs(1)).unwrap().initialized);
    drop(sender);
    assert!(matches!(await_initialize(&receiver, 10, Instant::now() + Duration::from_secs(1)), Err(CodexAppServerDiagnosticCode::CodexAppServerClosed)));
  }

  #[test] fn framing_limits_message_and_probe() {
    let (sender, receiver) = mpsc::sync_channel(2);
    read_frames(&b"{}\n"[..], sender);
    assert!(matches!(receiver.recv().unwrap(), Frame::Line(_)));
    let (sender, receiver) = mpsc::sync_channel(2);
    read_frames(&vec![b'a'; MAX_MESSAGE_BYTES + 1][..], sender);
    assert!(matches!(receiver.recv().unwrap(), Frame::TooLarge));
    let (sender, receiver) = mpsc::sync_channel(128);
    let many_lines = [vec![b'a'; 4095], vec![b'\n']].concat().repeat(MAX_PROBE_BYTES / 4096 + 1);
    let reader = thread::spawn(move || read_frames(&many_lines[..], sender));
    let mut too_large = false;
    while let Ok(frame) = receiver.recv() {
      if matches!(frame, Frame::TooLarge) { too_large = true; break; }
    }
    reader.join().unwrap();
    assert!(too_large);
  }

  // Manual gate only; ordinary cargo test remains independent of the Codex CLI.
  #[test] #[ignore = "requires a local Codex app-server"]
  fn real_app_server_handshake() {
    let probe = run_probe();
    assert!(probe.initialized, "sanitized diagnostic: {:?}", probe.diagnostic_code);
  }
}
