use super::*;
use std::{
    fs,
    os::unix::{fs::symlink, net::UnixListener},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    },
};

struct Fixture {
    _directory: tempfile::TempDir,
    workspace: std::path::PathBuf,
    service: Arc<AuthorityService>,
}
fn request(
    service: &AuthorityService,
    context: AgentOperationContext,
    ttl: Duration,
    boundary: bool,
    financial: bool,
) -> Result<(String, AgentAuthority)> {
    let digest = binding(&context);
    if let Err(error) = digest {
        if boundary && financial {
            return Err(error);
        }
    }
    let digest = digest.unwrap_or_default();
    let b = boundary.then(|| ExecutionBoundary {
        binding: digest.clone(),
        deadline: Instant::now() + Duration::from_secs(120),
    });
    let f = financial.then(|| FinancialAdmission {
        binding: digest,
        deadline: Instant::now() + Duration::from_secs(120),
        paid_use_allowed: false,
    });
    service.request(context, ttl, b.as_ref(), f.as_ref())
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        fs::write(workspace.join("existing"), "original").unwrap();
        let database = Database::new(directory.path().join("db"));
        database.open().unwrap();
        Self {
            _directory: directory,
            workspace,
            service: Arc::new(AuthorityService::new(database).unwrap()),
        }
    }
    fn context(&self) -> AgentOperationContext {
        AgentOperationContext {
            task_id: 1,
            session_id: "session-1".into(),
            specialist_id: "copilot".into(),
            profile: AgentApprovalPolicy::Assisted,
            workspace: self.workspace.clone(),
            tool: "narys.write".into(),
            operation: AgentOperation::Write {
                relative_path: "new".into(),
                content: "fixture-sensitive-content".into(),
            },
            policy_version: POLICY_VERSION,
        }
    }
    fn request(&self) -> (String, AgentAuthority) {
        request(
            &self.service,
            self.context(),
            Duration::from_secs(60),
            true,
            true,
        )
        .unwrap()
    }
    // Only a trusted Core test fixture can mint this proof. No production UI,
    // IPC request, TTY, process UID, environment or session mints it.
    fn approve(&self, id: &str) {
        self.service
            .approve(
                id,
                &HumanChannel {
                    epoch: self.service.epoch.clone(),
                },
                &binding(&self.context()).unwrap(),
            )
            .unwrap();
    }
}
#[test]
fn default_deny_and_independent_boundary_financial_gates() {
    let f = Fixture::new();
    assert_eq!(
        AgentApprovalPolicy::default(),
        AgentApprovalPolicy::Assisted
    );
    for (boundary, financial, expected) in [
        (false, true, "execution_boundary_unavailable"),
        (true, false, "financial_admission_required"),
    ] {
        assert_eq!(
            request(
                &f.service,
                f.context(),
                Duration::from_secs(60),
                boundary,
                financial
            )
            .err()
            .unwrap(),
            expected
        );
    }
    let fake = AgentAuthority([0; 32]);
    assert_eq!(
        f.service
            .execute::<()>(&fake, &f.context(), || panic!("forged authority effect")),
        Err::<(), _>("authority_unknown")
    );
    assert!(!f.workspace.join("new").exists());
}
#[test]
fn financial_and_boundary_proofs_are_exact_expiring_and_never_allow_paid_use() {
    let f = Fixture::new();
    let context = f.context();
    let digest = binding(&context).unwrap();
    let boundary = ExecutionBoundary {
        binding: digest.clone(),
        deadline: Instant::now() + Duration::from_secs(60),
    };
    for (proof_digest, deadline, paid) in [
        (
            "wrong".into(),
            Instant::now() + Duration::from_secs(60),
            false,
        ),
        (digest.clone(), Instant::now(), false),
        (
            digest.clone(),
            Instant::now() + Duration::from_secs(60),
            true,
        ),
    ] {
        let financial = FinancialAdmission {
            binding: proof_digest,
            deadline,
            paid_use_allowed: paid,
        };
        assert!(f
            .service
            .request(
                context.clone(),
                Duration::from_secs(60),
                Some(&boundary),
                Some(&financial)
            )
            .is_err());
    }
    let financial = FinancialAdmission {
        binding: digest,
        deadline: Instant::now() + Duration::from_secs(60),
        paid_use_allowed: false,
    };
    let expired_boundary = ExecutionBoundary {
        binding: binding(&context).unwrap(),
        deadline: Instant::now(),
    };
    assert!(f
        .service
        .request(
            context,
            Duration::from_secs(60),
            Some(&expired_boundary),
            Some(&financial)
        )
        .is_err());
    let (id, cap) = f.request();
    f.approve(&id);
    f.service
        .state
        .lock()
        .unwrap()
        .grants
        .get_mut(&cap.0)
        .unwrap()
        .financial_deadline = Instant::now();
    assert!(f
        .service
        .execute::<()>(&cap, &f.context(), || panic!("expired financial approval"))
        .is_err());
}
#[test]
fn persistence_failure_rolls_back_claim_and_unknown_effect_is_never_replayed() {
    let f = Fixture::new();
    let (id, cap) = f.request();
    f.approve(&id);
    let conn = connection(&f.service.database).unwrap();
    conn.execute_batch("CREATE TRIGGER synthetic_claim_failure BEFORE INSERT ON agent_approval_events WHEN NEW.state='consumed' BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(f
        .service
        .execute::<()>(&cap, &f.context(), || panic!("effect after failed claim"))
        .is_err());
    assert_eq!(f.service.get(&id).unwrap()["state"], "approved");
    conn.execute_batch("DROP TRIGGER synthetic_claim_failure;")
        .unwrap();
    assert_eq!(
        f.service
            .execute::<()>(&cap, &f.context(), || Err("effect_unknown")),
        Err("effect_unknown")
    );
    assert_eq!(f.service.get(&id).unwrap()["state"], "consumed");
    assert!(f
        .service
        .execute::<()>(&cap, &f.context(), || panic!("unknown effect replay"))
        .is_err());
}
#[test]
fn cancellation_and_shutdown_prevent_new_authority_and_ttl_is_bounded() {
    let f = Fixture::new();
    for ttl in [
        Duration::ZERO,
        Duration::from_millis(1),
        Duration::from_secs(301),
    ] {
        assert!(request(&f.service, f.context(), ttl, true, true).is_err());
    }
    f.service.cancel_task(1).unwrap();
    assert!(request(&f.service, f.context(), Duration::from_secs(60), true, true).is_err());
    let mut c = f.context();
    c.task_id = 2;
    f.service.revoke_all().unwrap();
    assert!(request(&f.service, c, Duration::from_secs(60), true, true).is_err());
}
#[test]
fn explicit_single_use_approval_and_receipt_is_not_authority() {
    let f = Fixture::new();
    let (id, capability) = f.request();
    assert!(f
        .service
        .execute::<()>(&capability, &f.context(), || Ok(()))
        .is_err());
    f.approve(&id);
    f.service
        .execute::<()>(&capability, &f.context(), || {
            fs::write(f.workspace.join("new"), "approved-effect").unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(
        fs::read_to_string(f.workspace.join("new")).unwrap(),
        "approved-effect"
    );
    assert_eq!(f.service.get(&id).unwrap()["state"], "consumed");
    assert!(f
        .service
        .execute::<()>(&capability, &f.context(), || panic!("receipt replay"))
        .is_err());
    assert!(f.service.approve_from_ipc(&id).is_err());
}
#[test]
fn denied_expired_and_no_approver_never_run_effects() {
    for mode in ["deny", "timeout", "absent"] {
        let f = Fixture::new();
        let (id, capability) = f.request();
        match mode {
            "deny" => {
                f.service.deny(&id).unwrap();
            }
            "timeout" => {
                f.service
                    .state
                    .lock()
                    .unwrap()
                    .grants
                    .get_mut(&capability.0)
                    .unwrap()
                    .deadline = Instant::now();
                connection(&f.service.database)
                    .unwrap()
                    .execute(
                        "UPDATE agent_approvals SET expires_at=0 WHERE approval_id=?1",
                        [&id],
                    )
                    .unwrap();
                assert_eq!(f.service.get(&id).unwrap()["state"], "expired");
            }
            _ => (),
        }
        assert!(f
            .service
            .execute::<()>(&capability, &f.context(), || panic!("unapproved effect"))
            .is_err());
        assert!(!f.workspace.join("new").exists());
    }
}
#[test]
fn duplicate_approval_and_receipt_consumption_are_atomic() {
    let f = Fixture::new();
    let (id, capability) = f.request();
    let start = Arc::new(Barrier::new(8));
    let results = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let start = start.clone();
                let service = f.service.clone();
                let id = id.clone();
                let digest = binding(&f.context()).unwrap();
                scope.spawn(move || {
                    start.wait();
                    service
                        .approve(
                            &id,
                            &HumanChannel {
                                epoch: service.epoch.clone(),
                            },
                            &digest,
                        )
                        .is_ok()
                })
            })
            .collect();
        threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .filter(|v| *v)
            .count()
    });
    assert_eq!(results, 1);
    let effects = AtomicUsize::new(0);
    let start = Arc::new(Barrier::new(8));
    std::thread::scope(|scope| {
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let start = start.clone();
                let service = &f.service;
                let capability = &capability;
                let context = f.context();
                let effects = &effects;
                scope.spawn(move || {
                    start.wait();
                    service
                        .execute::<()>(capability, &context, || {
                            effects.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        })
                        .is_ok()
                })
            })
            .collect();
        assert_eq!(
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .filter(|v| *v)
                .count(),
            1
        );
    });
    assert_eq!(effects.load(Ordering::SeqCst), 1);
}
#[test]
fn every_identity_profile_scope_tool_argument_and_policy_is_bound() {
    let f = Fixture::new();
    let (id, capability) = f.request();
    f.approve(&id);
    let original = f.context();
    let mut mutations = vec![];
    let mut c = original.clone();
    c.task_id = 2;
    mutations.push(c);
    let mut c = original.clone();
    c.session_id = "session-2".into();
    mutations.push(c);
    let mut c = original.clone();
    c.specialist_id = "codex".into();
    mutations.push(c);
    let mut c = original.clone();
    c.profile = AgentApprovalPolicy::Isolated;
    mutations.push(c);
    let mut c = original.clone();
    c.workspace = f._directory.path().into();
    mutations.push(c);
    let mut c = original.clone();
    c.tool = "bash".into();
    mutations.push(c);
    let mut c = original.clone();
    c.policy_version = 2;
    mutations.push(c);
    let mut c = original.clone();
    c.operation = AgentOperation::Write {
        relative_path: "other".into(),
        content: "changed".into(),
    };
    mutations.push(c);
    for c in mutations {
        assert!(f
            .service
            .execute::<()>(&capability, &c, || panic!("context substitution"))
            .is_err());
    }
    assert_eq!(f.service.get(&id).unwrap()["state"], "approved");
}
#[test]
fn workspace_inode_replacement_invalidates_a_grant() {
    let f = Fixture::new();
    let (id, cap) = f.request();
    f.approve(&id);
    fs::rename(&f.workspace, f.workspace.with_extension("old")).unwrap();
    fs::create_dir(&f.workspace).unwrap();
    assert!(f
        .service
        .execute::<()>(&cap, &f.context(), || panic!("replaced workspace"))
        .is_err());
}
#[test]
fn traversal_absolute_paths_symlinks_hardlinks_and_workspace_aliases_denied() {
    let f = Fixture::new();
    symlink(f._directory.path(), f.workspace.join("escape")).unwrap();
    fs::hard_link(f.workspace.join("existing"), f.workspace.join("alias")).unwrap();
    for path in [
        "../outside",
        "/etc/passwd",
        "escape/outside",
        "alias",
        "existing",
    ] {
        let mut c = f.context();
        c.operation = AgentOperation::Write {
            relative_path: path.into(),
            content: "no".into(),
        };
        assert!(
            request(&f.service, c, Duration::from_secs(60), true, true).is_err(),
            "{path}"
        );
    }
    let alias = f._directory.path().join("workspace-link");
    symlink(&f.workspace, &alias).unwrap();
    let mut c = f.context();
    c.workspace = alias;
    assert!(request(&f.service, c, Duration::from_secs(60), true, true).is_err());
}
#[test]
fn unknown_native_mcp_plugin_and_network_operations_denied() {
    let f = Fixture::new();
    for name in [
        "bash",
        "task",
        "skill",
        "mcp.server.tool",
        "plugin.exec",
        "HumanLocal",
        "future_tool",
    ] {
        let mut c = f.context();
        c.tool = name.into();
        assert!(request(&f.service, c, Duration::from_secs(60), true, true).is_err());
    }
    let mut c = f.context();
    c.tool = "narys.network".into();
    c.operation = AgentOperation::Network {
        destination: "https://invalid.local".into(),
    };
    assert!(request(&f.service, c, Duration::from_secs(60), true, true).is_err());
}
#[test]
fn cancellation_pending_approved_and_racing_execution_is_ordered() {
    for approve in [false, true] {
        let f = Fixture::new();
        let (id, cap) = f.request();
        if approve {
            f.approve(&id);
        }
        f.service.cancel_task(1).unwrap();
        assert_eq!(f.service.get(&id).unwrap()["state"], "cancelled");
        assert!(f
            .service
            .execute::<()>(&cap, &f.context(), || panic!("post cancel effect"))
            .is_err());
    }
    for _ in 0..32 {
        let f = Fixture::new();
        let (id, cap) = f.request();
        f.approve(&id);
        let start = Barrier::new(2);
        let effects = AtomicUsize::new(0);
        std::thread::scope(|s| {
            let a = s.spawn(|| {
                start.wait();
                f.service.cancel_task(1).unwrap();
            });
            start.wait();
            let result = f.service.execute::<()>(&cap, &f.context(), || {
                effects.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
            a.join().unwrap();
            assert_eq!(effects.load(Ordering::SeqCst), usize::from(result.is_ok()));
        });
        assert!(f
            .service
            .execute::<()>(&cap, &f.context(), || panic!("second effect"))
            .is_err());
    }
}
#[test]
fn recovery_revokes_pending_and_approved_preserves_consumed_without_replay() {
    let f = Fixture::new();
    let (pending, _) = f.request();
    let (approved, cap) = f.request();
    f.approve(&approved);
    let (consumed, used) = f.request();
    f.approve(&consumed);
    f.service
        .execute::<()>(&used, &f.context(), || Ok(()))
        .unwrap();
    let mut conn = connection(&f.service.database).unwrap();
    recover(&mut conn).unwrap();
    recover(&mut conn).unwrap();
    assert_eq!(f.service.get(&pending).unwrap()["state"], "interrupted");
    assert_eq!(f.service.get(&approved).unwrap()["state"], "interrupted");
    assert_eq!(f.service.get(&consumed).unwrap()["state"], "consumed");
    let restarted = AuthorityService::new(f.service.database.clone()).unwrap();
    assert!(restarted
        .execute::<()>(&cap, &f.context(), || panic!("restart authority replay"))
        .is_err());
    assert!(f
        .service
        .execute::<()>(&cap, &f.context(), || panic!("recovery replay"))
        .is_err());
}
#[test]
fn untrusted_channels_repeated_expired_and_forged_decisions_rejected() {
    let f = Fixture::new();
    let (id, _) = f.request();
    let digest = binding(&f.context()).unwrap();
    assert!(f
        .service
        .approve(
            &id,
            &HumanChannel {
                epoch: "agent-controlled".into()
            },
            &digest
        )
        .is_err());
    assert!(f
        .service
        .approve(
            &id,
            &HumanChannel {
                epoch: f.service.epoch.clone()
            },
            "changed arguments"
        )
        .is_err());
    assert!(f.service.approve_from_ipc(&id).is_err());
    assert_eq!(f.service.get(&id).unwrap()["state"], "pending");
}
#[test]
fn yolo_requires_scoped_human_consent_managed_policy_and_is_never_executable() {
    let f = Fixture::new();
    let mut c = f.context();
    c.profile = AgentApprovalPolicy::ExplicitYolo;
    let channel = HumanChannel {
        epoch: f.service.epoch.clone(),
    };
    assert!(f
        .service
        .activate_yolo(c.clone(), &channel, Duration::from_secs(60), false, false)
        .is_err());
    assert!(f
        .service
        .activate_yolo(c.clone(), &channel, Duration::from_secs(60), true, true)
        .is_err());
    assert!(f
        .service
        .activate_yolo(c.clone(), &channel, Duration::from_secs(301), true, false)
        .is_err());
    f.service
        .activate_yolo(c.clone(), &channel, Duration::from_secs(60), true, false)
        .unwrap();
    assert!(request(&f.service, c, Duration::from_secs(60), true, true).is_err());
    assert_eq!(f.service.availability()["yolo_execution"], "BLOCKED");
    f.service.cancel_task(1).unwrap();
    assert!(f.service.state.lock().unwrap().yolo.is_none());
    assert!(AuthorityService::new(f.service.database.clone())
        .unwrap()
        .state
        .lock()
        .unwrap()
        .yolo
        .is_none());
}
#[test]
fn sanitized_history_has_no_capabilities_private_arguments_or_file_contents() {
    let f = Fixture::new();
    let (id, cap) = f.request();
    let row = f.service.get(&id).unwrap().to_string();
    assert!(!row.contains("fixture-sensitive-content"));
    assert!(!row.contains(&hex(&cap.0)));
    let events = connection(&f.service.database)
        .unwrap()
        .query_row(
            "SELECT group_concat(details_json) FROM server_events",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap();
    assert!(!events.contains("fixture-sensitive-content"));
    assert!(!events.contains(&hex(&cap.0)));
    assert_eq!(
        f.service.list(0, 1, true).unwrap()["approvals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(f.service.list(u64::MAX, 1, false).is_err());
}
#[test]
fn real_sandbox_blocks_host_fs_credentials_ipc_environment_network_and_nested_shell() {
    let f = Fixture::new();
    let outside = f._directory.path().join("private-secret");
    fs::write(&outside, "synthetic-private-marker").unwrap();
    let socket = f._directory.path().join("control.sock");
    let _listener = UnixListener::bind(&socket).unwrap();
    let script = format!(
        r#"
import json, os, socket, subprocess
from pathlib import Path
denied=[]
for name, action in [
 ('outside-read', lambda: Path({outside:?}).read_text()),
 ('outside-write', lambda: Path({outside:?}).write_text('escaped')),
 ('host-proc', lambda: Path('/proc/{pid}/environ').read_bytes()),
 ('ipc-self-approval', lambda: socket.socket(socket.AF_UNIX).connect({socket:?})),
 ('network', lambda: socket.create_connection(('127.0.0.1',9),timeout=.2)),
 ('system-write', lambda: Path('/usr/lr10c-host-write').write_text('no')),
]:
 try: action(); raise AssertionError(name+' escaped')
 except (OSError,PermissionError): denied.append(name)
assert not os.environ.get('NARYS_PRIVATE_FIXTURE')
Path('/workspace/link').symlink_to({outside:?})
assert subprocess.run(['/bin/sh','-c','sh -c "cat /workspace/link"'],capture_output=True).returncode != 0
assert subprocess.run(['/usr/bin/unshare','-Ur','/usr/bin/true'],capture_output=True).returncode != 0
assert len(os.listdir('/proc/self/fd')) <= 4
Path('/workspace/positive').write_text('confined effect')
print(json.dumps({{'denied':denied,'positive':True,'private_environment':False}}))
"#,
        outside = outside.to_str().unwrap(),
        socket = socket.to_str().unwrap(),
        pid = std::process::id()
    );
    let mut command =
        sandbox::command(&f.workspace, "/usr/bin/python3", &["-I", "-c", &script]).unwrap();
    command.env("NARYS_PRIVATE_FIXTURE", "private-env-marker");
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["denied"].as_array().unwrap().len(), 6);
    assert_eq!(
        fs::read_to_string(&outside).unwrap(),
        "synthetic-private-marker"
    );
    assert_eq!(
        fs::read_to_string(f.workspace.join("positive")).unwrap(),
        "confined effect"
    );
}
#[tokio::test]
async fn real_sandbox_pid_namespace_reaps_daemonized_descendants_and_cancel() {
    let f = Fixture::new();
    let code="import os,time; from pathlib import Path; pid=os.fork();\nif pid == 0:\n os.setsid(); time.sleep(.7); Path('/workspace/orphan-effect').write_text('escaped')\nelse:\n Path('/workspace/started').write_text('yes'); time.sleep(30)";
    let command = sandbox::command(&f.workspace, "/usr/bin/python3", &["-I", "-c", code]).unwrap();
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let mut child = command.spawn().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !f.workspace.join("started").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(!f.workspace.join("orphan-effect").exists());
}
#[test]
fn sandbox_absent_disabled_or_partial_never_enables_profile() {
    let f = Fixture::new();
    // A successful subprocess diagnostic is deliberately not a runtime permit.
    assert!(sandbox::command(&f.workspace, "/usr/bin/true", &[])
        .unwrap()
        .status()
        .unwrap()
        .success());
    assert_eq!(f.service.availability()["isolated_execution"], "BLOCKED");
    for profile in [
        AgentApprovalPolicy::Isolated,
        AgentApprovalPolicy::ExplicitYolo,
    ] {
        let mut c = f.context();
        c.profile = profile;
        assert!(request(&f.service, c, Duration::from_secs(60), true, true).is_err());
    }
}
#[test]
fn additive_migration_from_21_preserves_history_and_is_idempotent() {
    let f = Fixture::new();
    let mut conn = connection(&f.service.database).unwrap();
    conn.execute_batch("DROP TABLE agent_approval_events; DROP TABLE agent_approvals; PRAGMA user_version=21; INSERT INTO server_events(namespace,code) VALUES('product','preserved_fixture');").unwrap();
    crate::persistence::migrations::apply(&conn).unwrap();
    crate::persistence::migrations::apply(&conn).unwrap();
    assert_eq!(
        conn.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        22
    );
    assert_eq!(
        conn.query_row::<i64, _, _>(
            "SELECT count(*) FROM server_events WHERE code='preserved_fixture'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        1
    );
    recover(&mut conn).unwrap();
    assert_eq!(
        conn.query_row::<String, _, _>("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap(),
        "ok"
    );
}
