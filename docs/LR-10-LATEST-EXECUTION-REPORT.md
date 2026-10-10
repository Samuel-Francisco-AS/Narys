# LR-10A H1 — Headless Authentication Feasibility

**LR-10A H1 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**

Recomendação: **HEADLESS_REQUIRES_USER_SETUP**. Preparação não disruptiva concluída;
B e C não testados. A transição gráfica ficou **BLOCKED_TRANSITION_SAFETY** porque
preservação do serviço de credenciais e recuperação confiável não foram
comprovadas. Não se afirma que headless seja impossível. Zero SDK/CLI reais
iniciados neste H1; zero inferências, sessões e claims do marker.

## 1. Identificação e Git

- Projeto Narys; fase LR-10A/H1; data10/10/2026, America/Fortaleza.
- Branch exclusiva `lr-10a-sdk-runtime-feasibility`.
- HEAD inicial local/remoto: `356ed0fdab673a40f58b772e81977916519f7101`; workspace limpo.
- Implementação testada e publicada: [`fa42aa9902449b56473cd45f8ab1888fa4bb2b54`](https://github.com/Samuel-Francisco-AS/Narys/commit/fa42aa9902449b56473cd45f8ab1888fa4bb2b54).
- HEAD remoto da implementação confirmado pelo `git ls-remote` após o primeiro push.
- HEAD documental final: este arquivo no [histórico da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility/docs/LR-10-LATEST-EXECUTION-REPORT.md), em commit posterior contendo somente este relatório; sem SHA autorreferencial.
- Main local/remota: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`, preservada.
- Nenhum merge/PR/rebase/reset/force-push, alteração de produção ou serviço.

## 2. Objetivo, implementação e arquivos

Investigar operação server sem GNOME, mantendo resolução normal de credenciais,
sem extração de segredos. Reutilizamos os helpers de consulta não ativadora e o
harness aprovado; nenhum novo sistema de autenticação, supervisor ou dispatcher SDK.

Arquivos da implementação:

- [h1_context.py](../experiments/lr-10a-sdk-runtime/h1_context.py): diagnóstico passivo sob subreaper/pidfds, saída sanitizada, PID/start-time validado duas vezes e cgroup reduzido a booleans. Consulta somente propriedades de sessão, NameHasOwner, Locked e PID do Secret Service. SHA da imagem pública validado sem iniciar CLI. Saída fixa O_EXCL; sem force, login/unlock, send ou transição.
- [test_h1_context.py](../experiments/lr-10a-sdk-runtime/tests/test_h1_context.py): 15 testes sintéticos de separação de cenários, desconhecidos, segurança de consultas e ausência de grants.
- [HEADLESS-FEASIBILITY.md](../experiments/lr-10a-sdk-runtime/HEADLESS-FEASIBILITY.md): contratos oficiais, diferenças de versão, limites e procedimento posterior inerte.
- [Adendo permanente](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md): conclusão H1 concisa, preservando todo o histórico.
- Evidências novas: [contexto](../experiments/lr-10a-sdk-runtime/evidence/h1-context.json), [lifecycle/fontes](../experiments/lr-10a-sdk-runtime/evidence/h1-static-lifecycle.json), [verificação offline](../experiments/lr-10a-sdk-runtime/evidence/h1-offline-verification.json) e cinco arquivos de [regressões](../experiments/lr-10a-sdk-runtime/evidence/h1-regressions/a9-host-owned-tests.json).
- Este relatório substitui exclusivamente o relatório anterior no commit documental final.

Os 205 arquivos experimentais previamente versionados permaneceram byte a byte
iguais ao HEAD inicial. Evidências anteriores não foram reclassificadas.

## 3. Evidências operacionais e cenários A/B/C

Observação passiva: **2026-10-10T10:19:08.485949+00:00**.

A sessão Wayland estava ativa, `graphical-session.target=active`; GNOME/GDM/Keyring
presentes. Sessão SSH distinta, sshd ativo, tmux e Codex observados. HOME,
DBUS_SESSION_BUS_ADDRESS e XDG_RUNTIME_DIR presentes; DISPLAY/WAYLAND_DISPLAY
absentes. Esses últimos booleans confirmam por que remover display não reproduz
headless. Nenhuma variável de token conhecida estava presente; só nomes/booleans
foram registrados.

Secret Service já possuía dono, socket do bus acessível e `Locked=false`. Dono
PID2602/start_ticks3099 no scope da sessão gráfica, fora do user manager.
Outro processo Keyring existia no user manager; não se presume transferência do
nome/desbloqueio entre eles. Serviços e identidade do dono permaneceram estáveis
nos dois pontos da observação. Linger=no. Unidades genéricas Keyring service/socket
inativas; sua declaração sem PartOf não prova lifecycle do daemon ativo.

**A — SSH com GNOME ativo:** autenticação real permanece comprovada apenas pelo
[ensaio A9-FIX-4R histórico](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-4r-metadata-confirmation.json),
2026-10-10T09:50:44.847325+00:00. Nenhum auth novo foi enviado no H1; validade atual
de credencial não foi reconsultada.

**B — GUI ausente após login gráfico:** NOT_TESTED. O processo dono das credenciais
está no scope gráfico. `terminate-session` encerraria esse dono. KillUserProcesses
é false e o PAM GNOME50 fecha sessão com no-op; portanto não afirmamos que um
logout normal necessariamente encerraria o serviço. Sua preservação, o comportamento
do GNOME e o retorno seguro tampouco foram demonstrados. Não houve transição,
logout, parada de display-manager ou solicitação de aprovação para uma ação ainda
sem proteção qualificada. Não se tratou de recusa do usuário. O estado não é
READY_FOR_HEADLESS_TEST: permanece bloqueada a segurança da transição.

**C — cold-start multi-user sem login gráfico anterior:** NOT_TESTED. Nenhum reboot
ou boot target alterado. Um eventual sucesso em B não provará C.

## 4. Conhecimento estático, versões e caminho seguro

Fedora44: GNOME Keyring50.0, GNOME Session50.1, systemd259.9. SDK instalado1.0.17,
features runtime/default-features=false, sem alteração de Cargo.lock/MSRV/Edition.
Fonte instalada SHA `23c99946ab6fa84ebee723fba58992ec4487f26e05fb209add570badeb39fa23`.
CLI1.0.95 manteve SHA
`9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`;
versão é referência do pin histórico, não nova execução --version.

O SDK1.0.17 conserva resolução normal em CopilotCli; Empty desabilita keytar,
base_directory altera COPILOT_HOME e use_logged_in_user=false adiciona
--no-auto-login. Nenhum desses overrides foi introduzido.

A [documentação GitHub atual](https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli)
suporta device flow remoto e keychain Linux/libsecret, mas também fallback plaintext
quando o keychain está indisponível. Esse fallback e tokens explícitos não são
admitidos aqui. Device flow não garante que o cofre estará desbloqueado no próximo
boot; a documentação atual não substitui contrato da versão instalada.

O [Secret Service](https://specifications.freedesktop.org/secret-service/latest/ch03.html)
separa coleção Locked de disponibilidade do barramento. Não se acessaram itens,
conteúdos nem métodos de Unlock. Conforme [GNOME50.0](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/docs/gnome-keyring-daemon.xml),
existe desbloqueio por stdin: caminho oficial sem dependência intrínseca de GUI,
para setup humano posterior, sem capturar senha na POC. Pode criar coleção se
inexistente; exige revisão/autorização própria, não um comando a executar agora.
[PAM50.0](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/pam/gkr-pam-module.c)
usa senha de autenticação e close_session é no-op; descrições antigas GNOME de
morte do daemon no logout não são prova da versão instalada. SSH por chave não
fornece automaticamente a senha de desbloqueio; a pilha PAM local não foi auditada
ou modificada para supor outra coisa.

[Lingering](https://raw.githubusercontent.com/systemd/systemd/v259/man/loginctl.xml)
conserva o user manager, sem disponibilizar uma senha do cofre. Não foi habilitado.
Não foi comprovado unlock automático seguro em cold-start. A alternativa simples
é setup humano via interface oficial para o cofre existente, depois uma validação
metadata-only coordenada. Não se propõe persistir tokens/senha em arquivo/env,
criar vault paralelo ou improvisar alterações PAM. Segurança de armazenamento
at-rest específica desta conta não foi comprovada pela observação Locked.

## 5. Gates independentes

| Gate | Estado | Escopo da evidência |
| --- | --- | --- |
| SSH_GUI_PRESENT_AUTH | PASS_REAL histórico | A9-FIX-4R; não novo auth H1. |
| GUI_ABSENT_AUTH | NOT_TESTED | B não estabelecido; transição não qualificada. |
| COLD_START_HEADLESS_AUTH | NOT_TESTED | Sem reboot/login exclusivamente headless. |
| CREDENTIAL_STORAGE_SAFETY | INCONCLUSIVE | Nenhuma extração; resolução normal anterior e cofre acessível não comprovam at-rest/unlock automático. |
| SDK_RUNTIME_COMPATIBILITY | PASS_REAL histórico | SDK1.0.17/CLI1.0.95/protocol3 anteriores; pin ELF/SHA revalidado no H1. |
| PROCESS_CLEANUP | PASS_REAL | Kernel ECHILD e identidades ausentes nos subprocessos passivos e fixtures H1; SDK não iniciado. |
| SESSION_ADMISSION | BLOCKED | Estado privado autenticado/permissões/interferência não promovidos. |
| FINANCIAL_ADMISSION | BLOCKED | Unidades, custo máximo e no-paid-fallback continuam não comprovados. |
| REAL_INFERENCE | NOT_TESTED | Zero envios neste H1; A9 real NOT_RUN. |

Autoridade agentiva permanece bloqueada. Autenticação, disponibilidade de serviço,
cenário gráfico e financial admission não herdam PASS um do outro.

## 6. Testes e comandos executados

1. `git status --short --branch`, `git rev-parse HEAD main`, `git ls-remote origin refs/heads/lr-10a-sdk-runtime-feasibility refs/heads/main`: base limpa/sem divergência.
2. Leitura estática da crate1.0.17, helpers/contratos/evidências anteriores e unidades públicas `/usr/lib/systemd/user/gnome-keyring-daemon.{service,socket}`; consultas oficiais referenciadas acima. Sem conteúdo pessoal/PAM/keyring.
3. `systemctl --user show ...` com somente Id/LoadState/ActiveState/SubState/PartOf/BindsTo/Requires/StopWhenUnneeded/RefuseManualStop/KillMode; `loginctl show-user/show-session` com propriedades não secretas selecionadas; `systemctl show sshd.service` estado; `rpm -q gnome-keyring gnome-session systemd`; busctl consulta booleana de KillUserProcesses. Sem mutações. Consultas iniciais de preparação não iniciaram SDK/CLI.
4. `python3 -m unittest discover -s experiments/lr-10a-sdk-runtime/tests -p test_h1_context.py -v`: **15/15 PASS**, fixtures, sem serviços reais.
5. `python3 experiments/lr-10a-sdk-runtime/h1_context.py inspect <CLI-nativo-pin>`: **PASS_PASSIVE_OBSERVATION**, uma coleta sob ownership/deadline25s, sem SDK. O filho observe está restrito ao harness; evidence O_EXCL já consumida como registro diagnóstico, independente do marker A9.
6. `python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir experiments/lr-10a-sdk-runtime/evidence/h1-regressions`: **52 Rust +128 Python PASS**. Inclui regressões FIXes1–4/A9, 15 novas, gateways/protocolos/permission handlers sintéticos e fixtures Linux. Cargo `test --offline --locked -- --test-threads=1`, COPILOT_SKIP_CLI_DOWNLOAD=1, CARGO_BUILD_JOBS=2, compiladores /usr/bin; nenhuma atualização/download. [Rust](../experiments/lr-10a-sdk-runtime/evidence/h1-regressions/a9-host-rust-tests.txt), [Python](../experiments/lr-10a-sdk-runtime/evidence/h1-regressions/a9-host-python-tests.txt).
7. `python3 -m py_compile` dos dois arquivos novos e `git diff --cached --check`: PASS. Revisão manual da formatação; ruff não disponível, não instalado. Rustfmt/Tauri/UI NOT_RUN: nenhum Rust/produção/UI modificado.
8. Parse de JSON/JSONL novos, comparação dos 205 arquivos experimentais históricos com git show da base, confirmação de source hash as-run, teste de ausência PID/start-time das invocações e existência do marker: PASS. Scan de formatos conhecidos de tokens sem imprimir conteúdos: zero correspondências; não é prova universal de ausência de todo segredo.
9. Commit/push da implementação para branch autorizada e confirmação do SHA remoto; depois este relatório em commit documental final. Não se executou driver autenticado, CLI help/version/status, catálogo/quota, inferência ou login/logout.

## 7. Cleanup, observabilidade e segurança

Diagnóstico passivo: wall1582.49ms, cleanup21.41ms, pico RSS amostrado21008384bytes,
CPU lower-bound1.16s. Regressões: wall11880.92ms, cleanup35.03ms, pico amostrado
209190912bytes. Métricas são amostradas; RSS pode duplicar páginas compartilhadas,
CPU é limite inferior. Não são medidas do startup/auth do Copilot.

Nos dois harness reports: timeout=false, cleanup_complete=true,
kernel_children_exhausted=true, ownership_errors=[], sobreviventes atribuídos=[],
recovery_signals=0. Identidades atribuídas conferidas ausentes depois. Fixtures
individuais verificam timeout/recuperação e preservação externa. Nenhum processo
externo, Codex, SSH/tmux, Keyring/GNOME/systemd foi sinalizado. O campo genérico
headless=true do harness significa execução sem interface própria; **GNOME continuou
ativo**. sdk_shutdown_verified=false é correto para H1 sem SDK.

Persistem limites de morte do worker, descendentes adversariais, reparenting/
namespaces não cobertos e tarefas kernel resistentes. Não se implementou supervisor
LR-10B. Ownership prova recuperação da invocação controlada, não contenção host.

Marker A9 verificado apenas por existência, ausente antes/depois e após regressões;
diretório privado validado pelo helper, sem leitura de conteúdo/claim. Nenhuma
configuração pessoal foi lida, copiada, bloqueada, restaurada ou alterada pela POC.
Não reabrimos observação do config.json: ausência de nova leitura não certifica
imutabilidade contra atividade concorrente de terceiros. Drift/autoria histórica
permanece como registrada. Não houve exportação de credenciais, mudanças de serviço,
PAM/permissões, instalação ou operação paga. Zero inferências enviadas não é uma
medição de despesas externas ou de saldo de quota.

## 8. Próximos passos, prazo e decisão

Antes de solicitar interrupção de GUI: qualificar lifecycle/retorno preservando
serviço de credenciais e todos os processos necessários, acesso físico de
recuperação, dois SSH e trabalho gráfico salvo. Apresentar exatamente a sessão e
dependências afetadas e obter autorização específica. O [procedimento inerte](../experiments/lr-10a-sdk-runtime/HEADLESS-FEASIBILITY.md)
prevê no máximo uma nova reserva independente metadata-only após B comprovado;
não reutilizar as reservas/binaries GUI já consumidos. Sem prova de segurança,
não executar a transição. Cold-start depende de boot futuro pelo usuário, sem
login gráfico prévio e setup humano seguro, seguido de ensaio independente.

**HEADLESS_REQUIRES_USER_SETUP**, sem PASS operacional completo. A autenticação
sem GUI é plausível por Secret Service + desbloqueio oficial no terminal, mas
continua não demonstrada nesta máquina. Não se justifica nova FIX para repetir A.
Estimativa de planejamento: janela assistida30–60min por cenário, não medição nem
promessa; revisão do gerenciamento de sessão pode exigir mais trabalho.

Para Narys0.1 em17/10/2026: Copilot não deve bloquear todas as funções do produto.
Priorizar caminhos/provedores já aprovados, mantendo Copilot experimental; H1 não
revalidou outros provedores. GNOME/Keyring pode permanecer como contexto inicial
com metadata auth comprovada, sem promover Copilot a executor, liberar sessão,
financeiro ou inferência. LR-10B não iniciado. A9 isolado mantém seus bloqueios de
auth/rede/supervisor; HOST_ASSISTED não é sandbox. Auditoria independente da Luna
pendente antes de qualquer avanço.
