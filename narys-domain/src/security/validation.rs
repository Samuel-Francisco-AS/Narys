pub const MAX_JS_SAFE_TASK_ID: u64 = 9_007_199_254_740_991;

pub fn task_id(raw: u64) -> Result<u64, &'static str> {
  if (1..=MAX_JS_SAFE_TASK_ID).contains(&raw) { Ok(raw) } else { Err("invalid_task_id") }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn task_id_rejects_zero_and_javascript_overflow() {
    assert_eq!(task_id(0), Err("invalid_task_id"));
    assert_eq!(task_id(1), Ok(1));
    assert_eq!(task_id(MAX_JS_SAFE_TASK_ID), Ok(MAX_JS_SAFE_TASK_ID));
    assert_eq!(task_id(MAX_JS_SAFE_TASK_ID + 1), Err("invalid_task_id"));
  }
}
