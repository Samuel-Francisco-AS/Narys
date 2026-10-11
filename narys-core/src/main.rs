use narys_core::{
    ipc,
    server::{serve, Config},
    worker,
};
use serde_json::json;
use tokio::io::AsyncReadExt;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    unsafe {
        libc::umask(0o077);
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
    let args: Vec<_> = std::env::args().collect();
    // FIX1 invokes the fixed worker executable with private job directory + pin.
    if args.len() == 3 && args[1].starts_with("/tmp/narys-task-") {
        let result = worker::run(
            std::path::Path::new(&args[1]),
            std::path::Path::new(&args[2]),
        )
        .await;
        println!("{result}");
        return;
    }
    let outcome=async {
        if args.get(1).map(String::as_str)==Some("boundary-serve") && args.len()==3 {
            return narys_core::agent_authority::local::serve(std::path::PathBuf::from(&args[2])).await;
        }
        let cfg=Config::discover()?;
        if args.get(1).map(String::as_str)==Some("serve"){return serve(cfg).await;}
        let operation=args.get(1).map(String::as_str).ok_or("operation_required")?;
        if operation=="unlock"{
            return narys_core::credentials::unlock(false);
        }
        let request=match operation{
            "status"|"credentials"|"stronghold"|"copilot"|"events"|"session-check"|"capabilities"|"sessions"|"providers"|"session-create"|"agent-status"|"agent-session-create"|"agent-runtime-recover"|"agent-runtime-stop"=>json!({"operation":operation}),
            "session-get"|"session-resume"|"session-close" if args.len()==3=>json!({"operation":operation,"session_id":args[2].parse::<i64>().map_err(|_|"session_invalid")?}),
            "conversation" if args.len()==4=>json!({"operation":"conversation","session_id":args[2].parse::<i64>().map_err(|_|"session_invalid")?,"text":args[3]}),
            "task-get"|"task-cancel" if args.len()==3=>json!({"operation":operation,"task":{"namespace":"product","id":args[2].parse::<u64>().map_err(|_|"invalid_task_id")?}}),
            "ipc" if args.len()==2=>{
                // Minimal typed terminal entry, intentionally no retries or full CLI UI.
                let mut bytes=vec![];
                tokio::io::stdin().take(ipc::MAX_REQUEST_BYTES as u64+1).read_to_end(&mut bytes).await.map_err(|_|"request_read_failed")?;
                if bytes.len()>ipc::MAX_REQUEST_BYTES {return Err("request_limit_or_timeout");}
                serde_json::from_slice(&bytes).map_err(|_|"invalid_command")?
            },
            "prepare" if args.len()==4=>json!({"operation":"prepare","task":{"objective":args[2],"model":"auto","included_only_approval":true},"expected":args[3]}),
            "submit"|"cancel"|"result"|"resume-check" if args.len()==3=>json!({"operation":operation,"task_id":args[2].parse::<u64>().map_err(|_|"invalid_task_id")?}),
            _=>return Err("invalid_command")
        };
        let command:ipc::Command=serde_json::from_value(request).map_err(|_|"invalid_command")?;
        let v=narys_core::client::request(command).await?;
        println!("{}",serde_json::to_string_pretty(&v).unwrap());
        if v["ok"]==true{Ok(())}else{Err("operation_blocked")}
    }.await;
    if let Err(code) = outcome {
        eprintln!("{code}");
        std::process::exit(1);
    }
}
