//! Opt-in, fixed native fixture for real Close/Reopen/Quit evidence. No IPC
//! command or arbitrary executable/input is accepted, no agent authority minted.
use super::*;
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tauri::{AppHandle, Manager};

struct Fixture {
    pty: PtySession,
    flood: ExecutionHandle,
    sleeper: ExecutionHandle,
}
static FIXTURE: OnceLock<Mutex<Option<Fixture>>> = OnceLock::new();
fn fixture() -> &'static Mutex<Option<Fixture>> {
    FIXTURE.get_or_init(|| Mutex::new(None))
}
fn request(cwd: &Path, program: &str, args: &[&str]) -> ExecutionRequest {
    ExecutionRequest {
        program: program.into(),
        args: args.iter().map(|s| (*s).into()).collect(),
        cwd: cwd.into(),
        origin: ExecutionOrigin::Human,
        task_id: None,
        correlation: None,
        workspace: None,
        environment: EnvironmentPolicy::Controlled(vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("PS1".into(), "".into()),
            ("ENV".into(), "/dev/null".into()),
        ]),
        timeout: Duration::from_secs(120),
        capture: CapturePolicy::default(),
    }
}
/// The existing isolated PERF-1C file harness calls only these fixed actions.
pub(crate) fn action(app: &AppHandle, action: &str) -> Option<Result<(), String>> {
    if !action.starts_with("lr9b_") {
        return None;
    }
    let operation = || -> Result<(), String> {
        let dir = crate::perf1c_probe::directory().ok_or("isolated probe required")?;
        let isolated = std::env::var_os("XDG_DATA_HOME").ok_or("isolated data required")?;
        if !dir.is_absolute() || !Path::new(&isolated).starts_with(&dir) {
            return Err("isolated data required".into());
        }
        let broker = app.state::<Arc<ExecutionBroker>>();
        let authority = ExecutionAuthority::human_local();
        let mut fixture = fixture().lock().unwrap();
        match action {
            "lr9b_start" => {
                if fixture.is_some() {
                    return Err("already started".into());
                }
                let script = "import os,pathlib,time,threading\nwhile not pathlib.Path('release').exists(): time.sleep(.01)\ndef burst(fd):\n for _ in range(768): os.write(fd,b'x'*8192)\na=threading.Thread(target=burst,args=(1,)); b=threading.Thread(target=burst,args=(2,)); a.start(); b.start(); a.join(); b.join()";
                let flood = broker
                    .submit(
                        &request(&dir, "/usr/bin/python3", &["-c", script]),
                        &authority,
                    )
                    .map_err(|e| format!("{e:?}"))?;
                let sleeper = broker
                    .submit(
                        &request(
                            &dir,
                            "/usr/bin/python3",
                            &["-c", "import time; time.sleep(90)"],
                        ),
                        &authority,
                    )
                    .map_err(|e| format!("{e:?}"))?;
                let pty = broker
                    .open_pty(
                        &request(&dir, "/bin/sh", &["-i"]),
                        PtyDimensions { rows: 24, cols: 80 },
                        &authority,
                    )
                    .map_err(|e| format!("{e:?}"))?;
                *fixture = Some(Fixture {
                    pty,
                    flood,
                    sleeper,
                });
            }
            "lr9b_burst" => {
                let f = fixture.as_ref().ok_or("not started")?;
                // cwd is the isolated probe directory; the strings are fixed.
                f.pty.send_input(&ExecutionOrigin::Human,&authority,b"stty -echo; /usr/bin/python3 -c \"import os,pathlib; [os.write(1,b'x'*8192) for _ in range(768)]; pathlib.Path('pty-flood-done').touch()\"; printf '%s%s\\n' PTY_ ALIVE\n").map_err(|e|format!("{e:?}"))?;
                std::fs::write(dir.join("release"), b"release").map_err(|_| "release failed")?;
            }
            "lr9b_resize" => {
                let f = fixture.as_ref().ok_or("not started")?;
                f.pty
                    .resize(
                        PtyDimensions {
                            rows: 39,
                            cols: 111,
                        },
                        &authority,
                    )
                    .map_err(|e| format!("{e:?}"))?;
                f.pty
                    .send_input(
                        &ExecutionOrigin::Human,
                        &authority,
                        b"stty size; printf '%s%s\\n' AFTER_ REOPEN\n",
                    )
                    .map_err(|e| format!("{e:?}"))?;
            }
            "lr9b_snapshot" => {}
            _ => return Err("unknown fixed execution fixture action".into()),
        }
        Ok(())
    };
    Some(operation())
}
pub(crate) fn snapshot(app: &AppHandle) -> Value {
    let broker = app.state::<Arc<ExecutionBroker>>();
    let fixture = fixture().lock().unwrap();
    let runs=fixture.as_ref().map(|f| {
        let b=f.pty.replay(0,MAX_BATCH_CHUNKS,MAX_BATCH_BYTES).unwrap();
        let tail=f.pty.replay(b.latest_sequence.saturating_sub(16),MAX_BATCH_CHUNKS,MAX_BATCH_BYTES).unwrap();
        let tail:Vec<u8>=tail.chunks.iter().flat_map(|c|c.bytes.iter().copied()).collect();
        json!({"ptyId":f.pty.id().get(),"ptyPid":f.pty.process_id(),"ptyState":format!("{:?}",f.pty.state()),"ptyLatest":b.latest_sequence,"ptyRetainedBytes":b.retained_bytes,"ptyRetainedChunks":b.retained_chunks,"ptyDroppedBytes":b.dropped_bytes,"ptyGap":b.gap,"ptyTail":String::from_utf8_lossy(&tail[tail.len().saturating_sub(4096)..]),"floodId":f.flood.id().get(),"floodPid":f.flood.process_id(),"floodState":format!("{:?}",f.flood.state()),"floodResult":f.flood.result().map(|r|json!({"reaped":r.reaped,"stdoutTotal":r.stdout.total_bytes,"stderrTotal":r.stderr.total_bytes,"stdoutDropped":r.stdout.dropped_bytes,"stderrDropped":r.stderr.dropped_bytes})),"sleeperId":f.sleeper.id().get(),"sleeperPid":f.sleeper.process_id(),"sleeperState":format!("{:?}",f.sleeper.state())})
    });
    json!({"brokerAddress":Arc::as_ptr(broker.inner()) as usize,"active":broker.active_count(),"sessions":broker.active_sessions(),"workers":broker.worker_count(),"runs":runs})
}
