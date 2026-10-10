# LR-10A — SDK & Runtime Feasibility POC

**Estado atual:** **PASS FINAL em 10/10/2026 após auditoria independente**, com escopo operacional host-assisted documentado; a matriz abaixo é o protocolo original, preservado historicamente.  
**Trilha mãe:** [LR-10 — GitHub Copilot SpecialistAgent](LR-10-COPILOT-SPECIALIST-AGENT.md).  
**Objetivo:** decidir, por evidência, se a integração Rust SDK + Copilot CLI é suportada no Fedora/Tauri da Narys **sem antecipar permissões de filesystem/shell**.

## 1. Condições de entrada

- Branch de implementação criada apenas quando a LR-10A for iniciada, a partir do SHA atual confirmado da `main`; sem commit de runtime/dep no registro documental.
- Congelar versão da crate e CLI usada no experimento. Referência investigada em 09/10/2026: SDK release `v1.0.18`; `rust-version=1.94.0` no crate Rust upstream. A versão exata publicada/resolvida em Cargo deve ser apurada, não presumida.
- Validar Rust/Cargo efetivos do ambiente, `rustc --version` / `cargo --version`, `cargo tree`; Narys declara `rust-version="1.77.2"` e `edition="2021"` hoje.
- Criar workspace **temporário e descartável**, sem secrets, sem repos de produção, sem push remoto e sem código real do usuário como alvo de efeitos durante a POC.
- Identidade autenticada existente via CLI/credential store; nenhuma leitura ou impressão de tokens; se ausente, registrar BLOCKED_AUTH e instruções ao usuário.
- Quota real não consumida por probes de versão, handshake e metadata sempre que possível; experimento de modelo somente com autorização separada, orçamento e registro de uso.

## 2. Matriz POC (preencher com observação, não com suposição)

| ID | Experimento | Prova esperada | Estado inicial |
| --- | --- | --- | --- |
| A0 | Baseline e pin de SDK/CLI | SHA, versões, provenance e feature flags | NOT_RUN |
| A1 | MSRV + Cargo/Tauri | cargo check/test/build no toolchain >=1.94; Edition 2021 da app mantida | NOT_RUN |
| A2 | Bundled vs runtime unbundled | cold-start, bytes/download, processo, transitive deps, caminho CLI explícito | NOT_RUN |
| A3 | Auth safe + entitlement | autenticado/erro codificado sem raw credentials e sem inferência | NOT_RUN |
| A4 | Model catalog / Auto | metadata real, flags e seleção, sem inventar modelos/limites | NOT_RUN |
| A5 | Account quota + signals | `account.getQuota` tipado, null/failure/refresh distinguidos | NOT_RUN |
| A6 | Session/create, events, resume | sessão opaca, ordering/correlation, bounded subscription, lifecycle | NOT_RUN |
| A7 | Cancel/stop/reap | abort antes/entre chamadas, cleanup, zero orphan, stop bounded | NOT_RUN |
| A8 | Hardware/perf/headless | cold start, RSS/CPU/processes, build size, sem abrir UI/3D | NOT_RUN |
| A9 | Teste mínimo real autorizado | prompt read-only em fixture e usage observado, sem efeitos | NOT_RUN |
| A10 | Security feasibility | permission handlers, hooks, tool surfaces, CLI sandbox verificável; **sem executar YOLO real** | NOT_RUN |

## 3. Contrato dos testes

- A0–A8/A10 preferencialmente usam mocks/fakes + CLI local para metadata, sem inferência. A9 só com autorização expressa do usuário para consumo de quota; capturar modelos e unidades.
- Reaproveitar infraestrutura de testes nativos/Rust do projeto. Sem criação de IPC genérico nem elevação de execution authority na A.
- Falhas de rede/entitlement/SDK e ausência de CLI devem ter códigos separados. `available` nunca é confundido com prova de chamada bem-sucedida.
- Reexecutar regressão relevante da Narys e medir delta debug/release vs baseline, não aceitar `cargo check` isoladamente como prova de compatibilidade.
- `Client::start` pode depender da feature `runtime`; `default-features=false` puro suporta streams externos, não runtime gerenciado. Com `runtime` e CLI não bundled, fornecer `CliProgram::Path` validado: resolver a partir de `PATH` **não é garantia do SDK Rust**.
- `Client::stop()` tenta cleanup gracioso; falha/timeout exige verificação de processos e relatório, não sucesso presumido. Separar `session.idle` de `task_complete` e sucesso de testes.

## 4. Saídas de evidência

Registrar, sem segredos:
- tabela de versões (Narys main SHA, crate version + crate features, CLI --version, rustc/cargo, OS);
- comandos e resultados dos testes, Cargo lock diff, falhas/debug/release;
- bundle size/disk/RSS/CPU/cold start/idle e stop/reap, com metodologia e número de amostras;
- catálogo de capabilities observado, quota e unidades reais ou `unavailable`;
- exemplo sanitizado da sequência de eventos/IDs, cancelamento e resume;
- decisão de boundary potencial (Broker-mediated? SDK hooks + sandbox? supervisor externo?) **sem alegar solução de segurança antes da LR-10C**.

Se A9 não puder rodar por falta de credenciais/quota, registrar `BLOCKED_REAL` e **não marcar POC real PASS** com base só em mocks.

## 5. Decisão de saída

**GO** somente se auth, gerenciamento de processo, capabilities e sessão forem viáveis, MSRV/toolchain/build ficarem comprovados e riscos C/D permanecerem tratáveis.  
**FIX-AND-RETEST** diante de lacunas corrigíveis: CLI/SDK incompatível, feature, cancelamento, lifecycle, artefatos oversized.  
**NO-GO** se impossibilidade material de invocar runtime ou de implantar política agentiva verificável sem riscos inaceitáveis.

Registrar especificamente se manter `edition = "2021"` com dependência Edition 2024 é validado; só então alterar `rust-version` no Cargo.toml de Narys no commit funcional autorizado. **Este documento não altera Rust, quotas nem política operacional por si só.**

Referências: [Rust SDK](https://github.com/github/copilot-sdk/blob/main/rust/README.md), [setup local do CLI](https://github.com/github/copilot-sdk/blob/main/docs/setup/local-cli.md), [limites de sessão](https://github.com/github/copilot-sdk/blob/main/docs/features/session-limits.md), [SDK hooks](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/hooks), [Copilot CLI perms](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli/allowing-tools).

## 6. Resultado final auditado — 10/10/2026

**Decisão GO / PASS_FINAL para a LR-10A exclusivamente.** Cold boot Fedora sem GNOME, Core Rust serviço systemd, Keyring existente desbloqueado manualmente em SSH privado, Stronghold aberto sem migração, SDK Rust1.0.17/CLI1.0.95 com auth/catálogo Auto/quota, sessão criada, uma tarefa textual real concluída com saída5/arquivo/TaskGraph/SQLite e sessão genuinamente retomada sem inferência adicional. Shutdown/cleanup completo e contrato anti-replay. Evidências: [relatório consolidado](LR-10-LATEST-EXECUTION-REPORT.md), [decisão de auditoria e dívidas](LR-10A-FINAL-CLOSURE-2026-10-10.md).

A0/A2/A3–A6/A9 passaram no escopo host-assisted real; A7 cancel/timeout em fixtures e shutdown real; A8 medição proporcional no Fedora, não benchmark frio longo; A10 DenyAll/zero tools e ausência de concessão HumanLocal, **não sandbox de produção**. A1 validou build na toolchain real Rust1.98.1/Edition2021 e compatibilidade de integração isolada; MSRV1.94 específico não foi novamente compilado. O modo `A9_ISOLATED` permanece negado e pertence à LR-10C, não foi reclassificado pela execução host-assisted.

**67 testes PASS** (35 Rust, 1 Unix, 6 Python e 25 ownership), uma inferência real concluída, duas tentativas remanescentes revogadas. O limite soft de AI Credits foi removido por incompatibilidade observada durante `session.create`; isso não significa autorização de cobrança adicional. Não extrapolar resultados para edição/shell, UI integrada, novos providers ou quota USD. LR-10B–F **pausadas** até depois de [NARYS-SERVER-1](NARYS-SERVER-1-HEADLESS-SERVER-RUNTIME.md), com prioridade e prazo máximo de 48h após início.
