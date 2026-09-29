use serde::Serialize;
use std::{
  io,
  process::{Child, Command, Output, Stdio},
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
  let started = Instant::now();
  loop {
    if child.try_wait()?.is_some() {
      let output = child.wait_with_output()?;
      return Ok(CommandResult::Completed(Output {
        status: output.status,
        stdout: cap_output(output.stdout),
        stderr: cap_output(output.stderr),
      }));
    }
    if started.elapsed() >= COMMAND_TIMEOUT {
      let _ = child.kill();
      let _ = child.wait();
      return Ok(CommandResult::TimedOut);
    }
    std::thread::sleep(POLL_INTERVAL);
  }
}

fn cap_output(mut output: Vec<u8>) -> Vec<u8> {
  output.truncate(MAX_OUTPUT_BYTES);
  output
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
    || (!successful && normalized.is_empty())
  {
    return (false, CodexAuthKind::None, Some(CodexDiagnosticCode::CodexNotAuthenticated));
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
  tauri::async_runtime::spawn_blocking(detect_status)
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
