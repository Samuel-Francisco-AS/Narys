pub mod runtime;
mod task;

use std::sync::Arc;

use tauri::{ipc::Channel, State};

use runtime::TaskRegistry;
use task::{TaskEvent, TaskId};

#[tauri::command]
pub fn start_mock_task(registry: State<'_, Arc<TaskRegistry>>, channel: Channel<TaskEvent>) -> Result<TaskId, String> {
  runtime::start(registry.inner().clone(), channel)
}

#[tauri::command]
pub fn cancel_task(registry: State<'_, Arc<TaskRegistry>>, task_id: u64) -> bool {
  registry.cancel(TaskId(task_id))
}
