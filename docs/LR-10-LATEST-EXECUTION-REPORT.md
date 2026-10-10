# LR-10A / A9-FIX-4 — Operation-Scoped Integrity Gates

## 1. Identificação e entrega

Narys, exclusivamente A9-FIX-4, 10/10/2026, Fedora44 via SSH/terminal.
Branch: `lr-10a-sdk-runtime-feasibility`.
HEAD inicial local/remoto: `499a2c37f4c7d19c62307d824bae122819ff8f0c`.
Main local/remota: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`, preservada.
Workspace inicial limpo, sem trabalho humano descartado.

Implementação testada, evidências e documentação técnica:
[ac11f1e19f64f38f9ebe3d5f5b180ff3163fc3b5](https://github.com/Samuel-Francisco-AS/Narys/commit/ac11f1e19f64f38f9ebe3d5f5b180ff3163fc3b5),
commitado e enviado antes deste relatório. Os hashes da implementação usada no
preflight coincidem com esse commit; nenhum código do diagnóstico mudou depois
da execução. O HEAD documental final/local/remoto é o commit que publica este
arquivo, verificável no
[histórico do relatório](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility/docs/LR-10-LATEST-EXECUTION-REPORT.md),
sem SHA autorreferencial. Commit final modifica somente o relatório.
Sem PR, merge, rebase, reset destrutivo, force-push ou alteração da main.

**PASS parcial do contrato/testes offline; confirmação real
NOT_RUN_CONCURRENCY_UNVERIFIED. Recomendação FIX_AND_RETEST, sem aprovação
operacional ou avanço à LR-10B.**

**A9-FIX-4 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**

## 2. Objetivo, autorização e escopo

Separar acesso estrutural, drift, resposta SDK, proveniência, admissão de sessão/
ação e segurança financeira. Imutabilidade do config.json não é exigência universal
para status/auth. Detectar drift não significa corrupção nem escrita legítima.
O contrato histórico e a mutação desconhecida não foram apagados ou legitimados.

Autorização desta FIX: **no máximo uma invocação start → getStatus → auth.getStatus
→ shutdown**, após pré-condições satisfeitas. Sem catálogo/quota, sessões, send,
inferência, ferramentas/MCP, login/logout, tokens explícitos, alteração de config/
serviço, update/download ou retry. A pré-condição de concorrência falhou antes
do SDK: **zero invocações reais**. Nenhuma tentativa de inferência foi consumida.

HOST_ASSISTED_WITH_GUI continua **não sandbox**, sob o mesmo usuário Linux.
HOST_ASSISTED_HEADLESS e A9_ISOLATED continuam separados. Não se ampliou filesystem,
rede, Keyring, D-Bus ou environment para obter PASS. Produção/Broker/ExecutionAuthority/
AgentRegistry/IPC/UI/TaskGraph/LR-8.5 não mudaram. Financeiro, SDK/CLI pins, Cargo.lock,
MSRV e Edition preservados. Nada foi instalado ou atualizado globalmente.

## 3. Contrato proporcional por operação

[Contrato](../experiments/lr-10a-sdk-runtime/METADATA-CONFIRMATION.md),
[política](../experiments/lr-10a-sdk-runtime/metadata_policy.py) e
[snapshot](../experiments/lr-10a-sdk-runtime/config_integrity.py).

| Operação | Política implementada | Limite |
| --- | --- | --- |
| METADATA_READ_ONLY | Resposta observada pode ser aceita junto de drift, com estrutura, serviço, protocolo e cleanup verificados. | Não aprova conteúdo/escrita/sessão/billing. Nesta FIX só status/auth; modelos/quota futuros exigem autorização própria. |
| SESSION_OPERATIONS | BLOCKED | Estado privado autenticado, deny policy e ausência de interferência não comprovados operacionalmente. |
| INFERENCE | BLOCKED | Modelo, unidades, custo máximo e ausência de fallback pago não verificados. |
| AGENT_ACTIONS | BLOCKED na POC | Autenticação não concede autoridade, escopo ou aprovação da Narys. |
| HEADLESS_OPERATION | NOT_PROVEN | GUI não comprova funcionamento sem GNOME. |

Classificador puro produz AUTH_TRUE_OBSERVED/AUTH_FALSE_OBSERVED, nunca PASS_REAL.
Somente o driver poderia estabelecer origem SDK real com hashes e cleanup.
`admission` é verificação observacional de escopo, não dispatcher executável.
Flags, claims de fixture, auth ou dicionário forjado não liberam sessão/inferência/
financeiro. Blockers financeiros anteriores não foram alterados.

Snapshot opt-in exige regular file, UID próprio, mode0600, link único e ancestrais
sem symlink, directories root/usuário sem group/world write. Exceção apenas /tmp
root-owned sticky para fixtures privadas. O_PATH/no-follow/stat ancorado verifica
identidade e acesso dos pais antes/depois do open. Missing/EACCES/interrupção/falha
bloqueiam verificação dependente. Nenhum conteúdo pessoal é aberto ou hashed.
Snapshot e classificação histórica por padrão mantêm comportamento anterior.

PASS_METADATA_ACCESS é pontual, não conteúdo seguro nem proteção contra mesmo UID.
Inode/tamanho/timestamps distintos podem coexistir com auth positiva e acesso
estrutural compatível. Igual tamanho/metadados não prova equivalência criptográfica;
auth não prova integridade, e estabilidade posterior não legitima mutação anterior.
Writer attribution continua INCONCLUSIVE, sem contrato de escrita real legítima.

## 4. Implementação do diagnóstico restrito

[Fluxo Rust](../experiments/lr-10a-sdk-runtime/src/metadata_confirmation.rs) e
[binário](../experiments/lr-10a-sdk-runtime/src/bin/a9-metadata-confirm.rs) só usam
start/status/auth/shutdown, sem APIs de modelos/quota/sessões/send. Start inclui
handshake normal; stop usa runtime.shutdown/EOF e bounded timeout anterior.
Cinco snapshots previstos: before_start/after_start/after_status/after_auth/
after_shutdown. Falha estrutural ou versão/protocolo incompatível interrompe RPCs
opcionais e entra em shutdown limitado. Auth observada sobrevive a falha posterior,
mas a verificação dependente continua bloqueada.

Helper Python fixo de stat é instrumentação confiável, não tool agentiva. Retorna
somente JSON sanitizado e descarta stderr. cwd/logs0700, normal credential resolution,
LogLevel::None, --disable-builtin-mcps e allowlist anteriores mantidos. Sem token,
base_directory ou COPILOT_HOME override. DenyAll/zero tools/MCP/skills/hooks dos
contratos anteriores foram preservados e retestados em fixtures. Nenhuma sessão
real os exercitou; não se declara cobertura de um agente real.

[Driver](../experiments/lr-10a-sdk-runtime/confirm_a9_metadata.py) exige hashes
offline/binary finais, native SHA, marker seguro, acesso estrutural, GUI/serviço
já acessível/desbloqueado e ausência de concorrência verificável. Só adquire contexto
não secreto allowlisted, nunca valores de tokens. Sondas de serviço seriam booleanas
não ativantes, mas não foram alcançadas no fluxo real desta FIX.

Output fixo/fresco O_EXCL impede rerun. O binário possui reserva separada exclusiva
`a9-fix-4-runtime-reservation.json` antes de Client start, persistente após erro/
crash/timeout, sem path/flag alternativo. Não é marker A9. **Binário não alcançado;
reserva runtime não criada.** Não apagar evidência para rerun; próximo ensaio
depende de execução revisada e autorização própria.

## 5. Preflight observado e bloqueio real

[Evidência](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-metadata-confirmation.json),
**2026-10-10T09:25:03.337813+00:00**:

| Pré-condição | Resultado real observado |
| --- | --- |
| Contrato offline/binary | PASS: hashes e regressões finais correspondentes. |
| Native pin | PASS SHA/ELF: `9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`. Manifest independente1.0.95/SDK1.0.17 intacto. |
| Marker A9 | Ausente; diretório validado sem conteúdo/claim/criação. |
| Estrutura config | PASS_METADATA_ACCESS: regular/owner/mode/link e caminho ancorado compatíveis. |
| Concorrência antes do serviço | BLOCKED: Copilot conhecido0, Node/Bun/npm/npx ambíguos0, inspeções same-UID indisponíveis3. |
| GNOME/serviço/Locked e segunda inspeção | NOT_RUN; fluxo parou antes desses passos. |
| Client start/status/auth/shutdown | NOT_RUN; zero CLI real iniciado. |

Inspeção lê comm e metadata de executável, nunca cmdline/environ/argumentos,
config content ou Keyring items. Dev/inode detectaria imagem pinada renomeada;
Copilot conhecido e Node/Bun ambíguo bloqueiam. Identidade indisponível não é
presumida segura. Observação pontual não impede lançamento posterior nem exclui
imagens disfarçadas: não é contenção adversarial ou exclusão mútua.

[Verificação independente posterior](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-final-safety.json),
09:26:25UTC, registrou três EACCES em executable metadata. Sem PIDs publicados ou
correlação com os três casos anteriores. **Não se estabeleceu que fossem Copilot
nem quais eram os processos.** Pode bloquear processos legítimos protegidos.
Não houve strace/ptrace/sudo, mudanças /proc ou sinais a processos externos.
Ausência de comm Copilot isoladamente não resolve a identidade indisponível.

Não ocorreu auth/status real, CLI help/version, catálogo/quota ou retry.
Comparação final config foi stat-only e concordou na janela do preflight; isso
não prova conteúdo nem legitima a mutação histórica. Versão CLI95 é identidade
do manifest histórico validado pelo SHA, não resultado de novo --version/RPC.

## 6. Gates independentes

| Gate | Resultado | Fonte/limite |
| --- | --- | --- |
| METADATA_AUTH_OBSERVATION | NOT_RUN_CONCURRENCY_UNVERIFIED | Pré-condição bloqueou antes de Client start; nenhum PASS_REAL novo. |
| CONFIG_STRUCTURAL_CHECK | PASS_METADATA_ACCESS | Snapshot ancorado pontual, sem conteúdo. |
| CONFIG_DRIFT_OBSERVED | null no gate SDK não executado | Comparação separada preflight→stat final false; histórico FIX-2 true preservado. |
| CONFIG_WRITER_ATTRIBUTION | INCONCLUSIVE | Sem comprovar escritor/semântica histórica. |
| SESSION_ADMISSION | BLOCKED | Zero criação/retomada/exclusão real. |
| FINANCIAL_ADMISSION | BLOCKED | Models/quota atuais não consultados; units/cost/fallback não resolvidos. |
| AGENT_ACTION_ADMISSION | BLOCKED | Metadata/auth não concede autoridade. |
| HEADLESS_AUTH | NOT_PROVEN | Não alterou nem testou ausência de GNOME. |
| REAL_INFERENCE | NOT_RUN | Zero prompts/envios/inferências/ferramentas reais. |
| PROCESS_CLEANUP | NOT_RUN SDK real; PASS fixtures owned | Kernel exhaustion/identidades ausentes nas regressões. |
| ATTEMPT_MARKER | ABSENT_NOT_CLAIMED | Existência antes/depois false; conteúdo não lido. |

SDK_AUTHENTICATED_WITH_GUI=OBSERVED_REAL_PASS permanece exclusivamente histórico,
na [A9-FIX-2 original](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-gui-sdk-metadata.json),
2026-10-10T03:34:13Z. Inode/timestamps alterados com tamanho470 continuam sem
prova de conteúdo ou autoria. Nenhuma escrita foi declarada legítima/corrupta,
e não se invalidou a auth historicamente observada.

Quota200/52 e Auto são históricos, não admissão financeira atual. Não se mediu
saldo/fatura externa. Zero inferências da POC não é comprovação de cobrança externa
zero. Uma chamada SDK futura pode envolver mais de uma unidade/requisição interna
faturável. Nenhuma tentativa autorizada de inferência foi consumida aqui.

## 7. Testes e comandos

**52 Rust e100 Python PASS, zero FAIL**, offline/locked/jobs2/test-threads1:
48+4 Rust, 86+14 Python. Primeiro ciclo incremental também passou; o ciclo final
após formatação/ajustes é a referência congelada usada no preflight.

| Caso obrigatório | Resultado executado | Limite |
| --- | --- | --- |
| Estável | PASS fixture | Metadata aceita; conteúdo NOT_VERIFIED. |
| Atomic replace mesmo tamanho | PASS fixture | Drift+auth positiva simultâneos; bytes sintéticos distintos, writer não inferido. |
| Size/mtime/ctime | PASS fixture | Campos detectados sem legitimidade presumida. |
| Unsafe path/symlink/traversal | PASS fixture | Tipo/UID/mode/link/parent access bloqueiam, sem conteúdo. |
| Ausente/inacessível/interrompido | PASS fixture | Missing sintético real; EACCES/interrupção injetados, sem exception prose. |
| Concurrent unknown writer | PASS fixture | Child próprio sincronizado por pipe/wait; stat não atribui autoria. |
| Auth positiva+drift | PASS fixture | Metadata aceita; escopos sensíveis bloqueados. |
| Auth negativa+stable | PASS fixture | AUTH_FALSE_OBSERVED, sem falso positivo. |
| Cleanup/service failure | PASS fixture | Auth independente, verificação dependente bloqueada. |
| Metadata→session/send | PASS fixture | Não transfere admissão nem oferece dispatcher sensível. |
| Finance via flag/fixture/auth | PASS fixture | Flag inválida rejeitada, forged dict não admite INFERENCE; blockers Rust anteriores passam. |
| RPC restrito/version mismatch | PASS SDK peer local | Status/auth uma vez, shutdown; erro/mismatch impede auth e retry, sem provedor. |
| Ownership/timeout/external preservation | PASS regressões Linux sintéticas | ECHILD/pidfd/PID-start-time, sem prova de contenção adversarial. |
| Auth/status CLI real | NOT_RUN | Concorrência não verificável; mocks não substituem. |
| Tauri/UI | NOT_RUN | Sem mudanças de produção/dependências. |

[Casos sanitizados](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-synthetic-cases.json),
[Rust final](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-final-regressions/a9-host-rust-tests.txt),
[Python final](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-final-regressions/a9-host-python-tests.txt),
[ownership final](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-final-regressions/a9-host-owned-tests.json),
[offline/hashes](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-offline-verification.json).
Logs incrementais ficam separados, sem reinterpretar como ensaio real. JSONL de
gateway/permissões são regressões fixtures das etapas anteriores.

Comandos executados:

```sh
git status --short --branch
git rev-parse HEAD main origin/lr-10a-sdk-runtime-feasibility origin/main
git ls-remote origin refs/heads/lr-10a-sdk-runtime-feasibility refs/heads/main
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-regressions
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir experiments/lr-10a-sdk-runtime/evidence/a9-fix-4-final-regressions
python3 experiments/lr-10a-sdk-runtime/confirm_a9_metadata.py
git diff --cached --check
git push origin lr-10a-sdk-runtime-feasibility
```

Também executados AST parse/py_compile, JSON/JSONL parse, rustfmt --edition2021 e
--check nos três Rust novos via toolchain stable já instalada. /usr/bin/rustfmt
não existe: tentativa inicial exit127, depois executável local disponível, sem
instalação. Runner usa /usr/bin/cargo test --offline --locked -- --test-threads=1,
COPILOT_SKIP_CLI_DOWNLOAD=1/CARGO_BUILD_JOBS=2/RUSTC/RUSTDOC locais. Rust/Cargo1.98.1
e Python3.14.7 preexistentes; não se atribui essa execução a Rust1.94. Só whitespace
final dos novos logs txt foi normalizado.

Revisão de escopo confirmou121 arquivos históricos evidence byte-idênticos,
wrappers/pins/Cargo/harness preservados e documentos permanentes só acrescidos.
Scan limitado token/Bearer/private-key nos novos artefatos: zero matches, sem
garantia universal de detectar todo segredo. Sources as-executed continuam iguais
aos hashes publicados. Suíte Tauri completa não repetida: sem produção alterada.

## 8. Segurança e processos

Nenhum conteúdo pessoal, config, token, keyring item, histórico, env integral,
/proc/environ, cmdline, memória ou tráfego autenticado examinado. CLI não foi
iniciado; não há stdout bruto dele. SHA leu só executável público instalado.
Sem chmod/read-only/lock/restore/copy/rename/delete pessoal ou token store paralelo.

Não se manipulou GNOME/GDM/Keyring, login/logout, serviços, PAM, boot, rede/firewall
ou credenciais. Estado atual de serviço/GUI NOT_RUN: bloqueio anterior às sondas.
Headless e boundary isolado/offline anteriores intactos, sem promover a resolvidos.

Regressões do subreaper FIX-1 atingiram kernel child exhaustion, sem ownership
errors/survivors. Identidades PID/start-time rechecadas ausentes; writer sintético
esperado/reaped. Externo preservado/timeout/setsid/filho não amostrado retestados.
Nenhum sinal a processo externo. Recuperação forçada não é chamada shutdown
gracioso SDK. Preflight bloqueado não criou processo SDK/CLI para encerrar.

Worker death, adversarial reparenting/nested namespace e processos ininterruptíveis
continuam riscos separados, sem supervisor LR-10B. Snapshots pontuais e mesmo UID
não garantem filesystem isolation, imutabilidade ou autoria. Nenhum PASS herdado
de outro gate ou mock foi usado como aprovação operacional.

## 9. Arquivos e preservação

Mudanças só experimentais/documentais:

- config_integrity.py: acesso opt-in, classificação histórica intacta.
- metadata_policy.py: operação/projeções independentes e helper stat.
- confirm_a9_metadata.py: preflight one-shot, pin/hashes/concorrência/gates.
- src/metadata_confirmation.rs, src/bin/a9-metadata-confirm.rs: fluxo limitado e fases.
- fixtures/metadata_confirmation_cli.py, tests/metadata_confirmation.rs, tests/test_metadata_policy.py: deterministic peers/testes.
- METADATA-CONFIRMATION.md; adendos HOST-ASSISTED.md e documento permanente, sem apagar histórico.
- evidence/a9-fix-4-regressions/ e a9-fix-4-final-regressions/: cinco arquivos cada.
- evidence/a9-fix-4-offline-verification.json, a9-fix-4-synthetic-cases.json, a9-fix-4-metadata-confirmation.json, a9-fix-4-final-safety.json.
- Este relatório substitui integralmente A9-FIX-3, preservada no histórico Git.

## 10. Pendências e recomendação

Contrato offline corrige imutabilidade para METADATA_READ_ONLY sem legitimar
escrita ou abrir escopos sensíveis. SDK atual não foi confirmado; concorrência
continua bloqueio. Sem fundamento para PASS_REAL novo, READY_FOR_A9, admissão
financeira ou avanço LR-10B.

Próximo passo: auditar a política e determinar não invasivamente se entradas
indisponíveis podem ser excluídas como CLI concorrente. Se impossível, definir
condição revisada de execução com autorização própria, sem mudar /proc, encerrar
processos externos ou criar flag de tolerância. Nenhum novo ensaio executado aqui.

Ainda separados: confirmação status/auth GUI, prova headless, estado privado/
sessões, modelo/unidades/custo máximo/no fallback pago, isolamento auth/rede e
falha do supervisor. Inferência/persistência genuínas NOT_RUN. A autorização de
uma tentativa futura não foi consumida nem utilizada como autorização desta FIX.

Release0.1 em17/10/2026: manter POC fora de produção e priorização humana. Prazo
não permite bypass de concorrência, financeiro ou autoridade. Decisão técnica:
**PASS parcial offline + BLOCKED real / FIX_AND_RETEST.** Entrega candidata,
auditoria independente da Luna pendente; nenhuma próxima etapa iniciada.
