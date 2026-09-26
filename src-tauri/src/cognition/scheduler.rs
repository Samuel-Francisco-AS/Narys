use std::{collections::HashMap, sync::{atomic::{AtomicBool, Ordering}, Mutex}, time::{Duration, Instant}};
use serde::Serialize;
use super::{registry::ProviderRegistry, types::{ProviderChunk, ProviderError, ProviderRequest, SchedulerError, SchedulerUsage, TaskBudget, TaskResult}};

#[derive(Clone, Debug)]
pub enum SchedulerEvent {
  Selected { provider_id: String, attempt: u32 },
  Chunk { provider_id: String, text: String },
  Retry { provider_id: String, reason_code: &'static str },
  Fallback { from: String, reason_code: &'static str },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus { pub id: String, pub enabled: bool, pub priority: u16, pub capabilities: super::types::ProviderCapabilities, pub cooldown_ms: u64 }

pub struct Scheduler { registry: ProviderRegistry, cooldowns: Mutex<HashMap<String, Instant>> }
impl Scheduler {
  pub fn new(registry: ProviderRegistry) -> Self { Self { registry, cooldowns: Mutex::new(HashMap::new()) } }
  fn cooling(&self, id: &str) -> bool { self.cooldowns.lock().unwrap_or_else(|p| p.into_inner()).get(id).is_some_and(|until| *until > Instant::now()) }
  pub fn status(&self) -> Vec<ProviderStatus> {
    let now = Instant::now();
    let cooldowns = self.cooldowns.lock().unwrap_or_else(|p| p.into_inner());
    self.registry.configs().into_iter().map(|config| ProviderStatus { id: config.id.clone(), enabled: config.enabled,
      priority: config.priority, capabilities: config.capabilities,
      cooldown_ms: cooldowns.get(&config.id).map(|until| until.saturating_duration_since(now).as_millis() as u64).unwrap_or(0) }).collect()
  }
  pub async fn run(&self, request: ProviderRequest, budget: TaskBudget, cancelled: &AtomicBool,
    on_event: &mut (dyn FnMut(SchedulerEvent) + Send)) -> Result<TaskResult, SchedulerError> {
    let output_limit = budget.max_output_tokens.min(request.max_output_tokens);
    let mut usage = SchedulerUsage::default();
    let mut last_error = None;
    let candidates: Vec<_> = self.registry.eligible(&request.required_capabilities).into_iter()
      .filter(|entry| !self.cooling(&entry.config.id)).collect();
    let mut used_any = false;
    for (index, entry) in candidates.iter().enumerate() {
      if cancelled.load(Ordering::Acquire) { return Err(SchedulerError::Cancelled); }
      used_any = true;
      let mut attempt = 0;
      loop {
        if cancelled.load(Ordering::Acquire) { return Err(SchedulerError::Cancelled); }
        if usage.provider_calls >= budget.max_provider_calls || usage.output_tokens >= output_limit { return Err(SchedulerError::BudgetExceeded); }
        attempt += 1;
        usage.provider_calls += 1;
        if attempt > 1 { usage.retries += 1; }
        if !usage.providers_used.contains(&entry.config.id) { usage.providers_used.push(entry.config.id.clone()); }
        on_event(SchedulerEvent::Selected { provider_id: entry.config.id.clone(), attempt });
        let mut chunks = String::new();
        let mut on_chunk = |chunk: ProviderChunk| -> Result<(), ProviderError> {
          if cancelled.load(Ordering::Acquire) { return Err(ProviderError::Cancelled); }
          chunks.push_str(&chunk.text);
          on_event(SchedulerEvent::Chunk { provider_id: entry.config.id.clone(), text: chunk.text });
          Ok(())
        };
        let attempt_request = ProviderRequest { input: request.input.clone(), context: request.context.clone(),
          max_output_tokens: output_limit - usage.output_tokens, required_capabilities: request.required_capabilities };
        // Keep the same structured context across retry/fallback; adapters decide serialization.
        let result = entry.provider.execute(&attempt_request, cancelled, &mut on_chunk).await;
        match result {
          Ok(response) => {
            if cancelled.load(Ordering::Acquire) { return Err(SchedulerError::Cancelled); }
            if response.usage.output_tokens > output_limit - usage.output_tokens { return Err(SchedulerError::BudgetExceeded); }
            usage.input_tokens += response.usage.input_tokens;
            usage.output_tokens += response.usage.output_tokens;
            let text = if response.text.is_empty() { chunks } else { response.text };
            return Ok(TaskResult { text, provider_id: entry.config.id.clone(), usage, context_metadata: request.context.metadata.clone() });
          }
          Err(ProviderError::Cancelled) => return Err(SchedulerError::Cancelled),
          Err(error) => {
            if cancelled.load(Ordering::Acquire) { return Err(SchedulerError::Cancelled); }
            let retry = matches!(error, ProviderError::Timeout | ProviderError::Unavailable) && attempt == 1;
            if retry {
              on_event(SchedulerEvent::Retry { provider_id: entry.config.id.clone(), reason_code: error.code() });
              // Cancellable asynchronous backoff.
              let until = tokio::time::Instant::now() + Duration::from_millis(80);
              while tokio::time::Instant::now() < until {
                if cancelled.load(Ordering::Acquire) { return Err(SchedulerError::Cancelled); }
                tokio::time::sleep(Duration::from_millis(15)).await;
              }
              continue;
            }
            if let ProviderError::RateLimited { retry_after_ms } = error {
              self.cooldowns.lock().unwrap_or_else(|p| p.into_inner()).insert(entry.config.id.clone(), Instant::now() + Duration::from_millis(retry_after_ms.unwrap_or(3_000).max(1)));
            }
            if index + 1 < candidates.len() {
              if usage.provider_calls >= budget.max_provider_calls { return Err(SchedulerError::BudgetExceeded); }
              on_event(SchedulerEvent::Fallback { from: entry.config.id.clone(), reason_code: error.code() });
              usage.fallbacks += 1;
            }
            last_error = Some(error);
            break;
          }
        }
      }
    }
    if cancelled.load(Ordering::Acquire) { return Err(SchedulerError::Cancelled); }
    if !used_any { Err(SchedulerError::NoProvider) } else { Err(SchedulerError::Provider(last_error.unwrap_or(ProviderError::Unavailable))) }
  }
}
