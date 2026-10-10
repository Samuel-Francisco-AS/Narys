use narys_core::{
    ipc,
    server::{serve, Config},
    worker,
};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
        let cfg=Config::discover()?;
        if args.get(1).map(String::as_str)==Some("serve"){return serve(cfg).await;}
        let operation=args.get(1).map(String::as_str).ok_or("operation_required")?;
        if operation=="unlock"{
            let status=std::process::Command::new("/usr/bin/python3")
                .env("DBUS_SESSION_BUS_ADDRESS",format!("unix:path={}/bus",cfg.runtime.parent().unwrap().display()))
                .arg(cfg.root.join("../experiments/lr-10a-sdk-runtime/h2_manual_unlock.py")).arg("unlock-existing-login").status().map_err(|_|"unlock_helper_unavailable")?;
            return if status.success(){Ok(())}else{Err("manual_unlock_failed")};
        }
        let request=match operation{
            "status"|"credentials"|"stronghold"|"copilot"|"events"|"session-check"|"capabilities"|"sessions"|"providers"|"session-create"=>json!({"operation":operation}),
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
        let request=ipc::Request{version:ipc::VERSION,request_id:format!("cli-{}",std::process::id()),command};
        request.validate()?;
        let request_bytes=serde_json::to_vec(&request).map_err(|_|"invalid_command")?;
        if request_bytes.len()>ipc::MAX_REQUEST_BYTES {return Err("request_limit_or_timeout");}
        let mut stream=tokio::net::UnixStream::connect(cfg.runtime.join("control.sock")).await.map_err(|_|"core_service_unavailable")?;
        if stream.peer_cred().map_err(|_|"peer_identity_unavailable")?.uid()!=unsafe{libc::geteuid()}{return Err("server_identity_mismatch");}
        stream.write_all(&request_bytes).await.map_err(|_|"request_write_failed")?;stream.shutdown().await.map_err(|_|"request_shutdown_failed")?;
        let mut out=String::new();tokio::time::timeout(std::time::Duration::from_secs(265),stream.take((ipc::MAX_RESPONSE_BYTES+1) as u64).read_to_string(&mut out)).await.map_err(|_|"response_timeout")?.map_err(|_|"response_read_failed")?;
        if out.len()>ipc::MAX_RESPONSE_BYTES{return Err("response_limit");}
        let v:serde_json::Value=serde_json::from_str(&out).map_err(|_|"response_invalid")?;if v["version"]!=ipc::VERSION || v["request_id"]!=request.request_id{return Err("response_correlation_mismatch");}
        println!("{}",serde_json::to_string_pretty(&v).unwrap());
        if v["ok"]==true{Ok(())}else{Err("operation_blocked")}
    }.await;
    if let Err(code) = outcome {
        eprintln!("{code}");
        std::process::exit(1);
    }
}
