# LR-10A / A9-FIX-3 — Configuration Integrity & Controlled Recovery

## 1. Identificação, Git e resultado

Narys, A9-FIX-3 exclusivamente; execução em 10/10/2026 no Fedora 44 via terminal/SSH.
Branch: `lr-10a-sdk-runtime-feasibility`.
HEAD inicial local/remoto conferido: `4a75c632fc8e69fcd9511cd447489b3c1d3d5fff`.
Main local/remota: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`, intocada.
Workspace inicial limpo; nenhum trabalho humano descartado.

Implementação testada e evidências:
[783d8693eeb504e8fee2f6fd17d056d310cbd595](https://github.com/Samuel-Francisco-AS/Narys/commit/783d8693eeb504e8fee2f6fd17d056d310cbd595),
commitado e enviado antes deste relatório. HEAD documental final/local/remoto é o
commit que publica este arquivo, verificável no
[histórico da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility/docs/LR-10-LATEST-EXECUTION-REPORT.md),
sem SHA autorreferencial. Entrega só nessa branch; sem PR/merge/rebase/reset/force-push.

**PASS parcial do contrato e testes sintéticos; CONFIG_WRITER_ATTRIBUTION=INCONCLUSIVE;
CONFIG_INTEGRITY_VERIFICATION=BLOCKED; recomendação FIX_AND_RETEST.** Não se comprovou
corrupção, segurança semântica da alteração nem autoria. Nenhum PASS operacional
foi obtido por mocks, e nenhum gate foi liberado.

**A9-FIX-3 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**

## 2. Escopo e proibições respeitadas

Investigar estaticamente a mutação já observada e implementar classificação
proporcional, reutilizável, com fixtures locais. Não houve CLI real nesta FIX,
sequer --version/help/auth/modelos/quota, nem SDK real contra o provedor. Zero
sessões reais criadas/retomadas/excluídas, zero mensagens/inferências/ferramentas.
Não se reclamou/criou/leu o conteúdo do marker A9. A autorização anterior para
uma futura tentativa continua não consumida, sem retry ou fallback pago.

Nenhum conteúdo de config pessoal, token, cookie, sessão particular, item do Keyring,
memória ou tráfego autenticado foi examinado. Nem mesmo stat da config pessoal foi
repetido nesta FIX. Nenhum chmod/read-only/lock/rename/copy/restore/delete pessoal;
nenhum login/logout, serviço/UI, GNOME/GDM/Keyring ou boot manipulado. Apenas fontes
públicas de software, artefatos Git publicados e arquivos/processos sintéticos foram
usados. Não se afirma que atividade externa ao experimento não alterou o host.

HOST_ASSISTED_WITH_GUI, HOST_ASSISTED_HEADLESS e A9_ISOLATED continuam distintos.
O primeiro é acesso sob o usuário, **não sandbox**. Boundary offline/minimal, DenyAll,
zero tools, MCP desabilitado, filtros, guard atômico, ownership e contratos anteriores
não foram enfraquecidos. Nada mudou em produção/Broker/ExecutionAuthority/IPC/UI/
TaskGraph/LR-8.5 ou MSRV/Edition/dependências. Não avançar à LR-10B.

## 3. Investigação da origem e limites do coletor histórico

Fontes: relatório A9-FIX-2 na base Git, documento permanente, HOST-ASSISTED,
diagnose_a9_gui_auth.py, diagnose_a9_auth.py, run_a9_host.py, src/host_assisted.rs,
runtime-gui-candidate.json e evidências/testes anteriores. Inspeção de
[run_fix2.config_stat](../experiments/lr-10a-sdk-runtime/run_fix2.py) confirma:
Path.home/.copilot/config.json.stat(), quatro campos, symlink seguido, todos
OSError agrupados como unavailable. Não lia conteúdo, device/mode/proprietário,
identidade do escritor ou eventos de escrita. Snapshots não eram ancorados.

[Evidência histórica original](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-gui-sdk-metadata.json),
SHA256 `1b76b3f4dfdacec1835ba774402897c202813c238c23b17a38500efd77b70916`,
coletada em 2026-10-10T03:34:13.229631Z, implementation
`42756450fb8b942fdc2f7ec42b6ece364f3e5c73`:

| Campo | Antes | Depois |
| --- | --- | --- |
| inode | 3768587 | 3768738 |
| bytes | 470 | 470 |
| mtime_ns/ctime_ns | 1791602000869867688 | 1791603250606145370 |

Mudança desses campos é fato. Não prova byte equality, corrupção ou rename específico.
O coletor seguia symlinks e não registrava device: não se exclui mudança de resolução
ou outras mudanças do filesystem. Não se pode reconstruir a fase exata a partir de
snapshots abrangendo startup/RPCs/shutdown, nem atribuir o escritor por tempo.
A estabilidade observada depois de identity/help na FIX-2 não legitima a janela SDK.

A crate **1.0.17** instalada foi lida sem executá-la contra CLI real. O código
[pinado/excertos](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-sdk-contract-excerpts.txt)
mostra spawn --server/--stdio/--no-auto-update, auth/modelos delegados por RPC,
shutdown por runtime.shutdown/EOF/reap. O SDK pode disparar comportamentos do CLI
nessas fases, mas os trechos examinados não oferecem contrato de imutabilidade
ou prova de escrita legítima de config.json. --no-auto-update é controle de update
do executável, não garantia de config read-only. POC não adicionava setters de
config ou login; ausência deles não prova ausência de escrita interna do CLI.

No pacote nativo instalado, o README público descreve JavaScript/addons embutidos
no ELF; o npm-loader público apenas encaminha ao pacote nativo. Não havia fonte
standalone do escritor nos arquivos legíveis examinados. Não se extraiu o bundle,
instrumentou o runtime ou inspecionou arquivos pessoais. Portanto não há call graph
verificado do escritor 1.0.95. Version/hash de SDK e CLI citados são os pins/identidade
históricos, não resultado de uma nova execução do CLI. Nada foi instalado/atualizado.

A [documentação oficial atual](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-config-dir-reference)
explica config.json como estado interno gerenciado e COPILOT_HOME como realocação
da configuração. Isso torna atualizações legítimas plausíveis, mas não prova a
operação desta versão ou seus bytes. O
[relato upstream de concorrência](https://github.com/github/copilot-cli/issues/1307)
refere-se a outra versão/plataforma (0.0.402, Windows); não reproduz Fedora1.0.95.
As [notas 1.0.95](https://github.com/github/copilot-cli/releases/tag/v1.0.95) não
atribuem a mutação aqui observada. Nenhuma dessas fontes concede aprovação da escrita.

| Hipótese | Estado desta investigação |
| --- | --- |
| Substituição atômica ou delete/create | POSSIBLE; forma compatível com metadados. Fixture reproduz tamanho igual com bytes diferentes. Não prova o mecanismo histórico. |
| Refresh interno legítimo em startup/auth/catálogo/quota/shutdown | POSSIBLE; documentação atual suporta app state mutável. Fase e contrato pinado não demonstrados. |
| CLI interativo/processo concorrente | POSSIBLE; não foi identificado um escritor na janela histórica. Fixture demonstra a insuficiência de stat para atribuir. |
| Corrupção, escrita maliciosa ou vazamento | NOT_ESTABLISHED; não há conteúdo/autor/tráfego que prove isso. |
| Tamanho igual ou auth positiva comprovam integridade | REJECTED como inferência; contraexemplos/testes independentes. |
| Snapshot posterior estável legitima mudança anterior | REJECTED; não acrescenta proveniência retroativa. |

Detalhes sanitizados: [investigação estática](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-static-investigation.json).

## 4. Contrato de integridade implementado

[Contrato/reprodução](../experiments/lr-10a-sdk-runtime/CONFIG-INTEGRITY.md) e
[módulo](../experiments/lr-10a-sdk-runtime/config_integrity.py). Não se exige imutabilidade
de app state de terceiros como regra universal; exige-se evidência de uma transição
permitida antes de aprová-la no experimento. Detectar metadata diferente não é
rotular uma corrupção. Quando falta essa evidência, a classificação continua explícita
sem presumir legitimidade nem bloquear o aplicativo por chmod/locks.

| Classificação | Evidência exigida/observada | Verificação |
| --- | --- | --- |
| OBSERVATIONALLY_STABLE | Campos selecionados iguais em dois snapshots | INCONCLUSIVE para conteúdo/autorização |
| METADATA_CHANGE_UNATTRIBUTED | Campo(s) diferente(s), escritor/semântica não comprovados | BLOCKED |
| LEGITIMATE_CHANGE_PROVEN | Revisão sintética fixa com arquivo/FD próprios, inode/device e bytes conhecidos verificados | PASS_SYNTHETIC_CONTRACT, só fixture |
| METADATA_UNAVAILABLE | Caminho ausente ou acesso a metadata negado | BLOCKED |
| VERIFICATION_FAILED | Schema/tipo inválido, symlink/tipo inesperado, race detectada ou I/O | BLOCKED |
| INCONCLUSIVE | Observação interrompida | INCONCLUSIVE, admissão BLOCKED |

**Não existe contrato implementado para aprovar escrita real do Copilot.** A única
prova de legitimidade cria seu próprio tempdir0700 e arquivo0600, mantém FD da
substituição durante rename e verifica revisão sintética fixa. Não aceita caminho
externo, flag legitimate, auth ou alegação/evento do chamador. PASS sintético não
se transfere ao host. A fixture cooperativa não é contenção contra adversário do
mesmo UID. As classificações sempre mantêm operational_admission=BLOCKED.

O snapshot novo só abre diretórios O_PATH/O_DIRECTORY/O_NOFOLLOW, observa metadata
ancorada sem seguir symlinks e rejeita traversal/relativos/tipos inesperados. Verifica
device/inode do pai entre stat/open; não abre o leaf para conteúdo. Retorna somente
campos/reasons controlados; distingue missing/EACCES/interrupção/I/O. Não substituiu
config_stat ou os wrappers reais: permanecem intactos, sem mudança de admissão.

Mesmo metadata estendida não é equivalência criptográfica. Há janelas entre os
pontos, possibilidade de ABA/reuso de inode e pai desanexado após anchoring, além de
resolução de clock/filesystem. Stat acessível não prova conteúdo legível. Ausência
de erro não é aprovação. Eventos de write/rename não identificam necessariamente
PID ou conteúdo seguro; nenhum watcher foi implantado nesta FIX.

O [replay offline](../experiments/lr-10a-sdk-runtime/review_a9_config.py) aceita somente
artefato Git fixo por SHA256 e output novo0600/O_EXCL/O_NOFOLLOW. Não possui argumento
CLI/config pessoal ou imports de subprocess/SDK. Exit0 significa review concluído,
não integridade PASS. Auth histórica positiva é projetada independentemente da
integridade bloqueada. Alteração dos bytes da evidência é rejeitada, sem novo probe.

## 5. Recuperação e separação de estado

**Nenhuma recuperação de arquivo pessoal foi executada.** A ação correta nesta
entrega foi preservar evidências e a autenticação histórica sem restaurar, copiar,
apagar, renomear, mudar permissões ou bloquear config. Atualidade da autenticação
continua NOT_RETESTED; não se afirma que a conta permaneceu válida sem consulta.

cwd/log-dir privados da POC separam output operacional, não config global. Código
pinado de base_directory exporta COPILOT_HOME para auth/sessões/telemetria juntos;
modo Empty também desabilita keytar. Nenhuma dessas opções foi alterada para forçar
separação. session_fs virtualiza armazenamento de sessões, não é guard de config
pessoal. Uma estratégia de estado separado exige prova de compatibilidade e não
pode copiar credenciais ou reconfigurar silenciosamente resolução de auth.

O replay histórico ficou METADATA_CHANGE_UNATTRIBUTED, coverage=historical_four_fields,
content_equivalence=NOT_VERIFIED, writer=INCONCLUSIVE e verification=BLOCKED.
[Resultado novo, sem adulteração do original](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-historical-review.json).

## 6. Testes e resultados

48 Rust e **86 Python PASS, zero FAIL**; 68 Python prévios +18 novos. Todos os testes
executados constam nos [logs Python](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-regressions/a9-host-python-tests.txt),
[logs Rust](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-regressions/a9-host-rust-tests.txt)
e [ownership](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-regressions/a9-host-owned-tests.json).
[Casos sanitizados](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-synthetic-cases.json)
foram extraídos de observações emitidas pelos testes realmente executados; não são
resultado inventado de CLI real. Tests SDK usam peers locais sintéticos, não Copilot.

| Caso requerido | Resultado de teste | Observação e limite |
| --- | --- | --- |
| T1 estável | PASS | Metadata concorda, conteúdo INCONCLUSIVE/admissão BLOCKED. |
| T2 rename atômico mesmo tamanho | PASS | inode diferente, bytes diferentes sintéticos; tamanho igual não prova equivalência. |
| T3 tamanho muda | PASS | Campo bytes detectado, escritor não inferido. |
| T4 timestamps mudam | PASS | utime controlado, mudança explícita sem sleeps/tolerância. |
| T5 escrita concorrente | PASS | Python próprio sincronizado por pipe/wait; stat não atribui PID, apesar do controle da fixture. |
| T6 ausente/inacessível | PASS | Ausência real sintética; EACCES injetado para tratamento determinístico. Não prova restrição de acesso ao host. |
| T7 symlink inesperado | PASS | Leaf/pai symlink e tipo não regular reais sintéticos rejeitados. |
| T8 falha/interrupção | PASS | I/O/InterruptedError injetados, saída sem exception prose; sem fallback. |
| T9 revisão legítima conhecida | PASS em fixture | FD próprio/rename/revisão fixa comprovados; nenhuma aprovação Copilot real. |
| T10 alteração não atribuída | PASS | Não aceita flag legítima nem caminho externo na prova sintética. |
| T11 auth positiva + integridade inconclusiva | PASS em fixture | Boolean auth independente; financeiro continua bloqueado. |
| T12 falha + financeiro | PASS em fixture | Finance BLOCKED, sessões/envios/claim zero. |
| T13 schema/tipos/booleans desconhecidos | PASS | Não aprovados nem exportados como prova. |
| T14 traversal/relativo | PASS | Rejeitado antes de abrir filesystem. |
| T15 race de identidade do pai | PASS | Mismatch injetado, VERIFICATION_FAILED. Não prova detectar todas as races. |
| T16 stat sem abrir leaf | PASS | Diretórios somente O_PATH/no-follow; sem leitura do conteúdo. |
| T17 estabilidade posterior | PASS | Não reclassifica mutação anterior como legítima. |
| T18 replay histórico/pin inválido | PASS | Histórico preservado e auth datada; evidência adulterada rejeitada. |

Comandos executados nesta FIX:

```sh
python3 experiments/lr-10a-sdk-runtime/review_a9_config.py --output experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-historical-review.json
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-regressions
python3 -m py_compile experiments/lr-10a-sdk-runtime/config_integrity.py experiments/lr-10a-sdk-runtime/review_a9_config.py experiments/lr-10a-sdk-runtime/tests/test_config_integrity.py
```

O runner anterior inalterado executa `/usr/bin/cargo test --offline --locked --
--test-threads=1`, COPILOT_SKIP_CLI_DOWNLOAD=1, CARGO_BUILD_JOBS=2 e RUSTC/RUSTDOC locais;
depois `/usr/bin/python3 -m unittest discover -s tests -p test_*.py -v` sob harness.
Rust/Cargo1.98.1 e Python3.14.7 preexistentes, sem instalação/download/update. Não se
atribui esta execução a Rust1.94. Hashes estão na
[verificação](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-3-verification.json).

Também executados: leitura estática/rg de fontes públicas locais e browsing oficial,
AST parse, parse JSON/JSONL novos, revisão diff/escopo/whitespace, comparação de
blobs/prefixos com a base, revisão de campos e busca limitada de padrões de tokens,
Bearer/private key (zero matches). Essa busca não garante detectar qualquer segredo.
Metadados do marker verificados por helper ancorado existente, sem leitura/claim.
Evidências txt novas tiveram somente whitespace final normalizado.

Regressões FIX1–4/A9/FIX1/FIX2 incluem child exhaustion/pidfd/setsid/descendente não
amostrado, externo preservado, timeout, worker failure inconclusivo, guard race/crash,
DenyAll/zero tools, negação de callbacks e boundary/gateway sintéticos. Nenhum teste
iniciou CLI Copilot real. Ownership de fixtures não valida auth/gui/billing reais.
Tauri/produção/UI: NOT_RUN, justificadamente sem diff Rust ou de produção. Não se
repetiu a suíte de 1.107 testes nem se declarou UI/headless service testado.

## 7. Gates preservados e processos

| Gate | Estado | Fonte/limite |
| --- | --- | --- |
| SDK_AUTHENTICATED_WITH_GUI | OBSERVED_REAL_PASS | A9-FIX-2/2026-10-10T03:34:13Z, não novo ensaio. |
| SDK_AUTHENTICATED_HEADLESS | NOT_PROVEN | Não se desligou GUI nem consultou auth nesta FIX. |
| CONFIG_MUTATION_OBSERVED | histórico true | inode/mtime/ctime diferentes; bytes470 iguais, conteúdo desconhecido. |
| CONFIG_WRITER_ATTRIBUTION | INCONCLUSIVE | Sem prova do escritor1.0.95 ou processo concorrente histórico. |
| CONFIG_INTEGRITY_VERIFICATION | BLOCKED | Contrato real de transição/proveniência ausente. |
| FINANCIAL_ADMISSION | BLOCKED | Preços/custo máximo/paid fallback/estado privado autenticado não resolvidos. |
| A9_REAL_INFERENCE | NOT_RUN | Zero inferências, sessões e ferramentas agentivas reais. |
| A9_ATTEMPT_MARKER | ABSENT_NOT_CLAIMED | Existência final false, diretório/arquivo não modificados, conteúdo não lido. |
| PROCESS_CLEANUP | PASS no escopo sintético owned | ECHILD, survivors vazios, identidades PID/start-time ausentes, ownership errors vazios. |
| Novo teste real | PENDING_USER_AUTHORIZATION | Somente proposta; nenhuma chamada nesta execução. |

Catálogo/quota da FIX-2 são históricos, não observações financeiras atuais. Só Auto
sem custo conhecido continua insuficiente. Zero inferências da POC não significa
medição de fatura externa. Uma chamada SDK não é necessariamente uma unidade interna
faturável. Não foi possível nem permitido retestar inferência/persistência genuínas.

O processo concorrente sintético foi esperado/reaped; a suíte inteira usou o
worker/subreaper FIX1 intacto com pidfds/identidades, bounded timeout e exhaustion.
Processos externos não atribuídos nunca são sinalizados pelo harness; fixture de
preservação externa passou. Nenhum runtime real precisou de shutdown nesta FIX;
cleanup SDK real da FIX-2 continua apenas histórico. Não se equipara recuperação
forçada a shutdown gracioso nem se proclama contenção adversarial/worker crash/
namespace/processo ininterruptível resolvida. Esses gates continuam separados.

## 8. Arquivos, preservação e publicação

Mudanças exclusivamente experimentais/documentais:

- Novos config_integrity.py, review_a9_config.py, tests/test_config_integrity.py e
  CONFIG-INTEGRITY.md na POC; HOST-ASSISTED.md recebeu só adendo.
- Novos evidence/a9-fix-3-historical-review.json, a9-fix-3-static-investigation.json,
  a9-fix-3-sdk-contract-excerpts.txt, a9-fix-3-synthetic-cases.json,
  a9-fix-3-verification.json.
- evidence/a9-fix-3-regressions/: a9-host-owned-tests.json, a9-host-rust-tests.txt,
  a9-host-python-tests.txt, a9-host-test-gateway.jsonl e a9-host-test-permissions.jsonl.
- [Documento permanente](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md) recebeu só adendo;
  este relatório foi substituído integralmente, sem histórico cumulativo.

Históricos de evidence e wrappers/config runtime/gui/headless/Cargo/harness foram
comparados por blobs Git com a base e estão intactos. Documentos permanentes/HOST
preservam o conteúdo anterior como prefixo. Nenhum arquivo de produção ou credencial
foi versionado. O relatório anterior permanece no histórico Git. Commit documental
final contém apenas este relatório; não requer repetir testes do código já testado.

## 9. Novo ensaio: proposta pendente, não executada

As evidências existentes não contêm fases/writer/bytes, e a implementação do escritor
nativo não foi demonstrada. Uma nova observação pode reduzir a janela temporal,
mas **metadata/eventos sozinhos não comprovam autoria ou segurança de conteúdo**.
Não se propõe um teste real automático para transformar o gate em PASS.

| Item | Proposta sujeita a revisão e autorização humana |
| --- | --- |
| Objetivo exato | Observar metadata por fase de um único Client, validar um contrato de update previamente revisado e delimitar a janela sem conteúdo privado. |
| Risco | Resolução normal de credenciais/cleanup pode atualizar state compartilhado; concorrência e shutdown podem causar novas mudanças. |
| Limite | Um start/getStatus/auth.getStatus/stop; sem models/quota/sessões/prompts/retry/inferência. |
| Proteção | Imagem já pinada, harness ownership, allowlist, cwd/logs privados, snapshots metadata-only ancorados; sem locks/read-only/cópias/restore ou mudança de auth. |
| Pré-condições | Autorização separada e auditoria do contrato/método; usuário confirmar ausência de atividade Copilot concorrente sem POC matar processos. |
| Interrupção | Metadata ausente, caminho/service/marker inesperado, timeout/cleanup incompleto ou primeira mutação não classificada: suspender fases opcionais e fazer shutdown bounded/evidências. |
| Limitação | Correlação de fases/eventos ainda não atribui autor ou valida semântica; não basta sozinha para aprovar. |

Estado **PENDING_USER_AUTHORIZATION**. Nenhuma autorização é solicitada durante esta
FIX, nenhum procedimento executável de novo probe foi incluído no replay. Propostas
que dependam de conteúdo/credenciais/intervenção sensível não serão implementadas
como workaround. Nenhuma cópia de token, proxy, alteração de HOME/config auth,
mount amplo ou restauração cega está prevista como solução.

## 10. Conclusão e operação futura

A mutação foi tratada como uma observação que exige atribuição/contrato, não como
corrupção presumida nem mudança automaticamente segura. Contrato/testes receberam
PASS parcial sintético. O gate real permanece **BLOCKED**, autoria **INCONCLUSIVE**,
com recomendação **FIX_AND_RETEST após auditoria**, sem avanço de etapa.

A operação headless futura não é comprovada por auth com GNOME. Deve validar resolução
normal de credenciais, dependências de serviço e política de estado mutável própria,
sem copiar credenciais ou exigir imutabilidade indevida do aplicativo terceiro.
Permanecem os bloqueios financeiros, de sessão privada/persistência, auth/rede isoladas
e contenção do supervisor. O prazo Narys0.1 em 17/10/2026 exige priorização humana,
sem dispensar esses gates; a POC permanece separada da produção.

**A9-FIX-3 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**
