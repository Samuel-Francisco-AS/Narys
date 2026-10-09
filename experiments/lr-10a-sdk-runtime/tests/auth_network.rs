#[path = "../src/auth_network.rs"]
mod auth_network;
use auth_network::FixtureGateway;
use github_copilot_sdk::{Client, ClientMode, ClientOptions};
use narys_lr10a_poc::{bounded, options, shutdown};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const SECRET: &str = "FIX4_SYNTHETIC_HOST_ONLY";
struct Provider {
    port: u16,
    seen: Arc<AtomicBool>,
    contacts: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Provider {
    fn start(behavior: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let seen = Arc::new(AtomicBool::new(false));
        let contacts = Arc::new(AtomicUsize::new(0));
        let count = contacts.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let s = seen.clone();
        let ending = stop.clone();
        let worker = std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(5);
            while Instant::now() < end && !ending.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        count.fetch_add(1, Ordering::SeqCst);
                        stream
                            .set_read_timeout(Some(Duration::from_millis(300)))
                            .unwrap();
                        let mut all = Vec::new();
                        let mut buf = [0; 128];
                        while all.len() < 512 && !all.ends_with(b"\r\n\r\n") {
                            match stream.read(&mut buf) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => all.extend_from_slice(&buf[..n]),
                            }
                        }
                        let text = String::from_utf8_lossy(&all);
                        s.store(
                            text.contains(&format!("Authorization: Bearer {SECRET}\r\n"))
                                && text.starts_with("GET /metadata "),
                            Ordering::SeqCst,
                        );
                        if behavior == "timeout" {
                            while !ending.load(Ordering::SeqCst) && Instant::now() < end {
                                std::thread::sleep(Duration::from_millis(5));
                            }
                        } else {
                            let response=match behavior {
                                "redirect"=>"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/forbidden\r\n\r\n",
                                "unauthorized"=>"HTTP/1.1 401 Unauthorized\r\n\r\n",
                                "echo"=>SECRET,
                                _=>"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"value\":5}",
                            };
                            let _ = stream.write_all(response.as_bytes());
                        }
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
            // Drain kernel-queued connections before closing the canary. This
            // negative proof does not depend on when the listener thread ran.
            while listener.accept().is_ok() {
                count.fetch_add(1, Ordering::SeqCst);
            }
        });
        Self {
            port,
            seen,
            contacts,
            stop,
            worker: Some(worker),
        }
    }
    fn finish(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.finish();
    }
}

async fn case(
    name: &str,
    spec: Value,
    port: Option<u16>,
    secret: Option<String>,
    expect_success: bool,
    explicit: bool,
) {
    let job = tempfile::Builder::new()
        .prefix("narys-fix4-test-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = job.path().join("workspace");
    let state = job.path().join("state");
    let store = state.join("session-state");
    let logs = job.path().join("logs");
    for p in [&workspace, &store, &logs] {
        std::fs::create_dir_all(p).unwrap();
    }
    let spec_path = workspace.join("probe.json");
    std::fs::write(&spec_path, serde_json::to_vec(&spec).unwrap()).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let program = root.join("target/debug/network-fixture");
    let mut opts = options(program.clone(), &workspace, &state);
    opts.mode = ClientMode::Empty;
    opts.use_logged_in_user = Some(false);
    if explicit {
        opts.github_token = Some(SECRET.into());
        if spec["strip_token"] != true {
            opts.env_remove
                .retain(|name| name != "COPILOT_SDK_AUTH_TOKEN");
        }
        opts.env.push(("FIX4_SPEC".into(), spec_path.into()));
        assert!(
            !format!("{opts:?}").contains(SECRET),
            "SDK debug must redact token"
        );
    } else {
        opts.program = github_copilot_sdk::CliProgram::Path("/usr/bin/python3".into());
        opts.prefix_args = vec![
            "-I".into(),
            root.join("fix4_boundary.py").into(),
            program.into(),
            workspace.clone().into(),
            state.clone().into(),
            store.into(),
            logs.into(),
            "--".into(),
        ];
        opts.extra_args = vec![
            "--disable-builtin-mcps".into(),
            "--log-dir".into(),
            "/logs".into(),
        ];
    }
    let gateway = Arc::new(FixtureGateway::new(port, secret));
    opts.request_handler = Some(gateway.clone());
    let started = bounded(Client::start(opts)).await;
    if spec["unsupported"] == true {
        assert!(matches!(started, Err("method_unavailable")));
        if let Ok(file) = std::env::var("FIX4_EVIDENCE") {
            let mut out = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file)
                .unwrap();
            writeln!(out,"{}",json!({"case":name,"status":"PASS","startup":"blocked_unsupported_handler","real_provider":false})).unwrap();
        }
        return;
    }
    let client = started.expect("owned SDK fixture startup");
    let pid = client.pid().unwrap();
    let registration = auth_network::require_registered(&client).await;
    if spec["registration_declined"] == true {
        assert_eq!(registration, Err("handler_registration_declined"));
        let summary = bounded(client.call("fixture.summary", None)).await.unwrap();
        assert_eq!(summary["registered"], false);
        assert_eq!(summary["statuses"], json!([]));
        assert_eq!(gateway.calls(), 0);
        assert_eq!(shutdown(&client).await, "graceful");
        drop(client);
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
        if let Ok(file) = std::env::var("FIX4_EVIDENCE") {
            let mut out = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file)
                .unwrap();
            writeln!(out,"{}",json!({"case":name,"status":"PASS","sdk_start_accepted_negative_ack":true,
                "poc_admission":"blocked_negative_ack","metadata_not_admitted":true,"fixture_pid":pid,"reaped":true,"real_provider":false})).unwrap();
        }
        return;
    }
    registration.expect("mandatory positive registration ACK");
    bounded(client.list_models()).await.unwrap();
    if spec["repeat"] == true {
        // list_models caches its catalog. Force ONLY this synthetic peer's second RPC.
        bounded(client.call("models.list", None)).await.unwrap();
    }
    let summary = bounded(client.call("fixture.summary", None)).await.unwrap();
    assert_eq!(summary["registered"], true);
    assert_eq!(summary["forbidden_methods"], 0);
    assert_eq!(summary["inference_calls"], 0);
    assert_eq!(summary["token_in_argv"], false);
    assert_eq!(summary["safe_response"], true);
    assert_eq!(summary["no_auto_login"], true);
    assert_eq!(summary["keytar_disabled"], true);
    let exposed = explicit && spec["strip_token"] != true;
    assert_eq!(summary["runtime_token_present"], exposed);
    if exposed {
        assert_eq!(summary["child_inherited_token"], true);
        assert_eq!(summary["child_reaped"], true);
    }
    if !explicit {
        assert_eq!(summary["direct_network_blocked"], true);
        assert_eq!(summary["child_network_blocked"], true);
        assert_eq!(summary["child_token_absent"], true);
        assert_eq!(summary["network_child_reaped"], true);
    }
    if spec["repeat"] == true {
        assert_eq!(summary["statuses"], json!([200, 502]));
        assert_eq!(summary["errors"], json!([true]));
        assert_eq!(gateway.calls(), 1);
    } else if expect_success {
        assert_eq!(summary["statuses"], json!([200]));
        assert_eq!(summary["errors"], json!([]));
    } else {
        assert_eq!(summary["errors"], json!([true]));
    }
    assert!(!summary.to_string().contains(SECRET));
    assert_eq!(shutdown(&client).await, "graceful");
    drop(client);
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "owned runtime not reaped"
    );
    if let Ok(file) = std::env::var("FIX4_EVIDENCE") {
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
            .unwrap();
        writeln!(out,"{}",json!({"case":name,"summary":summary,"gateway_attempts":gateway.calls(),"fixture_pid":pid,"reaped":true,"status":"PASS","real_provider":false})).unwrap();
    }
}
fn spec(port: u16) -> Value {
    json!({"direct_address":format!("127.0.0.1:{port}")})
}
#[tokio::test(flavor = "current_thread")]
async fn allowed_fixed_operation_only_through_host_in_offline_namespace() {
    let p = Provider::start("ok");
    case(
        "allowed",
        spec(p.port),
        Some(p.port),
        Some(SECRET.into()),
        true,
        false,
    )
    .await;
    assert!(p.seen.load(Ordering::SeqCst));
}
#[tokio::test(flavor = "current_thread")]
async fn absent_auth_and_absent_gateway_fail_closed() {
    for missing_auth in [true, false] {
        let p = Provider::start("ok");
        case(
            if missing_auth {
                "missing_auth"
            } else {
                "missing_gateway"
            },
            spec(p.port),
            if missing_auth { Some(p.port) } else { None },
            if missing_auth {
                None
            } else {
                Some(SECRET.into())
            },
            false,
            false,
        )
        .await;
        assert!(!p.seen.load(Ordering::SeqCst));
    }
}
#[tokio::test(flavor = "current_thread")]
async fn destinations_protocols_headers_bodies_and_paths_rejected() {
    for change in [
        json!({"url":"http://127.0.0.1:1/metadata"}),
        json!({"url":"https://fixture.invalid/metadata?exfil=canary"}),
        json!({"url":"https://fixture.invalid/../metadata"}),
        json!({"method":"CONNECT"}),
        json!({"method":"POST","body":"unauthorized"}),
        json!({"headers":{"authorization":["untrusted"]}}),
        json!({"transport":"websocket"}),
    ] {
        let p = Provider::start("ok");
        let mut s = spec(p.port);
        s.as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        case(
            "policy_reject",
            s,
            Some(p.port),
            Some(SECRET.into()),
            false,
            false,
        )
        .await;
        assert!(!p.seen.load(Ordering::SeqCst));
    }
}
#[tokio::test(flavor = "current_thread")]
async fn unavailable_gateway_has_no_direct_network_fallback() {
    // Keep the TCP port owned but NOT listening: no reuse by an external service.
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = socket.local_addr().unwrap().port();
    case(
        "unavailable",
        spec(port),
        Some(port),
        Some(SECRET.into()),
        false,
        false,
    )
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn timeout_redirect_unauthorized_and_secret_echo_fail_closed() {
    for behavior in ["timeout", "redirect", "unauthorized", "echo"] {
        let p = Provider::start(behavior);
        case(
            behavior,
            spec(p.port),
            Some(p.port),
            Some(SECRET.into()),
            false,
            false,
        )
        .await;
        assert!(p.seen.load(Ordering::SeqCst));
    }
}
#[tokio::test(flavor = "current_thread")]
async fn explicit_sdk_token_is_runtime_exposure_not_host_containment() {
    case(
        "explicit_token_exposes_runtime",
        json!({}),
        None,
        None,
        false,
        true,
    )
    .await;
}
#[test]
fn default_handler_is_never_used_and_inference_is_not_implemented() {
    let source = include_str!("../src/auth_network.rs");
    assert!(!source.contains("forward_http("));
    assert!(source.contains("fn open_websocket"));
    assert!(!source.contains("send_and_wait"));
    assert!(!source.contains(".send("));
    let _ = std::mem::size_of::<ClientOptions>();
    let _ = std::mem::size_of::<TcpStream>();
}

#[tokio::test(flavor = "current_thread")]
async fn second_operation_exceeds_fixed_budget_without_retry_or_fallback() {
    let p = Provider::start("ok");
    let mut s = spec(p.port);
    s["repeat"] = json!(true);
    case(
        "budget_one",
        s,
        Some(p.port),
        Some(SECRET.into()),
        true,
        false,
    )
    .await;
    assert!(p.seen.load(Ordering::SeqCst));
}
#[tokio::test(flavor = "current_thread")]
async fn unsupported_runtime_handler_is_a_startup_gate() {
    let mut s = spec(1);
    s["unsupported"] = json!(true);
    case("unsupported_handler", s, None, None, false, false).await;
    let mut declined = spec(1);
    declined["registration_declined"] = json!(true);
    case("declined_handler", declined, None, None, false, false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn invalid_synthetic_authorization_never_contacts_provider() {
    let p = Provider::start("ok");
    case(
        "invalid_auth",
        spec(p.port),
        Some(p.port),
        Some("not_an_authorized_fixture_credential".into()),
        false,
        false,
    )
    .await;
    assert!(!p.seen.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_token_removed_from_environment_does_not_become_ambient_auth() {
    case(
        "token_removed",
        json!({"strip_token":true}),
        None,
        None,
        false,
        true,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn prohibited_live_local_canary_receives_zero_connections() {
    let mut permitted = Provider::start("ok");
    let mut forbidden = Provider::start("ok");
    let mut s = spec(forbidden.port);
    s["url"] = json!(format!("http://127.0.0.1:{}/metadata", forbidden.port));
    case(
        "prohibited_live_destination",
        s,
        Some(permitted.port),
        Some(SECRET.into()),
        false,
        false,
    )
    .await;
    permitted.finish();
    forbidden.finish();
    assert_eq!(permitted.contacts.load(Ordering::SeqCst), 0);
    assert_eq!(forbidden.contacts.load(Ordering::SeqCst), 0);
    if let Ok(file) = std::env::var("FIX4_EVIDENCE") {
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
            .unwrap();
        writeln!(out,"{}",json!({"case":"live_canary_contacts","permitted_contacts":0,"prohibited_contacts":0,"status":"PASS","real_provider":false})).unwrap();
    }
}
