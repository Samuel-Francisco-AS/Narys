use super::*;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
fn fixture() -> (Database, PathBuf) {
  let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  let dir = std::env::temp_dir().join(format!("lr4-synthetic-{}-{n}",std::process::id()));
  fs::create_dir_all(&dir).unwrap();
  (Database::for_test(dir.join("test.sqlite3")),dir.join("bootstrap.json"))
}
fn data(version: &str) -> Value { json!({
  "identity":{"version":version,"canonicalName":"Synthetic","presentation":"neutral","primaryLanguage":"pt-BR","concept":"test",
    "traits":{"curiosity":"high"},"behavioralInvariants":["be_clear"],"modes":{"test":{"priority":"test","tone":"calm"}},
    "relationship":{"primaryPersonName":"Tester","relationModes":["testing"],
      "affectionStyle":{"warm":false,"provocative":false,"playfulJealousy":false,"playfulTerritoriality":false,"coercion":false,"isolation":false,"emotionalBlackmail":false},
      "interactionPreferences":{"wantsRealDisagreement":true,"wantsLunaToProposeDirectionsDuringStructuring":false,"prefersLinearFlowDuringImplementation":true}},
    "memoryPolicy":{"retrieval":"selective","history":"versioned","continuity":"revisable","storePrivateChainOfThought":false},
    "provenance":"synthetic test","effectiveFrom":"2026-01-01"},
  "memories":[
    {"importKey":"synthetic-a","type":"preference","domains":["tests"],"state":"active","title":"A","summary":"Synthetic A","importance":9,"confidence":"high","eventDate":"2026-01-02"},
    {"importKey":"synthetic-b","type":"decision","domains":["other"],"state":"active","title":"B","summary":"Synthetic B","importance":4,"confidence":"medium","eventDate":"2026-01-01"}
  ]}) }
fn write(path:&std::path::Path,value:&Value) { fs::write(path,serde_json::to_vec(value).unwrap()).unwrap(); }
#[test]
fn migration_empty_and_twice() {
  let (db,_) = fixture(); let conn=db.open().unwrap();
  let v:i64=conn.pragma_query_value(None,"user_version",|r|r.get(0)).unwrap(); assert_eq!(v,1);
  drop(conn); assert!(db.open().is_ok());
}
#[test]
fn identity_versions_and_memory_import() {
  let (db,path)=fixture(); write(&path,&data("v1"));
  assert_eq!(import_bootstrap(&db,&path).unwrap().memories_inserted,2);
  assert_eq!(import_bootstrap(&db,&path).unwrap().memories_inserted,0);
  let conn=db.open().unwrap();
  assert_eq!(identity::current_identity(&conn).unwrap().unwrap().input.version,"v1");
  let all=memory::list_active_memories(&conn,10).unwrap(); assert_eq!(all.len(),2); assert_eq!(all[0].title,"A");
  assert_eq!(memory::list_memories_by_domain(&conn,"tests",10).unwrap().len(),1);
  assert_eq!(memory::active_memories(&conn,memory::MemoryFilter{kind:Some("preference"),min_importance:Some(8),limit:10,..Default::default()}).unwrap().len(),1);
  drop(conn);
  let mut next=data("v2"); next["memories"]=json!([]); write(&path,&next);
  assert!(import_bootstrap(&db,&path).unwrap().identity_inserted);
  let conn=db.open().unwrap(); let current=identity::current_identity(&conn).unwrap().unwrap();
  assert_eq!(current.input.version,"v2"); assert!(current.supersedes_id.is_some());
  let counts:(i64,i64)=conn.query_row("SELECT COUNT(*),SUM(is_current) FROM identity_snapshots",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
  assert_eq!(counts,(2,1));
}
#[test]
fn conversation_and_task_survive_reopen() {
  let (db,_)=fixture(); let mut conn=db.open().unwrap();
  let id=conversation::create_diagnostic(&mut conn).unwrap(); assert_eq!(conversation::create_diagnostic(&mut conn).unwrap(),id);
  task_history::insert(&conn,&task_history::TaskRecord{task_id:7,kind:"mock".into(),state:"completed".into(),started_at:"2026-01-01T00:00:00Z".into(),finished_at:"2026-01-01T00:00:01Z".into(),summary:None,error_code:None}).unwrap();
  drop(conn); let conn=db.open().unwrap();
  assert_eq!(conversation::recent(&conn).unwrap().unwrap().messages.len(),2);
  assert_eq!(task_history::max_id(&conn).unwrap(),7);
}
#[test]
fn invalid_bootstrap_has_no_partial_import() {
  let (db,path)=fixture(); let mut bad=data("bad"); bad["memories"][1]["importance"]=json!(99); write(&path,&bad);
  assert!(import_bootstrap(&db,&path).is_err()); let conn=db.open().unwrap();
  assert!(identity::current_identity(&conn).unwrap().is_none());
  assert!(memory::list_active_memories(&conn,10).unwrap().is_empty());
}
#[test]
fn transaction_rolls_back_on_write_error() {
  let (db,path)=fixture(); let conn=db.open().unwrap();
  conn.execute_batch("CREATE TRIGGER reject_memory BEFORE INSERT ON memory_records BEGIN SELECT RAISE(ABORT, 'synthetic rejection'); END;").unwrap(); drop(conn);
  write(&path,&data("rollback")); assert!(import_bootstrap(&db,&path).is_err());
  let conn=db.open().unwrap(); assert!(identity::current_identity(&conn).unwrap().is_none());
  let count:i64=conn.query_row("SELECT COUNT(*) FROM memory_records",[],|r|r.get(0)).unwrap(); assert_eq!(count,0);
}
#[test]
fn conflicting_import_keys_are_rejected() {
  let (db,path)=fixture(); write(&path,&data("v1")); import_bootstrap(&db,&path).unwrap();
  let mut changed=data("v1"); changed["identity"]["canonicalName"]=json!("Changed"); write(&path,&changed);
  assert!(matches!(import_bootstrap(&db,&path),Err(database::PersistenceError::Conflict)));
  let mut changed=data("v1"); changed["memories"][0]["summary"]=json!("Changed summary"); write(&path,&changed);
  assert!(matches!(import_bootstrap(&db,&path),Err(database::PersistenceError::Conflict)));
  let conn=db.open().unwrap(); assert_eq!(identity::current_identity(&conn).unwrap().unwrap().input.canonical_name,"Synthetic");
}

#[test]
fn gemini_exchange_rolls_back_if_final_answer_cannot_be_saved() {
  let (db,_) = fixture();
  let mut conn = db.open().unwrap();
  conn.execute_batch("CREATE TRIGGER reject_gemini_answer BEFORE INSERT ON conversation_messages WHEN NEW.role='assistant' BEGIN SELECT RAISE(ABORT, 'synthetic rejection'); END;").unwrap();
  assert!(conversation::append_gemini_exchange(&mut conn,"pergunta neutra","resposta final").is_err());
  assert!(conversation::gemini_session(&conn).unwrap().is_none());
  conn.execute_batch("DROP TRIGGER reject_gemini_answer").unwrap();
  conversation::append_gemini_exchange(&mut conn,"pergunta neutra","resposta final").unwrap();
  let session = conversation::gemini_session(&conn).unwrap().unwrap();
  assert_eq!(session.messages.len(),2);
  task_history::insert(&conn,&task_history::TaskRecord { task_id:17,kind:"gemini_chat".into(),state:"completed".into(),
    started_at:"2026-01-01T00:00:00Z".into(),finished_at:"2026-01-01T00:00:01Z".into(),summary:None,error_code:None }).unwrap();
  task_history::mark_failed(&conn,17,"channel_closed").unwrap();
  let (state,code):(String,String)=conn.query_row("SELECT state,error_code FROM task_records WHERE task_id=17",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
  assert_eq!((state.as_str(),code.as_str()),("failed","channel_closed"));
}

#[test]
fn explicit_sessions_are_isolated_and_survive_restart_without_selection() {
  let (db, _) = fixture();
  let mut conn = db.open().unwrap();
  let a = conversation::create_session(&conn).unwrap();
  let b = conversation::create_session(&conn).unwrap();
  assert_ne!(a, b);
  conversation::append_exchange_to_session(&mut conn, a, "A pergunta", "A resposta").unwrap();
  conversation::append_exchange_to_session(&mut conn, b, "B pergunta", "B resposta").unwrap();
  assert_eq!(conversation::session(&conn, a).unwrap().unwrap().messages[0].content, "A pergunta");
  assert_eq!(conversation::session(&conn, b).unwrap().unwrap().messages[0].content, "B pergunta");
  let serialized = serde_json::to_value(conversation::session(&conn, a).unwrap().unwrap()).unwrap();
  assert_eq!(serialized["messages"][0]["sessionId"], json!(a));
  assert!(conversation::session(&conn, 0).unwrap().is_none());
  assert!(!conversation::is_active_session(&conn, i64::MAX).unwrap());
  assert!(conversation::append_exchange_to_session(&mut conn, i64::MAX, "x", "y").is_err());
  drop(conn);
  let conn = db.open().unwrap();
  assert_eq!(conversation::session(&conn, a).unwrap().unwrap().messages.len(), 2);
  assert_eq!(conversation::session(&conn, b).unwrap().unwrap().messages.len(), 2);
  // The database retains both sessions. The runtime starts without choosing either ID.
}

#[test]
fn explicit_exchange_is_atomic_on_assistant_failure() {
  let (db, _) = fixture();
  let mut conn = db.open().unwrap();
  let id = conversation::create_session(&conn).unwrap();
  conn.execute_batch("CREATE TRIGGER reject_answer BEFORE INSERT ON conversation_messages WHEN NEW.role='assistant' BEGIN SELECT RAISE(ABORT, 'synthetic'); END;").unwrap();
  assert!(conversation::append_exchange_to_session(&mut conn, id, "user", "assistant").is_err());
  assert!(conversation::session(&conn, id).unwrap().unwrap().messages.is_empty());
}
