#[tokio::main(flavor = "current_thread")]
async fn main() {
    unsafe {
        libc::umask(0o077);
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    if let Err(code) = narys_core::cli::run(args).await {
        if matches!(
            code,
            "core_operation_refused"
                | "human_unlock_refused_or_failed"
                | "diagnostic_failure_reported"
        ) {
            std::process::exit(1);
        }
        if json {
            println!(
                "{}",
                serde_json::json!({"version":1,"ok":false,"error_code":code,"category":"cli"})
            );
        } else {
            eprintln!("Erro: {code}");
        }
        std::process::exit(1);
    }
}
