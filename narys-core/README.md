# CLI operacional SERVER-1C

Use `narys` no shell SSH do host. [Guia completo](CLI.md): instalação sem sudo,
chat/sessões/tarefas/eventos/providers/modelos, JSON, configuração e desbloqueio
humano privado integrado. O comando e o diagnóstico/desbloqueio de credenciais
não precisam dos scripts do checkout. O Core continua o único proprietário do
estado; fechar o SSH não cancela tarefa admitida. SERVER-1D não iniciada.

# Narys Core — servidor modular (SERVER-1A)

Fundação operacional consolidada em `narys-domain`, independente de Tauri.
Core é dono de SQLite, recovery, leases e ciclo de vida; desktop legado usa os
mesmos contratos e fica bloqueado após takeover até migrar seus comandos ao IPC.
Conversation/provedores estão extraídos, mas ainda não conectados ao IPC do
servidor: integração e gate real pertencem à SERVER-1B. Ferramentas agentivas
continuam indisponíveis. [Relatório único](../docs/NARYS-SERVER-1A-REPORT.md).

SQLite autoritativo: `~/.local/state/narys/core/db/luna.sqlite3`. Primeiro boot
consolida snapshot da base desktop + tarefas LR-10A, com backups privados e
migration019 aditiva. IDs permanecem nos namespaces de origem; recibos não são
alterados. Nenhuma migração de Stronghold. Não iniciar binário desktop antigo
que desconheça a fence; backups não são autorização para rollback destrutivo.

Systemd --user com readiness notify, single-writer lease, socket0600 e peer UID,
limites de IPC, shutdown e restart sem replay. [Contrato v1](IPC.md).

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 \
  CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  /usr/bin/cargo test --offline --locked --manifest-path narys-core/Cargo.toml
~/.local/lib/narys/narys-core capabilities
~/.local/lib/narys/narys-core status
~/.local/lib/narys/narys-core events
```

## Registro histórico LR-10A (autorização encerrada)

**Validação integrada real concluída:** Core após cold boot sem GNOME, credenciais
existentes desbloqueadas manualmente, Copilot SDK textual, resposta `5`, resultado
persistido e TaskGraph completed. A mesma conversa foi retomada em outro runtime,
sem novo prompt. [Relatório para auditoria](../docs/LR-10-LATEST-EXECUTION-REPORT.md).
Esta é uma implementação candidata; não concede aprovação de produção ou LR-10B.

## Composição e limites

Core independente de Tauri/GTK/WebKit/X11/Wayland, residente sob systemd --user.
O baseline histórico reutilizava AgentBackend/Registry, PlanV1/TaskGraph, TaskId, OperationalTraceBus
limitado e migrações SQLite. Banco próprio em `~/.local/state/narys/core/db`;
interrupção não autoriza reenvio. Factory gráfica Codex, ExecutionBroker,
ExecutionAuthority/HumanLocal, IPC, Scheduler e políticas LR-8.5 preservados.

Controle local por socket Unix0600/SO_PEERCRED mesmo UID; sem porta TCP. Perfil
**HOST_ASSISTED_NOT_SANDBOX**: o usuário Linux é a fronteira de confiança; não
protege de processos do mesmo UID nem isola filesystem/rede do runtime. Copilot
não recebe ferramentas, shell, edição ou autoridade agentiva. O Core escreve
somente o resultado no workspace privado autorizado, por código confiável.

SDK Rust1.0.17 (runtime, sem bundle), CLI nativo1.0.95 com SHA verificado a cada
invocação. CLI sob demanda; shutdown e harness FIX1 (subreaper/pidfds/ECHILD)
separados. Morte do supervisor/descendentes adversariais ainda não representam
contenção comprovada. Unidade possui cgroup próprio e parada limitada270s;
Keyring permanece separado. Limites catálogo30s, inferência120s, harness240s.

## Compatibilidade comprovada

O limite opcional `sessionLimits.maxAiCredits=0.5` fazia `session.create` retornar
RPC -32603, categoria pública credits. Sem esse parâmetro, create e patch
`session.options.update` funcionaram. O motivo interno exato do serviço de
credits não foi exposto/atribuído. O limite é soft, não garantia de custo; os
controles financeiros obrigatórios e o guard de envios são independentes.

`--no-custom-instructions` é suportado pelo CLI pinado e complementa o patch
skipCustomInstructions=true. Mantidos DenyAll, availableTools=[], MCP{}, hooks,
skills, discovery, extensões e host Git desabilitados; estado configDir privado.
Infinite sessions/compaction automática desabilitadas. enable_session_store=false
não impediu transcript real/resume (não confundir índice com flush de conversa).

`RES_OPTIONS=no-aaaa` permanece somente no runtime: contorna a rota IPv6 local
que expirava no provedor. Sem mudança global de resolver/firewall/proxy/TLS.
Não restringe destinos, e tem limitação DNSSEC pela aplicação, conforme
[glibc](https://sourceware.org/glibc/manual/latest/html_node/Resolver-Options.html).

## Instalação e operação SSH

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 \
  CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  /usr/bin/cargo test --offline --locked --manifest-path narys-core/Cargo.toml
python3 -m unittest discover -s narys-core/tests -p 'test_*.py'
```

Host já instalado. `ops/install_user.py` prepara somente unidades de usuário;
recusa sobrescrever instalações divergentes. Atualização usa
`ops/update_user.py SHA_BINARIO_INSTALADO SHA_UNIDADE_REVISADA`, parando somente
narys-core.service. Scripts dependem desta checkout/caminho. Linger e boot
multi-user foram configurados pelo usuário; nenhum instalador altera boot/GDM,
PAM, Keyring, SSH ou serviços globais.

```sh
systemctl --user status narys-core.service
~/.local/lib/narys/narys-core status
~/.local/lib/narys/narys-core credentials
~/.local/lib/narys/narys-core unlock
~/.local/lib/narys/narys-core stronghold
~/.local/lib/narys/narys-core result 2
~/.local/lib/narys/narys-core events
```

**unlock somente em terminal SSH privado não capturado pela IA.** Helper H2/H3
fixado ao GNOME Keyring50.0 usa interface interna não suportada; coleção login
existente, libsecret DH/AES e senha /dev/tty sem eco/persistência. Não cria cofre,
copia tokens ou substitui senha. stronghold resolve normalmente a chave existente
e retorna status, sem valores/create_client/save/migração. Core funciona bloqueado
até desbloqueio humano; não há desbloqueio automático. Não é a composição completa
de todos os provedores/GUI/Android da Narys.

## Consentimento, tarefa e persistência

Consentimento final explícito do usuário: até3 sends, somatória mínima, franquia
existente, orçamento adicional desativado, nenhuma compra/overage autorizado.
Estado durável0700/0600 em `~/.local/state/narys/core/lr10a-final-authorization`:
consent fixo, receipt novo por task, slots O_EXCL+flock+fsync antes do send.
Crash/resultado incerto contam; replay/corrupção/limite/closure bloqueiam.
`record_final_consent.py` não sobrescreve consentimento;
`review_final_task.py` vincula somente task nova/objetivo exato, sem enviar.
Receipt expira30min. Antigo financial-review.json/task1 nunca reutilizados.

**Consumido1 de3, autorização encerrada após sucesso; não executar novamente.**
Prepare/submit/cancel/result são controles locais; prepare não envia. Nova operação
não ganha consentimento de auth, metadata, flags ou fixtures. O antigo helper
review_financial.py não admite novas submissões nesse contrato final. Marker A9
histórico não é criado/consumido. A API exige ainda auth, catálogo Auto elegível,
quota disponível e ambas flags de uso após esgotamento/overage false; missing/erro
bloqueiam. Requests legadas não são AI Credits nem teto USD. Uma chamada SDK pode
representar várias requisições internas; não há retry/fallback pago pela Narys.

Concluir depende de resposta final validada contra esperado humano, shutdown e
cleanup, result.txt0600/fsync/reread exato, grafo completed e SQLite persistido.
session.idle isolado não comprova sucesso. Eventos públicos e counters numéricos
sanitizados são evidência, sem reasoning/protocolos/credenciais publicados.
`resume-check ID` é one-shot, somente task completed e sessão única no estado
privado, sem create fallback/send. Task2 já foi verificada; não repetir o ensaio.
`copilot` consulta metadata; `session-check` diagnostica create/detach sem send;
não são autorização financeira ou agentiva.

[Evidência da tarefa](evidence/final-integrated-operation.json),
[resume genuíno](evidence/final-owned-session-resume.json),
[serviços finais](evidence/final-service-state.json). A falha histórica task1
continua [preservada](evidence/postboot-single-submission.json); não foi reclassificada.

SERVER-1B connects the original Groq, Gemini, Cloudflare and Mistral adapters to the
Core composition, using the existing Scheduler and read-only Stronghold. Conversation
is accepted asynchronously and persisted in the same authoritative SQLite database;
clients query product tasks and sessions after disconnect or restart. Unknown effects
are interrupted without replay. Adapters do not make requests at boot.

Use `providers`, `session-create`, `conversation <session> <text>`,
`task-get <id>`, `task-cancel <id>`, `session-get <session>` and typed stdin `ipc`.
See [IPC.md](IPC.md) for pagination, provider permissions and routing policy. Before
provider enablement, confirm that its existing account/model has free quota without
billing/overage. Credentials stay in Stronghold; no key values are accepted by IPC.
The complete SSH/Termux CLI belongs to SERVER-1C. Agent tools/approvals remain gated.
