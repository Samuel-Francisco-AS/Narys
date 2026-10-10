use narys_core::{
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
            "status"|"credentials"|"stronghold"|"copilot"|"events"|"session-check"=>json!({"operation":operation}),
            "prepare" if args.len()==4=>json!({"operation":"prepare","task":{"objective":args[2],"model":"auto","included_only_approval":true},"expected":args[3]}),
            "submit"|"cancel"|"result" if args.len()==3=>json!({"operation":operation,"task_id":args[2].parse::<u64>().map_err(|_|"invalid_task_id")?}),
            _=>return Err("invalid_command")
        };
        let mut stream=tokio::net::UnixStream::connect(cfg.runtime.join("control.sock")).await.map_err(|_|"core_service_unavailable")?;
        stream.write_all(serde_json::to_string(&request).unwrap().as_bytes()).await.map_err(|_|"request_write_failed")?;stream.shutdown().await.map_err(|_|"request_shutdown_failed")?;
        let mut out=String::new();stream.take(4*1024*1024).read_to_string(&mut out).await.map_err(|_|"response_read_failed")?;
        let v:serde_json::Value=serde_json::from_str(&out).map_err(|_|"response_invalid")?;println!("{}",serde_json::to_string_pretty(&v).unwrap());
        if v["ok"]==true{Ok(())}else{Err("operation_blocked")}
    }.await;
    if let Err(code) = outcome {
        eprintln!("{code}");
        std::process::exit(1);
    }
}
