use std::{fs, sync::{atomic::{AtomicBool, Ordering}, Arc}, time::{Duration, SystemTime, UNIX_EPOCH}};
use serde_json::json;
use crate::persistence::{database::Database, identity::{self, IdentityInput}, memory::{self, MemoryInput}, conversation};
use super::{context::{ContextBuilder, ContextError, ContextRequest}, mock::{MockProvider, MockScenario}, registry::ProviderRegistry,
  scheduler::{Scheduler, SchedulerEvent}, types::{ProviderCapabilities, ProviderConfig, ProviderError, ProviderRequest, SchedulerError, TaskBudget}};

fn fixture() -> (Database, std::path::PathBuf) {
  let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  let dir = std::env::temp_dir().join(format!("lr5-synthetic-{}-{n}", std::process::id()));
  fs::create_dir_all(&dir).unwrap();
  (Database::for_test(dir.join("test.sqlite3")), dir)
}
fn identity(version: &str) -> IdentityInput {
  serde_json::from_value(json!({
    "version":version,"canonicalName":"Synthetic","presentation":"neutral","primaryLanguage":"pt-BR","concept":"test",
    "traits":{"curiosity":"high"},"behavioralInvariants":["be_clear"],"modes":{"test":{"priority":"test","tone":"calm"}},
    "relationship":{"primaryPersonName":"Tester","relationModes":["testing"],
      "affectionStyle":{"warm":false,"provocative":false,"playfulJealousy":false,"playfulTerritoriality":false,"coercion":false,"isolation":false,"emotionalBlackmail":false},
      "interactionPreferences":{"wantsRealDisagreement":true,"wantsLunaToProposeDirectionsDuringStructuring":false,"prefersLinearFlowDuringImplementation":true}},
    "memoryPolicy":{"retrieval":"selective","history":"versioned","continuity":"revisable","storePrivateChainOfThought":false},
    "provenance":"synthetic test","effectiveFrom":"2026-01-01"
  })).unwrap()
}
fn seed(db: &Database) {
  let mut conn = db.open().unwrap();
  let tx = conn.transaction().unwrap();
  identity::insert_version(&tx, &identity("v1")).unwrap();
  identity::insert_version(&tx, &identity("v2")).unwrap();
  for i in 0..12 {
    memory::import(&tx, &MemoryInput { import_key: format!("synthetic-{i}"), kind: "project".into(),
      domains: vec![if i == 10 { "other" } else { "projects" }.into()],
      state: if i == 11 { "historical" } else { "active" }.into(), title: format!("Synthetic {i}"),
      summary: format!("Synthetic private marker {i}"), content: None, retrieval_hint: None, source_context: None,
      importance: i as i64 % 10, confidence: "high".into(), event_date: Some("2026-01-01".into()) }).unwrap();
  }
  tx.commit().unwrap();
}
fn context(db: &Database) -> super::types::ContextBundle {
  ContextBuilder::build(&db.open().unwrap(), ContextRequest { domain: Some("projects"), kind: None,
    min_importance: 0, memory_limit: 3, include_recent_conversation: false }).unwrap()
}
fn request(db: &Database) -> ProviderRequest { ProviderRequest { input: "synthetic".into(), context: Arc::new(context(db)),
  max_output_tokens: 30, required_capabilities: ProviderCapabilities::text_stream() } }
fn entry(id: &str, priority: u16, enabled: bool, caps: ProviderCapabilities, mock: Arc<MockProvider>, registry: &mut ProviderRegistry) {
  registry.register(ProviderConfig { id:id.into(), enabled, priority, capabilities:caps }, mock).unwrap();
}
fn budget(calls: u32) -> TaskBudget { TaskBudget { max_provider_calls:calls, max_output_tokens:30 } }

#[test]
fn context_builder_selects_current_active_filtered_bounded_and_optional_conversation() {
  let (db, dir) = fixture();
  let conn = db.open().unwrap();
  assert_eq!(ContextBuilder::build(&conn, ContextRequest { domain:None,kind:None,min_importance:0,memory_limit:5,include_recent_conversation:false }).unwrap_err(), ContextError::IdentityUnavailable);
  drop(conn); seed(&db);
  let conn = db.open().unwrap();
  let bundle = ContextBuilder::build(&conn, ContextRequest { domain:Some("projects"),kind:Some("project"),min_importance:5,memory_limit:4,include_recent_conversation:true }).unwrap();
  assert_eq!(bundle.identity.version,"v2");
  assert!(bundle.relevant_memories.len() <= 4);
  assert!(bundle.relevant_memories.iter().all(|m| m.state == "active" && m.domains.contains(&"projects".into()) && m.importance >= 5));
  assert!(bundle.relevant_memories.windows(2).all(|w| w[0].importance >= w[1].importance));
  assert!(bundle.recent_messages.is_empty());
  assert_eq!(context(&db).relevant_memories.len(),3);
  drop(conn);
  let mut conn=db.open().unwrap(); let session_id=conversation::create_diagnostic(&mut conn).unwrap();
  for i in 0..10 { conn.execute("INSERT INTO conversation_messages(session_id,role,content) VALUES (?1,'user',?2)",rusqlite::params![session_id,format!("Synthetic message {i}")]).unwrap(); }
  let with_recent = ContextBuilder::build(&conn, ContextRequest { domain:None,kind:None,min_importance:0,memory_limit:99,include_recent_conversation:true }).unwrap();
  assert_eq!(with_recent.recent_messages.len(),6);
  assert_eq!(with_recent.recent_messages[0].content,"Synthetic message 4");
  assert_eq!(with_recent.relevant_memories.len(),5);
  drop(conn); fs::remove_dir_all(dir).unwrap();
}

#[test]
fn registry_filters_disabled_capability_and_orders_priority() {
  let mut registry=ProviderRegistry::default();
  let disabled=Arc::new(MockProvider::new(MockScenario::Normal));
  let no_stream=Arc::new(MockProvider::new(MockScenario::Normal));
  let preferred=Arc::new(MockProvider::new(MockScenario::Normal));
  entry("disabled",0,false,ProviderCapabilities::text_stream(),disabled.clone(),&mut registry);
  entry("no-stream",1,true,ProviderCapabilities { text_generation:true,..Default::default() },no_stream.clone(),&mut registry);
  entry("preferred",2,true,ProviderCapabilities::text_stream(),preferred.clone(),&mut registry);
  assert!(registry.get("preferred").is_some());
  assert_eq!(registry.eligible(&ProviderCapabilities::text_stream()).iter().map(|e|e.config.id.as_str()).collect::<Vec<_>>(),vec!["preferred"]);
  assert_eq!(disabled.calls(),0); assert_eq!(no_stream.calls(),0);
  let second=Arc::new(MockProvider::new(MockScenario::Normal));
  entry("second",3,true,ProviderCapabilities::text_stream(),second,&mut registry);
  assert_eq!(registry.eligible(&ProviderCapabilities::text_stream()).iter().map(|e|e.config.id.as_str()).collect::<Vec<_>>(),vec!["preferred","second"]);
}

#[test]
fn scheduler_fallback_cooldown_budget_retry_and_usage() {
  let (db,dir)=fixture(); seed(&db);
  let primary=Arc::new(MockProvider::new(MockScenario::RateLimited));
  let fallback=Arc::new(MockProvider::new(MockScenario::Normal));
  let mut registry=ProviderRegistry::default();
  entry("mock-primary",1,true,ProviderCapabilities::text_stream(),primary.clone(),&mut registry);
  entry("mock-fallback",2,true,ProviderCapabilities::text_stream(),fallback.clone(),&mut registry);
  let scheduler=Scheduler::new(registry);
  let cancelled=AtomicBool::new(false);
  let mut events=vec![];
  let result=tauri::async_runtime::block_on(scheduler.run(request(&db),budget(3),&cancelled,&mut |e| events.push(e))).unwrap();
  assert_eq!(result.provider_id,"mock-fallback"); assert_eq!(result.usage.provider_calls,2);
  assert_eq!(result.usage.providers_used,vec!["mock-primary","mock-fallback"]);
  assert_eq!(result.usage.fallbacks,1); assert!(result.usage.input_tokens > 0); assert!(result.usage.output_tokens > 0);
  assert!(events.iter().any(|e|matches!(e,SchedulerEvent::Fallback { reason_code:"rate_limited",.. })));
  assert!(scheduler.status().iter().any(|s|s.id=="mock-primary" && s.cooldown_ms>0));
  let next=tauri::async_runtime::block_on(scheduler.run(request(&db),budget(3),&cancelled,&mut |_| {})).unwrap();
  assert_eq!(next.usage.provider_calls,1); assert_eq!(primary.calls(),1); assert_eq!(fallback.calls(),2);
  let mut registry=ProviderRegistry::default();
  let limited=Arc::new(MockProvider::new(MockScenario::RateLimited));
  let untouched=Arc::new(MockProvider::new(MockScenario::Normal));
  entry("a",1,true,ProviderCapabilities::text_stream(),limited.clone(),&mut registry);
  entry("b",2,true,ProviderCapabilities::text_stream(),untouched.clone(),&mut registry);
  assert_eq!(tauri::async_runtime::block_on(Scheduler::new(registry).run(request(&db),budget(1),&cancelled,&mut |_| {})).unwrap_err(),SchedulerError::BudgetExceeded);
  assert_eq!(untouched.calls(),0);
  let mut registry=ProviderRegistry::default();
  let timeout=Arc::new(MockProvider::new(MockScenario::Timeout));
  entry("timeout",1,true,ProviderCapabilities::text_stream(),timeout.clone(),&mut registry);
  let retry=tauri::async_runtime::block_on(Scheduler::new(registry).run(request(&db),budget(2),&cancelled,&mut |_| {})).unwrap();
  assert_eq!(retry.usage.provider_calls,2); assert_eq!(retry.usage.retries,1); assert_eq!(timeout.calls(),2);
  let mut registry=ProviderRegistry::default();
  let transient=Arc::new(MockProvider::new(MockScenario::TransientThenSuccess));
  entry("transient",1,true,ProviderCapabilities::text_stream(),transient.clone(),&mut registry);
  let recovered=tauri::async_runtime::block_on(Scheduler::new(registry).run(request(&db),budget(2),&cancelled,&mut |_| {})).unwrap();
  assert_eq!(recovered.usage.retries,1); assert_eq!(transient.calls(),2);
  fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mock_streaming_cancellation_and_error_classes() {
  let (db,dir)=fixture(); seed(&db);
  let cancelled=Arc::new(AtomicBool::new(false));
  let mock=Arc::new(MockProvider::new(MockScenario::Streaming));
  let mut registry=ProviderRegistry::default(); entry("stream",1,true,ProviderCapabilities::text_stream(),mock,&mut registry);
  let scheduler=Scheduler::new(registry);
  let mut chunks=vec![];
  let result=tauri::async_runtime::block_on(scheduler.run(request(&db),budget(1),&cancelled,&mut |e| { if let SchedulerEvent::Chunk{text,..}=e {chunks.push(text)} })).unwrap();
  assert_eq!(chunks,vec!["Analisando ","contexto ","local..."]);
  assert_eq!(chunks.concat(),result.text);
  let signal=cancelled.clone();
  let handle=std::thread::spawn(move || {std::thread::sleep(Duration::from_millis(145));signal.store(true,Ordering::Release)});
  let stopped=tauri::async_runtime::block_on(scheduler.run(request(&db),budget(2),&cancelled,&mut |_| {}));
  handle.join().unwrap(); assert_eq!(stopped.unwrap_err(),SchedulerError::Cancelled);
  for (scenario,error) in [(MockScenario::QuotaExceeded,ProviderError::QuotaExceeded),(MockScenario::Fatal,ProviderError::Fatal)] {
    let mut registry=ProviderRegistry::default(); entry("only",1,true,ProviderCapabilities::text_stream(),Arc::new(MockProvider::new(scenario)),&mut registry);
    let signal=AtomicBool::new(false);
    assert_eq!(tauri::async_runtime::block_on(Scheduler::new(registry).run(request(&db),budget(2),&signal,&mut |_| {})).unwrap_err(),SchedulerError::Provider(error));
  }
  fs::remove_dir_all(dir).unwrap();
}

#[test]
fn disabled_provider_is_never_called_and_output_budget_is_respected() {
  let (db,dir)=fixture(); seed(&db);
  let disabled=Arc::new(MockProvider::new(MockScenario::Normal));
  let enabled=Arc::new(MockProvider::new(MockScenario::Normal));
  let mut registry=ProviderRegistry::default();
  entry("disabled",0,false,ProviderCapabilities::text_stream(),disabled.clone(),&mut registry);
  entry("enabled",1,true,ProviderCapabilities::text_stream(),enabled.clone(),&mut registry);
  let signal=AtomicBool::new(false);
  let result=tauri::async_runtime::block_on(Scheduler::new(registry).run(request(&db),TaskBudget{max_provider_calls:1,max_output_tokens:2},&signal,&mut |_| {})).unwrap();
  assert_eq!(result.provider_id,"enabled"); assert_eq!(result.usage.output_tokens,2); assert_eq!(disabled.calls(),0); assert_eq!(enabled.calls(),1);
  fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cancellation_prevents_fallback_and_normal_is_deterministic() {
  let (db,dir)=fixture(); seed(&db);
  let primary=Arc::new(MockProvider::new(MockScenario::Streaming));
  let fallback=Arc::new(MockProvider::new(MockScenario::Normal));
  let mut registry=ProviderRegistry::default();
  entry("primary",1,true,ProviderCapabilities::text_stream(),primary,&mut registry);
  entry("fallback",2,true,ProviderCapabilities::text_stream(),fallback.clone(),&mut registry);
  let scheduler=Scheduler::new(registry);
  let signal=Arc::new(AtomicBool::new(false));
  let signal_for_thread=signal.clone();
  let handle=std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(145)); signal_for_thread.store(true,Ordering::Release); });
  assert_eq!(tauri::async_runtime::block_on(scheduler.run(request(&db),budget(3),&signal,&mut |_| {})).unwrap_err(),SchedulerError::Cancelled);
  handle.join().unwrap(); assert_eq!(fallback.calls(),0);
  let signal=AtomicBool::new(false);
  let mut registry=ProviderRegistry::default();
  entry("normal",1,true,ProviderCapabilities::text_stream(),Arc::new(MockProvider::new(MockScenario::Normal)),&mut registry);
  let result=tauri::async_runtime::block_on(Scheduler::new(registry).run(request(&db),budget(1),&signal,&mut |_| {})).unwrap();
  assert_eq!(result.provider_id,"normal"); assert!(result.text.contains("3 memórias relevantes"));
  assert!(!result.text.contains("Synthetic private marker"));
  fs::remove_dir_all(dir).unwrap();
}
