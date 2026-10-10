//! Thin terminal UI: all cognitive/operational state belongs to the Core.
use crate::{
    client,
    ipc::{self, Command},
};
use serde_json::{json, Value};
use std::io::{BufRead, IsTerminal, Read, Write};
const HELP: &str = "Narys — administração pelo terminal/SSH
  narys status | doctor | capabilities | providers | models
  narys agent status | recover | stop | new
  narys agent get|resume|attach|close SESSION_REF
  narys agent detach ATTACHMENT_ID
  narys sessions [--after ID] [--limit 1..100]
  narys session ID [--after ID] [--limit 1..100]
  narys session new | resume ID | close ID
  narys tasks [--namespace product|lr10a] [--after ID] [--limit 1..100]
  narys task ID [--namespace product|lr10a] [--wait] [--timeout 1..3600]
  narys cancel ID [--namespace product|lr10a]
  narys events [--after SEQUENCE] [--limit 1..128]
  narys credentials status | unlock
  narys chat [SESSION_ID]              (interativo; /exit, /history, /task ID, /cancel ID)
  narys send SESSION_ID               (texto via stdin, até 4096 bytes; nunca repete)
  narys provider ID disable | enable --confirm-free
  narys policy show | set             (set: política JSON via stdin)
  narys approval ID approve-once | deny
Acrescente --json para resposta IPC estruturada. Unlock requer shell SSH
interativo autenticado, TTY privado, sem redirecionamento. Não envie senha no chat.
Ctrl-C encerra o acompanhamento, preservando a tarefa admitida no servidor.
Após erro incerto, consulte tasks/session/events; não reenvie automaticamente.";
fn wire(v: Value) -> Result<Command, &'static str> {
    serde_json::from_value(v).map_err(|_| "invalid_command")
}
fn number(s: &str) -> Result<u64, &'static str> {
    s.parse().map_err(|_| "invalid_number")
}
fn input(max: usize) -> Result<String, &'static str> {
    let mut bytes = vec![];
    std::io::stdin()
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "input_read_failed")?;
    if bytes.len() > max {
        return Err("input_limit");
    }
    String::from_utf8(bytes).map_err(|_| "input_not_utf8")
}
// Untrusted stored/provider text must not inject terminal control sequences.
pub fn safe(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}
fn human(v: &Value, indent: usize) {
    let pad = " ".repeat(indent);
    match v {
        Value::Object(obj) => {
            for (k, v) in obj {
                match v {
                    Value::Array(_) | Value::Object(_) => {
                        println!("{pad}{}:", safe(k));
                        human(v, indent + 2);
                    }
                    _ => println!(
                        "{pad}{}: {}",
                        safe(k),
                        safe(
                            v.as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| v.to_string())
                                .as_str()
                        )
                    ),
                }
            }
        }
        Value::Array(rows) => {
            for row in rows {
                println!("{pad}—");
                human(row, indent + 2);
            }
        }
        _ => println!("{pad}{}", safe(&v.to_string())),
    }
}
fn output(v: &Value, structured: bool) {
    if structured {
        println!("{}", serde_json::to_string_pretty(v).unwrap());
    } else if v["ok"] == true {
        human(&v["data"], 0);
    } else {
        human(v, 0);
    }
}
async fn call(command: Command, structured: bool) -> Result<Value, &'static str> {
    let v = client::request(command).await?;
    output(&v, structured);
    if v["ok"] != true {
        return Err("core_operation_refused");
    }
    Ok(v["data"].clone())
}
async fn credential_preflight() -> Result<(), &'static str> {
    let v = client::request(Command::Credentials {}).await?;
    if v["ok"] != true {
        return Err("credential_status_unavailable");
    }
    if v["data"]["login_unlocked"] != true {
        return Err("credentials_locked_or_unavailable_use_narys_credentials_unlock");
    }
    Ok(())
}
async fn wait(
    id: u64,
    namespace: &str,
    timeout: u64,
    structured: bool,
) -> Result<(), &'static str> {
    let end = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout);
    let mut last = String::new();
    loop {
        let v = client::request(wire(
            json!({"operation":"task-get","task":{"namespace":namespace,"id":id}}),
        )?)
        .await?;
        if v["ok"] != true {
            output(&v, structured);
            return Err("core_operation_refused");
        }
        let state = v["data"]["state"].as_str().unwrap_or("unknown");
        if !matches!(state, "pending" | "running" | "prepared") {
            output(&v, structured);
            return Ok(());
        }
        if !structured && last != state {
            eprintln!("Tarefa {namespace}:{id}: {}", safe(state));
            last = state.into();
        }
        if tokio::time::Instant::now() >= end {
            return Err("wait_timeout_task_preserved");
        }
        tokio::select! { _=tokio::signal::ctrl_c()=>return Err("wait_interrupted_task_preserved"), _=tokio::time::sleep(std::time::Duration::from_millis(700))=>{} }
    }
}
// Tokio owns SIGINT while following tasks. During the synchronous TTY read,
// restore normal terminal interruption so a previous follow cannot trap Ctrl-C.
struct TerminalInterrupt(libc::sighandler_t);
impl Drop for TerminalInterrupt {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.0);
        }
    }
}
fn line(prompt: &str) -> Result<Option<String>, &'static str> {
    let previous = unsafe { libc::signal(libc::SIGINT, libc::SIG_DFL) };
    if previous == libc::SIG_ERR {
        return Err("terminal_signal_unavailable");
    }
    let _interrupt = TerminalInterrupt(previous);
    print!("{prompt}");
    std::io::stdout()
        .flush()
        .map_err(|_| "terminal_write_failed")?;
    let mut s = String::new();
    let n = std::io::stdin()
        .lock()
        .take(4098)
        .read_line(&mut s)
        .map_err(|_| "terminal_read_failed")?;
    if n == 0 {
        return Ok(None);
    }
    if n > 4097 || !s.ends_with('\n') {
        return Err("input_limit_or_incomplete");
    }
    Ok(Some(s.trim_end_matches(['\r', '\n']).into()))
}
async fn chat(selected: Option<&str>) -> Result<(), &'static str> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("interactive_terminal_required_use_send");
    }
    credential_preflight().await?;
    let id = if let Some(id) = selected {
        number(id)?
    } else {
        let mut after = 0;
        loop {
            let page = call(
                wire(json!({"operation":"sessions","after":after,"limit":20}))?,
                false,
            )
            .await?;
            let choice = line("Sessão (ID existente ou new; more para próxima página): ")?
                .ok_or("chat_closed")?;
            if choice == "more" {
                if page["has_more"] == true {
                    after = page["next_session"].as_u64().ok_or("response_invalid")?;
                } else {
                    eprintln!("Última página; escolha um ID ou new.");
                }
                continue;
            }
            if choice == "new" {
                break call(Command::SessionCreate {}, false).await?["session_id"]
                    .as_u64()
                    .ok_or("response_invalid")?;
            }
            match number(&choice) {
                Ok(id) => break id,
                Err(_) => eprintln!("Escolha um ID, new ou more."),
            }
        }
    };
    call(
        wire(json!({"operation":"session-resume","session_id":id}))?,
        false,
    )
    .await?;
    println!("Sessão {id}. /exit sai; /history consulta; Ctrl-C preserva execução.");
    while let Some(text) = line("Você> ")? {
        if text == "/exit" {
            break;
        }
        if text == "/history" || text.starts_with("/history ") {
            let after = text
                .strip_prefix("/history ")
                .map(number)
                .transpose()?
                .unwrap_or(0);
            call(
                wire(json!({"operation":"session-get","session_id":id,"after_message":after}))?,
                false,
            )
            .await?;
            continue;
        }
        for (prefix, op) in [("/task ", "task-get"), ("/cancel ", "task-cancel")] {
            if let Some(t) = text.strip_prefix(prefix) {
                call(
                    wire(json!({"operation":op,"task":{"namespace":"product","id":number(t)?}}))?,
                    false,
                )
                .await?;
            }
        }
        if text.starts_with('/') {
            if !text.starts_with("/task ") && !text.starts_with("/cancel ") {
                eprintln!("Comandos: /exit, /history [cursor], /task ID, /cancel ID");
            }
            continue;
        }
        if text.trim().is_empty() {
            continue;
        }
        credential_preflight().await?;
        let receipt = call(
            wire(json!({"operation":"conversation","session_id":id,"text":text}))?,
            false,
        )
        .await?;
        let task = receipt["task_id"].as_u64().ok_or("response_invalid")?;
        eprintln!("Recibo durável: product:{task}. Recuperação: narys task {task}");
        wait(task, "product", 300, false).await?;
    }
    Ok(())
}
pub async fn run(mut args: Vec<String>) -> Result<(), &'static str> {
    let structured = args.iter().any(|a| a == "--json");
    args.retain(|a| a != "--json");
    if args.is_empty() || args == ["help"] || args == ["--help"] {
        println!("{HELP}");
        return Ok(());
    }
    let mut after = 0;
    let mut limit = 50;
    let mut namespace = "product".to_string();
    let mut timeout = 300;
    let mut waiting = false;
    let mut confirm = false;
    let mut pos = vec![];
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--after" | "--limit" | "--namespace" | "--timeout" => {
                let key = &args[i];
                i += 1;
                let val = args.get(i).ok_or("option_value_required")?;
                match key.as_str() {
                    "--after" => after = number(val)?,
                    "--limit" => limit = number(val)?,
                    "--timeout" => timeout = number(val)?,
                    _ => namespace = val.clone(),
                }
            }
            "--wait" => waiting = true,
            "--confirm-free" => confirm = true,
            a if a.starts_with("--") => return Err("unknown_option"),
            _ => pos.push(args[i].clone()),
        }
        i += 1;
    }
    if limit > u16::MAX as u64
        || timeout == 0
        || timeout > 3600
        || !matches!(namespace.as_str(), "product" | "lr10a")
    {
        return Err("invalid_option");
    }
    let p: Vec<&str> = pos.iter().map(String::as_str).collect();
    for option in args.iter().filter(|a| a.starts_with("--")) {
        let allowed = match option.as_str() {
            "--namespace" => matches!(p.first(), Some(&"task" | &"tasks" | &"cancel")),
            "--wait" | "--timeout" => p.first() == Some(&"task"),
            "--after" | "--limit" => {
                matches!(p.first(), Some(&"sessions" | &"tasks" | &"events"))
                    || (p.len() == 2 && p[0] == "session" && number(p[1]).is_ok())
            }
            "--confirm-free" => p.len() == 3 && p[0] == "provider" && p[2] == "enable",
            _ => false,
        };
        if !allowed {
            return Err("option_not_applicable");
        }
    }
    let command = match p.as_slice() {
        ["status"] => Command::Status {},
        ["agent", "status"] => Command::AgentStatus {},
        ["agent", "stop"] => Command::AgentRuntimeStop {},
        ["agent", "recover"] => Command::AgentRuntimeRecover {},
        ["agent", "new"] => Command::AgentSessionCreate {},
        ["agent", "get" | "resume" | "attach" | "close", reference] => {
            wire(json!({"operation":format!("agent-session-{}",p[1]),"session_ref":reference}))?
        }
        ["agent", "detach", attachment] => Command::AgentSessionDetach {
            attachment_id: (*attachment).into(),
        },
        ["capabilities"] => Command::Capabilities {},
        ["providers"] => Command::Providers {},
        ["models"] => Command::Models {},
        ["credentials", "status"] => Command::Credentials {},
        ["credentials", "unlock"] => return crate::credentials::unlock(structured),
        ["doctor"] => {
            let mut checks = serde_json::Map::new();
            for (name, command) in [
                ("core", Command::Status {}),
                ("credentials", Command::Credentials {}),
                ("capabilities", Command::Capabilities {}),
            ] {
                checks.insert(
                    name.into(),
                    client::request(command)
                        .await
                        .unwrap_or_else(|e| json!({"ok":false,"error_code":e})),
                );
            }
            let healthy = checks["core"]["ok"] == true;
            output(
                &json!({"version":ipc::VERSION,"ok":true,"data":{"checks":checks,"core_reachable":healthy,"unlock_transport":"private_authenticated_ssh_tty","remote_inference":false}}),
                structured,
            );
            return if healthy {
                Ok(())
            } else {
                Err("diagnostic_failure_reported")
            };
        }
        ["sessions"] => wire(json!({"operation":"sessions","after":after,"limit":limit}))?,
        ["session", "new"] => Command::SessionCreate {},
        ["session", op, id] if *op == "resume" || *op == "close" => {
            wire(json!({"operation":format!("session-{op}"),"session_id":number(id)?}))?
        }
        ["session", id] => wire(
            json!({"operation":"session-get","session_id":number(id)?,"after_message":after,"limit":limit}),
        )?,
        ["tasks"] => {
            wire(json!({"operation":"tasks","namespace":namespace,"after":after,"limit":limit}))?
        }
        ["task", id] if waiting => return wait(number(id)?, &namespace, timeout, structured).await,
        ["task", id] | ["cancel", id] => wire(
            json!({"operation":if p[0]=="task"{"task-get"}else{"task-cancel"},"task":{"namespace":namespace,"id":number(id)?}}),
        )?,
        ["events"] => wire(json!({"operation":"events","after":after,"limit":limit}))?,
        ["chat"] if !structured => return chat(None).await,
        ["chat", id] if !structured => return chat(Some(id)).await,
        ["send", id] => {
            let text = input(4096)?;
            credential_preflight().await?;
            wire(json!({"operation":"conversation","session_id":number(id)?,"text":text}))?
        }
        ["provider", id, action] if *action == "enable" || *action == "disable" => wire(
            json!({"operation":"provider-configure","provider_id":id,"enabled":*action=="enable","free_tier_confirmed":confirm}),
        )?,
        ["policy", "show"] => Command::Providers {},
        ["policy", "set"] => {
            let policy: Value =
                serde_json::from_str(&input(8192)?).map_err(|_| "invalid_policy_json")?;
            wire(json!({"operation":"conversation-policy","policy":policy}))?
        }
        ["approval", id, decision] if *decision == "approve-once" || *decision == "deny" => wire(
            json!({"operation":"approval","approval_id":id,"decision":if *decision=="deny"{"deny"}else{"approve_once"}}),
        )?,
        _ => return Err("invalid_command_use_narys_help"),
    };
    call(command, structured).await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn stored_text_cannot_control_terminal() {
        assert_eq!(super::safe("a\x1b[31m\x07"), "a\\u{1b}[31m\\u{7}");
    }
}
