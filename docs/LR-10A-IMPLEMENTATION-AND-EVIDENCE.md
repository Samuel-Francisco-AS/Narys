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

## Adendo — FIX-1 / Process Ownership & Cleanup Reliability (09/10/2026)

Após a auditoria, o harness Python passou a usar um worker/subreaper privado por
invocação, atribuição por filhos do kernel, pidfds e confirmação de ECHILD com
inventário vazio. A recuperação independe dos processos vistos na amostragem e
do grupo/sessão originais. Falhas de identidade, sinalização ou comprovação de
reap não viram cleanup completo. Código de produção, SDK e CLI não foram alterados.

As 25 fixtures/testes Python passaram, incluindo os cinco testes anteriores;
36 identidades de fixtures/controle foram verificadas ausentes após a suíte.
Não houve nova execução do CLI real, inferência nem consumo de quota. As medições
reais históricas acima permanecem evidência do harness anterior: seus zeros de
sobreviventes observados não provam recuperação de descendentes não amostrados.

Detalhes, limites, resultados individuais, comandos e identificação Git estão no
[relatório reutilizável mais recente](LR-10-LATEST-EXECUTION-REPORT.md), no
[JSON FIX-1](../experiments/lr-10a-sdk-runtime/evidence/fix-1-verification.json) e no
[log da suíte](../experiments/lr-10a-sdk-runtime/evidence/fix-1-verification.txt).
Este adendo não reclassifica A7 real, aprova LR-10A ou libera LR-10B/A9.

**LR-10A FIX-1 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE.**

## Adendo FIX-2 — 2026-10-09: persistência de sessões vazias

Investigação exclusiva da FIX-2; resultados anteriores permanecem históricos.
[Relatório reutilizável da execução](LR-10-LATEST-EXECUTION-REPORT.md),
[probe final auth existente](../experiments/lr-10a-sdk-runtime/evidence/fix-2-real-existing-auth-final.json),
[probe final estado isolado](../experiments/lr-10a-sdk-runtime/evidence/fix-2-real-isolated-final.json),
[regressão FIX-1](../experiments/lr-10a-sdk-runtime/evidence/fix-2-python-regression.json).

SDK 1.0.17/CLI nativo com --version 1.0.91 mantidos, sem inferência. Sessões
vazias UUID explícitas/geradas criam workspace.yaml, mas não events.jsonl;
metadata persistida ausente e resume NotFound no mesmo Client e após restart.
Abort vazio e enable_session_store true/false não alteraram isso. Store trata
busca/indexação entre sessões, não é comando de flush. A causa imediata observada
é ausência do transcript; o instante do primeiro flush/internal policy do CLI
não foi comprovado e não se declara exigência universal de inferência.

Um transcript sintético próprio com apenas session.start pôde ser retomado pelo
mesmo SDK/CLI, sem mensagem ao modelo. É diagnóstico do leitor de disco, não
persistência de histórico produzido pelo SDK. Corrupção sintética retorna -32603
no runtime local, sem rewrite; -32075 da documentação atual é testado somente em
fixture. Delete de ID sintético previamente resumível torna resume NotFound.

22 testes Rust (10 novos, 12 anteriores) e 25 Python FIX-1 passaram. O harness,
seus testes e evidências históricas não foram modificados. Ambos os probes reais
finais comprovaram exhaustion do worker e zero recuperação forçada/sobreviventes,
com stat de config.json inalterado sob proteção read-only. Nenhum pacote SDK/CLI
atualizado, nenhum arquivo de produção alterado. Gate de conversa real permanece
BLOCKED_REAL/AWAITING_HUMAN_APPROVAL; não existe PASS definitivo da LR-10A.

**LR-10A FIX-2 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE.**

## Adendo — LR-10A FIX-3: fronteira experimental offline e preparação A9 (2026-10-09)

A guarda ampla read-only do host foi substituída por namespace Bubblewrap com
root vazio, um ELF fixado, oito libraries individuais de sistema e dados privados.
Sem home pessoal, keyring/D-Bus, config/session pessoal, rede do host ou ambiente
herdado. Fixture RO, state/logs RW, proc namespaced RO, dev null/urandom, capabilities
removidas, nested userns negado e seccomp x86_64 obrigatório. Launcher/flags falham
fechados; nenhuma inferência ou ferramenta agentiva real foi executada.

40 testes Python (25 herdados + 15 boundary) e 26 Rust (22 herdados + 4 security)
passaram. SDK/CLI real fez handshake/metadata e matriz de sessões vazias dentro da
fronteira mínima. Auth requerida, catálogo indisponível e quota desconhecida;
config.json stat inalterado, cleanup owned completo. Sessão vazia permanece sem
transcript; resume sintético continua somente diagnóstico do leitor, sem comprovar
conversa genuína. FIX-1/harness e evidências FIX-1/FIX-2 não foram sobrescritos.

DenyAll reaplicado no create/resume, tools vazios, MCP/plugins/extensões/discovery/
skills/hooks/Git desligados. Fixtures provam reject shell/write/unknown/managed,
precedência da policy sobre NoResult/panic/pending e falha de create/resume quando
options.update obrigatório não existe. Não comprovam enforcement completo do CLI.

**FIX-AND-RETEST; A9 BLOCKED, não READY_FOR_A9**: autenticação de menor privilégio,
egress provider separado de subprocessos e contenção da falha real do worker não
foram estabelecidos. Nenhum token/config/keyring foi copiado, nenhum login/logout,
YOLO/Autopilot, update global, serviço ou alteração em produção. Especificação A9
continua TXT inerte, agora por um request SDK futuro com nova autorização separada.

Evidências permanentes desta execução estão em
[fix-3-boundary-tests.json](../experiments/lr-10a-sdk-runtime/evidence/fix-3-boundary-tests.json),
[fix-3-python-regression.json](../experiments/lr-10a-sdk-runtime/evidence/fix-3-python-regression.json),
[fix-3-rust-tests.txt](../experiments/lr-10a-sdk-runtime/evidence/fix-3-rust-tests.txt),
[fix-3-real-metadata.json](../experiments/lr-10a-sdk-runtime/evidence/fix-3-real-metadata.json),
[fix-3-real-sessions.json](../experiments/lr-10a-sdk-runtime/evidence/fix-3-real-sessions.json),
[fix-3-auth-boundary-blocked.json](../experiments/lr-10a-sdk-runtime/evidence/fix-3-auth-boundary-blocked.json)
e [fix-3-verification.json](../experiments/lr-10a-sdk-runtime/evidence/fix-3-verification.json).
O [relatório reutilizável](LR-10-LATEST-EXECUTION-REPORT.md) contém a matriz de ameaças,
G1–G12, comandos, limitações, recomendações e referências de auditoria desta FIX.

**LR-10A FIX-3 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE**.


## Adendo permanente — FIX-4: Auth & Network Boundary Feasibility (09/10/2026)

**LR-10A FIX-4 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE**.
Recomendação **FIX_AND_RETEST**; A9 permanece bloqueado. Este adendo não altera
as conclusões históricas das FIXes 1–3 nem atribui PASS definitivo à LR-10A.

A crate efetivamente utilizada, SDK Rust 1.0.17, fornece interceptação oficial
HTTP/WebSocket por `ClientOptions::request_handler`, registrada pelo RPC
`llmInference.setProvider`. Os defaults encaminham ao upstream: é obrigatório
substituir ambos para uma política fechada. O CLI local de hash preservado aceitou
esse registro dentro do sandbox offline; versão RPC observada 1.0.90. Isso não
prova autenticação nem interceptação de todos os endpoints de auth/telemetria.
[Inspeção e versões](../experiments/lr-10a-sdk-runtime/evidence/fix-4-contract-inspection.json),
[trechos da crate consumida](../experiments/lr-10a-sdk-runtime/evidence/fix-4-upstream-contract-excerpts.txt),
[prova real offline](../experiments/lr-10a-sdk-runtime/evidence/fix-4-real-offline-metadata.json).

A mediação sintética usa stdio/RPC owned, sem proxy genérico nem rede no sandbox.
Apenas uma operação fixa de metadata pode alcançar um servidor HTTP de loopback
controlado pelo teste, com segredo sintético retido no host. Runtime e filho
permanecem sem acesso TCP ao host. Destinos/headers/body/query/traversal/CONNECT/
WebSocket, ausência de auth/gateway, erros/timeout/redirect/echo e segunda operação
são negados; nenhuma resposta arbitrária do servidor atravessa o gateway.
[Implementação](../experiments/lr-10a-sdk-runtime/src/auth_network.rs),
[testes SDK reais com peer sintético](../experiments/lr-10a-sdk-runtime/tests/auth_network.rs),
[observações](../experiments/lr-10a-sdk-runtime/evidence/fix-4-gateway-observations.jsonl).
HTTP local não comprova TLS/DNS/SNI/CDNs ou autenticação Copilot. IDs recebidos do
runtime não são identidade de segurança; herança de stdio continua uma limitação.

Token explícito do SDK chega por ambiente ao runtime e foi herdado por um filho
sintético: escopo reduzido não equivale a proteção contra exfiltração. O callback
GitHubTokenProvider devolve access_token por RPC; não é proxy de credenciais.
Documentação oficial atual descreve fine-grained PAT user-owned/Copilot Requests,
mas aceitação/entitlement no CLI instalado não foi testada com credencial real.
**BLOCKED_AUTH_BOUNDARY / AWAITING_HUMAN_AUTHORIZATION**; sem criação/extração de PAT,
keyring, D-Bus, login, config ou sessões pessoais. **BLOCKED_NETWORK_BOUNDARY** para
provedor real; **BLOCKED_SUPERVISOR_FAILURE_CONTAINMENT** e **BLOCKED_REAL** preservados.

Verificação final: 38 Rust (26 herdados + 12 novos), 44 Python (40 herdados + 4 novos).
Rust/Cargo efetivos 1.98.1 já instalados no Fedora; o toolchain 1.94 temporário expirou,
sem reinstalação/download. Edition/MSRV de produção intactos. Dois deps já
transitivos/cacheados tornaram-se explícitos somente na POC para construir o DTO;
SDK/CLI e versões dos packages do lock permanecem iguais. Sem recompilar Tauri,
pois nenhum código/dependência de produção foi alterado. FIX-1/subreaper e FIX-3
namespace/política foram reutilizados sem modificação. Config verificada por stat
somente; processos atribuídos reclamados, sem sinais a processos externos.
[Verificação](../experiments/lr-10a-sdk-runtime/evidence/fix-4-verification.json).

**Impacto no release Narys 0.1 (17/10):** foi encontrado um ponto oficial de mediação
que evita construir nesta etapa um proxy/supervisor de produção. Ainda faltam
provas de autenticação contida, transporte HTTPS estrito, tráfego completo do CLI,
contenção de morte do worker e auditoria antes de A9. Não há estimativa validada que
justifique DEFER_TO_POST_RELEASE somente pelo prazo; priorização/escopo do release
serão humanos. Recomenda-se manter a POC isolada e não acoplar esses gates ao código
estável antes da decisão. Nenhuma inferência ou implementação de release nesta FIX.
O relatório reutilizável descreve somente a execução FIX-4; este documento e as
outras evidências permanentes continuam preservando o histórico.

Revisão FIX-4: Client::start da crate 1.0.17 descarta setProvider.success.
A POC exige ACK positivo explícito antes das consultas; a fixture prova que
false aceito pelo SDK é negado pela POC, e o CLI real confirmou true no reteste.
Callbacks durante startup e contenção adversarial não foram universalmente
comprovados; o probe real mantém gateway sem endpoint/credencial desde o início.

## Adendo — A9-HOST-ASSISTED (09/10/2026): BLOCKED_PRE_SEND

Nova autorização humana: uma tentativa SDK real na cota Student, assistida no
host, aceitando ausência de isolamento completo. Não autoriza pagamentos/overage,
YOLO/Autopilot, ferramentas agentivas, alterações pessoais, login/credenciais,
segunda inferência, retries ou fallback. A9_HOST_ASSISTED é distinto de A9_ISOLATED.
Este adendo não modifica evidências/conclusões históricas das FIXes 1–4.

Um executável/runner independentes realizaram metadata pelo SDK 1.0.17 e CLI
nativo pinado. O probe inicial desabilitava o fallback de credenciais por
--no-auto-login; foi corrigido e preservado como evidência inicial. O preflight
final, com resolução normal habilitada, informou authenticated=false; modelos
indisponíveis e quota_unknown por erros RPC sanitizados. Não se conclui logout,
necessidade de GUI ou causa da falha do keyring; nenhuma tentativa de desbloqueio,
login, inspeção ou cópia de credenciais foi realizada. Não reusar quota/modelos
históricos como snapshot atual. [Preflight final](../experiments/lr-10a-sdk-runtime/evidence/a9-host-real-preflight.json).

**BLOCKED_PRE_SEND: zero envios reais**, nenhuma sessão/conversa genuína criada,
nenhum teste real de persistência. Custo/elegibilidade do modelo, unidades de
cobrança e ausência de paid fallback permanecem não verificados. Documentação
atual usa AI credits por tokens; o snapshot de requests não demonstra o custo.
Não se implementou bypass financeiro nem live-send entry point. O marcador
persistente foi preparado como diretório privado; ATTEMPTED não foi consumido.

46 testes Rust (38 anteriores + 8 novos) e 47 Python (44 anteriores + 3 novos)
passaram com Rust/Cargo 1.98.1 preinstalados, offline/locked, opt-out de download,
jobs=2 e harness FIX-1 intacto. Testes de SDK send/error/timeout/restart/resume são
somente peers sintéticos; não PASS operacional. A claim atômica/fsync impede
segunda tentativa inclusive após corrupção/crash. Controles DenyAll/zero tools e
isolamento das FIXes continuam sendo retestados sem alteração.
[Regressões owned](../experiments/lr-10a-sdk-runtime/evidence/a9-host-owned-tests.json),
[verificação/preservação](../experiments/lr-10a-sdk-runtime/evidence/a9-host-verification.json).

Headless, nenhum GNOME/GDM ativado, sem atualização SDK/CLI/deps ou mudanças de
produção; config somente stat, inalterado; zero sinais de recuperação no preflight,
ECHILD e nenhuma identidade conhecida sobrevivente. Isso não resolve morte do
worker/adversarial containment. Bloqueios AUTH/NETWORK/SUPERVISOR do modo isolado
permanecem. Não se observou quota autenticada/delta/fatura; zero SDK sends reais
não é uma medição inventada de saldo. Uma chamada SDK pode gerar múltiplas chamadas
internas faturáveis. Não avançar à LR-10B automaticamente.

**LR-10A A9-HOST-ASSISTED — BLOCKED_PRE_SEND; IMPLEMENTAÇÃO/EXECUÇÃO CANDIDATA,
AGUARDANDO AUDITORIA INDEPENDENTE**. [Relatório atual](LR-10-LATEST-EXECUTION-REPORT.md)
e [perfil/reprodução](../experiments/lr-10a-sdk-runtime/HOST-ASSISTED.md).

## Adendo — A9-FIX-1: diagnóstico headless, sem inferência (09/10/2026 local)

**AUTH_NOT_RECOVERED; A9 real NOT_RUN.** A execução não criou/retomou/excluiu
sessões reais nem consumiu o marcador persistente. A autorização anterior de uma
tentativa futura não foi utilizada. Zero inferências enviadas pela POC não é uma
medição de cobrança externa, de saldo ou de quota atual.

A comparação estática confirmou que a FIX-2 herdava contexto e usava bwrap
com montagens diferentes; seu fingerprint de ambiente não foi registrado. A9
filtrava contexto e reduzia PATH. SDK 1.0.17, CLI nativo pinado e modo CopilotCli
foram preservados. A guarda ampla histórica não foi restaurada. Os contextos
opcionais XDG_CONFIG_HOME/DATA_HOME/CACHE_HOME/GH_CONFIG_DIR estão ausentes hoje;
não se fabricaram valores nem se expuseram credenciais para preencher a lacuna.

Quatro variantes finais com SDK/CLI reais (baseline, PATH validado, remoção
individual de DBUS_SESSION_BUS_ADDRESS e XDG_RUNTIME_DIR) retornaram auth=false.
Catálogo indisponível e quota_unknown por erro RPC sanitizado, sem entitlement
atual ou admissão financeira. Todos os processos atribuídos foram reclamados
por exhaustion do kernel, sem sinais de recuperação, com stat de config.json
igual antes/depois e serviço de credenciais já existente. GNOME/GDM permaneceram
ausentes. Socket/serviço presente não prova que a credencial possa ser usada;
não se conclui logout, keyring bloqueado ou GUI obrigatória. Causa desconhecida.

48 testes Rust (46 anteriores + 2) e 57 Python (47 anteriores + 10) passaram,
sem atualizar dependências/toolchain e sem recompilar Tauri. Fixtures positivas
de metadata são sintéticas; não substituem auth real. O guard atômico foi retestado
em diretórios sintéticos e permaneceu intacto; o diagnóstico real só usa stat
ancorado do diretório existente. Mantidos os três bloqueios de custo máximo,
paid fallback e estado privado autenticado, além dos gates do modo isolado e
limites de morte do worker. Nenhuma correção de descoberta foi comprovada, por
isso os wrappers A9 originais não foram alterados.

Evidências: [matriz final](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-real-auth-matrix.json),
[contrato](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-contract.json),
[regressões finais](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-final-regressions/a9-host-owned-tests.json)
e [verificação](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-verification.json).
Próximo passo proporcional: Luna revisar a lacuna de contexto histórico e decidir
eventual ação humana de autenticação por procedimento separado; não executar
desbloqueio/login/GUI nem inferência por iniciativa desta POC.

**LR-10A A9-FIX-1 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE.**

## Adendo — A9-FIX-2: autenticação com GNOME existente (10/10/2026)

**SDK_AUTHENTICATED_WITH_GUI observado; verificação de integridade BLOCKED;
FINANCIAL_ADMISSION BLOCKED; A9 real NOT_RUN.** Após intervenção humana externa
(login gráfico, desbloqueio e login interativo informados pelo usuário), um único
Client SDK 1.0.17 reconheceu auth=true, catálogo e quota com CLI nativo/RPC 1.0.95,
protocolo 3. A POC não iniciou/parou GNOME/GDM/Keyring, nem realizou login/logout.
Leu somente a propriedade booleana Locked, presença de contexto e stat de config.
Não criou/retomou/excluiu sessão nem enviou inferência ou reclamou o marker.

O caminho historicamente pinado agora contém SHA distinto. Um manifesto novo e
independente identifica 1.0.95; o pin 1.0.91 permanece intacto. Sem imagem antiga
verificada no caminho conhecido, não há contraste controlado de versões. GUI,
credencial disponível e imagem mudaram; a causa individual da recuperação não foi
provada. O perfil headless continua recusando GNOME e não foi retestado com GUI
desligada. Autenticação do CLI informada pelo usuário não substitui a observação
SDK, nem metadata valida compatibilidade de sessões ou inferência.

Consulta atual em 2026-10-10T03:34:13.229631Z: models.list retornou somente Auto,
sem multiplicador/preços/capabilities/policy; account.getQuota reportou premium
entitlementRequests=200, usedRequests=52, remainingPercentage=74.2, overage=0 e
flags de uso/overage após esgotamento=false. Unidades: requests_as_reported_by_runtime.
Chat/completions indicaram unlimited explicitamente; isso não autoriza cobrança,
nem transforma ausência de preço em custo zero. A coincidência com números
históricos não é reutilização de evidência; houve nova consulta, sem comprovação
independente da idade/cache do snapshot do provedor. Sem modelo de custo conhecido,
custo máximo, enforcement de paid fallback ou estado privado autenticado comprovados,
não há admissão financeira. Zero inferências da POC não é medição de fatura externa.

**Configuração não comprovadamente preservada:** inode/timestamps de config.json
mudaram durante o probe, tamanho igual (470 bytes). Autor desconhecido. Nenhum
conteúdo foi lido, nenhum restore foi feito e não houve probe real adicional após
essa detecção. A evidência original e seu status BLOCKED foram preservados; auth
true está no campo SDK interno. A classificação final separa observação de auth
e verificação bloqueada, testada somente por fixtures após o incidente.

O SDK reportou shutdown graceful (start=569 ms, stop=810 ms); harness comprovou
ECHILD, ausência de sobreviventes atribuídos e zero sinais de recuperação. As
regressões finais incluem 48 Rust e 68 Python aprovados, com fixtures de GUI,
hash/versão inválidos, segredo sintético, marker, timeout e integridade. Sem código
de produção alterado, não se repetiu Tauri. SDK/CLI/toolchain não foram instalados
ou atualizados pela POC; os executáveis locais Rust/Cargo 1.98.1 já estavam presentes.
Os limites de morte do worker/descendentes adversariais e gates isolados permanecem.

Evidências: [metadata original](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-gui-sdk-metadata.json),
[identidades e contrato](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-contract-inventory.json),
[validação final](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-final-validation/a9-host-owned-tests.json),
[preservação e verificação](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-verification.json).
Recomendação: FIX_AND_RETEST da integridade antes de novos probes; auditoria
independente deve decidir como prevenir/atribuir alterações sem acessar segredos.
Não avançar à inferência ou LR-10B. Para Narys 0.1, até 17/10, esta descoberta
reduz a incerteza de auth com GUI, mas não libera integração de produção nem
justifica atalhos de segurança; a priorização do trabalho restante é humana.

**A9-FIX-2 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**

## Adendo — A9-FIX-3: integridade e recuperação controlada (10/10/2026)

Investigação exclusivamente estática/sintética, sem CLI real, SDK metadata real,
sessões/inferência ou claim do marker. A mutação histórica de inode/timestamps com
tamanho igual permanece observada; não prova igualdade de bytes, corrupção,
vazamento ou escritor. **CONFIG_WRITER_ATTRIBUTION=INCONCLUSIVE;
CONFIG_INTEGRITY_VERIFICATION=BLOCKED.** SDK_AUTHENTICATED_WITH_GUI=
OBSERVED_REAL_PASS refere-se somente à A9-FIX-2, headless=NOT_PROVEN,
finance=BLOCKED, A9=NOT_RUN. Nenhum resultado anterior foi adulterado.

O helper antigo segue symlinks, compara quatro campos e agrupa OSError. O módulo
novo usa somente metadata ancorada/no-follow, distingue ausência/acesso negado/
falha/interrupção e não abre conteúdos. O replay verifica um artefato Git fixo por
SHA256, sem consultar config pessoal. Metadata estável é observação, não prova de
integridade semântica/criptográfica. Mudanças desconhecidas falham fechadas. Apenas
uma revisão sintética fixa, com FD próprio e bytes conhecidos, recebe
LEGITIMATE_CHANGE_PROVEN no escopo da fixture. Nenhum contrato implementado aprova
escrita real do Copilot. Auth positiva, tamanho igual e estabilidade posterior não
são bypasses; wrappers reais antigos permanecem intactos.

Documentação oficial atual descreve config.json gerenciado; SDK pinado delega
RPC/startup/shutdown ao CLI. Atualização legítima é plausível, mas fase/escritor/
semântica da imagem 1.0.95 não são demonstrados. O pacote nativo descreve bundle
embutido; fonte standalone do escritor não foi encontrada nos arquivos públicos
legíveis examinados, sem extração/instrumentação do runtime. Atualização interna,
substituição atômica e concorrência continuam hipóteses. Não se inspecionou conteúdo
privado, Keyring, tráfego autenticado ou memória.

48 Rust e 86 Python passaram (68 anteriores +18 novos), Cargo offline/locked,
skip download, jobs=2 e ownership FIX-1 intacto. Fixtures: estabilidade, rename
atômico de tamanho igual, tamanho/timestamps, concorrência sincronizada, ausência/
EACCES injetado, symlinks, interrupção/I/O, revisão legítima sintética, proveniência
desconhecida e auth/finance independentes. ECHILD/identidades ausentes comprovados
no escopo cooperativo. Sem Tauri/GUI: nenhum código/dependência de produção ou Rust
mudou. Limites de morte do worker/adversariais não são resolvidos por esses testes.

Nenhum chmod/lock/read-only/restore/copy/delete pessoal, opção COPILOT_HOME/
base_directory ou serviço mudou. cwd/logs não confinam state; base_directory envolve
auth/sessões/telemetria e session_fs não protege config global. Proposta limitada
para eventual nova observação real permanece PENDING_USER_AUTHORIZATION, com
riscos/condições de interrupção; não executada.

Evidências: [replay](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-historical-review.json),
[investigação](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-static-investigation.json),
[casos](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-synthetic-cases.json),
[regressões](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-regressions/a9-host-owned-tests.json)
e [contrato](../experiments/lr-10a-sdk-runtime/CONFIG-INTEGRITY.md).
PASS parcial de fixtures/contrato; gate real BLOCKED, recomendação FIX_AND_RETEST,
sem LR-10B. Headless futuro requer prova própria de credenciais/estado; sucesso
com GNOME não é equivalente. O release 0.1 em 17/10 não autoriza bypass; manter
POC fora de produção e priorização humana.

**A9-FIX-3 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**

## Adendo — A9-FIX-4: gates proporcionais à operação (10/10/2026)

[Contrato experimental](../experiments/lr-10a-sdk-runtime/METADATA-CONFIRMATION.md)
separa resposta SDK, acesso estrutural, drift, autoria, conteúdo e admissão.
METADATA_READ_ONLY pode aceitar observação com drift de inode/tamanho/timestamps;
isso não prova escrita legítima nem equivalência de conteúdo. SESSION_OPERATIONS,
INFERENCE/financeiro, AGENT_ACTIONS e HEADLESS_OPERATION continuam bloqueados ou não
comprovados, sem transferência de PASS. O classificador/replay histórico mantém
METADATA_CHANGE_UNATTRIBUTED/BLOCKED e todas as evidências anteriores intactas.

Snapshot ancorado recebeu checagem estrutural opt-in: tipo regular, UID próprio,
mode0600/link único, diretórios não graváveis por outros e sem symlinks. Não lê
conteúdo pessoal. Novo executável independente limita SDK1.0.17 a start/status/auth/
shutdown, com observação por fase e interrupção de RPCs opcionais após falha
estrutural/versão. Sem catálogo/quota/sessões/send/retries. Preserva pin independente
CLI1.0.95, environment allowlist, private cwd/logs e harness FIX-1; profile GUI
continua NOT_SANDBOX. Nenhuma mudança financeira ou de produção foi feita.

52 Rust e 100 Python PASS (4 Rust/14 Python novos), testes offline/locked/jobs2,
incluindo drift+auth positiva simultâneos, auth negativa+estabilidade, falha de
acesso/cleanup, escopo financeiro fechado e lista restrita de métodos RPC em peer
local. Formatação, sintaxe, JSON e identidades/child exhaustion verificados. Testes
sintéticos não validam auth, writer ou autorização operacional reais. Tauri/UI
NOT_RUN pois produção, dependências, MSRV e Edition não mudaram.

O preflight real autorizado foi interrompido **antes de Client start**: zero
Copilot/Node/Bun conhecidos, mas três inspeções same-UID ficaram indisponíveis;
uma varredura posterior independente registrou três EACCES em executable metadata,
sem correlacionar PIDs/escritor. Ausência de concorrência não pôde ser comprovada. Resultado
NOT_RUN_CONCURRENCY_UNVERIFIED, sem retry/CLI help/version/autenticação/catálogo/
quota. Cofre/GUI não foram sondados após o bloqueio nem manipulados. O SHA nativo
foi validado por leitura apenas do executável público; a configuração passou no
stat estrutural. Metadados finais concordam na janela de preflight, sem provar
conteúdo ou legitimar a mutação histórica. Escritor continua INCONCLUSIVE.

Zero inferências/sessões reais, A9 marker ausente/não consumido, sem credenciais ou
conteúdo pessoal coletados. Cleanup PASS somente no escopo sintético owned; SDK
real NOT_RUN. Reserva separada do runtime diagnóstico não chegou a ser criada.
Bloqueios financeiros, headless, estado privado, isolamento/auth/network e morte
do supervisor continuam. Não se amplia /proc, serviços ou permissões para PASS.

Evidências: [preflight](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-metadata-confirmation.json),
[offline](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-offline-verification.json),
[casos](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-synthetic-cases.json),
[regressões](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-final-regressions/a9-host-owned-tests.json),
[verificação final](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-final-safety.json).
PASS parcial do contrato sintético; confirmação SDK atual BLOCKED, sem LR-10B.
O release0.1 em17/10 mantém a POC fora de produção e priorização humana; headless
precisa de prova própria. Novo ensaio exige resolver a pré-condição por método
não invasivo revisado e nova autorização, nunca encerrar processos externos.

**A9-FIX-4 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**

## Adendo — A9-FIX-4R: concorrência e metadata real (10/10/2026)

Correção pontual: exe EACCES isolado não comprova Copilot concorrente. O survey
preserva UID/comm/PPID/PID-start-time, revalida identidade e separa runtime pinado,
runtime potencial, contexto Codex, serviços essenciais, unrelated/partial e
identidade suspeita. Somente indicadores relevantes ou identidade básica
indisponível/instável bloqueiam METADATA_READ_ONLY; sem alegar exclusividade do UID,
isolamento ou prova contra adversário. Survey não concede autoridade de término;
nenhum processo externo foi encerrado. Sem force/env bypass ou financeiro aberto.

Dois surveys reais examinaram105 processos same-UID:6 próprios/ancestrais/descendentes
Codex,10 essenciais,88 unrelated e1 partial. EACCES em systemd/(sd-pam)/sshd-session
e executable missing em zypak-sandbox ficaram registrados com identidade básica
estável, não rotulados como Copilot. Copilot identificado0/runtime ambíguo0. Não se
inspecionou cmdline/environ, arquivos pessoais ou conteúdos de credenciais.

Nova identidade one-shot FIX-4R, evidência e reserva próprias, preservando FIX-4 e
marker A9. Após52 Rust/113 Python PASS offline/locked/jobs2, uma única invocação
SDK1.0.17/CLI1.0.95 pinado confirmou status e authenticated=true; GNOME/GDM/Keyring
já ativos e coleção login desbloqueada antes/depois, sem intervenção. Zero models/
quota/session/send/inferência/tools/login/logout. SDK shutdown graceful, harness
ECHILD, zero recovery signals/survivors e identidades atribuídas ausentes.

Drift observado before_start→after_start: inode3768738→3773129, tamanho470 constante,
mtime/ctime1791603250606145370→1791625843024779423. Estrutura600/UID/link compatível
em todas as fases, snapshots posteriores concordam. Correlação de fase não prova
escritor/legitimidade/igualdade de bytes; nenhuma leitura/restore/chmod/lock pessoal.
METADATA_AUTH_OBSERVATION=PASS_REAL, writer=INCONCLUSIVE, sessões/financeiro/ações
BLOCKED, headless NOT_PROVEN, inferência NOT_RUN, marker ausente/não consumido.

[Evidência real](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-metadata-confirmation.json),
[fixtures](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-concurrency-fixtures.json),
[regressões](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-regressions/a9-host-owned-tests.json),
[verificação](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-final-verification.json).
135 arquivos históricos de evidence preservados byte a byte. Produção/pins/SDK/
Cargo.lock/MSRV/Edition/Broker/IPC não alterados; Tauri completo NOT_RUN por escopo.
Morte do worker/adversariais, auth headless/isolada, rede, estado privado e financeiro
continuam gates separados. Release17/10: integração continua fora da produção;
nenhuma inferência ou LR-10B autorizada por este PASS de metadados.

**A9-FIX-4R IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**
