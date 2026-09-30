use serde::Serialize;
use serde_json::{json, Value};
use std::{
  collections::VecDeque,
  io::{self, Read, Write},
  process::{Child, Command, Stdio},
  sync::{atomic::{AtomicU64, Ordering}, mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender}},
  thread::{self, JoinHandle},
  time::{Duration, Instant},
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_PROBE_BYTES: usize = 256 * 1024;
const MAX_PLANNER_PROTOCOL_BYTES: usize = 512 * 1024;
// Independently bound queue metadata: charge at least 64 bytes per frame
// against the existing session budget, permitting at most 8192 frames.
const FRAME_METADATA_ALLOWANCE: usize = 64;
const MAX_PLANNER_FRAMES: usize = MAX_PLANNER_PROTOCOL_BYTES / FRAME_METADATA_ALLOWANCE;
const MAX_STDERR_BYTES: usize = 4 * 1024;
const MAX_NOTIFICATIONS: usize = 32;
// Reserve one additional slot for the initialize response after the permitted notifications.
const FRAME_CHANNEL_CAPACITY: usize = MAX_NOTIFICATIONS + 1;
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
  CodexAppServerUnexpectedServerRequest,
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

enum FrameSender { Probe(SyncSender<Frame>), Planner(Sender<Frame>) }
impl From<SyncSender<Frame>> for FrameSender {
  fn from(sender: SyncSender<Frame>) -> Self { Self::Probe(sender) }
}
impl FrameSender {
  fn enqueue(&self, frame: Frame) -> bool {
    match self {
      Self::Probe(sender) => sender.try_send(frame).is_ok(),
      // Unbounded channel API, but the producer caps wire bytes AND frames.
      // No wait for a consumer: shutdown can join even without draining.
      Self::Planner(sender) => sender.send(frame).is_ok(),
    }
  }
}

fn frame_channel(max_total: usize) -> (FrameSender, Receiver<Frame>) {
  if max_total == MAX_PROBE_BYTES {
    let (sender,receiver) = mpsc::sync_channel(FRAME_CHANNEL_CAPACITY);
    (FrameSender::Probe(sender),receiver)
  } else {
    let (sender,receiver) = mpsc::channel();
    (FrameSender::Planner(sender),receiver)
  }
}

struct CodexAppServerProcess {
  child: Child,
  stdout_reader: Option<JoinHandle<()>>,
  stderr_reader: Option<JoinHandle<Vec<u8>>>,
}

impl CodexAppServerProcess {
  fn spawn() -> io::Result<(Self, Receiver<Frame>)> {
    Self::spawn_at(None, MAX_PROBE_BYTES)
  }

  fn spawn_at(cwd: Option<&std::path::Path>, max_total: usize) -> io::Result<(Self, Receiver<Frame>)> {
    let mut command = Command::new("codex");
    command.args(["app-server", "--stdio"]);
    if let Some(cwd) = cwd { command.current_dir(cwd); }
    let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let (stdout, stderr) = match (child.stdout.take(), child.stderr.take()) {
      (Some(stdout), Some(stderr)) => (stdout, stderr),
      _ => {
        let _ = child.kill();
        let _ = child.wait();
        return Err(io::Error::other("app-server pipes unavailable"));
      }
    };
    let (sender, receiver) = frame_channel(max_total);
    let stdout_reader = thread::spawn(move || read_frames_limited(stdout, sender, max_total));
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
    let mut reaped = false;
    loop {
      match self.child.try_wait() {
        Ok(Some(_)) => { reaped = true; break; }
        Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
        _ => break,
      }
    }
    if !reaped {
      let _ = self.child.kill();
      reaped = self.child.wait().is_ok();
    }
    let stdout_joined = self.stdout_reader.take().map_or(true, |reader| reader.join().is_ok());
    let stderr_joined = self.stderr_reader.take().map_or(true, |reader| reader.join().is_ok());
    if reaped && stdout_joined && stderr_joined { Ok(()) }
    else { Err(CodexAppServerDiagnosticCode::CodexAppServerShutdownFailed) }
  }
}

impl Drop for CodexAppServerProcess {
  fn drop(&mut self) { let _ = self.shutdown(); }
}

#[cfg(test)]
fn read_frames<R: Read>(mut stdout: R, sender: SyncSender<Frame>) {
  read_frames_limited(&mut stdout, sender, MAX_PROBE_BYTES)
}

fn read_frames_limited<R: Read>(mut stdout: R, sender: impl Into<FrameSender>, max_total: usize) {
  let sender = sender.into();
  let max_frames = match &sender { FrameSender::Planner(_) => max_total / FRAME_METADATA_ALLOWANCE, FrameSender::Probe(_) => usize::MAX };
  let mut pending = Vec::new();
  let mut total = 0usize;
  let mut frames = 0usize;
  let mut buffer = [0u8; 4096];
  loop {
    match stdout.read(&mut buffer) {
      Ok(0) => { if !pending.is_empty() { sender.enqueue(Frame::ReadFailed); } break; }
      Ok(size) => {
        total += size;
        if total > max_total { sender.enqueue(Frame::TooLarge); break; }
        for byte in &buffer[..size] {
          if *byte == b'\n' {
            if frames >= max_frames { sender.enqueue(Frame::TooLarge); return; }
            frames += 1;
            if !sender.enqueue(Frame::Line(std::mem::take(&mut pending))) { return; }
          } else {
            if pending.len() >= MAX_MESSAGE_BYTES { sender.enqueue(Frame::TooLarge); return; }
            pending.push(*byte);
          }
        }
      }
      Err(_) => { sender.enqueue(Frame::ReadFailed); break; }
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

fn planner_initialize_params() -> Value {
  let mut params = initialize_request(0)["params"].clone();
  params["capabilities"] = json!({"experimentalApi":true});
  params
}

// Shared bounded transport for the D0.5B probe and the ephemeral planner. It never
// answers server-originated requests, including approvals and dynamic tool calls.
pub(super) struct CodexAppServerSession {
  process: CodexAppServerProcess,
  stream: CodexRpcStream,
}

pub(super) struct CodexRpcStream {
  receiver: Receiver<Frame>,
  pending: VecDeque<Value>,
}

enum RpcReply { Notification(Value), Result(Value) }

fn classify_rpc(message: Value, expected_id: u64) -> Result<RpcReply, CodexAppServerDiagnosticCode> {
  use CodexAppServerDiagnosticCode::CodexAppServerProtocolError;
  let object = message.as_object().ok_or(CodexAppServerProtocolError)?;
  if let Some(method) = object.get("method") {
    if !method.is_string() || object.contains_key("result") || object.contains_key("error") {
      return Err(CodexAppServerProtocolError);
    }
    reject_server_request(object)?;
    return Ok(RpcReply::Notification(message));
  }
  if object.get("id").and_then(Value::as_u64) != Some(expected_id) || object.contains_key("error") {
    return Err(CodexAppServerProtocolError);
  }
  object.get("result").cloned().map(RpcReply::Result).ok_or(CodexAppServerProtocolError)
}

fn await_reply(mut receive: impl FnMut() -> Result<Value, CodexAppServerDiagnosticCode>,
  expected_id: u64, pending: &mut VecDeque<Value>) -> Result<Value, CodexAppServerDiagnosticCode> {
  loop {
    match classify_rpc(receive()?, expected_id)? {
      RpcReply::Notification(value) => {
        if pending.len() >= MAX_PLANNER_FRAMES { return Err(CodexAppServerDiagnosticCode::CodexAppServerProtocolError); }
        pending.push_back(value);
      }
      RpcReply::Result(value) => return Ok(value),
    }
  }
}

impl CodexAppServerSession {
  pub(super) fn spawn(cwd: &std::path::Path) -> Result<Self, CodexAppServerDiagnosticCode> {
    let (process, receiver) = CodexAppServerProcess::spawn_at(Some(cwd), MAX_PLANNER_PROTOCOL_BYTES)
      .map_err(|_| CodexAppServerDiagnosticCode::CodexAppServerSpawnFailed)?;
    Ok(Self { process, stream: CodexRpcStream { receiver, pending: VecDeque::new() } })
  }

  pub(super) fn initialize(&mut self, deadline: Instant) -> Result<(), CodexAppServerDiagnosticCode> {
    let result = self.request("initialize", planner_initialize_params(), deadline)?;
    for field in ["codexHome", "userAgent", "platformFamily", "platformOs"] {
      if !result.get(field).is_some_and(Value::is_string) { return Err(CodexAppServerDiagnosticCode::CodexAppServerProtocolError); }
    }
    self.process.write_message(&json!({"method":"initialized"}))
  }

  pub(super) fn request(&mut self, method: &str, params: Value, deadline: Instant) -> Result<Value, CodexAppServerDiagnosticCode> {
    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    self.process.write_message(&json!({"id":id,"method":method,"params":params}))?;
    self.stream.await_response(id, deadline)
  }

  pub(super) fn next_notification(&mut self, deadline: Instant) -> Result<Value, CodexAppServerDiagnosticCode> {
    self.stream.next_notification(deadline)
  }

  pub(super) fn pop_pending_notification(&mut self) -> Option<Value> {
    self.stream.pop_pending_notification()
  }

  pub(super) fn shutdown(&mut self) -> Result<(), CodexAppServerDiagnosticCode> { self.process.shutdown() }
}

fn reject_server_request(object: &serde_json::Map<String,Value>) -> Result<(), CodexAppServerDiagnosticCode> {
  if let Some(id) = object.get("id") {
    if id.is_string() || id.as_i64().is_some() || id.as_u64().is_some() {
      return Err(CodexAppServerDiagnosticCode::CodexAppServerUnexpectedServerRequest);
    }
    return Err(CodexAppServerDiagnosticCode::CodexAppServerProtocolError);
  }
  Ok(())
}

impl CodexRpcStream {
  pub(super) fn await_response(&mut self, id: u64, deadline: Instant) -> Result<Value, CodexAppServerDiagnosticCode> {
    let mut pending = std::mem::take(&mut self.pending);
    let result = await_reply(|| self.receive(deadline), id, &mut pending);
    self.pending = pending;
    result
  }

  pub(super) fn next_notification(&mut self, deadline: Instant) -> Result<Value, CodexAppServerDiagnosticCode> {
    if let Some(value) = self.pop_pending_notification() { return Ok(value); }
    let value = self.receive(deadline)?;
    let object = value.as_object().ok_or(CodexAppServerDiagnosticCode::CodexAppServerProtocolError)?;
    if object.get("method").and_then(Value::as_str).is_none()
      || object.contains_key("result") || object.contains_key("error") {
      return Err(CodexAppServerDiagnosticCode::CodexAppServerProtocolError);
    }
    reject_server_request(object)?;
    Ok(value)
  }

  // Only notifications already parsed while awaiting an RPC reply. No IO,
  // no waiting and no replenishment: the producer's frame budget bounds this.
  pub(super) fn pop_pending_notification(&mut self) -> Option<Value> {
    self.pending.pop_front()
  }

  fn receive(&mut self, deadline: Instant) -> Result<Value, CodexAppServerDiagnosticCode> {
    receive_value(&self.receiver, deadline)
  }
}

#[cfg(test)]
pub(super) fn test_planner_stream(input: &[u8]) -> CodexRpcStream {
  let (sender,receiver) = frame_channel(MAX_PLANNER_PROTOCOL_BYTES);
  // Intentionally finish the producer before consuming to reproduce bursts.
  read_frames_limited(input,sender,MAX_PLANNER_PROTOCOL_BYTES);
  CodexRpcStream { receiver, pending: VecDeque::new() }
}

fn receive_value(receiver: &Receiver<Frame>, deadline: Instant) -> Result<Value, CodexAppServerDiagnosticCode> {
    use CodexAppServerDiagnosticCode::*;
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() { return Err(CodexAppServerHandshakeTimeout); }
    match receiver.recv_timeout(remaining) {
      Ok(Frame::Line(line)) => serde_json::from_slice(&line).map_err(|_| CodexAppServerProtocolError),
      Ok(Frame::TooLarge | Frame::ReadFailed) => Err(CodexAppServerProtocolError),
      Err(RecvTimeoutError::Timeout) => Err(CodexAppServerHandshakeTimeout),
      Err(RecvTimeoutError::Disconnected) => Err(CodexAppServerClosed),
    }
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

  #[test] fn interleaved_pending_survives_rpc_timeout_protocol_and_close() {
    use CodexAppServerDiagnosticCode::*;
    for failure in [CodexAppServerHandshakeTimeout, CodexAppServerProtocolError, CodexAppServerClosed] {
      let notification=json!({"method":"item/started","params":{"item":{"type":"commandExecution"}}});
      let mut incoming=VecDeque::from([Ok(notification.clone()),Err(failure)]);
      let mut pending=VecDeque::new();
      assert!(matches!(await_reply(|| incoming.pop_front().unwrap(),2,&mut pending),Err(code) if code==failure));
      assert_eq!(pending.pop_front(),Some(notification));
      assert!(pending.is_empty());
    }
  }

  #[test] fn regression_planner_burst_preserves_1024_frames() {
    let input=b"{\"method\":\"item/agentMessage/delta\",\"params\":{\"delta\":\"x\"}}\n".repeat(1024);
    let (sender,receiver)=frame_channel(MAX_PLANNER_PROTOCOL_BYTES);
    read_frames_limited(&input[..],sender,MAX_PLANNER_PROTOCOL_BYTES);
    assert_eq!(receiver.into_iter().filter(|frame| matches!(frame,Frame::Line(_))).count(),1024);
  }

  #[test] fn regression_planner_receives_more_than_256_notifications() {
    let input=b"{\"method\":\"item/agentMessage/delta\",\"params\":{\"delta\":\"x\"}}\n".repeat(1024);
    let mut stream=test_planner_stream(&input);
    for _ in 0..1024 { stream.next_notification(Instant::now()+Duration::from_secs(1)).unwrap(); }
  }

  #[test] fn planner_pending_rpc_reply_preserves_large_interleaved_burst() {
    let mut input=b"{\"method\":\"item/agentMessage/delta\",\"params\":{\"delta\":\"x\"}}\n".repeat(2048);
    input.extend_from_slice(b"{\"id\":7,\"result\":{\"ok\":true}}\n");
    let mut stream=test_planner_stream(&input);
    assert_eq!(stream.await_response(7,Instant::now()+Duration::from_secs(1)).unwrap()["ok"],true);
    for _ in 0..2048 { assert_eq!(stream.next_notification(Instant::now()+Duration::from_secs(1)).unwrap()["method"],"item/agentMessage/delta"); }
    assert!(matches!(stream.next_notification(Instant::now()+Duration::from_secs(1)),Err(CodexAppServerDiagnosticCode::CodexAppServerClosed)));
  }

  #[test] fn planner_queue_frame_budget_and_undrained_reader_completion() {
    assert_eq!(MAX_PLANNER_FRAMES,MAX_PLANNER_PROTOCOL_BYTES/FRAME_METADATA_ALLOWANCE);
    let input=b"{}\n".repeat(MAX_PLANNER_FRAMES+1);
    assert!(input.len()<MAX_PLANNER_PROTOCOL_BYTES);
    let (sender,receiver)=frame_channel(MAX_PLANNER_PROTOCOL_BYTES);
    let (done_sender,done_receiver)=mpsc::channel();
    let reader=thread::spawn(move || { read_frames_limited(&input[..],sender,MAX_PLANNER_PROTOCOL_BYTES); done_sender.send(()).unwrap(); });
    // Keep the consumer alive but don't drain it. Reader must not await capacity.
    done_receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    reader.join().unwrap();
    for _ in 0..MAX_PLANNER_FRAMES { assert!(matches!(receiver.recv().unwrap(),Frame::Line(_))); }
    assert!(matches!(receiver.recv().unwrap(),Frame::TooLarge));
    assert!(receiver.recv().is_err());
  }

  #[test] fn planner_message_and_total_wire_byte_budgets_fail_closed() {
    let (sender,receiver)=frame_channel(MAX_PLANNER_PROTOCOL_BYTES);
    read_frames_limited(&vec![b'a';MAX_MESSAGE_BYTES+1][..],sender,MAX_PLANNER_PROTOCOL_BYTES);
    assert!(matches!(receiver.recv().unwrap(),Frame::TooLarge));
    let (sender,receiver)=frame_channel(MAX_PLANNER_PROTOCOL_BYTES);
    let input=[vec![b'a';4095],vec![b'\n']].concat().repeat(MAX_PLANNER_PROTOCOL_BYTES/4096+1);
    read_frames_limited(&input[..],sender,MAX_PLANNER_PROTOCOL_BYTES);
    let frames:Vec<_>=receiver.into_iter().collect();
    assert!(matches!(frames.last(),Some(Frame::TooLarge)));
    assert!(frames.iter().filter(|f|matches!(f,Frame::Line(_))).count()<MAX_PLANNER_FRAMES);
    // Exact per-message limit remains accepted by framing.
    let (sender,receiver)=frame_channel(MAX_PLANNER_PROTOCOL_BYTES);
    let mut input=vec![b'a';MAX_MESSAGE_BYTES]; input.push(b'\n');
    read_frames_limited(&input[..],sender,MAX_PLANNER_PROTOCOL_BYTES);
    assert!(matches!(receiver.recv().unwrap(),Frame::Line(line) if line.len()==MAX_MESSAGE_BYTES));
  }

  #[test] fn planner_eof_read_failure_and_malformed_json_fail_closed() {
    use CodexAppServerDiagnosticCode::*;
    for (input,expected) in [(b"".as_slice(),CodexAppServerClosed),(b"{",CodexAppServerProtocolError),
        (b"not json\n",CodexAppServerProtocolError)] {
      let mut stream=test_planner_stream(input);
      assert_eq!(stream.next_notification(Instant::now()+Duration::from_secs(1)).unwrap_err(),expected);
    }
    struct FailedReader;
    impl Read for FailedReader { fn read(&mut self,_:&mut [u8])->io::Result<usize> { Err(io::Error::other("synthetic")) } }
    let (sender,receiver)=frame_channel(MAX_PLANNER_PROTOCOL_BYTES);
    read_frames_limited(FailedReader,sender,MAX_PLANNER_PROTOCOL_BYTES);
    assert_eq!(receive_value(&receiver,Instant::now()+Duration::from_secs(1)).unwrap_err(),CodexAppServerProtocolError);
  }

  #[test] fn server_request_has_distinct_closed_code_and_unsolicited_response_is_rejected() {
    use CodexAppServerDiagnosticCode::*;
    let request=json!({"id":"server-request","method":"item/commandExecution/requestApproval","params":{"private":"payload"}});
    assert!(matches!(classify_rpc(request.clone(),7),Err(CodexAppServerUnexpectedServerRequest)));
    let mut wire=request.to_string(); wire.push('\n');
    let mut stream=test_planner_stream(wire.as_bytes());
    assert_eq!(stream.next_notification(Instant::now()+Duration::from_secs(1)).unwrap_err(),CodexAppServerUnexpectedServerRequest);
    for raw in [b"{\"id\":7,\"result\":{}}\n".as_slice(),b"{\"id\":7,\"error\":{\"message\":\"private\"}}\n",
        b"{\"method\":\"request\",\"id\":null,\"params\":{}}\n"] {
      assert_eq!(test_planner_stream(raw).next_notification(Instant::now()+Duration::from_secs(1)).unwrap_err(),CodexAppServerProtocolError);
    }
  }

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

  #[test] fn planner_opts_into_local_experimental_thread_fields() {
    assert_eq!(planner_initialize_params()["capabilities"]["experimentalApi"], true);
    assert!(initialize_request(1)["params"].get("capabilities").is_none());
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

  fn queued_initialize_after_notifications(count: usize) -> Result<CodexAppServerProbe, CodexAppServerDiagnosticCode> {
    let mut input = Vec::new();
    for _ in 0..count { input.extend_from_slice(b"{\"method\":\"config/warning\",\"params\":{}}\n"); }
    input.extend_from_slice(&response(77));
    input.push(b'\n');
    let (sender, receiver) = mpsc::sync_channel(FRAME_CHANNEL_CAPACITY);
    // Produce the entire burst before consuming, exercising the bounded channel's capacity.
    read_frames(&input[..], sender);
    await_initialize(&receiver, 77, Instant::now() + Duration::from_secs(1))
  }

  #[test] fn maximum_notifications_then_response_succeeds() {
    assert_eq!(FRAME_CHANNEL_CAPACITY, MAX_NOTIFICATIONS + 1);
    assert!(queued_initialize_after_notifications(MAX_NOTIFICATIONS).unwrap().initialized);
  }

  #[test] fn notification_over_limit_is_protocol_error() {
    assert!(matches!(queued_initialize_after_notifications(MAX_NOTIFICATIONS + 1),
      Err(CodexAppServerDiagnosticCode::CodexAppServerProtocolError)));
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

  #[test] fn rpc_ids_notifications_and_server_requests() {
    let notification = json!({"method":"item/completed","params":{}});
    assert!(matches!(classify_rpc(notification, 42), Ok(RpcReply::Notification(_))));
    assert!(matches!(classify_rpc(json!({"id":42,"result":{}}), 42), Ok(RpcReply::Result(_))));
    assert!(classify_rpc(json!({"id":43,"result":{}}), 42).is_err());
    assert!(classify_rpc(json!({"id":42,"method":"item/commandExecution/requestApproval","params":{}}), 42).is_err());
  }

  #[test] fn fake_transport_correlates_interleaved_reply() {
    let mut messages = std::collections::VecDeque::from([
      json!({"method":"config/warning","params":{}}), json!({"id":7,"result":{"ok":true}})
    ]);
    let mut pending = VecDeque::new();
    let result = await_reply(|| messages.pop_front().ok_or(CodexAppServerDiagnosticCode::CodexAppServerClosed), 7, &mut pending).unwrap();
    assert_eq!(result["ok"],true); assert_eq!(pending.len(),1);
    let mut messages = std::collections::VecDeque::from([json!({"id":8,"result":{}})]);
    assert!(await_reply(|| messages.pop_front().ok_or(CodexAppServerDiagnosticCode::CodexAppServerClosed), 7, &mut VecDeque::new()).is_err());
  }

  #[test] fn transport_eof_and_timeout_fail_closed() {
    let (sender, receiver) = mpsc::sync_channel(1);
    assert!(matches!(receive_value(&receiver, Instant::now()), Err(CodexAppServerDiagnosticCode::CodexAppServerHandshakeTimeout)));
    drop(sender);
    assert!(matches!(receive_value(&receiver, Instant::now() + Duration::from_secs(1)), Err(CodexAppServerDiagnosticCode::CodexAppServerClosed)));
  }

  #[cfg(unix)]
  #[test] fn shutdown_reaps_stuck_child() {
    let mut child = Command::new("sleep").arg("30").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let stdout = child.stdout.take().unwrap(); let stderr = child.stderr.take().unwrap();
    let (sender, _receiver) = mpsc::sync_channel(1);
    let stdout_reader = thread::spawn(move || read_frames(stdout, sender));
    let stderr_reader = thread::spawn(move || drain_stderr(stderr));
    let mut process = CodexAppServerProcess { child, stdout_reader: Some(stdout_reader), stderr_reader: Some(stderr_reader) };
    assert!(process.shutdown().is_ok());
    assert!(process.child.try_wait().unwrap().is_some());
  }

  // Manual gate only; ordinary cargo test remains independent of the Codex CLI.
  #[test] #[ignore = "requires a local Codex app-server"]
  fn real_app_server_handshake() {
    let probe = run_probe();
    assert!(probe.initialized, "sanitized diagnostic: {:?}", probe.diagnostic_code);
  }
}
