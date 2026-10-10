use serde::Serialize;
use std::{
  io::{self, Read},
  process::{Child, Command, Output, Stdio},
  thread::{self, JoinHandle},
  time::{Duration, Instant},
};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CodexAuthKind {
  Chatgpt,
  ApiKey,
  Other,
  Unknown,
  None,
}

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CodexDiagnosticCode {
  CodexNotInstalled,
  CodexNotAuthenticated,
  CodexStatusTimeout,
  CodexStatusFailed,
  CodexStatusUnrecognized,
}

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeStatus {
  pub installed: bool,
  pub version: Option<String>,
  pub authenticated: bool,
  pub auth_kind: CodexAuthKind,
  /// True only when the installed runtime reports an authenticated status.
  pub available: bool,
  pub diagnostic_code: Option<CodexDiagnosticCode>,
}

#[derive(Debug)]
enum CommandResult {
  Completed(Output),
  TimedOut,
}

fn run_codex(args: &[&str]) -> Result<CommandResult, io::Error> {
  let child = Command::new("codex")
    .args(args)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;
  wait_with_timeout(child)
}

fn wait_with_timeout(mut child: Child) -> Result<CommandResult, io::Error> {
  let stdout = child.stdout.take().ok_or_else(|| io::Error::other("codex_stdout_unavailable"))?;
  let stderr = child.stderr.take().ok_or_else(|| io::Error::other("codex_stderr_unavailable"))?;
  let stdout_reader = thread::spawn(|| drain_stream(stdout));
  let stderr_reader = thread::spawn(|| drain_stream(stderr));
  let started = Instant::now();
  loop {
    if let Some(status) = child.try_wait()? {
      return completed_output(status, stdout_reader, stderr_reader);
    }
    if started.elapsed() >= COMMAND_TIMEOUT {
      let _ = child.kill();
      let _ = child.wait();
      let _ = stdout_reader.join();
      let _ = stderr_reader.join();
      return Ok(CommandResult::TimedOut);
    }
    std::thread::sleep(POLL_INTERVAL);
  }
}

fn drain_stream<R: Read>(mut stream: R) -> Vec<u8> {
  let mut retained = Vec::with_capacity(MAX_OUTPUT_BYTES);
  let mut buffer = [0u8; 8192];
  loop {
    match stream.read(&mut buffer) {
      Ok(0) | Err(_) => break,
      Ok(bytes_read) => {
        let remaining = MAX_OUTPUT_BYTES.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..bytes_read.min(remaining)]);
      }
    }
  }
  retained
}

fn completed_output(
  status: std::process::ExitStatus,
  stdout_reader: JoinHandle<Vec<u8>>,
  stderr_reader: JoinHandle<Vec<u8>>,
) -> Result<CommandResult, io::Error> {
  let stdout = stdout_reader.join().map_err(|_| io::Error::other("codex_stdout_reader_failed"))?;
  let stderr = stderr_reader.join().map_err(|_| io::Error::other("codex_stderr_reader_failed"))?;
  Ok(CommandResult::Completed(Output { status, stdout, stderr }))
}

fn output_text(output: &Output) -> String {
  let mut text = String::with_capacity(output.stdout.len() + output.stderr.len());
  text.push_str(&String::from_utf8_lossy(&output.stdout));
  text.push('\n');
  text.push_str(&String::from_utf8_lossy(&output.stderr));
  text
}

fn parse_version(text: &str) -> Option<String> {
  text.split_whitespace()
    .find_map(|token| {
      let candidate = token.trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.' && character != '-');
      let version = candidate.strip_prefix('v').unwrap_or(candidate);
      if version.contains('.') && version.chars().all(|character| character.is_ascii_digit() || character == '.' || character == '-') {
        Some(version.to_string())
      } else {
        None
      }
    })
}

fn classify_authentication(text: &str, successful: bool) -> (bool, CodexAuthKind, Option<CodexDiagnosticCode>) {
  let normalized = text.trim().to_ascii_lowercase();
  if normalized.contains("not logged in")
    || normalized.contains("logged out")
    || normalized.contains("not authenticated")
  {
    return (false, CodexAuthKind::None, Some(CodexDiagnosticCode::CodexNotAuthenticated));
  }
  if !successful {
    return (false, CodexAuthKind::Unknown, Some(CodexDiagnosticCode::CodexStatusFailed));
  }
  if normalized.contains("logged in using chatgpt") {
    return (true, CodexAuthKind::Chatgpt, None);
  }
  if normalized.contains("logged in using api key")
    || normalized.contains("logged in using api_key")
    || normalized.contains("logged in using apikey")
    || normalized.contains("authenticated with api key")
  {
    return (true, CodexAuthKind::ApiKey, None);
  }
  if normalized.contains("logged in") || normalized.contains("authenticated") {
    return (true, CodexAuthKind::Other, None);
  }
  (false, CodexAuthKind::Unknown, Some(CodexDiagnosticCode::CodexStatusUnrecognized))
}

fn unavailable(diagnostic_code: CodexDiagnosticCode, version: Option<String>) -> CodexRuntimeStatus {
  CodexRuntimeStatus {
    installed: !matches!(diagnostic_code, CodexDiagnosticCode::CodexNotInstalled),
    version,
    authenticated: false,
    auth_kind: CodexAuthKind::None,
    available: false,
    diagnostic_code: Some(diagnostic_code),
  }
}

fn detect_status() -> CodexRuntimeStatus {
  let version = match run_codex(&["--version"]) {
    Ok(CommandResult::Completed(output)) if output.status.success() => parse_version(&output_text(&output)),
    Ok(CommandResult::Completed(_)) => return unavailable(CodexDiagnosticCode::CodexStatusFailed, None),
    Ok(CommandResult::TimedOut) => return unavailable(CodexDiagnosticCode::CodexStatusTimeout, None),
    Err(error) if error.kind() == io::ErrorKind::NotFound => {
      return CodexRuntimeStatus {
        installed: false,
        version: None,
        authenticated: false,
        auth_kind: CodexAuthKind::None,
        available: false,
        diagnostic_code: Some(CodexDiagnosticCode::CodexNotInstalled),
      };
    }
    Err(_) => return unavailable(CodexDiagnosticCode::CodexStatusFailed, None),
  };

  let version = match version {
    Some(version) => version,
    None => return unavailable(CodexDiagnosticCode::CodexStatusUnrecognized, None),
  };
  match run_codex(&["login", "status"]) {
    Ok(CommandResult::TimedOut) => unavailable(CodexDiagnosticCode::CodexStatusTimeout, Some(version)),
    Ok(CommandResult::Completed(output)) => {
      let (authenticated, auth_kind, diagnostic_code) =
        classify_authentication(&output_text(&output), output.status.success());
      CodexRuntimeStatus {
        installed: true,
        version: Some(version),
        authenticated,
        auth_kind,
        available: authenticated,
        diagnostic_code,
      }
    }
    Err(_) => unavailable(CodexDiagnosticCode::CodexStatusFailed, Some(version)),
  }
}

pub async fn get_codex_runtime_status() -> Result<CodexRuntimeStatus, String> {
  crate::runtime::spawn_blocking(detect_status)
    .await
    .map_err(|_| "codex_status_failed".to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_valid_version() {
    assert_eq!(parse_version("codex-cli 0.158.0\n"), Some("0.158.0".into()));
    assert_eq!(parse_version("codex-cli v1.2.3"), Some("1.2.3".into()));
  }

  #[test]
  fn classifies_chatgpt_login() {
    assert_eq!(
      classify_authentication("Logged in using ChatGPT\n", true),
      (true, CodexAuthKind::Chatgpt, None)
    );
  }

  #[test]
  fn classifies_api_key_login() {
    assert_eq!(
      classify_authentication("Logged in using API key\n", true),
      (true, CodexAuthKind::ApiKey, None)
    );
  }

  #[test]
  fn positive_markers_fail_closed_when_command_fails() {
    assert_eq!(
      classify_authentication("Logged in using ChatGPT", false),
      (false, CodexAuthKind::Unknown, Some(CodexDiagnosticCode::CodexStatusFailed))
    );
    assert_eq!(
      classify_authentication("Logged in using API key", false),
      (false, CodexAuthKind::Unknown, Some(CodexDiagnosticCode::CodexStatusFailed))
    );
  }

  #[test]
  fn classifies_other_login() {
    assert_eq!(
      classify_authentication("Authenticated with enterprise SSO", true),
      (true, CodexAuthKind::Other, None)
    );
  }

  #[test]
  fn classifies_not_authenticated() {
    assert_eq!(
      classify_authentication("Not logged in", false),
      (false, CodexAuthKind::None, Some(CodexDiagnosticCode::CodexNotAuthenticated))
    );
  }

  #[test]
  fn arbitrary_failure_output_fails_closed() {
    assert_eq!(
      classify_authentication("unexpected error", false),
      (false, CodexAuthKind::Unknown, Some(CodexDiagnosticCode::CodexStatusFailed))
    );
  }

  #[test]
  fn fails_closed_for_unexpected_or_empty_success_output() {
    assert_eq!(
      classify_authentication("unexpected status", true),
      (false, CodexAuthKind::Unknown, Some(CodexDiagnosticCode::CodexStatusUnrecognized))
    );
    assert_eq!(
      classify_authentication("", true),
      (false, CodexAuthKind::Unknown, Some(CodexDiagnosticCode::CodexStatusUnrecognized))
    );
  }
}
