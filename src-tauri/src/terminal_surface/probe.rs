//! Fixed opt-in fixture only. Native files/commands never enter normal builds.
use super::*;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
fn eval(app: &AppHandle, js: &str) -> Result<(), String> {
    app.get_webview_window("main")
        .ok_or("no main")?
        .eval(js)
        .map_err(|e| e.to_string())
}
fn paste(app: &AppHandle, text: &str) -> Result<(), String> {
    let text = serde_json::to_string(text).unwrap();
    eval(app,&format!("{{const d=new DataTransfer();d.setData('text/plain',{text});const t=document.querySelector('.xterm-helper-textarea');t.dispatchEvent(new ClipboardEvent('paste',{{clipboardData:d,bubbles:true,cancelable:true}}));t.dispatchEvent(new KeyboardEvent('keydown',{{key:'Enter',code:'Enter',keyCode:13,which:13,bubbles:true,cancelable:true}}))}}"))
}
pub(crate) fn action(app: &AppHandle, action: &str) -> Option<Result<(), String>> {
    if !action.starts_with("lr9c_") {
        return None;
    }
    Some((|| {
        match action {
        "lr9c_terminal"=>eval(app,"document.querySelector('[aria-label=Terminal]').click()"),
        "lr9c_start"=>eval(app,"[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Iniciar terminal local').click()"),
        "lr9c_conversation"=>eval(app,"document.querySelector('[aria-label=Conversa]').click()"),
        "lr9c_reconnect"=>eval(app,"[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Reconectar').click()"),
        "lr9c_burst"=>paste(app,"stty -echo; /usr/bin/python3 -c \"import os; [os.write(1,b'x'*8192) for _ in range(768)]\"; printf '%s%s\\n' VISUAL_ ALIVE\n"),
        "lr9c_input"=>paste(app,"stty size; LR9C_INPUT_INDEX=$((LR9C_INPUT_INDEX+1)); printf 'INPUT_AFTER_%s\\n' \"$LR9C_INPUT_INDEX\"\n"),
        "lr9c_resize"=> { app.get_webview_window("main").ok_or("no main".to_owned())?.set_size(tauri::LogicalSize::new(900.0,640.0)).map_err(|e|e.to_string()) },
        "lr9c_narrow"=> { app.get_webview_window("main").ok_or("no main".to_owned())?.set_size(tauri::LogicalSize::new(640.0,600.0)).map_err(|e|e.to_string()) },
        "lr9c_exit"=>paste(app,"exit\n"),
        "lr9c_end_session"=>eval(app,"[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Encerrar sessão').click()"),
        "lr9c_headless_burst"=> {
            let human=app.state::<Arc<HumanTerminal>>(); let id=human.status().ok_or("no session")?.session_id;
            human.input(&id,b"/usr/bin/python3 -c \"import os; [os.write(1,b'y'*8192) for _ in range(768)]\"; printf '%s%s\\n' HEADLESS_ ALIVE\n").map_err(error)
        },
        "lr9c_trace"=> {
            let bus=app.state::<Arc<OperationalTraceBus>>();
            for i in 0..6000 { let p=Provenance{source:TraceSource{source_type:SourceType::Worker,id:TraceId::new(&format!("fixture-{}",i%4)).unwrap(),instance:None},task_id:Some(crate::luna::task::TaskId(i%4+1)),subtask_id:None,correlation_id:None,coalescing_key:Some(TraceId::new("fixture-stream").unwrap())};
                let kind=if i%100==0{OperationalKind::Critical{kind:CriticalKind::Completed,code:TraceId::new("fixture_critical").unwrap(),message:TraceText::new("synthetic critical").unwrap()}}
                else if i%100==1{OperationalKind::State{kind:StateKind::Checkpoint,code:TraceId::new("fixture_state").unwrap(),detail:TraceText::new("synthetic state").unwrap()}}
                else{OperationalKind::TextDelta{channel:TextChannel::Stdout,text:TraceText::new("synthetic exact delta á\n").unwrap()}};
                bus.publish(EventDraft::new(p,kind).unwrap()).map_err(|_|"fixture publish")?;
            } Ok(())
        },
        "lr9c_frontend"=>eval(app,"window.__TAURI_INTERNALS__.invoke('perf1c_ui_report',{report:{terminal:document.querySelector('.terminal-workspace')?.dataset,view:document.querySelector('.economy-shell')?.dataset.view,rows:document.querySelectorAll('[data-trace-row]').length,ptyGap:document.querySelector('[data-pty-gap]')?.textContent,traceGap:document.querySelector('[data-trace-gap]')?.textContent,traceUpdates:document.querySelector('[data-trace-updates]')?.dataset.traceUpdates,traceSources:[...document.querySelectorAll('.trace-filters select')[1]?.options??[]].map(o=>o.value),body:document.body.innerText.slice(-12000),ptyText:document.querySelector('.xterm-rows')?.innerText.slice(-1000),resources:performance.getEntriesByType('resource').map(r=>r.name),width:innerWidth,height:innerHeight,atMs:performance.now()}})"),
        "lr9c_snapshot"=>Ok(()),
        _=>Err("unknown fixed LR-9C action".into()),
    }
    })())
}
pub(crate) fn snapshot(app: &AppHandle) -> Value {
    let human = app.state::<Arc<HumanTerminal>>();
    let status = human.status();
    let hub = app.state::<SurfaceHub>();
    let broker = app.state::<Arc<ExecutionBroker>>();
    let session=status.as_ref().and_then(|d|human.find(&d.session_id).ok()).map(|s|{let b=s.replay(0,1,READ_CHUNK_BYTES).unwrap();let tail=s.replay(b.latest_sequence.saturating_sub(8),MAX_BATCH_CHUNKS,crate::execution::MAX_BATCH_BYTES).unwrap();let bytes:Vec<u8>=tail.chunks.iter().flat_map(|c|c.bytes.iter().copied()).collect();json!({"pid":s.process_id(),"latest":b.latest_sequence.to_string(),"gap":b.gap,"droppedBytes":b.dropped_bytes,"totalBytes":b.total_bytes,"retainedBytes":b.retained_bytes,"retainedChunks":b.retained_chunks,"tail":String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(512)..])})});
    json!({"status":status,"session":session,"bridgeWorkers":hub.worker_count(),"attachmentActive":hub.current.lock().unwrap().as_ref().is_some_and(|a|!a.stopped()),"brokerWorkers":broker.worker_count(),"brokerActive":broker.active_count(),"trace":app.state::<Arc<OperationalTraceBus>>().stats().retained_events})
}
