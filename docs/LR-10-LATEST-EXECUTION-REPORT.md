# Narys — relatório mais recente da LR-10

## 1. Identificação

- Projeto/fase: **Narys / LR-10A / FIX-1 — Process Ownership & Cleanup Reliability**.
- Data: **09/10/2026**, America/Fortaleza; desenvolvimento via SSH/headless no Fedora.
- Branch: `lr-10a-sdk-runtime-feasibility`.
- Commit base local/remoto verificado: `8de5b891c0698db812ea130c65806a1e8671e259`.
- `main` local e remota: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`, sem alteração.
- Commit da implementação testada: identificação literal adicionada no fechamento documental.
- Commit final e HEAD remoto: **a referência da branch que contém esta versão do relatório**,
  consultável em [HEAD final publicado](https://github.com/Samuel-Francisco-AS/Narys/commit/lr-10a-sdk-runtime-feasibility)
  e no [histórico deste arquivo](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility/docs/LR-10-LATEST-EXECUTION-REPORT.md).
  Esta identificação é autorreferente: o SHA literal do commit que contém um
  relatório não pode ser inserido no próprio conteúdo sem mudar aquele SHA.
  A referência acima identifica o fechamento documental, separadamente do SHA
  imutável da implementação testada. O HEAD local deve coincidir com `ls-remote`.
- Estado: **LR-10A FIX-1 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE**.

Este arquivo contém somente esta execução. Documentos permanentes e evidências
históricas foram preservados; o próximo trabalho substituirá este relatório.

## 2. Objetivo e escopo

Corrigir a perda de ownership quando a raiz cria um descendente, termina antes
da primeira amostra e deixa esse processo vivo, inclusive após `setsid()`.
O conjunto de identidades observadas em `/proc` deixava uma lacuna: não observar
um sobrevivente era insuficiente para afirmar cleanup completo.

O diff está restrito ao harness experimental Python, suas fixtures, README,
evidências FIX-1, adendo histórico e este relatório. Não implementa supervisor
LR-10B, FIX-2/FIX-3, approval engine, sandbox, execution authority, UI ou mudanças
no Execution Broker. Nenhum arquivo Rust, Cargo manifest/lockfile ou dependência
foi alterado. Não se executou o Copilot CLI real nem chamadas do SDK nesta FIX.

Foram lidos integralmente o plano LR-10A, a evidência anterior, a trilha LR-10,
README, measure.py e test_measure.py. Branch, status, remotes e HEAD foram
conferidos; `git fetch origin` confirmou a base esperada e workspace inicialmente
limpo. Nenhum reset, force-push, merge, rebase, sudo ou serviço persistente.

## 3. Mudanças executadas

| Arquivo relativo à raiz | Alteração |
| --- | --- |
| [measure.py](../experiments/lr-10a-sdk-runtime/measure.py) | Worker privado, ownership por filhos do kernel, pidfds, reap iterativo e schema 2 |
| [test_measure.py](../experiments/lr-10a-sdk-runtime/tests/test_measure.py) | 25 testes, sincronização, inventário independente e coletor reproduzível de evidências |
| [README.md](../experiments/lr-10a-sdk-runtime/README.md) | Requisitos, semântica, limites e comando de reprodução |
| [fix-1-verification.json](../experiments/lr-10a-sdk-runtime/evidence/fix-1-verification.json) | Resultados individuais, medições de fixtures, identidades e hashes SHA-256 |
| [fix-1-verification.txt](../experiments/lr-10a-sdk-runtime/evidence/fix-1-verification.txt) | Log completo da execução final da suíte |
| [LR-10A-IMPLEMENTATION-AND-EVIDENCE.md](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md) | Adendo FIX-1; evidência anterior preservada sem reinterpretação |
| LR-10-LATEST-EXECUTION-REPORT.md | Novo ponto de referência reutilizável da entrega |

### Decisões técnicas

1. **Isolar adoção e reap por invocação.** `measure()` cria um worker por fork e
   aguarda somente aquele PID. O worker habilita PR_SET_CHILD_SUBREAPER e inicia
   somente a raiz controlada. O processo chamador preserva seu estado de subreaper
   e SIGCHLD; seus filhos externos não entram no domínio de recuperação. O kernel
   reparenteia órfãos ao subreaper ancestral vivo mais próximo, independentemente
   de sessão ou process group. [Linux PR_SET_CHILD_SUBREAPER](https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html).
2. **Separar métricas de autoridade para sinais.** O inventário de filhos diretos
   vem de `/proc/self/task/<worker>/children`. PPID e start_ticks são validados
   antes e depois de abrir o pidfd; não ocorre reap entre essas duas leituras.
   O worker usa SIGCHLD default. Um filho terminado permanece reclamável, sem
   reutilização de PID antes do reap. Identidades divergentes ou inacessíveis
   não recebem sinais. [Linux pidfd_open](https://man7.org/linux/man-pages/man2/pidfd_open.2.html).
3. **Sinalizar apenas handles estáveis.** Todas as interrupções usam
   `signal.pidfd_send_signal`; não existem killpg nem kill por PID numérico.
   A validação repetida protege a atribuição; o handle mantém o alvo estável
   mesmo entre validação e sinalização. [Linux pidfd_send_signal](https://man7.org/linux/man-pages/man2/pidfd_send_signal.2.html).
4. **Recuperar até esgotar filhos do kernel.** Após a saída da raiz ou timeout,
   coletar filhos, sinalizar somente atribuídos, reclamar e repetir. Isso cobre
   netos revelados após a morte de um filho adotado. `waitpid` usa WNOHANG e
   `__WALL`, incluindo filhos clone; ECHILD precisa coincidir com inventário de
   filhos vazio. Cleanup completo também exige ausência de sobreviventes
   atribuídos e de erros. O orçamento nominal de recuperação é dois segundos.
   [Linux waitpid](https://man7.org/linux/man-pages/man2/waitpid.2.html).
5. **Falhar com evidência explícita.** Recursos ausentes, caller multithread ou
   SIGCHLD não default são recusados antes da invocação. Erros de atribuição e
   sinalização permanecem registrados mesmo após uma recuperação posterior.
   Falha inesperada do worker não gera declaração de cleanup completo.

### Semântica do schema 2

`operation_outcome` distingue timeout, saída zero, saída não zero e estado
desconhecido. `cleanup_status` descreve separadamente a responsabilidade do harness:

| Estado | Interpretação |
| --- | --- |
| graceful_no_recovery | Saída dos processos sem sinais do harness; não atesta client.stop() |
| descendants_recovered | Descendentes detectados e recuperados; esgotamento comprovado |
| timeout_recovered | Timeout operacional e recuperação comprovada pelo harness |
| recovery_incomplete | Prazo/esgotamento/inventário não permitem comprovar cleanup |
| inconclusive | Falha de atribuição, recuperação, capability ou worker |

`cleanup_complete=true` pode coexistir com retorno **1** quando houve recuperação
forçada ou timeout. Saída zero da raiz não promove a operação a sucesso se restar
descendente. `sdk_shutdown_verified=false` em todos os resultados desta FIX.
`external_not_attributed` é erro seguro de atribuição; processos externos são
excluídos e nunca sinalizados. Uma lista vazia de sobreviventes conhecidos não
substitui `kernel_children_exhausted`, inventário vazio e ausência de erros.

## 4. Testes e evidências

Ambiente observado: Fedora Linux 44 Workstation, kernel
`7.2.8-200.fc44.x86_64`, Python **3.14.7**, Git **2.55.0**.
`os.pidfd_open` e `signal.pidfd_send_signal` disponíveis; probes de capability
também executados em cada worker antes de lançar a raiz.

### Comandos executados e resultados

```sh
git status --short --branch
git rev-parse HEAD
git remote -v
git fetch origin
git rev-parse origin/lr-10a-sdk-runtime-feasibility main origin/main
git ls-remote origin refs/heads/lr-10a-sdk-runtime-feasibility refs/heads/main
python3 --version
git --version
cat /etc/os-release
python3 -m unittest discover -s experiments/lr-10a-sdk-runtime/tests -p 'test_*.py' -v
python3 experiments/lr-10a-sdk-runtime/tests/test_measure.py --evidence experiments/lr-10a-sdk-runtime/evidence/fix-1-verification.json
python3 -m py_compile experiments/lr-10a-sdk-runtime/measure.py experiments/lr-10a-sdk-runtime/tests/test_measure.py
git diff --check
git diff --stat
git diff -- src-tauri src package.json package-lock.json
```

- Descoberta completa durante desenvolvimento: 19/19, depois 21/21, 23/23 e 24/24 PASS,
  conforme a cobertura evoluiu. São execuções intermediárias, não o artefato final.
- Execução final, por **unittest discovery** dentro do coletor: **25/25 PASS**;
  `Ran 25 tests in 2.983s`, elapsed externo **2.992s**. Nenhum skip/erro/falha.
- Sintaxe/compilação Python: PASS. Formatação: `git diff --check` e validação de
  whitespace/indentação por tokenize: PASS; sem instalação de formatter novo.
- Todos os **13 JSONs** do diretório de evidências parseados; nenhum histórico
  reescrito. SHA-256 dos três arquivos de harness/README corresponde ao código
  testado; quantidade/status de testes e ausência de survivors validados.
- Diff de produção vazio; escopo revisto arquivo a arquivo.

### Resultado de cada teste

Os nomes abaixo estão completos no [log executado](../experiments/lr-10a-sdk-runtime/evidence/fix-1-verification.txt)
e no [JSON](../experiments/lr-10a-sdk-runtime/evidence/fix-1-verification.json).
PASS significa que a expectativa do teste foi cumprida, inclusive retornar
falha/inconclusivo nas provas negativas.

| Teste (prefixo `test_`) | Gate/evidência | Resultado |
| --- | --- | --- |
| T1_unsampled_child_after_root_exit | Raiz terminou antes de qualquer amostra; filho adotado e reclamado | PASS |
| T2_unsampled_child_new_session | Filho com setsid, nenhuma amostra prévia, recuperação por kernel | PASS |
| T3_zero_exit_with_live_child_requires_recovery | Raiz zero, harness retorna 1; sem alegação de SDK shutdown | PASS |
| T4_timeout_active_tree_new_session | Árvore pronta por handshake; raiz -9, filho recuperado, timeout distinto | PASS |
| T5_external_child_untouched_and_unreaped | Controle externo vivo e ausente das atribuições; reclamado somente pelo teste | PASS |
| T6_attribution_failure_remains_inconclusive_after_recovery | Identidade divergente simulada; retry limpa, mas resultado continua inconclusivo | PASS |
| T6_external_identity_never_opens_handle | PPID externo: nenhum pidfd aberto e nenhum sinal | PASS |
| T6_failed_recovery_signal_not_reported_complete | Negação simulada de SIGKILL; retry limpa; erro seguro permanece | PASS |
| T6_inventory_failure_blocks_false_empty_cleanup | Inventário indisponível impede falso cleanup completo | PASS |
| T6_missing_identity_remains_inconclusive | Identidade inicialmente indisponível; recovery posterior não elimina erro | PASS |
| T6_pid_identity_change_closes_handle_without_signal | start_ticks muda entre validações; FD fechado, nenhum sinal | PASS |
| T6_unverifiable_wait_exhaustion_is_incomplete | Esgotamento de wait indisponível; recovery_incomplete explícito | PASS |
| caller_subreaper_and_sigchld_unchanged | Estado do chamador preservado | PASS |
| crashed_root_unsampled_child | Raiz SIGKILL; descendente não amostrado recuperado | PASS |
| inherited_stdout_does_not_block_cleanup | T7: stdout herdado não bloqueia reap | PASS |
| malformed_evidence_fails_closed_without_echoing_content | T7: saída malformada falha sem eco de conteúdo | PASS |
| missing_pidfd_fails_before_launch | Capability ausente simulada; manifesto de fixture vazio | PASS |
| nondefault_sigchld_refused_before_launch | Caller não admissível; nenhuma fixture iniciada | PASS |
| normal_exit_needs_no_recovery | T7: saída sem intervenção, ECHILD/inventário vazio | PASS |
| premature_parent_exit_detects_and_reaps_descendant | T7: regressão de recuperação anterior | PASS |
| repeated_adoption_recovers_previously_hidden_grandchild | Dois descendentes vivos; neto revelado por recuperação do pai | PASS |
| threaded_caller_refused_before_launch | Caller multithread simulado; nenhuma fixture iniciada | PASS |
| timeout_kills_owned_group_and_reports_failure | T7: nome histórico preservado; mecanismo atual usa pidfd, não grupo | PASS |
| unsampled_double_fork_new_session | Double-fork + sessão nova, sem amostragem da árvore | PASS |
| worker_failure_is_inconclusive | Crash simulado antes do launch; nenhuma alegação de cleanup | PASS |

T1/T2/T3 usam readiness via pipe e `after_launch(proc)` substituído por uma
barreira que aguarda a saída da raiz **antes** da primeira amostra. Não dependem
da sorte de uma corrida de milissegundos. T4 usa marcador privado depois do
handshake. A seam é no-op em produção do experimento e não é opção CLI.

### Observações e limites das medições

| Fixture | Processos amostrados | Descendentes atribuídos sem amostra | cleanup_ms | wall_ms |
| --- | ---: | ---: | ---: | ---: |
| T1 | 0 | 1 | 35,14 | 68,46 |
| T2 setsid | 0 | 1 | 35,25 | 68,51 |
| T3 saída zero | 0 | 1 | 35,05 | 68,32 |
| T4 timeout | 2 | 0 | 47,92 | 253,31 |
| Adoção iterativa de neto | 0 | 2 | 46,33 | 79,61 |

Em T1/T2/T3 e adoção iterativa: `cleanup_complete=true`,
`kernel_children_exhausted=true`, retorno do harness 1 e recuperação forçada.
Em T4: `timeout_recovered`, raiz -9 e cleanup comprovado. T6 produz
`inconclusive` ou `recovery_incomplete` com `cleanup_complete=false`, inclusive
quando a lista final de sobreviventes observados é vazia.

As fixtures registram identidades PID/start_ticks independentes, inclusive forks;
o coletor combina esses manifestos, identidades atribuídas e controle externo.
**36 identidades distintas verificadas; zero identidades sobreviventes**, inclusive
zombies, após cada fixture aplicável e novamente depois da suíte. O worker é
reclamado pelo wait do seu chamador; pidfds e arquivos temporários são fechados.
O processo externo de T5 permaneceu vivo após o harness e foi interrompido/reclamado
somente pelo dono do teste. Não se sinalizou qualquer processo encontrado em scan
global. Identidades reutilizadas são distinguidas por start_ticks, não pelo PID só.

Estes valores são amostras únicas de **fixtures Python**, sem benchmark do SDK.
cleanup_ms inclui verificação final/snapshot e coleta do relatório; o orçamento
nominal não é deadline hard contra bloqueios de kernel/I/O. wall_ms começa dentro
do worker, após snapshot inicial; não mede o custo completo de fork do worker.
CPU/RSS continuam aproximações amostradas da raiz/árvore, excluindo o worker;
zero amostras não significa zero uso de recursos. Não houve nova medição de
startup/memória/quota/sessões/modelos do Copilot. Os JSONs reais antigos usam o
harness anterior e não constituem reteste desta FIX.

## 5. Segurança e regressões

Preservados ExecutionAuthority/HumanLocal, Codex planner read-only, AgentRegistry,
IPC release, OperationalTraceBus, TaskGraph, Scheduler e LR-8.5. Nenhum executor
registrado, shell genérico, alteração na WebView ou mediação nova pelo Broker.

Somente `/proc/*/stat` e o inventário de filhos do worker são lidos. Não há dump
de environ, cmdline, credenciais ou protocolos brutos. A evidência contém IDs,
números, estados/códigos e `{}` das fixtures; conteúdo malformado/exception prose
não é reproduzido. Nenhuma alteração global de configuração ou credenciais;
nenhum SDK/CLI atualizado, runtime real, inferência ou YOLO. Consumo de quota
Copilot pela FIX-1: **zero** (nenhum processo Copilot e nenhuma requisição).

Os cinco testes Python anteriores permanecem aprovados. A suíte de 1.107 testes
Narys e os testes Rust **não foram repetidos**: o diff não altera Rust, Cargo,
produção ou integração Tauri. A compatibilidade Rust 1.94 documentada antes não
foi revalidada nesta FIX; nenhuma nova conclusão de SDK/CLI foi inferida.

Riscos residuais: o contrato assume worker íntegro, caller controlado e
single-threaded, kernel/proc utilizáveis e fixtures com criação de processos
Linux normal. Não é sandbox ou contenção de processo malicioso. Morte abrupta
do próprio worker após launch, namespace/reparenting adversarial, tarefas em
estado kernel não interrompível, fork bomb e uso embutido em runtime multithread
não foram validados. Se atribuição ou recuperação falhar de forma permanente,
o harness informa incomplete/inconclusive; não pode prometer recuperação por
meios sem autoridade. O teste de crash do worker ocorre **antes** da raiz para
não deixar um órfão deliberadamente. A proteção do worker em produção pertence
ao futuro supervisor, não foi implementada aqui.

## 6. Pendências

- Auditoria independente da Luna desta FIX-1, inclusive pressupostos de kernel,
  isolamento do worker e semântica dos estados.
- FIX-2/FIX-3 e demais lacunas da auditoria anterior permanecem fora desta entrega.
- Reteste com SDK/CLI real: **NOT_RUN por restrição explícita desta FIX**; a prova
  apresentada valida o harness, não a responsabilidade de shutdown do SDK.
- A9: **BLOCKED_REAL / AWAITING_HUMAN_APPROVAL**; não enviado ao Copilot.
- Stress prolongado, namespace/adversário, morte do worker depois do launch e
  performance real do overhead adicional: NOT_RUN; exigiriam outro escopo/fixture.
- Nenhum avanço à LR-10B autorizado. As pendências A2/A4/A6/A7 da evidência
  histórica não são encerradas globalmente por estes testes locais.

## 7. Conclusão

**PASS técnico candidato da FIX-1 no contrato experimental testado.** T1–T7
foram satisfeitos: recuperação não depende da primeira amostra nem do grupo
original, timeout/crash da raiz são tratados, controle externo é preservado e
ausência de prova gera falha explícita. Recomenda-se auditoria independente desta
correção; se aprovado esse contrato, prosseguir somente com as FIXes ainda
pendentes mediante escopo próprio.

**LR-10A permanece FIX-AND-RETEST**, sem PASS definitivo e sem recomendação de
avanço imediato à LR-10B. Não houve PR, merge ou alteração da main.

**LR-10A FIX-1 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE.**
