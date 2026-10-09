# LR-10A — Implementation & Evidence

**LR-10A — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE.**

Data: 09/10/2026. Recomendação: **FIX-AND-RETEST**. O gate operacional completo
continua pendente; **A9 = BLOCKED_REAL / AWAITING_HUMAN_APPROVAL**. Nenhum PASS
definitivo é atribuído à LR-10A ou LR-10. Não avançar à LR-10B sem resolução dos
bloqueios e decisão da auditoria. LR-10B–F não foram implementadas.

## 1. Autoridade, Git e escopo

Fontes lidas: LR-10-COPILOT-SPECIALIST-AGENT, LR-10A-FEASIBILITY-POC,
NARYS-TERMINAL-RUNTIME-TRACK, seção 14 do PLANO-OPERACIONAL-LUNA,
src-tauri/Cargo.toml e contratos de agents/execution. A autorização desta entrega
substitui o estado de planejamento da documentação, sem ampliar os gates.

- Base: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`.
- `main` e `origin/main` tinham esse mesmo HEAD após `git fetch origin`.
- Workspace inicialmente limpo; nenhum trabalho humano foi descartado.
- Branch criada: `lr-10a-sdk-runtime-feasibility`.
- Remoto: `git@github.com:Samuel-Francisco-AS/Narys.git`.
- Entrega exclusivamente nessa branch, com commits descritivos; sem PR, merge,
  rebase, reset, force-push, sudo ou mudança da main.

Não há alteração em src-tauri, React, inicialização da Narys ou contratos públicos.
ExecutionAuthority/HumanLocal, Codex planner read-only, AgentRegistry, release IPC,
OperationalTraceBus, TaskGraph, Scheduler e LR-8.5 permanecem idênticos à base.
A supervisão experimental descrita abaixo não concede authority agentiva e não
substitui o Execution Broker.

## 2. Ambiente, versões e dependências efetivas (A0/A1)

| Item | Observação |
| --- | --- |
| OS | Fedora Linux 44 Workstation, x86_64, kernel 7.2.8-200.fc44.x86_64 |
| CPU/RAM | Intel i7-3770, 4 cores/8 threads; aproximadamente 7.64 GiB RAM; swap presente |
| Ambiente | shell/SSH, execução sem DISPLAY/WAYLAND_DISPLAY pelo harness; nenhuma GUI aberta |
| Rust/Cargo host | rustc 1.98.1 (48a229cea); Cargo 1.98.1 (797e8a9bc), pacotes Fedora |
| Rust/Cargo isolados | rustc 1.94.0 (4a4ef493e); Cargo 1.94.0 (85eff7c80) |
| Node | 24.18.0, instalação fnm existente |
| Copilot CLI | 1.0.91, instalação npm existente; loader e binário nativo verificados |
| bwrap | 0.12.0; probe de namespace/true executado com sucesso |
| SDK da POC | `github-copilot-sdk = 1.0.17`, registry crates.io oficial |
| Features | `default-features=false`, `runtime`; comparação opcional `bundled-cli` |
| POC | Edition 2021, rust-version 1.94.0; crate SDK é Edition 2024 |
| Narys | Edition 2021, rust-version 1.77.2 e Cargo.lock **inalterados** |

Rustup não está instalado. Rust 1.94 foi instalado **somente em /tmp** a partir dos
componentes oficiais rustc/rust-std/cargo de 05/03/2026, com SHA256 conferidos contra
`channel-rust-1.94.0.toml`; install.sh usou prefix local e `--disable-ldconfig`.
Nenhum default/toolchain/config global foi modificado. O diretório /tmp pode expirar.

A release GitHub `v1.0.18` (08/10/2026) contém `version=0.0.0-dev` no Cargo.toml
Rust do tag. A investigação de registry encontrou também a crate **1.0.18** e
**1.0.19-preview.3**; portanto não se inferiu a versão Cargo a partir do tag.
A POC mantém o pin estável **1.0.17**, selecionado no primeiro baseline compilado,
sem adotar preview nem atualizar CLI global. Não é uma alegação de “SDK mais novo”.
A candidata deve retestar um par SDK/CLI compatível e atualmente publicado.

A crate 1.0.17 contém snapshot de CLI/runtime **1.0.93**; a 1.0.18 aponta **1.0.94**.
O pin da POC e checksums transitivos estão no seu Cargo.lock. VCS empacotado:
`0e56b9a0033f48e4358ee027a4077cd6ebd61f32`, `dirty=true`, path `src/sdk/rust`.
Esse metadata não é apresentado como checkout limpo equivalente ao tag público.
Checksum registry, SHA256/tamanho do CLI nativo e versões estão em
[baseline.json](../experiments/lr-10a-sdk-runtime/evidence/baseline.json).

`cargo metadata --locked` resolveu 219 packages incluindo a POC (todos os targets);
SDK é o maior MSRV declarado: **1.94.0**. UUID 1.27 exige 1.89; ICU 2.3 exige 1.88.
Narys resolve 662 packages incluindo root; o maior MSRV declarado observado no
lock existente é 1.88 (darling/time/plist/ICU etc.). Packages sem declaração não
provam compatibilidade com um compilador antigo. A compilação e os testes em
**1.94.0 exato** comprovam o limite efetivo testado; não foi testado 1.77.2.

A prova isolada precedeu a regressão da app. Edition 2021 consumindo SDK Edition
2024 passou sem migração. Tauri 2 e a árvore nativa existente passaram a suíte
Rust em 1.94. O SDK isolado resolve reqwest 0.13, enquanto a produção fixa 0.11;
ambos podem coexistir em crates distintos, mas unificação/linkagem conjunta SDK
+ Tauri na app **não foi executada nem é pressuposta** nesta subfase.

Não se elevou rust-version da app: SDK não foi introduzido em produção e não há
necessidade funcional nesta candidata de modificar seu manifesto. A compatibilidade
em 1.94 está demonstrada para a árvore atual; uma futura inclusão do SDK exigirá
mínimo 1.94 e nova revisão do lock/CI. O MSRV declarado antigo já não é atestado
pela árvore existente. Nenhuma conclusão oculta esse débito herdado.

## 3. Arquitetura e arquivos

Tudo reside em `experiments/lr-10a-sdk-runtime/`, workspace Cargo independente:

| Arquivo | Responsabilidade |
| --- | --- |
| Cargo.toml / Cargo.lock | versões diretas exatas, checksum e features |
| src/lib.rs | projeções seguras de erros, auth/model/quota; configs deny-all; lifecycle; shutdown bounded |
| src/main.rs | comandos manuais sem inferência, fixture e estado temporários, proteção bwrap de auth existente |
| measure.py | processo próprio, amostragem /proc, recuperação e reap experimentais; saída JSON |
| fixtures/mock_cli.py | servidor local JSON-RPC Content-Length, sem rede/inferência/credenciais |
| tests/protocol.rs | 12 testes determinísticos usando o SDK real contra fixture |
| tests/test_measure.py | 5 testes de subprocessos/timeout/descendentes/stdout herdado/evidência inválida |
| fixtures/a9-read-only.txt | dados mínimos para gate humano futuro |
| fixtures/A9-COMMAND-NOT-AUTHORIZED.txt | comando inerte e procedimento A9 não executado |
| README.md / .gitignore | reprodução, limites e exclusão de build/pycache |
| evidence/*.json | resultados sanitizados, versões e checks executados |

Este documento é o único arquivo novo fora do experimento. Logs brutos do runtime,
credenciais, prompts internos, payloads de eventos, reasoning e ferramentas não
são entregues. O código nunca chama session.send/send_and_wait, fleet ou equivalente.
Nenhum runtime inicia com Narys, UI, Settings ou polling: somente comandos manuais
explícitos da POC; processos não são mantidos entre comandos.

## 4. Runtime, descoberta e transportes (A2)

`CliProgram::Path` recebe um caminho absoluto validado/canonicalizado/executável.
O PATH foi consultado pelo humano/driver para localizar a instalação; **não** se
presume descoberta do SDK por PATH. O caminho encontrado é npm-loader.js, que
inicia o CLI nativo. O binário nativo adjacente na instalação npm também foi testado;
nenhum desses testes atualizou/baixou CLI.

O SDK usa stdio JSON-RPC 2.0 com framing Content-Length. Handshake protocolo **3**
foi viável com CLI 1.0.91. Mocks cobrem connect, versão inválida, JSON inválido,
saída prematura, ausência de método e timeout. O protocolo aceito não comprova
compatibilidade de todos os RPCs ou de persistência de sessão.

| Modo | Resultado |
| --- | --- |
| sem features | inspeção oficial: streams externos; Client::start recusa configuração; não compilado separadamente |
| runtime sem bundle | check/test 1.94, launch/metadata/stop reais; dependência de instalação existente |
| bundled-cli | árvore de features + cargo check 1.94 com aquisição desativada passaram |
| bundle físico/in-process | NOT_RUN; nenhuma aquisição, extração ou benchmark desse runtime |

**Armadilha comprovada em código upstream:** `runtime` sem bundle ainda pode
baixar/extrair runtime em build.rs. Todos os builds da POC usaram
`COPILOT_SKIP_CLI_DOWNLOAD=1`. A feature bundled com essa variável **não** incorpora
um bundle; check de feature não é prova de extração/launch do bundle.

Build upstream pode incluir tanto CLI completo como wrapper/runtime.node/assets.
Metadata oficial de release 1.0.93: CLI linux-x64 tar.gz **112.302.204 bytes**;
package runtime linux-x64 tgz **72.741.899 bytes**. São tamanhos comprimidos
**publicados**, não medidos por download. O custo físico final, cache, extração,
RSS/startup e delta release bundled permanecem NOT_RUN. Download de bundles: **0**.
O delta de dependências/feature inclui tar/flate2/zip, parte já presente em build-deps.

### Incidente de preservação da configuração

O primeiro probe contra autenticação existente foi feito sem proteção de montagem.
Um snapshot **somente de stat**, sem ler conteúdo de nenhum arquivo, detectou
alteração de tamanho/mtime em `~/.copilot/config.json` durante o probe. Não se afirma
qual conteúdo mudou nem que bytes permaneceram iguais. Isso é uma violação do
requisito de preservação no experimento inicial, mesmo sem setter/login da POC.
Não foram copiados, impressos, examinados ou restaurados tokens/credenciais/config.
Uma restauração cega poderia corromper estado; não se fez rollback sem original.

A implementação entregue obriga bwrap para todos os comandos existing-auth:
host montado read-only, logs/workspace temporários writable, sem fallback sem
proteção. Reteste com stat antes/depois: **inalterado**, conforme
[global-config-stat-protected.json](../experiments/lr-10a-sdk-runtime/evidence/global-config-stat-protected.json).
Isso corrige o caminho entregue; não apaga a falha histórica. Deve ser auditado.

## 5. Auth, modelos e quota (A3–A5)

O CLI resolve sua autenticação existente; a POC não lê arquivos de credenciais nem
extrai tokens. GH_TOKEN/GITHUB_TOKEN/COPILOT_SDK_AUTH_TOKEN são removidos do ambiente
do filho, sem leitura de valores. Identidade/login/statusMessage não são exportados.

- COPILOT_HOME descartável: `authentication_required`; não implica logout da conta.
- Auth existente com proteção read-only: **authenticated**.
- Catálogo real devolveu **apenas Auto**, capabilities `{}`, policy/billing `null`.
  Não se inventam modelos extras, capacidades de ferramentas/visão/contexto, preço
  ou disponibilidade efetiva de inferência. Não se atesta plano Student pelo login.
- account.getQuota real: premium_interactions entitlementRequests **200**,
  usedRequests **52**, remainingPercentage **74,2**, overage **0**, flags de overage
  false. Chat/completions reportaram isUnlimitedEntitlement=true e entitlement=0.
  A flag explícita é registrada como `unlimited_reported`; não é autorização de gasto.
- Campos e resetDate são observações do runtime, não calendário mensal garantido.
  Não se deduz saldo como 200−52 nem se equiparam requests a AI Credits/tokens.

Quota ausente/map vazio/erro de RPC = **quota_unknown**. Entitlement zero ou
remainingPercentage zero, quando não unlimited, = **limit_reached**; dados inválidos
permanecem unknown. Erro RPC desconhecido não é rotulado auth/quota/entitlement com
base em texto livre. `-32601` = método indisponível. A POC não executa admission de
inferência e nenhum estado de metadata concede execução.

Os snapshots autenticados mantiveram usedRequests premium **52** e overage **0**.
**Chamadas de inferência da POC: 0; consumo de quota esperado e compatível com as
observações: nenhum.** Não houve assistant.usage nem prova independente de fatura;
a ausência desses eventos sozinha não seria prova de consumo zero.

## 6. Sessões, eventos e cancelamento (A6/A7)

Config explícita: Auto, availableTools vazio, deny_all_permissions, file hooks,
skills, host git operations e descoberta de instruções desabilitados. Session store
habilitado para investigar persistência; resume desabilita recuperação destrutiva
do transcript. Sessões de usuário não são listadas/resumidas/deletadas.

Prepare/subscribe precedem startup para evitar o bootstrap tardio unbounded do SDK.
A projeção local conserva no máximo 32 IDs/32 eventos e observa até 30 ms por evento;
lag/gap, pais ausentes e duplicados são diagnósticos. Roteamento de uma notificação
de outra sessão é testado. Isso não elimina filas internas ou blobs grandes do SDK.

Fixtures passaram create, abort sem turno, detach, persistência própria em arquivo,
restart de cliente, resume, delete e rejeição de resume depois de delete. A fixture
não é prova de armazenamento do provider. session.idle e mensagens sintéticas de
“sucesso” jamais alteram task_completed; fechamento de operação é fato separado.

**Observação real:** create/detach foram atravessados, mas resume devolveu
`session_not_found`, tanto no estado descartável como com auth existente e overlay
descartável de session-state. CLI nativo e loader reproduziram o problema. Nenhuma
retomada real, streaming de modelo, conclusão de tarefa ou persistência após restart
foi aprovada. Hipótese não demonstrada: sessão vazia pode não persistir antes de
uma mensagem, ou haver diferença de versão/storage. Não se resolveu isso enviando
inferência nem se marcou lifecycle real PASS.

Oficial: session.abort solicita interrupção do turno; disconnect preserva estado;
destroy é alias depreciado de disconnect, **não** deletion. client.delete_session
é descarte durável. client.stop tenta detach/shutdown/EOF/reap; upstream possui
bounds separados de 10s. A POC limita stop a 5s e startup/RPC a 15s; força recovery
quando necessário e reporta erro. Abort tardio/timeout é testado em fixture e não
transformado em cancelamento efetivo de trabalho real.

No Linux desta crate, process_tree::spawn retorna **None**: ownership de árvore via
Job Object existe para Windows, não para Linux. force_stop e Drop são caminhos de
terminação do filho direto e não comprovam reap de todos os descendentes/flush.
Um probe inicial com loader/bwrap excedeu 15s no startup e deixou **dois processos
observados** depois da saída do host. O driver os encerrou; o sample antigo registra
sobreviventes **antes** dessa recuperação, sem medição final equivalente ao harness
novo. Retestes nativo/loader completaram startup/stop, falhando apenas em resume,
sem sobreviventes observados.

O harness final usa grupo novo, /proc PID+start-time, rastreamento de descendentes
observados, subreaper Linux local e kill/reap bounded de recuperação. Os cinco
fixtures provam saída normal, pai prematuro com filho, timeout e stdout herdado sem
bloqueio de cleanup, além de evidência inválida fail-closed. Recovery produz FAIL e evidencia o que o SDK não garantiu;
não é apresentado como sucesso gracioso do SDK nem supervisor LR-10B implementado.
Não se deixou processo órfão conhecido; scans amostrados não provam resistência a
escape adversarial, mudança de namespace ou descendente não observado.

## 7. Medições (A8)

Metodologia: executável debug fora da UI, nova árvore por sample, caches do OS já
quentes; /proc/stat a cada 50 ms, sem cmdline/environ. RSS soma páginas residentes
(inclui POC/CLI/wrapper; páginas compartilhadas podem contar duas vezes); baseline
owned-tree é zero. CPU é soma de ticks dos processos observados, **limite inferior**.
Contagem global de processos varia com build/SSH e não é atribuída ao Copilot.
Samples antigos e finais são identificados pelos JSONs; não são benchmark estatístico.

| Sample real | Start ms | Stop ms | Pico RSS da árvore | CPU observada | Picos de processos | Sobreviventes observados |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| metadata descartável inicial, loader | 2695 | 74 | 368,62 MiB | 1,91s | 4 | 0 |
| metadata auth protegido inicial, loader | 3929 | 903 | 323,64 MiB | 1,80s | 5 | 0 |
| metadata auth protegido final, loader | 1782 | 985 | 304,70 MiB | 1,70s | 4 | 0 |
| sessions descartável final, falha resume | 3111 | 31 | 330,87 MiB | 1,91s | 4 | 0 |
| sessions auth protegido, CLI nativo, falha resume | 734 | 802 | 266,89 MiB | 1,42s | 3 | 0 |
| sessions auth protegido, loader reteste, falha resume | 795 | 992 | 317,75 MiB | 1,41s | 4 | 0 |

Os valores da tabela são extraídos dos JSONs anexos; divergência de ambiente/build
ou campo deve ser verificada no artefato, sem inventar média/p95. RSS incremental
físico do sistema/PSS, cold-cache, idle prolongado, release runtime, bundle físico,
consumo GPU e GUI são **NOT_RUN**. RSS não é orçamento rígido de produção.

Binário debug POC: aproximadamente **98 MiB**, sem runtime incorporado. Target POC
check/test: aproximadamente **1,8 GiB**; inclui símbolos/intermediários, não tamanho
de distribuição. Target Narys preexistente: **46 GiB**; após regressão 1.94, **50 GiB**
no ponto medido, antes do check release adicional. Sem limpeza de caches. Bytes
exatos do executável/CLI estão no baseline JSON. Build/test em geral limitados a
CARGO_BUILD_JOBS=2; check release e bundle em jobs=1 cada, sem paralelismo excessivo.

## 8. Segurança e fronteiras potenciais (A10)

| Superfície | Executor efetivo / implicação |
| --- | --- |
| builtin bash/shell/comandos | CLI inicia subprocessos internamente; não passam automaticamente pelo Broker |
| view/glob/grep/edição/write e operações git/plan/workspace do provider | implementação interna do CLI/RPC; cwd não confina filesystem |
| MCP stdio/local / plugins / LSP | CLI/servidor correspondente pode iniciar processos; permissões do SDK não substituem OS boundary |
| MCP remoto HTTP/SSE/web/rede | executor remoto/rede; sandbox local não isola o servidor remoto |
| custom tools com handler Rust explícito | callback host-owned pode futuramente encaminhar uma operação tipada ao Broker, somente com authority agentiva própria |
| processo runtime principal | pode futuramente ser supervisionado pelo Core; isso não medeia builtin tools |

Nenhuma ferramenta foi registrada no Execution Broker nem recebeu HumanLocal.
Alternativas de Broker-mediated custom tools exigem substituir/desabilitar rotas
nativas equivalentes e provar cobertura; callbacks, tool_call_id e provenance não
mintam authority. Manter ferramentas nativas provavelmente exigirá boundary/supervisor
próprio verificável; a decisão pertence à LR-10C.

PermissionHandler responde deny/approve_once/user_not_available; `NoResult` pode
ceder a outro cliente, portanto não é fail-closed. **Omitir handler** pode serializar
requestPermission=false e suprimir prompts. A POC instala deny explicitamente.
DenyAllHandler real foi testado com request desconhecido e managed approval;
nenhum approve-all, YOLO ou aprovação humana simulada é ativado.

Hooks pré-tool podem negar/alterar argumentos; pós-tool podem modificar resultado,
mas não desfazem efeitos. Coverage, exceções, managed settings e callbacks faltantes
precisam de provas por classe de ferramenta. Observação de tool events não é
approval; Activity/TraceBus não integra o fluxo decisório. A documentação web de
hooks consultada não tem exemplos Rust; tipos/comportamento Rust foram inspecionados
na crate pinada, sem assumir paridade de exemplos de outras linguagens.

CLI help 1.0.91 descreve sandbox **experimental MXC**, Linux bubblewrap com rede
namespace/slirp4netns/util-linux/iptables/TUN; builtin edits não são OS-sandboxed,
apenas seguem policy em best effort. bwrap/unshare/nsenter/iptables foram encontrados,
slirp4netns não apareceu no PATH. TUN tem acesso local, mas o conjunto operacional
completo **não foi testado** nem instalado. Não se habilitou sandbox CLI/YOLO real.
A proteção read-only da POC é distinta, e não valida essa modalidade de produto.

YOLO/--allow-all amplia permissões de ferramentas/paths/URLs; Autopilot amplia
continuidade. Nenhum dos dois significa isolamento, autorização perpétua ou hard
budget. sessionLimits.maxAiCredits é soft e verificado após resposta; pode exceder
uma chamada. Não há approval engine, sandbox de produção, plano operacional ou
execution authority agentiva nesta entrega.

## 9. Verificações executadas e matriz

Comandos exatos/sumários/checksums de logs em
[checks.json](../experiments/lr-10a-sdk-runtime/evidence/checks.json); reprodução no
[README](../experiments/lr-10a-sdk-runtime/README.md). Logs completos locais em
`/tmp/narys-lr10a-evidence`, sem inclusão de stderr/protocolo bruto do runtime.

- Baseline Narys Rust 1.98.1, cargo test --locked --lib: **1107 PASS / 0 FAIL / 2 ignored**, 189,04s de testes.
- Narys Rust **1.94.0**, cargo test --locked --lib: **1107 PASS / 0 FAIL / 2 ignored**, 241,47s de testes.
- Prova isolada SDK/Rust 1.94 cargo check --locked: PASS, 1m52s, antes da regressão da app.
- POC Rust 1.94: **12 PASS / 0 FAIL**; doctests executados (0 casos).
- Cleanup Python Linux: **5 PASS / 0 FAIL**, incluindo stdout herdado.
- bundle-comparison check Rust 1.94 com downloads bloqueados: PASS, 1m48s.
- npm run typecheck e npm run build: PASS; build reteve aviso de chunk grande existente.
- Check release Narys Rust 1.94: **PASS**, 8m41s; 48 warnings existentes, sem mudança de código de produção.
- Build executável release/GUI Tauri e teste real A9: NOT_RUN/BLOCKED; não se apresentam checks como testes de UI ou bundle físico.

Falhas intermediárias: fixtures inicialmente omitiram campos obrigatórios de
connect/options.update; foram corrigidas e retestadas. RUSTC isolado com rustdoc do
host causou E0514 em doctests; corrigido fixando também RUSTDOC, sem cargo clean.
Essas tentativas não são contadas como PASS; os checks finais refletem o código entregue.

| ID | Estado técnico do experimento | Evidência / limite |
| --- | --- | --- |
| A0 | PASS | SHA/ambiente/versionamento/checksum/baseline fixados |
| A1 | PASS | Edition 2021 + SDK Edition 2024; compilação/tests 1.94; produção sem dependência SDK |
| A2 | FAIL | runtime sob demanda viável; incidente config inicial; bundle físico NOT_RUN; versões divergem |
| A3 | PASS | auth real observado com proteção; entitlement numérico observado, inferência não atestada |
| A4 | BLOCKED | Auto observado, capabilities vazio; catálogo completo/capacidades efetivas inconclusivos |
| A5 | PASS | quota real e classificações unknown/limit/unlimited; nenhum admission ou gasto autorizado |
| A6 | FAIL | mocks passam; resume real session_not_found; persistence/streaming operacional não comprovados |
| A7 | FAIL | mocks/harness passam; Linux SDK sem ownership de árvore; sobreviventes em startup timeout inicial |
| A8 | PASS | headless/CPU/RSS/start/stop/processos/artefatos medidos com limites; bundle/release perf NOT_RUN |
| A9 | BLOCKED | BLOCKED_REAL / AWAITING_HUMAN_APPROVAL; fixture/comando inertes, 0 inferências |
| A10 | PASS | investigação/provas locais de negação; segurança operacional/sandbox real continuam pendentes LR-10C |

PASS nesta matriz significa somente o experimento/inspeção indicado, **não** aprovação
definitiva da subfase, completude operacional ou transferência de authority.

## 10. Decisão e próximos gates humanos

**FIX-AND-RETEST**, sem avanço automático à LR-10B. A viabilidade do SDK Rust,
Edition 2021 e runtime on-demand para handshake/metadata/stop está sustentada;
resume e cleanup de árvore Linux ainda exigem solução/reteste. O custo de memória
medido cabe como experimento nesta máquina, mas não fundamenta runtime residente.

Antes de propor LR-10B:
1. Auditar o incidente de config e manter proteção de gravação sem manipular secrets.
2. Fixar/retestar par SDK/CLI e persistência de sessão vazia sem inferência, ou registrar
   a dependência do gate humano A9 com precisão; não atualizar ambiente global implicitamente.
3. Provar ownership/reap de descendentes em cancel, timeout e crash; o harness desta
   POC não é uma solução de produto nem elimina o requisito de supervisor/boundary.
4. Completar catálogo/capabilities e confirmar comportamento de extensão/MCP/managed
   permissions, com gates de LR-10C claramente separados.
5. Obter autorização humana **separada** para um único gate A9 com fixture, orçamento,
   modelo/Auto e boundary revisados. O comando preparado não foi enviado ao Copilot.

**LR-10A — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE.**

## Referências oficiais consultadas em 09/10/2026

- [Rust SDK README / resolução, transportes, stop](https://github.com/github/copilot-sdk/blob/main/rust/README.md)
- [Cargo.toml do tag v1.0.18](https://github.com/github/copilot-sdk/blob/v1.0.18/rust/Cargo.toml)
- [Crate 1.0.17 / source pinado](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/)
- [Release v1.0.18](https://github.com/github/copilot-sdk/releases/tag/v1.0.18)
- [CLI/runtime 1.0.93 / assets publicados](https://github.com/github/copilot-cli/releases/tag/v1.0.93)
- [Setup CLI local](https://github.com/github/copilot-sdk/blob/main/docs/setup/local-cli.md)
- [Usage e account.getQuota](https://github.com/github/copilot-sdk/blob/main/docs/features/usage-and-billing.md)
- [Session persistence](https://github.com/github/copilot-sdk/blob/main/docs/features/session-persistence.md)
- [Session limits soft](https://github.com/github/copilot-sdk/blob/main/docs/features/session-limits.md)
- [Hooks](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/hooks)
- [CLI allow/deny/YOLO](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli/allowing-tools)
- [MXC](https://github.com/microsoft/mxc) e `copilot help sandbox` 1.0.91 inspecionado localmente.
- [Rust 1.94 manifest oficial](https://static.rust-lang.org/dist/channel-rust-1.94.0.toml)
