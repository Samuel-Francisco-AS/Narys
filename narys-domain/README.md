# Narys domain

Uma única implementação Rust reutilizável de Conversation/Context, Scheduler,
ProviderRegistry/provedores, políticas LR-8.5, TaskGraph/recovery/persistência,
AgentRegistry/Codex planner, trace e ExecutionBroker. Sem dependência de Tauri,
GTK, WebKit, X11 ou Wayland. Tauri mantém apresentação, comandos e adaptação de
`Channel<TaskEvent>`; Core compõe os mesmos serviços sob systemd.

`channel` oferece subscribers tipados, sem conceder autoridade. `runtime` usa
Tokio do host quando disponível e um runtime lazy para entradas síncronas.
`snapshot` retém o wrapper Stronghold upstream com licença e comportamento
originais, usando iota_stronghold diretamente. Testes usam cofres sintéticos;
nenhuma migração ou leitura do Stronghold pessoal é feita pela extração.
Core compõe `SecretStore::existing`, que recusa criação, chmod, writes e migração;
provê leitura sob o contrato Keyring/Stronghold existente somente por demanda.

`Database` conserva uma lease de escritor por toda a vida do handle e clones.
Outro processo é recusado; handles do mesmo processo compartilham a lease. Connections não devem sobreviver
ao handle proprietário; as composition roots conservam Database. A migração
SERVER-1A instala uma fence na base desktop: `desktop_migrated_use_core_ipc`.
O desktop legado não funciona operacionalmente depois do takeover; precisa
consumir o serviço. Dados e backups permanecem preservados.

`desktop-tests` é exclusivamente suporte dos testes downstream do desktop.
Não habilitar em builds de serviço/produção. Fixture authority continua vinculada
a TestFixture e programa fixo; IPC não serializa ExecutionAuthority. HumanLocal
permanece restrita ao terminal humano nativo. WorkspaceScope valida cwd, sem
promessa de sandbox.

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  cargo test --offline --locked --manifest-path narys-domain/Cargo.toml --lib -- --test-threads=2
```

SERVER-1B acrescenta o adapter durável `start_durable_conversation` ao mesmo engine
Conversation: TaskRegistry, Scheduler e providers continuam únicos. A persistência
`conversation_runs` compõe admissão/resultado/recovery com mensagens, TaskRecord e
eventos no banco autoritativo. `SecretStore::existing` utiliza no Linux a leitura do
backend Secret Service sem Unlock/Prompt e sem reconstruir chave ou snapshot.
