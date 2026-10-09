# NARYS — LR-10A FIX-3 — Security Boundary & A9 Readiness

## 1. Identificação

- Execução: 2026-10-09, Fedora 44 via SSH, sem interface gráfica.
- Branch: `lr-10a-sdk-runtime-feasibility`.
- Base local/remota verificada: `25327feb88207a465945453d32dba11c10538e1c`.
- Main local/remota verificada: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`.
- Commit da implementação testada: identificado no fechamento documental abaixo.
- HEAD documental final/remoto: referência verificável no [histórico da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility). O SHA do próprio commit documental não é inserido circularmente neste arquivo.
- Estado: **LR-10A FIX-3 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE**.
- Recomendação: **FIX-AND-RETEST; A9 BLOCKED**, sem PASS definitivo da LR-10A.

O workspace inicial estava limpo; HEAD esperado e remoto coincidiram. Nenhuma alteração humana foi descartada. Sem merge, rebase, reset, force-push, PR ou alteração da main.

## 2. Objetivo e escopo

Substituir a exposição read-only praticamente integral do host por uma fronteira experimental de visibilidade mínima; verificar controles por canários sintéticos e fixtures do SDK. Preparar uma especificação inerte de A9 pelo SDK Rust.

Somente experimentos e documentação foram alterados. Não se implementaram supervisor de produção, approval engine, sandbox LR-10C, autoridade agentiva, integração operacional ou inferência. SDK, CLI, lockfile, toolchain e dependências de produção foram mantidos. FIX-1 e a matriz/lógica de persistência da FIX-2 foram preservadas; a guarda de filesystem foi substituída deliberadamente, e os modos de autenticação pessoal agora bloqueiam antes de iniciar o Client.

## 3. Modelo de ameaças

A fronteira cobre um runtime experimental potencialmente capaz de executar código, não apenas um modelo obediente. O launcher/harness, binários fixados e fontes locais controladas são confiáveis; um processo host do mesmo usuário que altere maliciosamente esses recursos não é contido por esta POC. Autoridade humana futura não pode ser derivada de textos da fixture.

| Ameaça / superfície | Controle proposto e implementado | Teste / resultado observado | Limitação residual |
| --- | --- | --- | --- |
| 1. Leitura de arquivos pessoais: mounts, HOME, descoberta ambiente | Root vazio; somente ELF/libraries individuais e diretórios privados; nenhum home/repositório pessoal montado | G2: canário externo e raízes pessoais invisíveis, PASS kernel | Não é prova contra vulnerabilidades do kernel/runtime nem contra alteração host por mesmo UID |
| 2. Escrita externa: ferramentas, root e fixture | Root/fixture/libs RO; somente state/logs/tmp privados RW | G1/G2: escrita autorizada funciona, fixture/root/canário externo negados, PASS | Código ainda pode escrever em áreas privadas autorizadas; não é sandbox de produção |
| 3. SSH/Git/keyring/tokens/config | Sem homes, gh/git/ssh, bus ou credential files; keytar desativado; seccomp nega key APIs | G2/G5/G7: roots ausentes, keyctl retorna EPERM, socket canário inacessível, PASS; auth real ausente | Disponibilizar bus/keyring poderia expor serviços além de uma credencial; não realizado |
| 4. Segredos no ambiente / FDs | `--clearenv`, allowlist fixa, somente stdio/filter FD herdados | G4: 18 nomes sensíveis ausentes, inclusive tokens/BYOK, PASS; plano não monta sockets | Trusted host SDK/launcher fica fora da fronteira; nenhuma coleta de environ nem valores |
| 5. Shell/subprocessos | Zero tools/DenyAll, shell inexistente/PATH inválido, namespaces/caps/seccomp, harness | G5: shell ausente; wire reject shell/write/unknown, PASS fixture; G8 timeout PASS | fork/exec/self e código gerado dentro da área privada não são globalmente proibidos; internos do CLI não passam pelo Broker |
| 6. MCP/extensões/skills | builtin MCP desativado, servers/maps vazios, requestExtensions/McpApps false, discovery/hooks/skills desligados | G6: flags wire create/resume + update reconhecido na fixture, PASS; CLI aceita criação vazia | Nenhuma ferramenta agentiva real foi acionada; cobertura completa dos internos CLI permanece BLOCKED |
| 7. Symlinks/traversal | Fontes de mounts canônicas, sem symlink, owned; fixture RO dentro do root vazio | G3/G10: escape link/traversal e mount source symlink negados, PASS | Sem garantia contra corrida adversarial de outro processo host mesmo UID entre check e mount |
| 8. Dispositivos/sockets/serviços | Só null/urandom, PID proc próprio RO, sem D-Bus/socket/FD host | G5/G7: dispositivos mínimos, proc reduzido, canário Unix inacessível, PASS | Stdio é canal deliberado do SDK; vulnerabilidades do kernel não foram testadas |
| 9. Rede/exfiltração | `--unshare-all`, rede privada offline, sem resolver/certs/proxies | G1–G9 kernel: TCP canário do host inacessível, PASS offline | **BLOCKED_NETWORK_BOUNDARY**: não existe egress por destino nem separação provider/subprocessos; offline não serve para A9 |
| 10. Descendentes/timeouts/cleanup | FIX-1: subreaper privado, ownership/PPID/start-time, pidfds, ECHILD | G8 e 25 testes herdados: normal/timeout/setsid/double-fork/crash/external control, PASS | Morte real do worker, descendentes adversariais, reparenting/namespaces não cobertos e tarefas kernel irrecuperáveis não têm garantia |
| 11. Logs/traces/protocolo | LogLevel None, logs privados, códigos fixos, resumos de flags/contagens, sem raw RPC/stderr | G9: marcadores de canários/payloads ausentes nos JSONs, PASS; testes de classificação herdados PASS | Redação não é um sanitizador universal para futuras novas APIs; novos campos exigem auditoria |
| 12. Escalonamento indireto por config/instruções | Empty mode; env allowlist; no discovery/plugins/custom dirs/hooks; root mínimo; sem fallback | G6/G10/G12: opções desconhecidas/capacidade ausente/prompt/YOLO/Autopilot rejeitados, PASS | Sucesso do RPC não prova enforcement de todo flag no CLI antigo; sem inferência, cobertura fica limitada |

## 4. Arquitetura de isolamento

[boundary.py](../experiments/lr-10a-sdk-runtime/boundary.py) gera/valida o plano e execve Bubblewrap. [src/boundary.rs](../experiments/lr-10a-sdk-runtime/src/boundary.rs) aplica-o ao Client do SDK. A árvore começa em tmpfs vazio, nunca em `--ro-bind / /`.

Mounts efetivos:

- `/runtime/program`: somente CLI nativo fixado ou o ELF sintético controlado, RO.
- Oito arquivos individuais de sistema para o CLI: loader, libc, libdl, libgcc_s, libm, libpthread, libstdc++ e libutil; nenhum diretório inteiro de libraries. Manifesto e hashes em [fix-3-boundary-manifest.json](../experiments/lr-10a-sdk-runtime/evidence/fix-3-boundary-manifest.json).
- `/fixture` RO; `/state` e `/logs` RW dentro de uma única raiz owned em `/tmp`, criada **0700**; troca de estado da FIX-2 altera apenas o source privado.
- `/tmp` tmpfs; `/proc` do PID namespace e remount RO; `/dev` tmpfs com somente `/dev/null` e `/dev/urandom`.
- Root remount RO, capabilities ALL removidas, nova sessão, die-with-parent, user/PID/IPC/UTS/network namespaces e nested userns desativado.
- Seccomp obrigatório x86_64: rejeita outras ABIs/x32 e nega keyring, introspecção selecionada, mount/namespace, bpf/perf e io_uring; números conferidos no UAPI público instalado. **Denylist, não allowlist universal**.

Identidade/pin são verificados antes de invocar ldd (inclusive em teste negativo com ELF não aprovado). Dependências são inspecionadas com ldd sobre ELF confiável e ambiente limpo, sem ler conteúdo privado. ldd não é closure completa: o primeiro startup protegido revelou dlopen de `libutil.so.1`; somente esse arquivo adicional foi montado. Não houve retorno ao filesystem amplo para corrigir a falha. Certificados, DNS, bus e serviços não foram necessários ao handshake offline e não foram expostos.

Falhas de arquitetura, layout/ownership, binário não ELF/pin incorreto, dependency/mount, Bubblewrap ou memfd bloqueiam a execução. Launcher só aceita os flags stdio fixados do SDK 1.0.17; login, prompts, plugins, YOLO e Autopilot não fazem parte da allowlist.

## 5. Política de autenticação

**BLOCKED_AUTH_BOUNDARY**. Nenhuma credencial foi lida/exportada/copiada; nenhum keyring/config pessoal foi montado. ClientMode Empty, base_directory owned, COPILOT_HOME virtual `/state`, `COPILOT_DISABLE_KEYTAR=1`, `use_logged_in_user=false` e `--no-auto-login`. Ambiente não permite tokens, BYOK, LD/Python injection, SSH ou D-Bus. Não houve login/logout/troca de conta.

[Metadata real](../experiments/lr-10a-sdk-runtime/evidence/fix-3-real-metadata.json): handshake do SDK/CLI, auth `authentication_required`, catálogo `models_unavailable/rpc_error_unknown`, quota `quota_unknown/rpc_error_unknown`. Isso descreve somente o processo sem credenciais/rede, não a conta pessoal. [Modo existing-auth](../experiments/lr-10a-sdk-runtime/evidence/fix-3-auth-boundary-blocked.json): `BLOCKED_AUTH_BOUNDARY`, exit 2 da POC, Client/CLI não iniciado.

A [documentação oficial atual de autenticação CLI](https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli) descreve OAuth, env tokens, keychain Linux/libsecret, eventual configuração plaintext e fallback gh; descreve fine-grained PAT com permissão de conta Copilot Requests. Não foi investigado qual armazenamento contém a credencial pessoal. Esse possível mecanismo de menor escopo requer escolha/auditoria/autorização futura; nenhum token foi criado ou fornecido aqui. Disponibilizar D-Bus inteiro não seria mediação estreita de uma única operação.

A [documentação SDK atual](https://github.com/github/copilot-sdk/blob/main/docs/auth/authenticate.md) não comprova todas as capacidades no par antigo. Na crate publicada 1.0.17, a inspeção de [lib.rs](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/lib.rs) mostrou Empty/base_directory/keytar/no-auto-login e suporte a token explícito, mas não uma capability OS que elimine a exposição da credencial ao próprio runtime. Fontes/hash/versionamento estão no manifesto; `.cargo_vcs_info` marca dirty=true, por isso a crate publicada é a referência efetiva, não uma suposição sobre a árvore Git upstream.

## 6. Ferramentas, MCP, permissões e rede

[session/resume config](../experiments/lr-10a-sdk-runtime/src/lib.rs): `available_tools=[]`, DenyAll, MCP map vazio, MCP apps false, extensions false, plugins/skills/instructions/custom agents/additional directories vazios, hooks/file hooks/Git/discovery/telemetry desligados. Os mesmos controles são reaplicados ao resume; nenhuma sessão pessoal é retomada.

[Testes SDK](../experiments/lr-10a-sdk-runtime/tests/security.rs) e [observações sanitizadas](../experiments/lr-10a-sdk-runtime/evidence/fix-3-permission-observations.jsonl): shell, write, unknown e managedApprovalRequired recebem **reject** pelo protocolo. SDK 1.0.17 resolve DenyAll antes do handler; handlers NoResult/panic/pending fornecidos como controles negativos não são invocados na configuração obrigatória. Sem policy, ausência de handler envia requestPermission=false; NoResult/panic/pending não fornecem reject no intervalo observado. Isso não prova aprovação nem timeout final pelo CLI: demonstra que esses caminhos não substituem negação explícita.

`skipCustomInstructions` é aplicado pelo SDK em **session.options.update após create/resume**, não no payload inicial. As fixtures usam o mesmo ClientMode Empty e verificam o update/ack, installedPlugins=[] e includedBuiltinSkills=[]. Quando rejeita essa capacidade obrigatória com -32601, create/resume falham e não entregam sessão utilizável; não há fallback. Uma primeira asserção de teste usava o campo wire errado (enableMcpApps em vez de requestMcpApps); corrigiu-se a fixture conforme fonte da crate, sem relaxar o controle.

Inspeção de [permission.rs](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/permission.rs), [handler.rs](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/handler.rs), [types.rs](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/types.rs) e [session.rs](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/session.rs), mais [hooks oficiais atuais](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/hooks), separa hooks/callbacks de isolamento kernel. Hooks de arquivo podem executar scripts internos no CLI; mantidos desabilitados. Não se criou engine de approvals.

Builtin shell/edição/view/Git e servidores MCP podem agir internamente no CLI, fora do Execution Broker. Ferramentas custom SDK poderiam ser mediadas por um Broker explícito futuro, mas nenhuma foi registrada aqui; negar permissões não redireciona ferramentas internas ao Broker. YOLO/Autopilot podem ampliar autonomia/permissões e são excluídos, sem teste real.

**BLOCKED_NETWORK_BOUNDARY**: offline isola o host e impede serviço remoto; não oferece política por destino nem separa conexões do runtime de ferramentas/subprocessos. Abrir share-net não é correção aceitável. Não se provou proxy estreito, egress filtrado ou TLS/DNS mínimo online. A9 continua bloqueado.

## 7. Matriz G1–G12

| Gate | Resultado | Evidência e limites |
| --- | --- | --- |
| G1 FS permitido | PASS kernel | Fixture legível/RO; state/logs privados writable; checks booleanos nos testes boundary |
| G2 FS proibido | PASS kernel | Canário externo não lido/modificado; homes/config/Git ausentes; root RO |
| G3 Symlink/traversal | PASS kernel | Link e traversal não expõem host; source symlink/layout externo rejeitados |
| G4 Ambiente | PASS kernel | 18 nomes sensíveis ausentes; valores nunca exportados; incluindo token/BYOK sintéticos herdados |
| G5 Ferramentas | PASS fixture + kernel parcial | SDK reject shell/write/unknown/managed; shell externo ausente; execução interna CLI completa BLOCKED sem inferência |
| G6 MCP/extensões | PASS configuração wire; BLOCKED cobertura real completa | Empty maps/dirs e flags create/resume/update comprovados na fixture; CLI real aceita sessão vazia, não se acionou MCP/tool real |
| G7 Autenticação | BLOCKED_AUTH_BOUNDARY | Handshake real PASS; auth required; personal-auth negada antes de iniciar Client; nenhum proxy/bus/credencial exposto |
| G8 Processos | PASS testes controlados; BLOCKED contenção adversarial | Normal/timeout/descendentes/kernel ownership/external preservation PASS; falha real do worker/limites adversariais não comprovados |
| G9 Logs/evidências | PASS escopo observado | Códigos/flags/contagens, sem canário/payload/env values/raw protocolo; artefatos JSON/JSONL parseados e revisados |
| G10 Falha segura | PASS kernel/fixture | Mount/dependência/binário/policy/arquitetura/capacidade/flags inválidos e options.update indisponível impedem operação; sem fallback |
| G11 Regressão | PASS | 40 Python (25 herdados + 15 novos), 26 Rust (22 herdados + 4 novos); FIX-1 e matriz FIX-2 preservadas |
| G12 A9 bloqueado | PASS estrutural | Sem SDK send/send_and_wait; modo a9 rejeitado antes de runtime; spec TXT inerte; zero inferências/tool calls agentivas reais |

PASS aqui é resultado limitado ao teste indicado, não PASS definitivo da fase nem READY_FOR_A9.

## 8. Comandos e testes executados

A partir de `experiments/lr-10a-sdk-runtime`, Rust/Cargo **1.94.0** isolados em `/tmp/narys-lr10a-rust-1.94.0/bin`; Edition 2021; SDK 1.0.17 `default-features=false`, `runtime`. CLI instalado permanece o nativo --version 1.0.91, RPC 1.0.90; referência da crate 1.0.93. Nenhuma versão foi atualizada, nenhuma nova compatibilidade online foi afirmada. Python 3.14.7, Bubblewrap 0.12.0, Linux 7.2.8-200.fc44.x86_64.

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 \
RUSTC=/tmp/narys-lr10a-rust-1.94.0/bin/rustc \
RUSTDOC=/tmp/narys-lr10a-rust-1.94.0/bin/rustdoc \
FIX3_SECURITY_EVIDENCE=evidence/fix-3-permission-observations.jsonl \
/tmp/narys-lr10a-rust-1.94.0/bin/cargo test --offline --locked -- --test-threads=2
# stdout/stderr arquivados em evidence/fix-3-rust-tests.txt

COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 \
RUSTC=/tmp/narys-lr10a-rust-1.94.0/bin/rustc \
RUSTDOC=/tmp/narys-lr10a-rust-1.94.0/bin/rustdoc \
/tmp/narys-lr10a-rust-1.94.0/bin/cargo build --offline --locked --bins
```

Da raiz do repositório:

```sh
python3 experiments/lr-10a-sdk-runtime/tests/test_boundary.py --evidence experiments/lr-10a-sdk-runtime/evidence/fix-3-boundary-tests.json
python3 experiments/lr-10a-sdk-runtime/tests/test_measure.py --evidence experiments/lr-10a-sdk-runtime/evidence/fix-3-python-regression.json
python3 experiments/lr-10a-sdk-runtime/run_fix3.py metadata "$CLI_NATIVE" --output experiments/lr-10a-sdk-runtime/evidence/fix-3-real-metadata.json
python3 experiments/lr-10a-sdk-runtime/run_fix3.py sessions "$CLI_NATIVE" --output experiments/lr-10a-sdk-runtime/evidence/fix-3-real-sessions.json
python3 experiments/lr-10a-sdk-runtime/run_fix3.py metadata-existing-auth "$CLI_NATIVE" --output experiments/lr-10a-sdk-runtime/evidence/fix-3-auth-boundary-blocked.json
python3 -m compileall -q experiments/lr-10a-sdk-runtime/boundary.py experiments/lr-10a-sdk-runtime/run_fix3.py experiments/lr-10a-sdk-runtime/fixtures experiments/lr-10a-sdk-runtime/tests
git diff --check
```

`CLI_NATIVE` foi o arquivo instalado em `.local/share/fnm/node-versions/v24.18.0/installation/lib/node_modules/@github/copilot/node_modules/@github/copilot-linux-x64/copilot`, SHA-256 `be17b42705ca17490098d7b87f293300d72a094d125b6bb2b2557dc4a0a4f8a8` (nenhum npm loader). Também executados rustfmt Edition 2021/check nos seis arquivos Rust alterados, Git status/HEAD/remotes/fetch/ls-remote e checagem dos JSONs, hashes e identidades owned finais.

[Build](../experiments/lr-10a-sdk-runtime/evidence/fix-3-build.txt) e [Rust](../experiments/lr-10a-sdk-runtime/evidence/fix-3-rust-tests.txt): sucesso; suites persistence 10, protocol 12, security 4. [Python](../experiments/lr-10a-sdk-runtime/evidence/fix-3-python-regression.txt): 40/40 (collector 4,324 s; unittest 4,315 s). [Boundary](../experiments/lr-10a-sdk-runtime/evidence/fix-3-boundary-tests.txt): 15/15 em 1,449 s, também incluídos nos 40, sem somar duplicadamente. Cada nome/resultado consta nos logs e JSONs. O collector FIX-1 permaneceu idêntico; seu label histórico foi explicitado/normalizado como proveniência da nova execução FIX-3.

Duas invocações iniciais de Cargo foram interrompidas (130) antes de completar ao detectar ausência do opt-out de downloads; a que aguardava lock também foi interrompida. Os builds finais usaram explicitamente COPILOT_SKIP_CLI_DOWNLOAD=1; nenhum novo runtime foi adotado, nem instalação global feita. Cargo --offline sozinho não desativa rede de build.rs; o script upstream confirma a necessidade desse opt-out. Não há medição de bytes de rede dessas invocações interrompidas nem alegação de que --offline as isolou por kernel.

A suíte Tauri de 1.107 testes **NOT_RUN** nesta FIX: diff restrito à POC/documentação, Cargo.toml/lock/contratos de produção inalterados, sem integração Narys. Recompilar toda a aplicação não acrescentaria prova da fronteira experimental.

## 9. Evidências positivas, negativas e medições

- [Manifesto](../experiments/lr-10a-sdk-runtime/evidence/fix-3-boundary-manifest.json): runtime/libs/features de segurança, hashes da crate e gates bloqueados.
- [Boundary final](../experiments/lr-10a-sdk-runtime/evidence/fix-3-boundary-tests.json): canários, ambiente, dispositivos/sockets, timeout, external control e rejeição A9; todas as identidades atribuídas da execução ausentes depois.
- [Python regressão](../experiments/lr-10a-sdk-runtime/evidence/fix-3-python-regression.json): 36 identidades controladas verificadas após a suíte, zero sobreviventes, incluindo controles externos após encerramento pelo próprio teste dono.
- [Permissões](../experiments/lr-10a-sdk-runtime/evidence/fix-3-permission-observations.jsonl): nove casos identificados; somente flags/decisões/contagens, sem IDs/payloads de permission requests. Raw callbacks negativos não viram sucesso de segurança.
- [Sessões reais](../experiments/lr-10a-sdk-runtime/evidence/fix-3-real-sessions.json): protocolo 3/RPC 1.0.90, auth required, explicit/generated create/detach/abort aceitos, nenhum events.jsonl de sessão vazia; same-client/restart/fresh-state retornam session_not_found. Diagnóstico de transcript authored retomou o mesmo ID; corrupto retornou -32603 sem rewrite; deletes owned aceitos. **Não é conversa genuína nem gate de persistência real**. Exit não-zero intencional porque BLOCKED_REAL continua.
- [Tentativa inicial boundary](../experiments/lr-10a-sdk-runtime/evidence/fix-3-boundary-tests-initial.json): 8 falhas/13 testes por --disable-userns exigir --unshare-user explícito; correção técnica, sem ampliar mounts. [Metadata inicial](../experiments/lr-10a-sdk-runtime/evidence/fix-3-real-metadata-initial.json), [diagnóstico](../experiments/lr-10a-sdk-runtime/evidence/fix-3-real-metadata-diagnostic.json) e [preflight](../experiments/lr-10a-sdk-runtime/evidence/fix-3-real-metadata-preflight.json) preservam falhas, não são resultados da versão final. A raiz Rust passou a ser criada 0700 explicitamente: tempdir padrão não satisfazia o gate privado. Nenhum erro foi escondido com sleep/retry/fallback.

| Medida final, uma execução/cache OS aquecido | Metadata real | Matriz sessões real |
| --- | --- | --- |
| Wall do harness | 3.299,41 ms | 14.919,14 ms |
| Inicialização SDK | 3196 ms | Não coletada por startup individual |
| Shutdown reportado pelo SDK | 40 ms | 6 shutdowns graceful (4 matriz + 2 diagnósticos), sem tempo individual |
| Cleanup harness | 29,24 ms | 31,84 ms |
| Pico somado RSS da árvore amostrada | 464.453.632 bytes | 529.313.792 bytes |
| CPU amostrada, limite inferior | 3,72 s | 16,22 s |
| Recovery signals / sobreviventes depois | 0 / 0 atribuídos | 0 / 0 atribuídos |

Sem benchmark prolongado. Amostragem 50 ms pode perder processos curtos e somar páginas compartilhadas; RSS não é RAM incremental exata do host. Contagem global de processos varia por trabalhos externos e não prova ownership; a prova usada é identidade/adoção/pidfd/ECHILD. O campo herdado `sdk_shutdown_verified=false` é intencional: harness não certifica SDK; `sdk_report.shutdown=graceful` e PID gone são observações separadas. Timeout sintético recuperado não foi relatado como shutdown gracioso do SDK.

Artefatos debug locais: POC 104.180.160 bytes, fixture 6.653.688 bytes; CLI 178.457.408 bytes. Não versionados. Nenhuma comparação release/benchmark frio, overhead preciso de todo sistema ou prontidão online foi medida nesta FIX.

## 10. Preservação de configuração, credenciais e processos

[run_fix3.py](../experiments/lr-10a-sdk-runtime/run_fix3.py) reutiliza o measure.py aprovado sem alteração. Antes/depois dos probes registra **somente stat** de config.json (inode/tamanho/mtime/ctime); os três probes finais registram stat inalterado. Não lê config/credenciais/transcripts pessoais; só inspeciona artefatos dos UUIDs criados na fixture privada. Nenhum token, banco do keyring ou config foi copiado. Não houve mudanças globais, sudo, serviços, shells genéricos em WebView ou IPC.

Recuperação permanece limitada a ownership demonstrável com start-time/pidfd, nunca a processos apenas semelhantes. Controles externos permaneceram vivos durante recovery e foram depois terminados apenas pelo teste que os criou. A verificação final consolidada está em [fix-3-verification.json](../experiments/lr-10a-sdk-runtime/evidence/fix-3-verification.json). Não se enviaram sinais a Codex, SSH, tmux, GNOME ou outros processos do usuário.

ExecutionAuthority/HumanLocal, Execution Broker, AgentRegistry, Codex planner read-only, OperationalTraceBus, TaskGraph/Scheduler/LR-8.5, IPC release, UI React e runtime 3D permanecem fora do diff. measure.py, test_measure.py, run_fix2.py, fixtures/tests herdados e evidências históricas FIX-1/FIX-2 permanecem byte a byte iguais à base; a documentação permanente recebeu apenas adendo FIX-3.

## 11. Riscos residuais

- Sem autenticação segura/isolada; disponibilizar credencial/bus para “fazer passar” violaria o escopo atual. Oficial PAT reduzido é hipótese futura, não solução validada.
- Sem egress por destino/provider separado de subprocessos; somente offline comprovado.
- Sem contenção comprovada de morte inesperada do worker. O teste herdado simula reporting inconclusivo, não prova sobrevivência/cleanup numa morte real. Não foi provocado orphan adversarial para executar esse teste.
- Não foram cobertos descendentes adversariais que escapem de ownership, namespaces/reparenting não previstos ou processos presos no kernel; não prometer recovery nessas condições.
- Seccomp denylist e namespace não são prova contra todo syscall/exploit; exec/fork privado permanece possível. Não é sandbox LR-10C.
- CLI pode ignorar opções experimentais: fixtures provam wire e SDK, não enforcement universal interno. Atualização de versões não foi feita por conveniência.
- Conversa genuína/persistência/aplicação do resultado e billing do provedor seguem bloqueados; histórico sintético não supre inferência autorizada.

## 12. Estado de prontidão para A9

**A9 BLOCKED — NÃO READY_FOR_A9**. Zero inferências executadas, zero requests SDK send/send_and_wait, zero chamadas agentivas reais. Consumo de quota atribuível à POC: **nenhum request de inferência**; não há snapshot autenticado/delta financeiro medido nesta FIX, e quota indisponível não foi tratada como zero ou ilimitada.

[Especificação A9 inerte](../experiments/lr-10a-sdk-runtime/fixtures/A9-COMMAND-NOT-AUTHORIZED.txt) substitui o antigo comando copilot -p/broad mount por desenho de **um único request via SDK Rust**, Auto se elegível, fixture não sensível, zero tools/MCP/YOLO/Autopilot, orçamento/overage explícitos, timeout/cleanup e transcript genuíno/detach/restart/resume sem uma segunda inferência. Não há comando executável nem modo implementado para dispará-lo. A autorização humana separada será decidida depois da auditoria; não foi solicitada nesta execução.

## 13. Pendências e recomendação

| Requisito bloqueado | Causa / tentativa permitida / evidência | Próxima solução recomendada e risco de prosseguir |
| --- | --- | --- |
| Auth de menor privilégio | Empty/offline sem credenciais iniciou, mas auth required; existing-auth recusada; metadata/auth JSONs | Auditar mecanismo oficial explicitamente opt-in, escopo e exposição ao runtime, sem copiar credenciais pessoais. Bus amplo permitiria acesso a serviços/credenciais extras |
| Rede provider vs ferramentas | Namespace offline passa canário; sem policy egress online | Definir e verificar fronteira por destino e identidade, TLS/DNS mínimos, sem share-net irrestrito. Conectividade genérica permitiria exfiltração |
| Worker failure/adversarial cleanup | FIX-1 passa árvores controladas; morte real do supervisor não comprovada | Auditar estratégia estreita de contenção antes de A9; não antecipar supervisor LR-10B nesta FIX. Prosseguir poderia deixar processos fora da atribuição |
| Enforcement interno CLI | Wire configs/rejects/update failure passam; nenhuma ferramenta real disparada | Auditoria das capacidades necessárias e teste aprovado correspondente; jamais assumir flags aceitos = enforcement completo |
| Transcript/billing/A9 | Sessões vazias sem transcript; quota unknown; inferência proibida | Após todos gates e nova autorização, executar único request budgetado e validação independente. Sintético/unknown não sustentam PASS operacional |

**FIX-AND-RETEST**, com evidência positiva da fronteira offline e dos testes de negação, mantendo bloqueadores materiais explícitos. Não avançar a A9, LR-10B ou LR-10C automaticamente.

Arquivos: novos boundary.py, run_fix3.py, src/boundary.rs, src/bin/boundary-fixture.rs, fixtures/permission_cli.py, tests/security.rs e tests/test_boundary.py; alterações limitadas a lib.rs/main.rs/guarda em persistence.rs, README, spec A9, adendo permanente, este relatório e novas evidências `fix-3-*`. A listagem/hash final e preservação dos arquivos herdados constam na verificação consolidada.

## 14. Auditoria independente pendente

Esta entrega é uma implementação candidata, com recomendação FIX-AND-RETEST e A9 bloqueado. Os PASS indicados são testes preparatórios de escopo limitado. Luna deverá auditar código, evidências negativas/positivas e limites descritos diretamente neste GitHub. Nenhum PASS definitivo é atribuído à LR-10A ou LR-10.

**LR-10A FIX-3 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE**.
