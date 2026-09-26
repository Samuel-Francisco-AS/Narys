use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
pub enum Action {
  CommandInvoked,
  TaskCancelRequested,
  SecretTestWritten,
  SecretTestDeleted,
  SecurityError,
}

impl Action {
  fn code(self) -> &'static str {
    match self {
      Self::CommandInvoked => "command_invoked",
      Self::TaskCancelRequested => "task_cancel_requested",
      Self::SecretTestWritten => "secret_test_written",
      Self::SecretTestDeleted => "secret_test_deleted",
      Self::SecurityError => "security_error",
    }
  }
}

#[derive(Clone, Copy)]
pub enum Outcome { Allowed, Denied, Succeeded, Failed }

impl Outcome {
  fn code(self) -> &'static str {
    match self {
      Self::Allowed => "allowed",
      Self::Denied => "denied",
      Self::Succeeded => "succeeded",
      Self::Failed => "failed",
    }
  }
}

pub struct AuditEvent {
  action: Action,
  outcome: Outcome,
  task_id: Option<u64>,
  detail_code: Option<&'static str>,
}

impl AuditEvent {
  pub fn new(action: Action, outcome: Outcome) -> Self {
    Self { action, outcome, task_id: None, detail_code: None }
  }

  pub fn with_task_id(mut self, task_id: u64) -> Self {
    self.task_id = Some(task_id);
    self
  }

  pub fn with_detail(mut self, detail_code: &'static str) -> Self {
    self.detail_code = Some(detail_code);
    self
  }

  fn line(&self) -> String {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    format!(
      "security_audit timestamp_ms={timestamp} action={} result={} task_id={} detail_code={}",
      self.action.code(), self.outcome.code(),
      self.task_id.map_or_else(|| "-".to_string(), |id| id.to_string()),
      self.detail_code.unwrap_or("-"),
    )
  }

  pub fn emit(&self) { eprintln!("{}", self.line()); }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn audit_format_has_only_allowed_fields() {
    let line = AuditEvent::new(Action::SecretTestWritten, Outcome::Succeeded).line();
    assert!(line.contains("action=secret_test_written result=succeeded"));
    assert!(!line.contains("lr3_test_secret"));
    assert!(!line.contains("secret_value"));
  }
}
