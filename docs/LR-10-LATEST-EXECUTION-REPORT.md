# LR-10A — implementação consolidada e validação operacional final

> **AUDITORIA INDEPENDENTE RATIFICADA EM 10/10/2026: LR-10A = PASS FINAL técnico** no escopo servidor headless/Copilot textual host-assisted. A designação “candidata/aguardando auditoria” no corpo abaixo é o estado anterior à ratificação. **LR-10B–F PAUSADAS.** Próxima trilha urgente: [NARYS-SERVER-1](NARYS-SERVER-1-HEADLESS-SERVER-RUNTIME.md), até 48h após início e no máximo quatro etapas. Ver [decisão e dívidas](LR-10A-FINAL-CLOSURE-2026-10-10.md).

## Identificação e decisão

Narys, Fedora44, 10/10/2026. **Core headless + Copilot textual: PASS_REAL no escopo
host-assisted autorizado.** Task2 respondeu5, resultado/TaskGraph persistidos,
conversa genuína retomada e runtime encerrado. Uma inferência efetiva, nenhuma
falha/incerteza de envio; duas tentativas restantes revogadas após evidência suficiente.

**LR-10A — IMPLEMENTAÇÃO CONSOLIDADA CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE DA LUNA.**
Não se atribui aprovação definitiva da LR-10A ou integração de produção.

- Branch exclusiva: `lr-10a-sdk-runtime-feasibility`.
- Base inicial local/remota: `25fe0b4c2cccbda890b40036bc640e9b84825e24`.
- [Implementação da inferência testada/executada](https://github.com/Samuel-Francisco-AS/Narys/commit/cdf698cd5cefb28e519283aa7b95f6c59fa5bcd3): compatibilidade e consentimento global durável.
- [Implementação final, resume, evidências e documentos permanentes](https://github.com/Samuel-Francisco-AS/Narys/commit/4603202eda58eb62f8ebe74961d7d59dab5109d3): publicada e HEAD remoto confirmado antes deste relatório.
- HEAD documental final verificável no [histórico da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility), evitando SHA autorreferencial.
- `main` local/remota permanece `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`. Sem PR, merge, rebase, reset ou force-push. H1/H2/H3, FIXes e task1 preservados.

## Objetivo, escopo e autorização

Concluir a viabilidade operacional do Core sem desktop e do SpecialistAgent
textual, sem novas fases/POCs ou auditorias intermediárias. O usuário autorizou
investigar/corrigir/instalar autonomamente e até **três envios reais cumulativos**,
incluindo resultados incertos, somente tarefas mínimas em workspaces descartáveis.
Reafirmou franquia existente e proibição de cobrança adicional/overage/compras.
A confirmação humana anterior de orçamento adicional desativado e aceitação da
incerteza requests/AI Credits foi preservada. Não houve autorização de shell
agentivo, ferramentas perigosas, credenciais explícitas, YOLO ou fallback pago.

Não se repetiu reboot, desbloqueio humano, abertura do Stronghold pessoal ou
provas gráficas. Keyring, SSH/tmux, GDM, boot target e credenciais não modificados.
A única unidade reiniciada para instalar código foi a própria narys-core.service.

## Causa e correção concreta

[Diagnóstico real delimitado](../narys-core/evidence/final-session-diagnosis.json):
três invocações sem send. As duas primeiras preservaram parâmetros anteriores:
SDK autenticado, erro **-32603 em session.create**, categoria pública credits,
shutdown gracioso/cleanup completo. A primeira capturava apenas método; a segunda
ampliou a classificação por vocabulário público fixo, sem publicar texto de erro.
Nenhuma Session retornada ou inferência nessas falhas.

O parâmetro opcional `sessionLimits.maxAiCredits=0.5` foi omitido. O terceiro
ensaio criou/desconectou a sessão; **session.options.update também funcionou**.
A tarefa real subsequente e a retomada confirmaram compatibilidade operacional.
O erro deixou de existir com essa configuração. Não se atribui o motivo interno
exato da falha no serviço de credits, nem incompatibilidade de todo o SDK/CLI.

A [documentação oficial](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/session-limits)
descreve esse limite como soft, aferido após chamadas, não teto financeiro.
Não era o controle obrigatório de cobrança. A correção preserva todos os gates
financeiros, DenyAll, zero tools e estado privado. `--no-custom-instructions`,
confirmado pelo help do CLI pinado, complementa skipCustomInstructions=true;
foi adicionado no terceiro ensaio. Assim, o contraste não é uma prova isolada do
motivo interno de credits: identifica a configuração funcional e o erro anterior.
O patch SDK permanece ativo, sem fallback que esconda falhas.

Contrato da crate efetiva1.0.17: start inclui create e patch posterior; a
[inspeção publicada anteriormente](../narys-core/evidence/sdk-session-contract.json)
continua válida. A falha histórica task1 não foi reclassificada como sucesso:
permanece failed, zero sends e receipt antigo não reutilizado.

## Composição e inventário

[Core](../narys-core/README.md) independente de Tauri/GTK/WebKit/X11/Wayland,
systemd --user, socket Unix0600/SO_PEERCRED mesmo UID, sem listener TCP. Copilot
somente sob demanda. AgentBackend/Registry, PlanV1/TaskGraph, TaskId, SQLite e
OperationalTraceBus existentes são reutilizados. Banco do Core separado da GUI.
O especialista tem capacidade textual/planning; não recebe HumanLocal ou
ExecutionAuthority. Nenhuma ação precisou ser enviada ao ExecutionBroker.

| Arquivo/grupo | Alteração nesta implementação |
|---|---|
| [authorization.rs](../narys-core/src/authorization.rs) | Consentimento fixo, três slots globais, tarefa única, flock/O_EXCL/fsync, corrupção/replay/closure fail-closed |
| [worker.rs](../narys-core/src/worker.rs) | Configuração compatível, classificação RPC sanitizada, diagnóstico sem send, guard global antes do send, eventos numéricos, resume/histórico sem recriação |
| [server.rs](../narys-core/src/server.rs), [main.rs](../narys-core/src/main.rs), [lib.rs](../narys-core/src/lib.rs) | Receipts novos, controles locais, fsync do diretório, evidência factual do grafo, tratamento de falha de persistência e resume one-shot |
| [record_final_consent.py](../narys-core/ops/record_final_consent.py), [review_final_task.py](../narys-core/ops/review_final_task.py) | Registro da autorização humana já recebida e vínculo a tarefa nova; não enviam, sobrescrevem ou renovam consentimento |
| [sdk_peer.py](../narys-core/tests/sdk_peer.py), [test_final_authorization.py](../narys-core/tests/test_final_authorization.py) | Fixtures de lifecycle/resume/anti-replay sem provedor |
| [Evidências](../narys-core/evidence/final-integrated-operation.json) e logs abaixo | Diagnóstico, inferência, resume, serviços e testes sanitizados |
| [README](../narys-core/README.md), [documento permanente](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md) | Operação atual e conclusão adicional, mantendo registros históricos |
| Este arquivo | Relatório único final substitui o anterior |

Nenhum diff novo em produção, Broker, ExecutionAuthority, TaskGraph original,
Scheduler/LR-8.5, IPC release, UI ou runtime3D desde a base inicial desta execução.
Extrações puras anteriores permanecem preservadas; factory gráfica Codex read-only
não passou a registrar o Copilot como executor.

## Versões, artefatos e instalação

- SDK `github-copilot-sdk=1.0.17`, runtime não bundled, dependências/lock preservados.
- CLI nativo1.0.95, RPC3, SHA256 `9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`, validado em cada worker.
- Core da inferência: `c035fc8b0342e96b31b86e14b87bd3637ab984d4c4f8aadcba8a2a1289f7f44a`, implementação cdf698c.
- Core final instalado, usado no resume: `a25c2b0e517bf55e939e6e322185245a0912f5d40dfe92e6b310a2fec267169b`, implementação4603202. Fonte de cada módulo também identificada na evidência do resume.
- Unidade preservada SHA256 `4c95860a4aad56ba526a05c5aef3899508ea99e2944c57b5802204e8dd8ff22c`.
- Compilador executado Fedora rustc1.98.1; Core Edition2021/MSRV declarado1.94.0. Nenhum novo ensaio em1.94 nesta execução; não se inventa essa comprovação.

Sem download/atualização/instalação global de SDK, CLI ou toolchain. Build offline,
locked, dois jobs. Update validou hashes e substituiu somente o Core próprio;
Keyring PID1395 permaneceu igual. Estado headless já comprovado foi mantido.

## Inferência integrada, persistência e eventos

[Evidência real completa](../narys-core/evidence/final-integrated-operation.json):
Task2 nova, workspace `/tmp/narys-task-5Cidtj/workspace`0700. Objetivo:

> Responda somente o número da soma de alpha=2 e beta=3. Não utilize ferramentas, não execute comandos, não acesse arquivos e não faça outras solicitações.

Core submit2 aceitou running; novo preflight real autenticado/catalog/quota aprovou
condições restritas. Sessão criada, **um send_and_wait**, resposta final exatamente5,
detach acknowledged e shutdown graceful. Sem retry, ferramenta ou segundo prompt.

Core comparou resposta ao esperado humano, criou result.txt0600/O_EXCL, fsync e
releitura, marcou o TaskGraph completed/all_completed e gravou SQLite. Leitura
independente confirmou **um byte `5`**, SHA256
`ef2d127de37b942baad06145e54b0c619a1f22327b2ebbcfbec78f5564afe39d`.
Result2 permaneceu completed/5 após atualização/restart apenas do Core.
A mensagem do modelo ou session.idle sozinhos não determinaram conclusão.

Eventos observados: assistant.turn_start, assistant.message, assistant.turn_end,
assistant.usage, session.idle e session.usage_checkpoint, um de cada. Trace local
registrou started/lifecycle/response_received/completed. Bus é limitado em memória;
seu replay reinicia com o Core. Evidências/resultados/grafo foram persistidos.

[Resume genuíno](../narys-core/evidence/final-owned-session-resume.json): após
encerramento do Client/CLI original, nova invocação retomou o **mesmo ID** encontrado
no único diretório de sessão do estado privado da task completed. session.resume,
patch/skills reload e getMessages retornaram10 eventos incluindo resposta5;
detach e stop completos. Sem session.create, send ou transcript fabricado.
Reserva exclusiva por task impede repetir o check. Não foi lida sessão pessoal.
A conversa real persistida distingue este resultado das sessões vazias da FIX-2.

## Consentimento durável e observação financeira

Estado privado0700/0600: `~/.local/state/narys/core/lr10a-final-authorization`.
Consentimento imutável por criação exclusiva, escopo fixo, três slots globais e
receipt novo vinculado à Task2/SHA do objetivo/zero USD adicional/expiração30min.
flock + O_EXCL + fsync do arquivo/diretório antes do envio; crash/incerto contam,
TaskId repetido/corrupção/limite/closure bloqueiam. Não é proteção contra usuário
malicioso do mesmo UID. Guard por task mantido; marker A9 histórico ausente/intacto.

**Envios efetivos cumulativos:1/3; falhos:0; incertos:0.** Task1 histórica e três
diagnósticos tiveram0 sends; resume teve0. Após validação independente, closed.json
revogou as duas tentativas restantes. Nenhum receipt/TaskId antigo reutilizado.
Não executar novamente os comandos históricos de submissão.

Antes do send: Auto realmente disponível, multiplicador/pricing ausentes; quota
premium200/used52/remaining74.2%, overage0, ambas flags de uso após esgotamento e
overage false. O usuário confirmou orçamento adicional desativado e aceitou a
incerteza de unidades. Autenticação, modelo, quota e consentimento foram verificados
separadamente; falta/erro/flags abertas bloqueiam.

Uso informado pela sessão: **totalPremiumRequests=1; totalNanoAiu=68836500**;
inputTokens3036/outputTokens5/cacheWriteTokens3033/cacheReadTokens0/duration1006.
cost1.0 é campo multiplicador do schema SDK, **não USD**. Quota after manteve52/74.2%
e overage0; não se presume atualização imediata, saldo de AI Credits ou consumo0.
Uma chamada SDK não garante uma requisição interna faturável. Nenhuma fatura USD
foi consultada; **zero pagamento adicional autorizado** não é medição independente
de zero cobrança externa. Não houve mudança de plano, compra ou fallback pago
implementado. Admissão genérica futura continua bloqueada; este consentimento está fechado.

## Matriz final

| Gate | Resultado e limite |
|---|---|
| Core/cold boot/keyring manual/Stronghold existente | PASS_REAL histórico preservado; estado atual saudável, sem repetir essas operações |
| Sessão privada create/detach | PASS_REAL, SDK1.0.17/CLI1.0.95 configuração compatível |
| Auth/modelo/quota no preflight da tarefa | PASS_REAL metadata; sem equivalência presumida com saldo financeiro integral |
| Consentimento/anti-replay/cap3 | PASS fixtures + reserva real1 e closure; orçamento adicional confirmado pelo humano |
| Send/resposta independente5 | PASS_REAL, exatamente1 envio efetivo |
| Resultado/arquivo/TaskGraph/SQLite | PASS_REAL, arquivo relido e completed persistido após restart |
| Conversa persistida/resume após runtime restart | PASS_REAL, mesmo ID/10 eventos/5, zero novo send/create |
| Uso/quota após operação | Uso PASS_REAL; atualização imediata do saldo INCONCLUSIVE |
| Shutdown/cleanup/headless | PASS_REAL, SDK graceful e kernel ECHILD, sem recovery signals |
| Config pessoal | Estrutura PASS_METADATA_ACCESS; drift observado, escritor INCONCLUSIVE; conteúdo não lido |
| Autoridade de ferramentas/ações | BLOCKED, zero tools/DenyAll; nenhuma HumanLocal concedida |
| Admissão financeira genérica | BLOCKED, não herda autorização do ensaio já fechado |
| A9_ISOLATED/sandbox/supervisor adversarial | BLOCKED, não resolvidos pelo perfil host-assisted |
| LR-10A definitiva | PENDING_INDEPENDENT_AUDIT; sem atribuição automática de PASS global |

A0–A10: baseline/versões e runtime real preservados (A0/A2); compatibilidade
observada com compiler1.98.1/Edition2021, sem novo reteste1.94/Tauri (A1); auth,
Auto e quota reais (A3–A5); conversa persistida/resume reais (A6); cancelamento e
timeout fixtures, shutdown real (A7); medições parciais acima/abaixo (A8); tarefa
real host-assisted autorizada (A9); zero ferramentas e permissões negadas, sem
sandbox completo ou autoridade de produção (A10). Fontes baseline e limitações
continuam no documento permanente; resultados históricos não foram apagados.

## Processos, segurança e recursos

[Serviços finais](../narys-core/evidence/final-service-state.json): Core enabled/active,
Keyring enabled/active PID1395, user bus/coleção login disponíveis, GDM inactive,
nenhum GNOME Shell/Copilot observado, multi-user.target/linger=yes preservados.
SSH/tmux/Codex continuaram funcionais. Keyring e Stronghold conservaram metadados
antes/depois; isso não é equivalência criptográfica de conteúdo. Nenhum token,
senha, item do Keyring ou conteúdo de configuração pessoal lido/publicado pela POC.
Resolução normal de credenciais pelo CLI continua dentro da autorização host-assisted.

Config.json apresentou drift durante os runtimes, sem anomalia estrutural; autoria
inconclusiva. Não se restaurou, bloqueou, copiou ou alterou sua configuração pela
POC. Metadados são observações; não provam legitimidade da mutação. Política
proporcional de metadata não autoriza sessões, pagamento ou ferramentas.

| Medição | Tarefa | Resume sem inferência |
|---|---:|---:|
| Wall do worker completo |16757.18ms|12664.8ms|
| Peak RSS da árvore amostrada |304160768bytes|287555584bytes|
| Cleanup do harness |18.6ms|32.39ms|
| Recovery signals/sobreviventes atribuídos |0/0|0/0|

CPU amostrada da tarefa>=12.72s, caches do SO quentes; RSS idle final do Core15236KiB.
Não se isolou startup/stop SDK em métricas separadas nesta execução; cleanup_ms não
é latência exclusiva de Client.stop. MemoryCurrent inclui cache/cgroup, não só heap.
Sem benchmark prolongado ou downloads. Processos externos não foram sinalizados.

Harness FIX1 preservado: PID/start-time, pidfds, subreaper privado e ECHILD real.
SDK shutdown é fonte independente de measure.sdk_shutdown_verified=false, que
não atesta protocolo. Morte inesperada do supervisor/descendentes adversariais,
rede genérica do host e processos do mesmo UID permanecem riscos; não se declara
sandbox ou supervisor de produção. DenyAll/zero tools não equivalem a isolamento
por kernel. Logs brutos/sessões ficam privados, nunca versionados; padrões de
credenciais e JSONs publicados foram verificados, sem promessa universal de detector.

## Testes e comandos executados

[35 unitários Rust +1 integração Unix](../narys-core/evidence/rust-final-resume.txt),
[6 Python](../narys-core/evidence/python-final.txt),
[checks e25 ownership Python](../narys-core/evidence/final-checks.json): PASS.
Log bruto standalone do último ownership25 não foi retido; resultado observado
25/25 em2.739s está registrado separadamente dos logs históricos.

Cobertura: cap global/race/crash/replay/corrupt/closure, receipts únicos, create e
patch failure, diagnóstico sem send, resume sem create/send, caminho externo
negado, envio/cancel/timeout/abort/detach sintéticos, grafo/trace, socket/SQLite,
cofre sintético e produção sem ferramentas. Fixtures não substituem o sucesso
real, documentado separadamente. Erros locais de compilação/assertion durante
implementação foram corrigidos antes dos ensaios correspondentes; logs finais
registram a implementação aprovada nos testes.

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 \
RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc \
CARGO_TARGET_DIR="$PWD/src-tauri/target" \
/usr/bin/cargo test --offline --locked --manifest-path narys-core/Cargo.toml
python3 -m unittest discover -s narys-core/tests -p 'test_*.py'
python3 -m unittest discover -s experiments/lr-10a-sdk-runtime/tests -p test_measure.py
python3 -m py_compile narys-core/ops/*.py narys-core/tests/*.py
# rustfmt do toolchain já instalado: --check --edition2021, skip_children=true
systemd-analyze --user verify ~/.config/systemd/user/narys-core.service
git diff --check
```

Rustfmt, sintaxe, JSON, sanitização de padrões, scope/diff e unidade systemd PASS.
Tauri inteira/UI NOT_RUN: nenhum código de produção afetado nesta continuação;
regressão anterior das extrações compartilhou117 agentes PASS/2ignored, sem
reinterpretar esse histórico como novo reteste. Sem downloads ou builds redundantes
extensos. [33+1 antes da inferência](../narys-core/evidence/rust-final.txt) e35+1
antes do resume têm logs distintos e correspondem às versões executadas.

Histórico operacional, **não repetir**: prepare nova task2, receipt novo,
submit2 uma vez, result2/events/readback, update apenas Core, resume-check2 uma
vez, result2 pós-restart. Três session-check sem sends precederam a correção.

## Operação disponível, release e fechamento

Comandos seguros de administração via SSH: `status`, `credentials`, `result 2`,
`events`. Unlock deve ser executado pelo humano em SSH privado conforme
[guia](../narys-core/README.md); nunca senha no chat/argumentos/logs. Core fica
residente bloqueado até unlock. Copilot permanece ausente quando idle.
`prepare/submit/cancel/result` controlam tarefas; submissão exige receipt e gates,
não autenticação isolada. O consentimento deste ensaio já foi encerrado.

**Recomendação: GO técnico candidato para fechamento da LR-10A neste escopo de
servidor headless com Copilot textual e sessão privada**, sujeito à auditoria da
Luna. Não é autorização de ferramentas, pagamento genérico ou LR-10B. Nenhuma nova
fase/FIX foi criada; nenhum avanço automático foi feito.

Para Narys0.1 em17/10/2026, Core/credenciais/especialista já trabalharam juntos no
servidor sem desktop. Permanecem fora desta entrega Android, todos os provedores
da GUI, edição/shell, approvals de produção e sandbox/supervisor definitivo.
Sessões e artefatos em /tmp são descartáveis e podem desaparecer com limpeza/reboot;
resultado SQLite é persistente. Não se promete resume após perda desse estado.
Instalação depende da checkout atual e helper interno pinado ao Keyring50.0;
upgrade futuro requer compatibilidade revisada. Limitações de billing units/cache,
filesystem/rede host-assisted e falha do supervisor estão delimitadas; não impedem
reconhecer a tarefa textual real comprovada, nem justificam atribuir controles ainda
não implementados. A decisão final de encerramento cabe à auditoria independente.

## Ratificação posterior / controle de versão — 10/10/2026

**PASS FINAL LR-10A** atribuído após revisão independente dos artefatos de inferência, resume, serviços, autorização e regressões direcionadas. O relatório precedente permanece fiel ao estado da candidata no instante de sua publicação; esta ratificação prevalece sobre os marcadores PENDING_INDEPENDENT_AUDIT e “aguardando” históricos. Nenhum novo SDK send, reboot ou test suite foi executado para registrar este fechamento. `main` recebe a integração da branch aprovada por fast-forward; a branch de trabalho fica disponível até sincronização local.

A LR-10 inteira **não** está concluída: B–F pausadas; a NARYS-SERVER-1 é a próxima trilha a iniciar. Dívidas mapeadas em [fechamento](LR-10A-FINAL-CLOSURE-2026-10-10.md).
