//! Lifecycle fakes use only memory and a virtual monotonic clock.
use super::*;
use std::{collections::VecDeque, sync::atomic::AtomicUsize};

type Received = Result<Value, CodexAppServerDiagnosticCode>;
struct Fake<'a> {
    control: &'a LifecycleControl,
    clock: Instant,
    calls: Vec<&'static str>,
    notifications: VecDeque<(Received, bool)>,
    interrupt_failed: bool,
    detach_failed: bool,
    shutdown_failed: bool,
    maximum_poll: Duration,
}
impl<'a> Fake<'a> {
    fn new(control: &'a LifecycleControl, notifications: Vec<Value>) -> Self {
        Self { control, clock: Instant::now(), calls: vec![],
            notifications: notifications.into_iter().map(|n| (Ok(n), false)).collect(),
            interrupt_failed: false, detach_failed: false, shutdown_failed: false,
            maximum_poll: Duration::ZERO }
    }
    fn valid(control: &'a LifecycleControl) -> Self { Self::new(control, vec![message(), completed("completed")]) }
    fn interrupts(&self) -> usize { self.calls.iter().filter(|c| **c == "turn/interrupt").count() }
    fn cleaned(&self) { assert!(self.calls.ends_with(&["thread/unsubscribe", "shutdown"])); }
}
impl PlannerProtocol for Fake<'_> {
    fn initialize(&mut self, _: Instant) -> Result<(), ()> { self.calls.push("initialize"); Ok(()) }
    fn request(&mut self, method: &str, params: Value, _: Instant) -> Result<Value, ()> {
        match method {
            "config/read" => { self.calls.push("config/read"); Ok(json!({"config":{}})) }
            "thread/start" => {
                self.calls.push("thread/start");
                Ok(json!({"sandbox":{"type":"readOnly"},"approvalPolicy":"never","cwd":params["cwd"],
                    "runtimeWorkspaceRoots":[],"instructionSources":[],"thread":{"id":"t"}}))
            }
            "turn/start" => { self.calls.push("turn/start"); Ok(json!({"turn":{"id":"v"}})) }
            "turn/interrupt" => {
                self.calls.push("turn/interrupt");
                assert_eq!(params, json!({"threadId":"t","turnId":"v"}));
                if self.interrupt_failed { Err(()) } else { Ok(json!({})) }
            }
            "thread/unsubscribe" => {
                self.calls.push("thread/unsubscribe");
                if self.detach_failed { Err(()) } else { Ok(json!({})) }
            }
            _ => panic!("only lifecycle requests are allowed"),
        }
    }
    fn shutdown(&mut self) -> Result<(), ()> {
        self.calls.push("shutdown");
        if self.shutdown_failed { Err(()) } else { Ok(()) }
    }
}
impl PlannerTurnProtocol for Fake<'_> {
    fn now(&self) -> Instant { self.clock }
    fn next_notification(&mut self, deadline: Instant) -> Received {
        assert!(deadline >= self.clock);
        self.maximum_poll = self.maximum_poll.max(deadline - self.clock);
        match self.notifications.pop_front() {
            Some((result, cancel)) => {
                if cancel { self.control.cancelled.store(true, Ordering::Release); }
                result
            }
            None => { self.clock = deadline; Err(CodexAppServerDiagnosticCode::CodexAppServerHandshakeTimeout) }
        }
    }
}
fn plan() -> Value {
    json!({"version":1,"objective":"Objetivo","steps":[{"id":"a","description":"Passo",
        "requiredCapabilities":["planning"],"dependsOn":[]}],"risks":[],"needsUserInput":false,"questions":[]})
}
fn message() -> Value { json!({"method":"item/completed","params":{"threadId":"t","turnId":"v",
    "item":{"type":"agentMessage","text":plan().to_string()}}}) }
fn completed(status: &str) -> Value {
    json!({"method":"turn/completed","params":{"threadId":"t","turn":{"id":"v","status":status}}})
}
fn delta() -> Value { json!({"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"v","delta":"untrusted"}}) }
fn run(fake: &mut Fake<'_>, events: &mut Vec<AgentEvent>) -> Result<String, OperationFailure> {
    let control = fake.control;
    run_controlled_prepared_turn(fake, "t", "Objetivo", control, &mut |event| { events.push(event); Ok(()) })
}
fn cancelled() -> OperationFailure { OperationFailure::Agent(AgentError::Cancelled) }

#[tokio::test] async fn precancelled_trait_object_starts_no_operation_or_event() {
    let backend: &dyn AgentBackend = &CodexAgentBackend;
    let request = AgentRequest { objective:"Objetivo".into(), required_capabilities:production_config().capabilities };
    let mut events = Vec::new();
    assert_eq!(backend.execute(&request, &AtomicBool::new(true), &mut |event| { events.push(event); Ok(()) }).await,
        Err(AgentError::Cancelled));
    assert!(events.is_empty());
}
#[test] fn cancel_after_thread_preparation_never_starts_inference() {
    let control = LifecycleControl::default();
    let mut fake = Fake::valid(&control);
    let mut created = None;
    assert_eq!(prepare_thread(&mut fake, Path::new("/isolated"), &mut created).unwrap(), "t");
    control.cancelled.store(true, Ordering::Release);
    let mut events = Vec::new();
    assert_eq!(run(&mut fake, &mut events), Err(cancelled()));
    assert!(!fake.calls.contains(&"turn/start")); fake.cleaned();
    assert_eq!(events, [AgentEvent::Cancelled]);
}
#[test] fn cancel_between_session_ready_and_turn_start_is_observed() {
    let control = LifecycleControl::default();
    let mut fake = Fake::valid(&control);
    assert_eq!(run_controlled_prepared_turn(&mut fake,"t","Objetivo",&control,&mut |event| {
        if event == AgentEvent::SessionReady { control.cancelled.store(true,Ordering::Release); }
        Ok(())
    }), Err(cancelled()));
    assert!(!fake.calls.contains(&"turn/start")); fake.cleaned();
}
#[test] fn active_cancel_sends_one_interrupt_and_waits_for_terminal() {
    let control = LifecycleControl::default();
    let mut fake = Fake::new(&control, vec![delta(), delta(), delta(), completed("interrupted")]);
    for notification in &mut fake.notifications { notification.1 = true; }
    let mut events = Vec::new();
    assert_eq!(run(&mut fake, &mut events), Err(cancelled()));
    assert_eq!(fake.interrupts(),1); fake.cleaned();
    assert_eq!(events,[AgentEvent::SessionReady,AgentEvent::WorkStarted,AgentEvent::OutputObserved,
        AgentEvent::CancellationRequested,AgentEvent::Cancelled]);
}
#[test] fn cancel_before_reading_terminal_wins_even_when_runtime_completes() {
    let control = LifecycleControl::default();
    let mut fake = Fake::valid(&control);
    let mut events = Vec::new();
    assert_eq!(run_controlled_prepared_turn(&mut fake,"t","Objetivo",&control,&mut |event| {
        if event == AgentEvent::WorkStarted { control.cancelled.store(true,Ordering::Release); }
        events.push(event); Ok(())
    }), Err(cancelled()));
    assert_eq!(fake.interrupts(),1); fake.cleaned();
    assert!(!events.contains(&AgentEvent::Completed));
}
#[test] fn terminal_received_before_cancel_observation_preserves_success() {
    let control = LifecycleControl::default();
    let mut fake = Fake::valid(&control);
    fake.notifications.back_mut().unwrap().1 = true;
    let mut events = Vec::new();
    assert!(run(&mut fake,&mut events).is_ok());
    assert_eq!(fake.interrupts(),0); fake.cleaned();
    assert_eq!(events.last(),Some(&AgentEvent::Completed));
}
#[test] fn interrupt_failure_still_reclaims_and_returns_cancelled() {
    let control = LifecycleControl::default();
    let mut fake = Fake::new(&control,vec![delta()]);
    fake.notifications.front_mut().unwrap().1 = true;
    fake.interrupt_failed = true;
    assert_eq!(run(&mut fake,&mut vec![]),Err(cancelled()));
    assert_eq!(fake.interrupts(),1); fake.cleaned();
}
#[test] fn interrupt_without_terminal_has_bounded_virtual_deadline() {
    let control = LifecycleControl::default();
    let mut fake = Fake::new(&control,vec![delta()]);
    fake.notifications.front_mut().unwrap().1 = true;
    let start = fake.clock;
    assert_eq!(run(&mut fake,&mut vec![]),Err(cancelled()));
    assert_eq!(fake.clock-start,INTERRUPT_TIMEOUT);
    assert!(fake.maximum_poll<=CANCEL_POLL);
    assert_eq!(fake.interrupts(),1); fake.cleaned();
}
#[test] fn idle_turn_observes_global_timeout_without_busy_loop() {
    let control = LifecycleControl::default();
    let mut fake = Fake::new(&control,vec![]);
    let start = fake.clock;
    assert_eq!(run(&mut fake,&mut vec![]),Err(PlannerTurnDiagnosticCode::PlannerTurnTimeout.into()));
    assert_eq!(fake.clock-start,TURN_TIMEOUT); assert_eq!(fake.maximum_poll,CANCEL_POLL); fake.cleaned();
}
#[test] fn failure_then_independent_call_starts_clean_with_one_terminal_each() {
    for code in [CodexAppServerDiagnosticCode::CodexAppServerClosed,
        CodexAppServerDiagnosticCode::CodexAppServerProtocolError,
        CodexAppServerDiagnosticCode::CodexAppServerUnexpectedServerRequest] {
        let control = LifecycleControl::default();
        let mut failed = Fake::new(&control,vec![]);
        failed.notifications.push_back((Err(code),false));
        let mut events = Vec::new(); assert!(run(&mut failed,&mut events).is_err()); failed.cleaned();
        assert_eq!(events.iter().filter(|e| **e==AgentEvent::Failed).count(),1);
        let fresh_control = LifecycleControl::default();
        let mut fresh = Fake::valid(&fresh_control);
        let mut events = Vec::new(); assert!(run(&mut fresh,&mut events).is_ok()); fresh.cleaned();
        assert_eq!(events.iter().filter(|e| **e==AgentEvent::Completed).count(),1);
    }
}
#[test] fn success_events_are_facts_coalesced_and_do_not_contain_output() {
    let control = LifecycleControl::default();
    let mut fake = Fake::new(&control,vec![]);
    for _ in 0..3000 { fake.notifications.push_back((Ok(delta()),false)); }
    fake.notifications.push_back((Ok(message()),false)); fake.notifications.push_back((Ok(completed("completed")),false));
    let mut events=Vec::new(); assert!(run(&mut fake,&mut events).is_ok());
    assert_eq!(events,[AgentEvent::SessionReady,AgentEvent::WorkStarted,AgentEvent::OutputObserved,AgentEvent::Completed]);
    assert!(!format!("{events:?}").contains("untrusted")); fake.cleaned();
}
#[test] fn unrelated_activity_never_emits_output_observed() {
    let control = LifecycleControl::default();
    let mut other = delta(); other["params"]["turnId"] = json!("other");
    let mut fake = Fake::new(&control,vec![other,completed("completed")]);
    let mut events=Vec::new(); assert!(run(&mut fake,&mut events).is_err());
    assert!(!events.contains(&AgentEvent::OutputObserved));
}
#[test] fn sink_closed_during_work_interrupts_and_reclaims() {
    let control = LifecycleControl::default();
    let mut fake = Fake::new(&control,vec![completed("interrupted")]);
    assert_eq!(run_controlled_prepared_turn(&mut fake,"t","Objetivo",&control,&mut |event| {
        if event==AgentEvent::WorkStarted { Err(AgentError::BackendFailed) } else { Ok(()) }
    }),Err(OperationFailure::Agent(AgentError::EventSinkClosed)));
    assert_eq!(fake.interrupts(),1); fake.cleaned();
}
#[test] fn sink_closed_on_output_interrupts_and_emits_no_text() {
    let control = LifecycleControl::default();
    let mut fake = Fake::new(&control,vec![delta(),completed("interrupted")]);
    assert_eq!(run_controlled_prepared_turn(&mut fake,"t","Objetivo",&control,&mut |event| {
        if event==AgentEvent::OutputObserved { Err(AgentError::EventSinkClosed) } else { Ok(()) }
    }),Err(OperationFailure::Agent(AgentError::EventSinkClosed)));
    assert_eq!(fake.interrupts(),1); fake.cleaned();
}
#[test] fn sink_closed_before_turn_never_spends_inference() {
    let control = LifecycleControl::default(); let mut fake = Fake::valid(&control);
    assert_eq!(run_controlled_prepared_turn(&mut fake,"t","Objetivo",&control,&mut |_| Err(AgentError::Protocol)),
        Err(OperationFailure::Agent(AgentError::EventSinkClosed)));
    assert!(!fake.calls.contains(&"turn/start")); fake.cleaned();
}
#[test] fn terminal_event_rejection_cannot_return_success_or_duplicate_terminal() {
    let control = LifecycleControl::default(); let mut fake = Fake::valid(&control); let mut terminals=0;
    assert_eq!(run_controlled_prepared_turn(&mut fake,"t","Objetivo",&control,&mut |event| {
        if event==AgentEvent::Completed { terminals+=1; Err(AgentError::EventSinkClosed) } else { Ok(()) }
    }),Err(OperationFailure::Agent(AgentError::EventSinkClosed)));
    assert_eq!(terminals,1); assert_eq!(fake.interrupts(),0); fake.cleaned();
}
#[test] fn cleanup_failure_and_security_failure_keep_precedence() {
    let control = LifecycleControl::default();
    let mut fake=Fake::valid(&control); fake.shutdown_failed=true;
    let mut events=Vec::new(); assert_eq!(run(&mut fake,&mut events),Err(PlannerTurnDiagnosticCode::PlannerCleanupFailed.into()));
    assert_eq!(events.last(),Some(&AgentEvent::Failed));
    let mut fake=Fake::new(&control,vec![json!({"method":"item/started","params":{"threadId":"t","turnId":"v","item":{"type":"commandExecution"}}})]);
    fake.shutdown_failed=true;
    assert_eq!(run(&mut fake,&mut vec![]),Err(PlannerTurnDiagnosticCode::PlannerTurnUnexpectedItem.into())); fake.cleaned();
}
#[test] fn forbidden_item_while_cancelling_cannot_be_hidden_by_cancel() {
    let control = LifecycleControl::default(); let mut fake=Fake::new(&control,vec![delta(),
        json!({"method":"item/started","params":{"threadId":"t","turnId":"v","item":{"type":"mcpToolCall"}}})]);
    fake.notifications.front_mut().unwrap().1=true;
    assert_eq!(run(&mut fake,&mut vec![]),Err(PlannerTurnDiagnosticCode::PlannerTurnUnexpectedItem.into())); fake.cleaned();
    // Only terminal statuses present in the compatible protocol are accepted.
    for status in ["unknown","cancelled"] {
        let control=LifecycleControl::default(); let mut fake=Fake::new(&control,vec![delta(),completed(status)]);
        fake.notifications.front_mut().unwrap().1=true;
        assert_eq!(run(&mut fake,&mut vec![]),Err(PlannerTurnDiagnosticCode::PlannerTurnFailed.into())); fake.cleaned();
    }
}
#[tokio::test] async fn async_bridge_acknowledges_events_and_maps_closed_sink() {
    let count=Arc::new(AtomicUsize::new(0)); let copy=count.clone();
    let result=drive_worker(&AtomicBool::new(false),&mut |_| { copy.fetch_add(1,Ordering::Relaxed); Err(AgentError::Protocol) },
        |control,emit| {
            assert_eq!(emit(AgentEvent::WorkStarted),Err(AgentError::EventSinkClosed));
            assert_eq!(control.stop_reason(),Some(AgentError::EventSinkClosed));
            Err(OperationFailure::Agent(AgentError::EventSinkClosed))
        }).await.unwrap();
    assert_eq!(result,Err(OperationFailure::Agent(AgentError::EventSinkClosed)));
    assert_eq!(count.load(Ordering::Relaxed),1);
}
#[tokio::test] async fn async_bridge_mirrors_cancel_without_waiting_for_full_turn() {
    let cancel=AtomicBool::new(false);
    let result=drive_worker(&cancel,&mut |_| { cancel.store(true,Ordering::Release); Ok(()) },|control,emit| {
        emit(AgentEvent::WorkStarted).unwrap();
        // The event delivery ACK synchronizes cancellation deterministically.
        assert_eq!(control.stop_reason(),Some(AgentError::Cancelled));
        Err(cancelled())
    }).await.unwrap();
    assert_eq!(result,Err(cancelled()));
}
#[tokio::test] async fn dropping_async_future_signals_worker_to_reclaim() {
    let (started,mut ready)=tokio::sync::mpsc::channel(1);
    let (done,finished)=tokio::sync::oneshot::channel();
    let cancel=AtomicBool::new(false); let mut sink=|_| Ok(());
    let mut operation=Box::pin(drive_worker(&cancel,&mut sink,move |control,emit| {
        emit(AgentEvent::WorkStarted).unwrap(); started.blocking_send(()).unwrap();
        let (_sender,receiver)=std::sync::mpsc::channel::<()>();
        let _=receiver.recv_timeout(CANCEL_POLL);
        let _=done.send(control.stop_reason()); Err(OperationFailure::Agent(AgentError::EventSinkClosed))
    }));
    tokio::select! { _=ready.recv()=>{}, _=&mut operation=>panic!("worker should still be active") }
    drop(operation);
    assert_eq!(tokio::time::timeout(Duration::from_secs(1),finished).await.unwrap().unwrap(),
        Some(AgentError::EventSinkClosed));
}

#[test] fn partial_cleanup_or_cancel_does_not_poison_next_call() {
    for mode in 0..3 {
        let control=LifecycleControl::default(); let mut first=Fake::valid(&control);
        if mode==0 { control.cancelled.store(true,Ordering::Release); }
        else if mode==1 { control.sink_closed.store(true,Ordering::Release); }
        else { first.detach_failed=true; first.shutdown_failed=true; }
        assert!(run(&mut first,&mut vec![]).is_err()); first.cleaned();
        let next_control=LifecycleControl::default(); let mut next=Fake::valid(&next_control);
        assert!(run(&mut next,&mut vec![]).is_ok()); next.cleaned();
    }
}
#[test] fn server_request_during_cancel_remains_closed_and_cleanup_runs() {
    let control=LifecycleControl::default(); let mut fake=Fake::new(&control,vec![delta()]);
    fake.notifications.front_mut().unwrap().1=true;
    fake.notifications.push_back((Err(CodexAppServerDiagnosticCode::CodexAppServerUnexpectedServerRequest),false));
    assert_eq!(run(&mut fake,&mut vec![]),Err(PlannerTurnDiagnosticCode::PlannerTurnUnexpectedNotification.into()));
    assert_eq!(fake.interrupts(),1); fake.cleaned();
}
// Actual wire framing + await_response/pending, without a process or file.
struct InterruptWire {
    stream:super::super::app_server::CodexRpcStream,
    interrupts:usize,
    cleanup:Vec<&'static str>,
    pending_inspected:usize,
    notification_reads:usize,
    eof_fault:Option<CodexAppServerDiagnosticCode>,
}
impl InterruptWire {
    fn new(interleaved:Vec<Value>, ack:&str, eof_fault:Option<CodexAppServerDiagnosticCode>) -> Self {
        let mut wire=format!("{}\n",json!({"id":1,"result":{"turn":{"id":"v"}}}));
        for notification in interleaved { wire.push_str(&notification.to_string()); wire.push('\n'); }
        wire.push_str(ack);
        Self { stream:super::super::app_server::test_planner_stream(wire.as_bytes()),interrupts:0,
            cleanup:vec![],pending_inspected:0,notification_reads:0,eof_fault }
    }
    fn assert_reclaimed_once(&self) {
        assert_eq!(self.interrupts,1);
        assert_eq!(self.cleanup,["unsubscribe","shutdown"]);
    }
}
impl PlannerProtocol for InterruptWire {
    fn initialize(&mut self,_:Instant)->Result<(),()> { unreachable!() }
    fn request(&mut self,method:&str,_:Value,deadline:Instant)->Result<Value,()> {
        match method {
            "turn/start" => self.stream.await_response(1,deadline).map_err(|_|()),
            "thread/unsubscribe" => { self.cleanup.push("unsubscribe"); Ok(json!({})) }
            _=>panic!("no tools or server replies are permitted"),
        }
    }
    fn shutdown(&mut self)->Result<(),()> { self.cleanup.push("shutdown"); Ok(()) }
}
impl PlannerTurnProtocol for InterruptWire {
    fn interrupt(&mut self,thread_id:&str,turn_id:&str,deadline:Instant)->Result<(),CodexAppServerDiagnosticCode> {
        assert_eq!((thread_id,turn_id),("t","v")); self.interrupts+=1;
        let response=self.stream.await_response(2,deadline).map_err(|code| {
            // Deterministic timeout fault: replace EOF only AFTER real
            // await_response has preserved its interleaved notifications.
            if code==CodexAppServerDiagnosticCode::CodexAppServerClosed { self.eof_fault.unwrap_or(code) } else { code }
        })?;
        if response.as_object().is_some_and(|object|object.is_empty()) { Ok(()) }
        else { Err(CodexAppServerDiagnosticCode::CodexAppServerProtocolError) }
    }
    fn pop_pending_notification(&mut self)->Option<Value> {
        let pending=self.stream.pop_pending_notification();
        if pending.is_some() { self.pending_inspected+=1; }
        pending
    }
    fn next_notification(&mut self,deadline:Instant)->Received {
        self.notification_reads+=1; self.stream.next_notification(deadline)
    }
}
fn run_interrupt_wire(wire:&mut InterruptWire, sink_closed:bool)->Result<String,OperationFailure> {
    let control=LifecycleControl::default();
    run_controlled_prepared_turn(wire,"t","Objetivo",&control,&mut |event| {
        if event==AgentEvent::WorkStarted {
            if sink_closed { control.sink_closed.store(true,Ordering::Release); }
            else { control.cancelled.store(true,Ordering::Release); }
        }
        Ok(())
    })
}
fn forbidden(kind:&str)->Value {
    json!({"method":"item/started","params":{"threadId":"t","turnId":"v","item":{"type":kind}}})
}
#[test] fn queued_completed_during_interrupt_ack_is_drained_in_order() {
    let mut wire=InterruptWire::new(vec![completed("interrupted")],"{\"id\":2,\"result\":{}}\n",None);
    assert_eq!(run_interrupt_wire(&mut wire,false),Err(cancelled()));
    assert_eq!(wire.pending_inspected,1); assert_eq!(wire.notification_reads,0); wire.assert_reclaimed_once();
}
#[test] fn forbidden_pending_survives_interrupt_ack_timeout() {
    for kind in ["commandExecution","fileChange","mcpToolCall","dynamicToolCall","webSearch","unknown"] {
        let mut wire=InterruptWire::new(vec![forbidden(kind)],"",Some(CodexAppServerDiagnosticCode::CodexAppServerHandshakeTimeout));
        assert_eq!(run_interrupt_wire(&mut wire,false),Err(PlannerTurnDiagnosticCode::PlannerTurnUnexpectedItem.into()));
        assert_eq!(wire.pending_inspected,1); assert_eq!(wire.notification_reads,0); wire.assert_reclaimed_once();
    }
}
#[test] fn forbidden_pending_survives_interrupt_protocol_or_transport_failure() {
    for ack in ["{\"id\":2,\"error\":{\"message\":\"synthetic-private-payload\"}}\n","not json\n",""] {
        for sink_closed in [false,true] {
            let mut wire=InterruptWire::new(vec![forbidden("commandExecution")],ack,None);
            let error=PlannerTurnDiagnosticCode::PlannerTurnUnexpectedItem;
            assert_eq!(run_interrupt_wire(&mut wire,sink_closed),Err(error.into()));
            assert_eq!(error.code(),"planner_turn_unexpected_item");
            assert!(!error.code().contains("private"));
            assert_eq!(wire.pending_inspected,1); wire.assert_reclaimed_once();
        }
    }
}
#[test] fn queued_terminal_is_inspected_even_after_interrupt_ack_failure() {
    for (ack,fault) in [("",Some(CodexAppServerDiagnosticCode::CodexAppServerHandshakeTimeout)),
        ("{\"id\":2,\"error\":{}}\n",None),("",None)] {
        let mut wire=InterruptWire::new(vec![completed("interrupted")],ack,fault);
        assert_eq!(run_interrupt_wire(&mut wire,false),Err(cancelled()));
        assert_eq!(wire.pending_inspected,1); assert_eq!(wire.notification_reads,0);
        assert!(wire.stream.pop_pending_notification().is_none()); wire.assert_reclaimed_once();
    }
}
#[test] fn benign_pending_after_failed_interrupt_keeps_cancel_or_sink_error() {
    for (ack,fault) in [("",Some(CodexAppServerDiagnosticCode::CodexAppServerHandshakeTimeout)),
        ("{\"id\":2,\"error\":{}}\n",None),("",None)] {
        for sink_closed in [false,true] {
            let mut wire=InterruptWire::new(vec![delta(),delta()],ack,fault);
            let expected=if sink_closed { OperationFailure::Agent(AgentError::EventSinkClosed) } else { cancelled() };
            assert_eq!(run_interrupt_wire(&mut wire,sink_closed),Err(expected));
            assert_eq!(wire.pending_inspected,2); assert_eq!(wire.notification_reads,0); wire.assert_reclaimed_once();
        }
    }
}
#[test] fn queued_terminal_cannot_hide_later_forbidden_pending_item() {
    for ack in ["","{\"id\":2,\"result\":{}}\n"] {
        let mut wire=InterruptWire::new(vec![completed("interrupted"),forbidden("fileChange")],ack,None);
        assert_eq!(run_interrupt_wire(&mut wire,false),Err(PlannerTurnDiagnosticCode::PlannerTurnUnexpectedItem.into()));
        assert_eq!(wire.pending_inspected,2); assert_eq!(wire.notification_reads,0); wire.assert_reclaimed_once();
    }
}
#[test] fn server_request_during_interrupt_ack_stays_closed_without_reply() {
    let request="{\"id\":77,\"method\":\"item/commandExecution/requestApproval\",\"params\":{}}\n";
    let mut wire=InterruptWire::new(vec![delta()],request,None);
    assert_eq!(run_interrupt_wire(&mut wire,false),Err(PlannerTurnDiagnosticCode::PlannerTurnUnexpectedNotification.into()));
    assert_eq!(wire.pending_inspected,1); assert_eq!(wire.notification_reads,0); wire.assert_reclaimed_once();
}
#[test] fn malformed_pending_notification_preserves_protocol_diagnostic_on_ack_failure() {
    let mut wire=InterruptWire::new(vec![json!({"method":"item/completed"})],"",None);
    assert_eq!(run_interrupt_wire(&mut wire,false),Err(PlannerTurnDiagnosticCode::PlannerTurnUnexpectedNotification.into()));
    assert_eq!(wire.pending_inspected,1); wire.assert_reclaimed_once();
}
#[tokio::test] async fn successful_async_bridge_preserves_event_order_and_has_no_global_busy_state() {
    for _ in 0..2 {
        let mut events=vec![];
        let result=drive_worker(&AtomicBool::new(false),&mut |event| { events.push(event); Ok(()) },|_,emit| {
            emit(AgentEvent::SessionReady)?;
            emit(AgentEvent::WorkStarted)?;
            emit(AgentEvent::Completed)?;
            Ok("validated".into())
        }).await.unwrap();
        assert_eq!(result,Ok("validated".into()));
        assert_eq!(events,[AgentEvent::SessionReady,AgentEvent::WorkStarted,AgentEvent::Completed]);
    }
}
#[tokio::test]
#[ignore = "manual only: requires authenticated Codex, isolated inference and quota"]
async fn manual_final_codex_agent_bridge_gate() {
    let request = AgentRequest {
        objective: "Proponha um plano curto de investigação de interface, sem executar ações.".into(),
        required_capabilities: production_config().capabilities,
    };
    let cancellation = AtomicBool::new(false);
    let mut cancelled_events = Vec::new();
    let cancelled_result = CodexAgentBackend.execute(&request, &cancellation, &mut |event| {
        if event == AgentEvent::WorkStarted {
            cancellation.store(true, Ordering::Release);
        }
        cancelled_events.push(event);
        Ok(())
    }).await;

    assert_eq!(cancelled_result, Err(AgentError::Cancelled));
    assert!(matches!(cancelled_events.first(), Some(AgentEvent::SessionReady)));
    assert!(cancelled_events.iter().position(|event| *event == AgentEvent::SessionReady)
        < cancelled_events.iter().position(|event| *event == AgentEvent::WorkStarted));
    assert_eq!(cancelled_events.iter().filter(|event| **event == AgentEvent::WorkStarted).count(), 1);
    assert_eq!(cancelled_events.iter().filter(|event| **event == AgentEvent::CancellationRequested).count(), 1);
    assert_eq!(cancelled_events.iter().filter(|event| **event == AgentEvent::Cancelled).count(), 1);
    assert_eq!(cancelled_events.iter().filter(|event| **event == AgentEvent::Completed).count(), 0);
    assert_eq!(cancelled_events.iter().filter(|event| **event == AgentEvent::Failed).count(), 0);
    assert!(!cancelled_events.iter().any(|event| matches!(event, AgentEvent::Output { .. })));
    assert!(matches!(cancelled_events.last(), Some(AgentEvent::Cancelled)));

    let recovery_cancellation = AtomicBool::new(false);
    let recovery_request = AgentRequest {
        objective: "Planeje como investigar um botão Tauri que não responde ao clique. Não execute nenhuma alteração.".into(),
        required_capabilities: production_config().capabilities,
    };
    let mut recovery_events = Vec::new();
    let recovery_result = CodexAgentBackend.execute(
        &recovery_request,
        &recovery_cancellation,
        &mut |event| {
            recovery_events.push(event);
            Ok(())
        },
    ).await.expect("independent recovery call should complete");
    let plan = PlanV1::parse(&recovery_result.output)
        .expect("recovery output must be a valid PlanV1");
    plan.validate().expect("recovery PlanV1 must pass Core validation");

    assert_eq!(recovery_events, [
        AgentEvent::SessionReady,
        AgentEvent::WorkStarted,
        AgentEvent::OutputObserved,
        AgentEvent::Completed,
    ]);
    assert_eq!(recovery_events.iter().filter(|event| **event == AgentEvent::Completed).count(), 1);
    assert_eq!(recovery_events.iter().filter(|event| **event == AgentEvent::Cancelled).count(), 0);
    assert_eq!(recovery_events.iter().filter(|event| **event == AgentEvent::Failed).count(), 0);
    assert!(!recovery_events.iter().any(|event| matches!(event, AgentEvent::Output { .. })));
}

#[tokio::test] async fn async_ready_delivery_checks_cancel_before_releasing_worker() {
    let cancel=AtomicBool::new(false);
    let result=drive_worker(&cancel,&mut |event| {
        assert_eq!(event,AgentEvent::SessionReady); cancel.store(true,Ordering::Release); Ok(())
    },|control,emit| {
        emit(AgentEvent::SessionReady).unwrap();
        assert_eq!(control.stop_reason(),Some(AgentError::Cancelled));
        Err(cancelled())
    }).await.unwrap();
    assert_eq!(result,Err(cancelled()));
}

// LR-9D uses this exact fake protocol/controlled worker path, never an app-server process.
use crate::operational_trace::{
    adapters::{
        tests::{code as trace_code, events as trace_events, payloads, RejectPublisher},
        AgentTraceAdapter, AgentTraceContext, PassiveTracePublisher,
    },
    OperationalKind, OperationalTraceBus, SourceType, TextChannel,
};
fn lr9d_adapter(publisher: PassiveTracePublisher) -> AgentTraceAdapter {
    AgentTraceAdapter::new(
        publisher,
        AgentTraceContext {
            source_id: "codex".into(),
            task_id: None,
            subtask_id: None,
        },
    )
}
fn lr9d_delta(method: &str, text: &str) -> Value {
    json!({"method":method,"params":{"threadId":"t","turnId":"v","delta":text}})
}
fn lr9d_run(
    fake: &mut Fake<'_>,
    sink: &dyn AgentTraceSink,
    events: &mut Vec<AgentEvent>,
    cancel_on_work: bool,
) -> Result<String, OperationFailure> {
    let control = fake.control;
    run_controlled_prepared_turn_observed(
        fake,
        "t",
        "AGENT-OBJECTIVE-SECRET",
        control,
        &mut |event| {
            if cancel_on_work && event == AgentEvent::WorkStarted {
                control.cancelled.store(true, Ordering::Release);
            }
            events.push(event);
            Ok(())
        },
        sink,
    )
}
#[test]
fn lr9d_codex_display_notifications_are_exact_private_reasoning_is_never_exposed() {
    let bus = OperationalTraceBus::isolated();
    let trace = lr9d_adapter(PassiveTracePublisher::new(bus.clone()));
    let control = LifecycleControl::default();
    let message_text = format!("{}😀\nTAIL", "á".repeat(5000));
    let display_text = "Resumo de exibição 🦀\n".repeat(500);
    let mut wrong_thread = lr9d_delta("item/agentMessage/delta", "WRONG-THREAD-SECRET");
    wrong_thread["params"]["threadId"] = json!("other");
    let mut wrong_turn = lr9d_delta("item/reasoning/summaryTextDelta", "WRONG-TURN-SECRET");
    wrong_turn["params"]["turnId"] = json!("other");
    let mut missing = lr9d_delta("item/agentMessage/delta", "bad");
    missing["params"].as_object_mut().unwrap().remove("delta");
    let malformed = json!({"method":"item/agentMessage/delta","params":{"threadId":"t","turnId":"v","delta":123}});
    let mut fake = Fake::new(
        &control,
        vec![
            lr9d_delta("item/reasoning/textDelta", "PRIVATE-REASONING-SECRET"),
            wrong_thread,
            wrong_turn,
            missing,
            malformed,
            lr9d_delta("item/agentMessage/delta", ""),
            lr9d_delta("item/reasoning/otherDelta", "AMBIGUOUS-SECRET"),
            lr9d_delta("item/agentMessage/delta", &message_text),
            lr9d_delta("item/reasoning/summaryTextDelta", &display_text),
            message(),
            completed("completed"),
        ],
    );
    let mut events = vec![];
    assert!(lr9d_run(&mut fake, &trace, &mut events, false).is_ok());
    fake.cleaned();
    assert_eq!(
        events,
        [
            AgentEvent::SessionReady,
            AgentEvent::WorkStarted,
            AgentEvent::OutputObserved,
            AgentEvent::Completed
        ]
    );
    let observed = trace_events(&bus);
    for (channel, text) in [
        (TextChannel::AgentMessage, message_text),
        (TextChannel::DisplayReasoningSummary, display_text),
    ] {
        let pieces: Vec<_> = observed
            .iter()
            .filter_map(|e| match e.kind() {
                OperationalKind::TextDelta { channel: c, text } if *c == channel => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(pieces.concat(), text);
        assert!(pieces.iter().all(|s| s.len() <= 8192));
    }
    assert!(!payloads(&bus).contains("SECRET")); // Includes private reasoning, objective and unrelated activity.
    assert_eq!(
        observed
            .iter()
            .filter(|e| trace_code(e) == "output_observed")
            .count(),
        1
    );
    assert!(observed.iter().all(|e| e.provenance().source.source_type
        == SourceType::SpecialistAgent
        && e.provenance().source.id.as_str() == "codex"));
    assert_eq!(fake.calls, ["turn/start", "thread/unsubscribe", "shutdown"]);
}
#[test]
fn lr9d_private_reasoning_only_keeps_factual_output_observed_without_text() {
    let bus = OperationalTraceBus::isolated();
    let trace = lr9d_adapter(PassiveTracePublisher::new(bus.clone()));
    let control = LifecycleControl::default();
    let mut fake = Fake::new(
        &control,
        vec![
            lr9d_delta("item/reasoning/textDelta", "PRIVATE-REASONING-SECRET"),
            message(),
            completed("completed"),
        ],
    );
    assert!(lr9d_run(&mut fake, &trace, &mut vec![], false).is_ok());
    assert!(trace_events(&bus)
        .iter()
        .any(|e| trace_code(e) == "output_observed"));
    assert!(trace_events(&bus)
        .iter()
        .all(|e| !matches!(e.kind(), OperationalKind::TextDelta { .. })));
    assert!(!payloads(&bus).contains("SECRET"));
}
#[test]
fn lr9d_forbidden_protocol_still_fails_closed_without_payload_or_private_marker() {
    for kind in [
        "commandExecution",
        "fileChange",
        "mcpToolCall",
        "dynamicToolCall",
        "webSearch",
        "unknown",
    ] {
        let bus = OperationalTraceBus::isolated();
        let trace = lr9d_adapter(PassiveTracePublisher::new(bus.clone()));
        let control = LifecycleControl::default();
        let mut item = forbidden(kind);
        item["params"]["item"]["payload"] = json!("/private/raw FORBIDDEN-SECRET ENV=SECRET");
        let mut fake = Fake::new(&control, vec![item]);
        assert_eq!(
            lr9d_run(&mut fake, &trace, &mut vec![], false),
            Err(PlannerTurnDiagnosticCode::PlannerTurnUnexpectedItem.into())
        );
        fake.cleaned();
        assert_eq!(
            trace_events(&bus).last().map(|e| trace_code(e)),
            Some("agent_failed")
        );
        assert!(!payloads(&bus).contains("/private/raw"));
        assert!(!payloads(&bus).contains("SECRET"));
    }
    for notification in [
        json!({"id":77,"method":"item/commandExecution/requestApproval","params":{"payload":"/private/raw"}}),
        json!({"method":"item/commandExecution/outputDelta","params":{"payload":"/private/raw"}}),
        json!({"method":"item/webSearch/delta","params":{"payload":"/private/raw"}}),
    ] {
        let bus = OperationalTraceBus::isolated();
        let trace = lr9d_adapter(PassiveTracePublisher::new(bus.clone()));
        let control = LifecycleControl::default();
        let mut fake = Fake::new(&control, vec![notification]);
        assert!(lr9d_run(&mut fake, &trace, &mut vec![], false).is_err());
        fake.cleaned();
        assert!(!payloads(&bus).contains("/private/raw"));
    }
}
#[test]
fn lr9d_observer_absent_failed_slow_and_3000_deltas_preserve_requests_result_and_cleanup() {
    let mut expected = None;
    for mode in 0..4 {
        let bus = OperationalTraceBus::isolated();
        let subscriber = if mode == 2 {
            Some(bus.subscribe().unwrap())
        } else {
            None
        };
        let trace: Box<dyn AgentTraceSink> = match mode {
            0 => Box::new(NoopAgentTrace),
            1 => Box::new(lr9d_adapter(PassiveTracePublisher::new(Arc::new(
                RejectPublisher,
            )))),
            _ => Box::new(lr9d_adapter(PassiveTracePublisher::new(bus.clone()))),
        };
        let control = LifecycleControl::default();
        let mut fake = Fake::new(&control, vec![]);
        for _ in 0..3000 {
            fake.notifications.push_back((
                Ok(lr9d_delta("item/agentMessage/delta", "Natural 🦀")),
                false,
            ));
        }
        fake.notifications.push_back((Ok(message()), false));
        fake.notifications
            .push_back((Ok(completed("completed")), false));
        let mut events = vec![];
        let result = lr9d_run(&mut fake, trace.as_ref(), &mut events, false).unwrap();
        fake.cleaned();
        let current = (result, events, fake.calls.clone());
        if let Some(expected) = &expected {
            assert_eq!(&current, expected);
        } else {
            expected = Some(current);
        }
        assert_eq!(fake.calls.iter().filter(|c| **c == "turn/start").count(), 1);
        assert_eq!(fake.interrupts(), 0);
        if mode == 2 {
            assert!(bus.stats().live_delivery_dropped > 0);
        }
        assert!(bus.stats().retained_events <= crate::operational_trace::MAX_RETAINED_EVENTS);
        assert!(bus.stats().retained_bytes <= crate::operational_trace::MAX_RETAINED_BYTES);
        drop(subscriber);
    }
}
#[test]
fn lr9d_cancellation_observer_failure_never_changes_interrupt_or_cleanup() {
    let mut expected = None;
    for mode in 0..3 {
        let bus = OperationalTraceBus::isolated();
        let trace: Box<dyn AgentTraceSink> = match mode {
            0 => Box::new(NoopAgentTrace),
            1 => Box::new(lr9d_adapter(PassiveTracePublisher::new(Arc::new(
                RejectPublisher,
            )))),
            _ => Box::new(lr9d_adapter(PassiveTracePublisher::new(bus.clone()))),
        };
        let control = LifecycleControl::default();
        let mut fake = Fake::new(&control, vec![completed("interrupted")]);
        let mut events = vec![];
        let result = lr9d_run(&mut fake, trace.as_ref(), &mut events, true);
        assert_eq!(result, Err(cancelled()));
        fake.cleaned();
        assert_eq!(fake.interrupts(), 1);
        let current = (result, events, fake.calls.clone());
        if let Some(expected) = &expected {
            assert_eq!(&current, expected);
        } else {
            expected = Some(current);
        }
        if mode == 2 {
            assert_eq!(
                trace_events(&bus)
                    .iter()
                    .map(|e| trace_code(e))
                    .collect::<Vec<_>>(),
                [
                    "session_ready",
                    "work_started",
                    "cancellation_requested",
                    "agent_cancelled"
                ]
            );
        }
    }
}
#[test]
fn lr9d_functional_sink_error_is_not_masked_by_successful_passive_trace() {
    let b = OperationalTraceBus::isolated();
    let trace = lr9d_adapter(PassiveTracePublisher::new(b.clone()));
    let control = LifecycleControl::default();
    let mut fake = Fake::new(&control, vec![completed("interrupted")]);
    let result = run_controlled_prepared_turn_observed(
        &mut fake,
        "t",
        "objective",
        &control,
        &mut |event| {
            if event == AgentEvent::WorkStarted {
                Err(AgentError::EventSinkClosed)
            } else {
                Ok(())
            }
        },
        &trace,
    );
    assert_eq!(
        result,
        Err(OperationFailure::Agent(AgentError::EventSinkClosed))
    );
    assert_eq!(fake.interrupts(), 1);
    fake.cleaned();
}
// Shared multi-source gate exercises actual controlled Codex protocol concurrently.
pub fn lr9d_fake_trace_operation(publisher: PassiveTracePublisher, burst: usize) -> usize {
    let trace = lr9d_adapter(publisher);
    let control = LifecycleControl::default();
    let mut fake = Fake::new(&control, vec![]);
    for _ in 0..burst {
        for (method, text) in [
            ("item/agentMessage/delta", "Natural agent 🦀"),
            ("item/reasoning/summaryTextDelta", "Display summary 🦀"),
        ] {
            fake.notifications
                .push_back((Ok(lr9d_delta(method, text)), false));
        }
    }
    fake.notifications.push_back((
        Ok(lr9d_delta(
            "item/reasoning/textDelta",
            "PRIVATE-REASONING-SECRET",
        )),
        false,
    ));
    fake.notifications.push_back((Ok(message()), false));
    fake.notifications
        .push_back((Ok(completed("completed")), false));
    assert!(lr9d_run(&mut fake, &trace, &mut vec![], false).is_ok());
    fake.cleaned();
    assert_eq!(fake.interrupts(), 0);
    assert_eq!(fake.calls, ["turn/start", "thread/unsubscribe", "shutdown"]);
    1
}
#[test]
fn lr9d_two_concurrent_operations_have_distinct_local_correlation() {
    let b = OperationalTraceBus::isolated();
    std::thread::scope(|s| {
        for _ in 0..2 {
            let b = b.clone();
            s.spawn(move || lr9d_fake_trace_operation(PassiveTracePublisher::new(b), 1));
        }
    });
    let e = trace_events(&b);
    let ids: Vec<_> = e
        .iter()
        .filter(|e| trace_code(e) == "session_ready")
        .map(|e| e.provenance().correlation_id.clone().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    assert!(ids.iter().all(|id| id.as_str().starts_with("agent-call-")));
    assert!(e
        .iter()
        .all(|e| ids.contains(e.provenance().correlation_id.as_ref().unwrap())));
}
