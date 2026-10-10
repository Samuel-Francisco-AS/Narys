# LR-10A / A9-FIX-4R — Concorrência e confirmação real de metadados

## 1. Identificação e resultado

Narys, exclusivamente A9-FIX-4R, 10/10/2026, Fedora 44 via SSH/terminal.
Branch `lr-10a-sdk-runtime-feasibility`.
Base local/remota conferida: `9c7b673144741fd34a4da1f3d3d9039d065da4ec`.
Main local/remota preservada: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`.
Workspace inicial limpo; nenhum trabalho humano descartado.

Implementação testada e usada no ensaio:
[cbb7b93636de8c68852f8d480c4f05911ebb838a](https://github.com/Samuel-Francisco-AS/Narys/commit/cbb7b93636de8c68852f8d480c4f05911ebb838a),
commitada/enviada antes deste relatório. Os hashes as-executed coincidem com seus
arquivos; nenhum código foi alterado após o ensaio. O HEAD documental final é o
commit que publica este arquivo, verificável no
[histórico da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility/docs/LR-10-LATEST-EXECUTION-REPORT.md),
sem SHA autorreferencial. Commit final modifica somente o relatório. Sem PR,
merge, rebase, reset destrutivo, force-push ou alteração da main.

**PASS técnico candidato da correção e da observação SDK de status/auth.
Uma invocação real, authenticated=true, shutdown gracioso e cleanup owned completo.
Sessões, financeiro, ações, headless e inferência não foram promovidos a PASS.**

**A9-FIX-4R IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**

## 2. Escopo e autorização

Corrigir o bloqueio falso causado por EACCES isolado em /proc/PID/exe e executar
uma nova invocação autorizada: Client start, getStatus, auth.getStatus, shutdown.
Sem models.list/account.getQuota, sessão create/resume/delete, send, inferência,
tools, login/logout, credencial explícita, mudanças de serviço/boot ou download.
O usuário confirmou Codex como única atividade de desenvolvimento intencional e
autorizou limpeza restrita de processos comprovadamente dispensáveis. **Não houve
concorrente identificado que exigisse limpeza; zero processos externos encerrados.**

HOST_ASSISTED_WITH_GUI é acesso sob o usuário, **não sandbox**. Não se enfraqueceu
o modo isolado/offline nem se comprovou operação sem GNOME. Produção, Broker,
ExecutionAuthority, TaskGraph, LR-8.5, IPC, UI, dependências, MSRV e Edition intactos.

## 3. Correção pontual de concorrência

[concurrency/proc_identity/classificação](../experiments/lr-10a-sdk-runtime/confirm_a9_metadata.py)
preservam UID, comm, PID, PPID e start_ticks antes de tentar executable metadata.
Identidade básica é revalidada após a inspeção, incluindo UID e nome; PID/start-time
mudando ou identidade básica indisponível bloqueiam. Alternância de estado do
scheduler não é reutilização de PID. Nenhum argumento, environ ou conteúdo de
processos/configurações é lido.

| Categoria | Evidência/decisão para METADATA_READ_ONLY |
| --- | --- |
| CONFIRMED_COPILOT | Executable dev/inode corresponde ao native pinado, mesmo renomeado; bloqueia. |
| POTENTIALLY_CONCURRENT_RUNTIME | Nome Copilot sem imagem comprovada ou Node/Bun/npm/npx fora do contexto próprio; bloqueia. |
| OWN_CODEX_OR_HARNESS | Relações observadas de ancestralidade e descendência do Codex; protegido, sem bloquear por si só. Indicador Copilot tem prioridade. |
| ESSENTIAL_PROCESS | Nomes conhecidos GNOME/GDM/Keyring/D-Bus/systemd/SSH/tmux; protegidos, EACCES isolado não bloqueia. |
| OBSERVED_UNRELATED | Identidade básica e executable metadata observados, sem indicador relevante; não bloqueia metadata. |
| PARTIALLY_INACCESSIBLE | UID/comm/PPID/start-time estáveis, exe negado/ausente e sem indicador relevante; registra limite, não bloqueia só por isso. |
| SUSPICIOUS_OR_UNASSESSABLE | Identidade básica desconhecida, instável ou reutilizada; bloqueia. |

Aceitação significa METADATA_SURVEY_ACCEPTED_WITH_LIMITS, **exclusivity_proven=false**,
não ausência absoluta de concorrência ou proteção contra outro ator do mesmo UID.
Nomes podem ser disfarçados; survey não é atômico nem impede lançamentos posteriores.
Admissão de metadata não autoriza outras operações.

Observação e encerramento são separados: termination_admission nega processos
protegidos e também nega os demais por falta de disposability comprovada. Não foi
implementado terminador automático, force=true ou override de environment.
Não houve kill/pkill/killall, SIGTERM ou SIGKILL sobre processos externos.

## 4. Processos efetivamente observados

Dois surveys imediatamente antes do serviço e do SDK, ambos com105 processos
same-UID:6 no contexto próprio/Codex,10 essenciais,88 unrelated e1 partial;
Copilot pinado0, runtime potencial0, identidade básica suspeita0.
As linhas individuais PID/start-time/UID/comm estão na
[evidência real](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-metadata-confirmation.json).

| Processo | PID/start_ticks observados | Limite | Tratamento |
| --- | --- | --- | --- |
| systemd | 2584 /3076 | exe access_denied | Essencial/protegido; nenhum sinal. |
| (sd-pam) | 2586 /3080 | exe access_denied | Essencial/protegido; nenhum sinal. |
| sshd-session | 4330 /79186 | exe access_denied | Essencial/protegido; nenhum sinal. |
| zypak-sandbox | 3793 /3575 | exe missing, identidade básica estável | Partial, sem indicador Copilot/runtime; nenhum sinal. |

UID1000 e comm foram preservados apesar de executable indisponível. Esses dados
não demonstram exclusividade do host nem que um processo parcialmente acessível
seria seguro de encerrar. A evidência histórica FIX-4 de unavailable3/EACCES3 foi
preservada, sem reclassificação retroativa ou alegação de três Copilots.

## 5. Identidade one-shot e pré-condições

[Entry FIX-4R](../experiments/lr-10a-sdk-runtime/confirm_a9_metadata_recovery.py)
seleciona identidade fixa no driver compartilhado; somente FIX-4 e FIX-4R são
nomes admitidos, sem paths/flags/env para criar retries. Evidência fresca
a9-fix-4r-metadata-confirmation.json usa O_EXCL; o
[binário independente](../experiments/lr-10a-sdk-runtime/src/bin/a9-metadata-confirm-r.rs)
reserva atomicamente
[a9-fix-4r-runtime-reservation.json](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-runtime-reservation.json)
antes de Client start. Reserva0600/owner próprio verificada; crash/timeout não
liberaria nova tentativa. Não apagar ou rerun este diagnóstico concluído.

FIX-4 evidence permaneceu byte-idêntica; sua reserva runtime antes ausente não foi
criada. Reserva FIX-4R é diagnóstica, não o marker de inferência A9. Marker A9
ausente antes/depois, sem leitura do conteúdo, criação ou claim.

Pré-condições PASS: offline tests/hashes finais, estrutura config, surveys
proporcionais, GUI e serviço já presentes, socket acessível e coleção login
Locked=false. GNOME/GDM/Keyring permaneceram ativos e Locked=false depois.
Sondas booleanas não ativantes, sem login/logout/unlock/serviço novo pela POC.

Native1.0.95 SHA validado antes/depois:
`9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`.
SDK crate1.0.17/lockfile e pin histórico anterior preservados. Sem CLI --version/help
nesta FIX; getStatus confirmou o contrato version1.0.95/protocol3 pelo check do
código pinado. Binary experimental SHA:
`7bac116dd43b10ba12d3d25d54776c7cfdf3ed228857ce1190f8f83689c3988b`.
[Hashes das fontes/testes](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-offline-verification.json).

## 6. Resultado real, drift e gates

Ensaio concluído em **2026-10-10T09:50:44.847325+00:00**. Um Client start,
um status.get e um auth.getStatus, sem retry; startup inclui handshake SDK normal.
auth retornou true; identidade pessoal não publicada. Stop/EOF controlados.

| Gate | Estado observado |
| --- | --- |
| METADATA_AUTH_OBSERVATION | PASS_REAL, SDK1.0.17/CLI1.0.95 neste ensaio. |
| CONFIG_STRUCTURAL_CHECK | PASS_METADATA_ACCESS em todas as fases. |
| CONFIG_DRIFT_OBSERVED | true; inode/mtime/ctime mudaram. |
| CONFIG_WRITER_ATTRIBUTION | INCONCLUSIVE. |
| SESSION_ADMISSION | BLOCKED; zero operações reais. |
| FINANCIAL_ADMISSION | BLOCKED; nenhuma nova consulta de catálogo/quota. |
| AGENT_ACTION_ADMISSION | BLOCKED; nenhuma autoridade nova. |
| HEADLESS_AUTH | NOT_PROVEN. |
| REAL_INFERENCE | NOT_RUN; zero mensagens/inferências reais. |
| PROCESS_CLEANUP | PASS no escopo atribuído; graceful_no_recovery. |
| ATTEMPT_MARKER | ABSENT_NOT_CLAIMED. |

Config before_start→after_start: inode3768738→3773129, tamanho470 em ambos,
mtime/ctime1791603250606145370→1791625843024779423. Device49, mode0600, UID/GID1000,
link1 permaneceram compatíveis; after_status/after_auth/after_shutdown concordaram.
**Correlação na janela de start não prova autoria, legitimidade ou igualdade de
conteúdo.** Nenhum conteúdo foi lido/hashed nem config restaurada/alterada pela POC.
Não se invalidou auth por drift isolado nem se autorizou qualquer sessão/financeiro.
A mutação histórica FIX-2 permanece desconhecida, independente desta nova observação.

Start2985ms, stop1057ms, wall4323,77ms e cleanup harness34,08ms; observação única,
sem benchmark prolongado. RSS agregado amostrado pico326598656bytes, CPU lower
bound1,59s, owned pico3. Processos globais330→332 não são um inventário de ownership;
não se atribui essa diferença ao experimento nem se sinaliza processos externos.
ECHILD, survivors vazios, ownership errors vazios e identidades PID/start-time
ausentes confirmados. Zero recovery signals. sdk_shutdown_verified=false é o
campo genérico do harness; o relatório SDK separado confirma stop graceful.

## 7. Testes e verificações

**52 Rust +113 Python PASS, zero FAIL**, executados antes do SDK real;13 novos
Python e ajuste do fixture antigo para os campos de identidade adicionais.
Nenhum teste sintético foi tratado como auth operacional real.

| Regressão requerida | Resultado |
| --- | --- |
| EACCES exe de processo não Copilot, identidade básica estável | PASS fixture: partial registrado, metadata aceita com limites. |
| GNOME/serviços legítimos | PASS fixture: essential/protegidos, sem falsa concorrência. |
| Codex/ancestrais/descendentes necessários | PASS fixture: relações protegidas; sem sinais. |
| Copilot pinado renomeado e nome Copilot com exe negado | PASS fixture: confirmed/potential bloqueiam. |
| Node/Bun fora do próprio contexto | PASS fixture: ambiguidade bloqueia, não autoriza matar. |
| Desaparecimento durante scan | PASS fixture: evento registrado sem falso concorrente. |
| PID reuse/identidade instável | PASS fixture: bloqueio. |
| Troca de scheduler state | PASS fixture: não confundida com PID reuse. |
| Core identity inacessível versus exe-only EACCES | PASS fixture: somente core ausente continua bloqueando. |
| Tentativa de encerrar essencial externo | PASS fixture: DENIED_PROTECTED_PROCESS, nenhuma chamada os.kill. |
| Concorrência/auth/force não liberam sessões/financeiro/tools | PASS fixture: gates sensíveis fechados; force parâmetro rejeitado. |
| Identidades independentes/reserva/retry arbitrário | PASS fixture: nomes fechados, O_EXCL/create_new, sem marker A9. |
| Regressões anteriores ownership/pidfd/timeout/DenyAll/boundary/gateway | PASS fixtures/Linux owned. |

[Log Rust](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-regressions/a9-host-rust-tests.txt),
[log Python](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-regressions/a9-host-python-tests.txt),
[casos de survey](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-concurrency-fixtures.json),
[ownership tests](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-regressions/a9-host-owned-tests.json),
[verificação final](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-final-verification.json).
12 observações de survey foram emitidas pelos13 testes novos; o teste de identidade
fixa examina o plano/guard sem fazer survey.

Comandos principais realmente executados:

```sh
git status --short --branch
git rev-parse HEAD main
git ls-remote origin refs/heads/lr-10a-sdk-runtime-feasibility refs/heads/main
python3 -m py_compile experiments/lr-10a-sdk-runtime/confirm_a9_metadata.py experiments/lr-10a-sdk-runtime/confirm_a9_metadata_recovery.py experiments/lr-10a-sdk-runtime/tests/test_metadata_concurrency.py experiments/lr-10a-sdk-runtime/tests/test_metadata_policy.py
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-regressions
python3 experiments/lr-10a-sdk-runtime/confirm_a9_metadata_recovery.py
git diff --cached --check
git push origin lr-10a-sdk-runtime-feasibility
```

Runner inalterado: Cargo --offline --locked, COPILOT_SKIP_CLI_DOWNLOAD=1,
CARGO_BUILD_JOBS=2/test-threads1/RUSTC/RUSTDOC locais, depois unittest sob harness.
Rust/Cargo1.98.1 e Python3.14.7 preexistentes; não se declara novo reteste Rust1.94.
rustfmt local já instalado --edition2021 --check no binário novo passou; AST/sintaxe,
JSON/JSONL, links/diff e escopo revisados. Whitespace final normalizado apenas em
logs txt novos. Scan limitado token/Bearer/private-key teve zero matches, não uma
garantia universal. Tauri/UI completo NOT_RUN: produção/dependências não mudaram.

## 8. Segurança, arquivos e preservação

135 artefatos históricos evidence byte-idênticos à base. Harness, shared Rust
metadata flow, host_assisted/options, GUI/headless helpers, boundary, runtime pins,
Cargo e contratos financeiros intactos. Nenhuma mudança de credenciais, Keyring,
PAM, configuração explícita, serviços ou boot. Normal credential resolution pelo
CLI permitida; nenhum token/config pessoal/keyring item extraído ou publicado.
Sem cmdline/environ/strace/memória/tráfego autenticado; somente metadados sanitizados.

Arquivos alterados: confirm_a9_metadata.py e tests/test_metadata_policy.py;
novos confirm_a9_metadata_recovery.py, src/bin/a9-metadata-confirm-r.rs e
tests/test_metadata_concurrency.py; adendos METADATA-CONFIRMATION.md e documento
permanente; evidências a9-fix-4r-* e cinco arquivos em a9-fix-4r-regressions/.
Este relatório substitui só a versão anterior reutilizável, preservada no Git.

Ownership/subreaper FIX-1 e timeouts preservados; recovery forçado não foi necessário.
Limites de morte inesperada do worker, descendentes adversariais, namespaces,
reparenting e processos ininterruptíveis não foram resolvidos. Survey por nome/
identidade parcial não é contenção nem prova contra spoofing de mesmo UID. Profile
host-assisted não impede leitura genérica do host pelo runtime; não virou sandbox.

## 9. Decisão e pendências

Correção falsa-concorrência e confirmação metadata: **PASS técnico candidato**,
aguardando auditoria independente. Não é PASS definitivo LR-10A nem prontidão A9.
Nenhuma inferência enviada, nenhuma tentativa A9 consumida, nenhum modelo/quota
atual observado. Saldo histórico200/52 não é admissão; não se presume cobrança
externa zero. Uma futura chamada SDK não implica uma única unidade faturável.

Permanecem pendentes: financeiro/modelo/unidades/custo máximo/no fallback pago,
estado privado e persistência genuína, operação headless, auth/rede isoladas e
contenção de falha do supervisor. Autenticação positiva não concede ações Narys.
Recomendação: auditar este resultado e decidir humanamente o próximo gate separado;
não avançar automaticamente para inferência, sessões ou LR-10B.

Release Narys0.1 em17/10/2026: correção encerrada sem investigação expansiva ou
limpeza desnecessária; POC continua fora da produção. Prazo não substitui os gates
financeiros/autoridade e não autorizou pagamento ou perda de trabalho humano.
