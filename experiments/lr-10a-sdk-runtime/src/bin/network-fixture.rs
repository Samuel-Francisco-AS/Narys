//! Synthetic RPC peer: no inference, model, tool or real credential operation.
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, BufRead, Read, Write},
    net::TcpStream,
    time::Duration,
};
fn emit(v: Value) {
    let body = serde_json::to_vec(&v).unwrap();
    let mut out = io::stdout().lock();
    write!(out, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
    out.write_all(&body).unwrap();
    out.flush().unwrap();
}
fn main() {
    let arguments: Vec<_> = std::env::args().collect();
    if arguments.get(1).is_some_and(|a| a == "--network-child") {
        let denied =
            TcpStream::connect_timeout(&arguments[2].parse().unwrap(), Duration::from_millis(200))
                .is_err();
        println!(
            "{}",
            json!({"network_blocked":denied,"token_absent":std::env::var_os("COPILOT_SDK_AUTH_TOKEN").is_none()})
        );
        return;
    }
    if std::env::args().any(|a| a == "--auth-child") {
        println!(
            "{}",
            json!({"received":std::env::var_os("COPILOT_SDK_AUTH_TOKEN").is_some()})
        );
        return;
    }
    let spec_path = std::env::var("FIX4_SPEC").unwrap_or("/fixture/probe.json".into());
    let spec: Value = serde_json::from_slice(&fs::read(spec_path).unwrap()).unwrap();
    let mut input = io::stdin().lock();
    let mut pending = Value::Null;
    let mut sequence = 0;
    let mut report = json!({"registered":false,"statuses":[],"errors":[],
        "runtime_token_present":std::env::var_os("COPILOT_SDK_AUTH_TOKEN").is_some(),
        "token_in_argv":std::env::args().any(|s| s.starts_with("FIX4_SYNTHETIC_")),
        "no_auto_login":std::env::args().any(|s| s=="--no-auto-login"),
        "keytar_disabled":std::env::var("COPILOT_DISABLE_KEYTAR").ok().as_deref()==Some("1"),
        "forbidden_methods":0,"inference_calls":0,"safe_response":true});
    if report["runtime_token_present"] == true {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--auth-child")
            .output()
            .unwrap();
        let flags: Value = serde_json::from_slice(&child.stdout).unwrap();
        report["child_inherited_token"] = flags["received"].clone();
        report["child_reaped"] = json!(child.status.success());
    }
    if let Some(address) = spec["direct_address"].as_str() {
        report["direct_network_blocked"] = json!(TcpStream::connect_timeout(
            &address.parse().unwrap(),
            Duration::from_millis(200)
        )
        .is_err());
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--network-child", address])
            .output()
            .unwrap();
        let flags: Value = serde_json::from_slice(&child.stdout).unwrap();
        report["child_network_blocked"] = flags["network_blocked"].clone();
        report["child_token_absent"] = flags["token_absent"].clone();
        report["network_child_reaped"] = json!(child.status.success());
    }
    loop {
        let mut size = 0;
        loop {
            let mut line = String::new();
            if input.read_line(&mut line).unwrap() == 0 {
                return;
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length: ") {
                size = n.trim().parse::<usize>().unwrap();
            }
        }
        if size > 65536 {
            return;
        }
        let mut body = vec![0; size];
        input.read_exact(&mut body).unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        if v.get("method").is_none() {
            continue;
        }
        let method = v["method"].as_str().unwrap();
        let mut result = json!({});
        let mut error = None;
        match method {
            "connect" | "ping" => {
                result = json!({"ok":true,"protocolVersion":3,"version":"synthetic","timestamp":1,"message":v["params"]["message"]})
            }
            "llmInference.setProvider" => {
                if spec["unsupported"] == true {
                    error = Some(json!({"code":-32601,"message":"fixture_unsupported"}));
                } else {
                    let accepted = spec["registration_declined"] != true;
                    report["registered"] = json!(accepted);
                    result = json!({"success":accepted});
                }
            }
            "models.list" => {
                pending = v["id"].clone();
                sequence += 1;
                let request_id = format!("fixture-request-{sequence}");
                emit(
                    json!({"jsonrpc":"2.0","id":9001,"method":"llmInference.httpRequestStart","params":{
                    "requestId":request_id,"method":spec["method"].as_str().unwrap_or("GET"),
                    "url":spec["url"].as_str().unwrap_or("https://fixture.invalid/metadata"),
                    "headers":spec["headers"].as_object().cloned().unwrap_or_default(),
                    "transport":spec["transport"].as_str().unwrap_or("http")}}),
                );
                emit(
                    json!({"jsonrpc":"2.0","id":9002,"method":"llmInference.httpRequestChunk","params":{
                    "requestId":request_id,"data":spec["body"].as_str().unwrap_or(""),"end":true}}),
                );
                continue;
            }
            "llmInference.httpResponseStart" => {
                report["statuses"]
                    .as_array_mut()
                    .unwrap()
                    .push(v["params"]["status"].clone());
                result = json!({"accepted":true});
            }
            "llmInference.httpResponseChunk" => {
                result = json!({"accepted":true});
                let p = &v["params"];
                if p["error"].is_object() {
                    report["errors"].as_array_mut().unwrap().push(json!(
                        p["error"]["message"].as_str().unwrap_or("") == "fixture_gateway_blocked"
                            || p["error"]["message"] == "fixture_websocket_blocked"
                    ));
                }
                // Base64 encoding of the sole permissible DTO, or an empty terminator.
                if p["data"] != "" && p["data"] != "eyJ2YWx1ZSI6NX0=" {
                    report["safe_response"] = json!(false);
                }
                if p["end"] == true {
                    emit(json!({"jsonrpc":"2.0","id":pending,"result":{"models":[]}}));
                }
            }
            "fixture.summary" => result = report.clone(),
            "runtime.shutdown" => result = json!({"success":true}),
            _ => {
                report["forbidden_methods"] =
                    json!(report["forbidden_methods"].as_u64().unwrap() + 1);
                error = Some(json!({"code":-32601,"message":"fixture_forbidden"}));
            }
        }
        let response = if let Some(e) = error {
            json!({"jsonrpc":"2.0","id":v["id"],"error":e})
        } else {
            json!({"jsonrpc":"2.0","id":v["id"],"result":result})
        };
        emit(response);
    }
}
