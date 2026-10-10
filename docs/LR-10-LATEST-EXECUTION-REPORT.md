# NARYS — LR-10A / H3

**Real Headless Credential Validation — Fedora44 — 10/10/2026**

**HEADLESS_MANUAL_UNLOCK_PASS_REAL**, limitado a desbloqueio humano do cofre
existente e autenticação metadata-only do Copilot **após login gráfico anterior,
sem reboot**. Não equivale a cold-start, acesso Stronghold pessoal, admissão
financeira, inferência, sandbox ou integração operacional da Narys.

**LR-10A H3 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**

## 1. Identificação e Git

- Branch exclusiva: `lr-10a-sdk-runtime-feasibility`.
- HEAD inicial local/remoto: `d7d9eb4c4d97ffb19edba2e2015bdb93b849eb66`.
- Implementação testada e ensaio real: [78392e29dcb20014d12cc808f8207073cdff7477](https://github.com/Samuel-Francisco-AS/Narys/commit/78392e29dcb20014d12cc808f8207073cdff7477).
  Seus hashes de fontes/binário correspondem à execução; não houve mudança de
  código depois do ensaio. O commit técnico foi publicado e HEAD remoto conferido.
- Main local/remota preservada: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`.
- HEAD documental final: commit mais recente deste arquivo no
  [histórico verificável da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility/docs/LR-10-LATEST-EXECUTION-REPORT.md).
  Este commit documental altera **somente este relatório**, sem SHA autorreferencial.
- Workspace inicial limpo. Alterações intermediárias eram preparação H3 do
  próprio agente; nenhum trabalho humano foi descartado. Sem PR, merge, rebase,
  reset, force-push ou alterações em main.

## 2. Autorização e limites

O usuário autorizou encerrar a sessão gráfica, depois confirmou especificamente
parada temporária do GDM, trabalho gráfico salvo e acesso SSH/Termux independente.
Ele armou rollback administrativo em SSH privado, parou o GDM, desbloqueou a
coleção no terminal privado e iniciou novamente o GDM. Senhas nunca foram
solicitadas no chat/capturadas pelo agente. Não houve reboot, target permanente,
PAM, senha alterada, cofre novo, migração, --replace, token explícito, login/logout
Copilot, plano/pagamento ou inferência.

H3 autorizou no máximo **uma** invocação SDK metadata-only, após desbloqueio:
Client start, getStatus, auth.getStatus e shutdown. A futura autorização A9 não
foi consumida. Sessões/modelos/quota/ferramentas/send/retry permanecem proibidos.

## 3. Transição, rollback e recuperação observados

[Pré-condições autorizadas](../experiments/lr-10a-sdk-runtime/evidence/h3-authorized-transition-preconditions.json)
identificaram a sessão Wayland2, sshd ativo, Codex/tmux fora do scope gráfico.
O Codex no user manager tinha PartOf apenas no scope remoto, sem BindsTo gráfico;
exigir PartOf totalmente vazio inicialmente bloqueou a preparação, e a inspeção
passiva subsequente comprovou a dependência remota segura. Nenhuma sessão foi
encerrada por essa inspeção.

O primeiro timer foi verificado no system manager: waiting/transient, root,
oneshot, vínculo e ação exata `/usr/bin/systemctl start gdm.service`, prazo30min.
[StopUnit sem interação](../experiments/lr-10a-sdk-runtime/evidence/h3-gdm-stop-request.json)
e [sudo -n restrito](../experiments/lr-10a-sdk-runtime/evidence/h3-gdm-stop-noninteractive-authority.json)
retornaram exit1; GDM continuou ativo. Não houve prompt/captura de senha.
O humano efetuou a parada no SSH privado; nenhuma terminação por nome/PID genérico
foi usada. Ele substituiu o timer por `narys-h3-gui-rollback-2.timer` e desativou o
anterior. O novo vínculo, autoridade, ação e prazo foram novamente verificados.

[Após parar GDM](../experiments/lr-10a-sdk-runtime/evidence/h3-post-user-gdm-stop-context.json):
sem Wayland/X11 do usuário, GDM ou GNOME Shell; alvo gráfico inativo; user manager,
bus, SSH/tmux/Codex presentes. Ambos os daemons Keyring anteriores saíram.
**Não houve transferência automática do Secret Service.**

[Antes do SDK](../experiments/lr-10a-sdk-runtime/evidence/h3-after-manual-unlock.json)
restavam1283,30s no rollback. O agente não cancelou nenhum timer.
[Retorno final](../experiments/lr-10a-sdk-runtime/evidence/h3-gdm-recovery-confirmed.json):
o usuário informou ativação do GDM; active/running confirmado, identidades
protegidas preservadas, mesmo owner do Secret Service e metadados do cofre iguais.
Não houve login gráfico nem inspeção da tela física. Nenhum GNOME Shell, inclusive
no UID do greeter, foi observado nessa inspeção; pode aparecer posteriormente.
Restaurar o serviço GDM não comprova uma sessão gráfica autenticada.

## 4. Serviço existente e desbloqueio humano

[Pré-condições do serviço](../experiments/lr-10a-sdk-runtime/evidence/h3-service-start-preconditions.json):
nenhum owner/daemon ativo; login.keyring preexistente, regular, UID1000,0600,
sem symlink nos componentes verificados. Conteúdo não lido. Daemon instalado50.0
confirmado por pacote e SHA
`c7c5ad270c98fc0c9466031a08a037d650a918786036579003d9bcca4844481e`.

Iniciamos **somente a unidade user instalada** gnome-keyring-daemon.service e
sua socket, sem enable/linger. [Override somente runtime](../experiments/lr-10a-sdk-runtime/evidence/h3-credential-service-start.json)
em `/run/user/1000/systemd/user/gnome-keyring-daemon.service.d/90-narys-h3-context.conf`:

```ini
[Service]
UnsetEnvironment=XDG_SESSION_ID DISPLAY WAYLAND_DISPLAY
LimitCORE=0
TimeoutStopFailureMode=terminate
```

Não altera ambiente global do user manager, HOME/COPILOT_HOME, armazenamento,
PAM ou unidade em /usr. Remove dependência de contexto gráfico herdado; não se
inspecionou environ do daemon para alegar qual variável anterior ele possuía.
LimitCORE impede core dumps desta unidade; não constitui proteção integral de RAM.
Override permanece em /run; serviço/socket intencionalmente ativos não são órfãos.

[Pré-unlock](../experiments/lr-10a-sdk-runtime/evidence/h3-before-manual-unlock.json):
owner PID21550/start1642719 no user manager, fora de scope gráfico, alias login
existente e Locked=true. Helper H2 pinado, inalterado, sem coleção nova/replace.
O humano executou em outro SSH privado:

```sh
cd /home/sam/Projetos/Narys
python3 experiments/lr-10a-sdk-runtime/h2_manual_unlock.py unlock-existing-login
```

Informou `LOGIN_UNLOCKED`; verificamos Locked=false sem itens/segredos.
A extensão GNOME é interna **não suportada**, versionada50.0; helper exige
libsecret DH/AES e nega plain. Esse contrato não vira API pública estável.
Não gravamos/capturamos senha e não afirmamos zeroização/swap seguro de Python.
Metadados do login.keyring mantiveram-se iguais após startup/unlock/SDK/retorno;
isso não prova equivalência criptográfica do conteúdo.

## 5. Implementação e arquivos

No [commit técnico](https://github.com/Samuel-Francisco-AS/Narys/commit/78392e29dcb20014d12cc808f8207073cdff7477):

- [h3_recovery.py](../experiments/lr-10a-sdk-runtime/h3_recovery.py): classificador
  passivo do rollback inicial; para o segundo, vinculamos explicitamente a unidade
  `-2.service` antes de aplicar o mesmo contrato de ação/root. Identidade efetiva
  preservada nas evidências; não há force ou concessão de autoridade SDK.
- [h3_headless.py](../experiments/lr-10a-sdk-runtime/h3_headless.py): contexto real,
  ausência GUI, owner/PID/start/UID/SHA, alias existente, Locked e marker.
- [h3_metadata.py](../experiments/lr-10a-sdk-runtime/h3_metadata.py) e
  [entrypoint Rust](../experiments/lr-10a-sdk-runtime/src/bin/h3-metadata-confirm.rs):
  reutilizam opções, protocolo e harness existentes; identidade H3 independente,
  duas reservas fixas O_EXCL, sem argumentos de retry/force. Corrige somente o
  rótulo GUI do confirmer compartilhado para o perfil headless qualificado.
- [Testes headless](../experiments/lr-10a-sdk-runtime/tests/test_h3_headless.py)
  e [recuperação](../experiments/lr-10a-sdk-runtime/tests/test_h3_recovery.py): nove
  casos novos; GUI/erro não viram ausência, pin/marker negam acesso, one-shot
  permanece consumido após falha, ação/autoridade/timer inválidos bloqueiam.
- [Guia H3](../experiments/lr-10a-sdk-runtime/HEADLESS-HOST-VALIDATION.md), README,
  adendo permanente e evidências novas `evidence/h3-*`.

Não alteramos FIX1–H2, suas evidências, produção, Broker/ExecutionAuthority,
AgentRegistry, TaskGraph/LR8.5, IPC, UI, MSRV/Edition ou dependências.
[Verificação](../experiments/lr-10a-sdk-runtime/evidence/h3-delivery-verification.json)
comparou552 arquivos históricos experimentais/produção: somente README mudou;
os demais permaneceram byte a byte iguais. Documentos permanentes receberam adendo.

## 6. Única execução SDK real

[Evidência](../experiments/lr-10a-sdk-runtime/evidence/h3-metadata-real.json),
concluída **2026-10-10T13:35:47.432477Z**, contém hashes exatos de fontes/binário.
SDK `github-copilot-sdk=1.0.17`, CLI nativo1.0.95 SHA
`9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`;
protocolo3 esperado. Binário H3 SHA
`e4a39f123ff97cf5341e4b1e619a5a6f59aa01e6b20c60535a0bf1aad77b748f`.

Uma inicialização, um status, um auth status: **authenticated=true**, shutdown
Rust graceful. SDK pode fazer connect/ping internos; não se alegam somente quatro
pacotes RPC nem equivalência a unidades faturáveis. Zero inferências/sessões,
modelos/quota/tools; sem retry. Estado/autenticação por resolução normal do host,
sem token explícito ou cópia de credenciais. Não isolamos qual armazenamento
interno o CLI utilizou; o sucesso combinado não prova exclusividade causal do
Keyring. Não introduzimos fallback plaintext.

Config apresentou drift inode/mtime/ctime em after_start; tamanho permaneceu
estável, estrutura PASS_METADATA_ACCESS. Autoria INCONCLUSIVE e conteúdo
NOT_VERIFIED; não lemos/restauramos config nem declaramos legítima a escrita.
Metadata pode ser aceito observacionalmente com drift; nenhuma admissão sensível
herda esse resultado. O HEADLESS_AUTH da política genérica continua NOT_PROVEN:
o gate H3 é confirmado separadamente por contexto real antes/depois e SDK.

Reserva [h3-runtime-reservation.json](../experiments/lr-10a-sdk-runtime/evidence/h3-runtime-reservation.json)
consumida, evidência O_EXCL também. **Não repetir/apagar reservas.** Marker de
inferência A9 não criado/lido/consumido: somente existência verificada.

## 7. Gates H3

| Gate | Resultado | Evidência/limite |
|---|---|---|
| USER_GUI_SESSION_ABSENT | PASS_REAL durante ensaio | logind sem Wayland/X11 do usuário |
| GNOME_SHELL_PROCESS_ABSENT | PASS_REAL durante ensaio | ps exit1, GDM parado; não DISPLAY filtrado |
| SSH_ACCESS_PRESERVED | PASS_REAL | SSH/tmux/Codex e identidades preservados |
| RECOVERY_PATH_VERIFIED | PASS_REAL | timer root registrado; retorno GDM active/running observado |
| USER_DBUS_AVAILABLE | PASS_REAL | socket/bus acessíveis, sem ativação arbitrária |
| HEADLESS_SECRET_SERVICE | PASS_REAL | mesma unidade instalada, owner no user manager |
| EXISTING_LOGIN_COLLECTION | PASS_REAL | alias existente; identidade/metadados do arquivo preservados |
| MANUAL_UNLOCK | PASS_REAL | humano informou sucesso; Locked true→false observado |
| STRONGHOLD_PERSONAL_ACCESS | NOT_TESTED | secret_presence pode escrever/ajustar/migrar; não chamado |
| COPILOT_SDK_HEADLESS_AUTH | PASS_REAL | única invocação real authenticated=true |
| PROCESS_CLEANUP | PASS_REAL | ECHILD, PID/start ausentes, sem recuperação forçada |
| COLD_START_HEADLESS | NOT_TESTED | nenhuma reinicialização; cenário pós-login gráfico |
| FINANCIAL_ADMISSION | BLOCKED | nenhum modelo/quota/custo/fallback novo comprovado |
| REAL_INFERENCE | NOT_RUN | zero send, prompts, inferências ou ferramentas |
| A9_ATTEMPT_MARKER | PASS_REAL preservação | ausente/não reclamado; conteúdo não lido |

SESSION_ADMISSION e AGENT_ACTION_ADMISSION **BLOCKED**. Gates de A9_ISOLATED
(auth/rede/contenção) não foram resolvidos por HOST_ASSISTED.

## 8. Testes, comandos e recursos

[Verificação offline](../experiments/lr-10a-sdk-runtime/evidence/h3-offline-verification.json)
e [harness de regressão](../experiments/lr-10a-sdk-runtime/evidence/h3-regressions/a9-host-owned-tests.json):
**52 Rust +158 Python PASS**, incluindo nove novos H3, zero CLI real nessas fixtures.
Quatro testes Rust direcionados de metadata passaram antes da regressão completa;
21 H2 também passaram. Nove H3 retestados após formatação final do novo entrypoint.
Formatação, py_compile, JSON/JSONL e diff check aprovados. /usr/bin/rustfmt não
existia; utilizamos rustfmt da toolchain stable já instalada, sem download.
Uma quebra vazia extra no final do log Rust novo foi removida para diff check,
sem alterar testes/resultados. Logs [Rust](../experiments/lr-10a-sdk-runtime/evidence/h3-regressions/a9-host-rust-tests.txt)
e [Python](../experiments/lr-10a-sdk-runtime/evidence/h3-regressions/a9-host-python-tests.txt).

Comandos principais executados:

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc /usr/bin/cargo build --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml --bin h3-metadata-confirm --offline --locked
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc /usr/bin/cargo test --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml --test metadata_confirmation --offline --locked -- --test-threads=1
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir experiments/lr-10a-sdk-runtime/evidence/h3-regressions
python3 -m unittest discover -s experiments/lr-10a-sdk-runtime/tests -p 'test_h3_*.py' -v
python3 -m py_compile experiments/lr-10a-sdk-runtime/h3_recovery.py experiments/lr-10a-sdk-runtime/h3_headless.py experiments/lr-10a-sdk-runtime/h3_metadata.py
python3 experiments/lr-10a-sdk-runtime/h3_metadata.py
```

O último comando foi executado **uma vez** e está bloqueado por reservas existentes.
Não é instrução para reexecutar. Cargo usa compilador instalado1.98.1; não foi
instalado/atualizado1.94. SDK/lock/MSRV/Edition preservados. Suíte Tauri/UI completa
NOT_RUN por ausência de alteração em produção; não houve prova UI.

Medição real com caches aquecidos: start2781ms, stop889ms, total4787,38ms,
cleanup30,8ms, pico RSS somada267.878.400bytes (~255,47MiB), CPU amostrada como
limite inferior1,92s, pico3 processos possuídos. Contagem global229→230 não prova
órfãos: as provas são ECHILD/identidades possuídas ausentes. [Amostra daemon](../experiments/lr-10a-sdk-runtime/evidence/h3-daemon-resource-sample.json)
RSS10.948.608bytes (~10,44MiB), PID/start/UID validados. Não são memória incremental
nem benchmark frio; sem alegação de economia total comparada ao GNOME.

## 9. Segurança e limites residuais

[Safety pós-SDK](../experiments/lr-10a-sdk-runtime/evidence/h3-post-sdk-safety.json)
confirma zero sinais de recuperação, nenhum survivor possuído, ECHILD e arquivo
Keyring observacionalmente estável. O harness genérico tem sdk_shutdown_verified
false por schema legado; shutdown específico vem do relatório Rust graceful,
com cleanup independente do kernel. Não reclassificamos FIX1.

Daemon de credenciais intencionalmente gerido por systemd é serviço do host;
não matar para “limpar” fixtures. SSH/sshd, tmux, Codex, systemd e bus preservados.
Não há orfandade experimental conhecida. Morte inesperada do worker, descendentes
adversariais, reparenting/namespaces não cobertos e tarefas D-state continuam
limites: nenhum supervisor definitivo foi implementado.

Sem tokens, itens, conteúdo config/Keyring/Stronghold, argv/env de processos,
strace, dumps ou tráfego autenticado nos artefatos. Scan de padrões conhecidos
de segredo é um check adicional, não prova universal de ausência de segredos.
Metadados seguros/IDs Linux são publicados; identificadores de conta não.
Builtin MCP desabilitado; nenhuma sessão/caminho de ferramenta foi exercido.
DenyAll/zero tools anteriores preservados, sem alegação de cobertura de todos os
hooks/extensões pelo teste de metadata. HOST_ASSISTED não oferece sandbox.

Arquivo de preparação inicial h3-pretransition-context conserva a observação
anterior à correção do classificador de root ausente: seu resultado já era BLOCKED,
nunca autorizou SDK. Hash dessa preparação não equivale ao código final. As
pré-condições aprovadas e o ensaio real usam hashes posteriores verificáveis.

## 10. Produto, pendências e decisão

Recomendação: manual unlock tem **viabilidade real pós-login gráfico** para o
Copilot metadata. GNOME Shell não é requisito permanente desse cenário observado.
Não declarar servidor Narys/Stronghold pessoal/headless cold-start prontos.
Linger=no: continuidade após último SSH e boot inteiramente headless não foram
validados. Serviço instalado é sob demanda e override é temporário em /run.

Próximos passos proporcionais, sob autorização/auditoria futura: validação de
boot feito pelo humano, mesmo mecanismo manual e ciclo do user manager; contrato
Stronghold de status sem efeitos incidentais; financeiro/modelo/custo máximo e
estado de sessão privado antes de A9. Não iniciar isso nesta H3 nem consumir a
inferência futura. Não escolher migração criptográfica por conveniência.

Release0.1 alvo17/10/2026: a prova reduz a dependência gráfica para credenciais
Copilot neste cenário, sem tornar esse SDK requisito de todas as funcionalidades.
Priorizar caminhos já aprovados enquanto cold-start/Stronghold/finanças permanecem
pendentes. Não houve LR10B, integração operacional, Android ou nova autoridade.

**Zero inferências enviadas pela POC nesta H3; saldo/cobrança externa não medidos.**
Não houve consulta atual de quota nem afirmação de saldo zero/ilimitado. Uma futura
chamada SDK não garante uma única requisição/unidade faturável interna.

**LR-10A H3 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**
Nenhum PASS definitivo da LR10A/LR10 ou autorização de avanço foi atribuído.
