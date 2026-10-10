# LR-10A H2 — Headless Credential Unlock & Operational Feasibility

**LR-10A H2 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**

Decisão: **HEADLESS_PREPARED_PENDING_USER_TRANSITION**. Desbloqueio de coleção
existente, sem GNOME Shell, e reabertura pelo backend Rust foram comprovados com
**componentes reais e credenciais inteiramente sintéticas em namespaces privados**.
O host pessoal continua com GUI. Não é HEADLESS_READY_MANUAL_UNLOCK; auth Copilot
headless, Stronghold pessoal, serviço user-manager real e cold-start permanecem
não testados. Nenhuma transição, serviço pessoal, inferência ou claim A9 ocorreu.

## 1. Identificação, Git e escopo

- Data:10/10/2026, America/Fortaleza; projeto Narys; exclusivamente LR-10A/H2.
- Branch: `lr-10a-sdk-runtime-feasibility`.
- HEAD inicial local/remoto: `60f601dbcaf0fb1f6aaf78baf5c7c904e02a8a2c`; workspace limpo.
- Componentes reais sintéticos/Rust testados: [`887477a631cf519811b357aad5a6407dafaebabd`](https://github.com/Samuel-Francisco-AS/Narys/commit/887477a631cf519811b357aad5a6407dafaebabd).
- Implementação final do helper/testes: [`e7ea1b60a1ad5992cad6d34c44d084e9fc37afa2`](https://github.com/Samuel-Francisco-AS/Narys/commit/e7ea1b60a1ad5992cad6d34c44d084e9fc37afa2).
  A revisão só endurece a verificação de contexto GUI; classe encrypted e leitor
  TTY têm AST idêntica à prova anterior. Ambos commits técnicos publicados antes
  deste relatório; HEAD remoto confirmado por `git ls-remote` no segundo SHA.
- HEAD documental final: commit posterior contendo **somente este relatório**, no
  [histórico verificável da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility/docs/LR-10-LATEST-EXECUTION-REPORT.md).
  Sem SHA autorreferencial; o fechamento verifica HEAD remoto e bytes publicados.
- Main local/remota preservada em `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`.
- Sem PR, merge, rebase, reset, force-push, instalações, atualizações, sudo ou linger.
- Produção, Broker, ExecutionAuthority, IPC, TaskGraph/LR8.5, SDK/CLI pins,
  Cargo.lock/MSRV/Edition e harness FIX1 não foram alterados.

Objetivo: preparar serviço de credenciais sob demanda e desbloqueio humano por
SSH, sem senha persistida e sem exigir GNOME Shell permanentemente. Não foi
implementado servidor Narys, aplicativo Android, produção ou LR10B.

## 2. Arquivos entregues

- [h2_keyring.py](../experiments/lr-10a-sdk-runtime/h2_keyring.py): launcher
  sintético-only, Bubblewrap/estado privado, FIX1 intacta, timeout45s, evidência O_EXCL.
- [h2_keyring_fixture.py](../experiments/lr-10a-sdk-runtime/fixtures/h2_keyring_fixture.py):
  D-Bus/Keyring reais privados, senha pública sintética, rejeição de senha incorreta,
  restart da mesma coleção, hashes internos de snapshot/cofre e medições.
- [h2_stronghold.rs](../experiments/lr-10a-sdk-runtime/fixtures/h2_stronghold.rs):
  inclui backend/audit de produção **sem modificá-los**, somente sentinel/HOME e
  caminho de fixture; nenhuma opção de cofre pessoal. Publica resultado/presença,
  nunca valor de item/chave. Binário debug fica ignorado em target.
- [h2_manual_unlock.py](../experiments/lr-10a-sdk-runtime/h2_manual_unlock.py):
  helper humano experimental, TTY privado, eco desativado, encrypted libsecret,
  existente-only, pin GNOME50, owner/UID/PID/start-time/user-manager e GUI ausente.
  Não foi usado para desbloquear credenciais pessoais.
- [test_h2_keyring.py](../experiments/lr-10a-sdk-runtime/tests/test_h2_keyring.py):
  21 regressões novas, inclusive pseudo-terminal real com senha **sintética**.
- [Contrato, reprodução e plano humano](../experiments/lr-10a-sdk-runtime/HEADLESS-MANUAL-UNLOCK.md),
  README e [adendo permanente](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md).
- Evidências novas `evidence/h2-*`, discriminadas abaixo. Este relatório é a única
  substituição documental; documentos/evidências históricos permanecem preservados.

## 3. Contrato real de desbloqueio

GNOME Keyring instalado50.0/libsecret0.21.8.2/systemd259.9; Python3.14.7/GI instalados.
Binário Keyring SHA256:
`c7c5ad270c98fc0c9466031a08a037d650a918786036579003d9bcca4844481e`.

A [fonte GNOME50.0](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/daemon/gkd-main.c)
mostra que `--unlock` lê stdin e aplica a senha **na inicialização** do daemon.
O [fluxo de login](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/daemon/login/gkd-login.c)
pode criar login ausente/inicializar slots. No teste inicial, executá-lo ao lado
de um daemon existente retornou0 sem desbloquear/criar a coleção visível no
bus; não é um dispatcher geral de unlock desse dono. Não foi repetido no host.
O ensaio correto iniciou o daemon foreground com senha sintética, comprovando
criação explícita apenas da coleção de teste. Não recomendar --replace/--unlock
como tentativa cega no cofre pessoal.

O método [UnlockWithMasterPassword](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/daemon/dbus/gkd-secret-service.c)
procura a coleção específica e falha quando ausente, sem criar coleção. Está na
[interface que o GNOME marca explicitamente como não suportada](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/daemon/dbus/org.gnome.keyring.InternalUnsupportedGuiltRiddenInterface.xml).
A proposta é experimental/versionada, não API pública portátil. Seu uso pessoal
precisa ser revisado e especificamente aceito; atualizações futuras falham no pin.

O helper reutiliza [encode_dbus_secret da libsecret](https://gnome.pages.gitlab.gnome.org/libsecret/method.Service.encode_dbus_secret.html)
e verifica o [algoritmo de sessão](https://gnome.pages.gitlab.gnome.org/libsecret/method.Service.get_session_algorithms.html).
Contra nome D-Bus único não ativável, exige DH/AES; fallback plain é rejeitado
**antes** da solicitação de senha. Não chama CreateCollection/GetSecrets/Store/Prompt.
A sessão criptográfica Secret Service não é uma sessão Copilot.

## 4. Evidências reais sintéticas e observação do host

[Ensaio final](../experiments/lr-10a-sdk-runtime/evidence/h2-keyring-synthetic.json):

1. Serviço privado sem GNOME; coleção ausente foi rejeitada pelo guard.
2. Criação **deliberadamente sintética** por startup --unlock; coleção desbloqueada.
3. Backend Narys criou snapshot/chave de teste usando SystemCredentialStore real.
4. Coleção bloqueada, senha incorreta negada, senha correta aceita pelo método
   existente-only usando libsecret DH/AES. Nenhum password em argv/env/logs.
5. Backend consultou presença; reinício do daemon recuperou a mesma coleção
   bloqueada; novo desbloqueio e reabertura do mesmo snapshot passaram.
6. Bytes do login.keyring e snapshot sintéticos permaneceram iguais depois de
   unlock/restart/presença; keyring files0600 e diretórios privados. Nenhum legacy
   plaintext foi criado. Nenhum conteúdo/chave foi publicado.

O namespace começa vazio e usa /usr read-only para software instalado confiável
**desse teste**, etc sintético, /proc e /dev privados, estado0700, sem rede, home,
/run/user ou bus pessoal. Não é o boundary do Copilot, não amplia FIX3/4 e não é
uma prova universal contra host/root/processos adversariais do mesmo UID.

[Contexto passivo atual](../experiments/lr-10a-sdk-runtime/evidence/h2-context.json),
11:47:46UTC, reutiliza H1 sob FIX1, **sem executar CLI**: GNOME/GDM ativos, socket
user-bus acessível, login Locked=false. Secret Service dono PID2602/start_ticks3099
no scope gráfico, fora do user manager; outro daemon PID2800 no user manager.
sshd ativo; tmux/Codex fora do scope gráfico; user manager ativo, Linger=no.
Não foi comprovada transferência de nome/credenciais/desbloqueio entre daemons.

Fonte50.0 observa fechamento logind se houver XDG_SESSION_ID: pode sair ao fechar
sessão, apesar de PAM close_session no-op. Não lemos /proc/environ nem afirmamos
que o daemon atual herdou essa variável. Não assumimos sobrevivência ao logout.
Logout simples pode deixar GNOME Shell do greeter ativo; não equivale a headless.

## 5. Matriz independente de gates

| Gate | Resultado e alcance |
| --- | --- |
| HEADLESS_KEYRING_SERVICE | PASS componentes reais em namespace sintético; user-manager/cofre pessoal sem GUI NOT_TESTED. |
| MANUAL_UNLOCK_METHOD | PASS sintético: existente-only, libsecret encrypted, TTY sem eco; uso pessoal PENDING_HUMAN_SETUP, extensão não suportada. |
| DBUS_USER_SESSION | PASS_REAL no host com GUI; bus privado headless PASS sintético. Bus de usuário real sem GUI NOT_TESTED. |
| STRONGHOLD_ACCESS | PASS backend real em fixture; snapshot pessoal NOT_TESTED. |
| COPILOT_SDK_AUTH | NOT_TESTED nesta H2; GUI conserva PASS_REAL histórico A9-FIX4R, sem novo ensaio. Headless não comprovado. |
| GUI_ABSENT_VERIFIED | PASS no namespace sintético; host pessoal NOT_TESTED, GUI permaneceu ativa. |
| COLD_START_HEADLESS | NOT_TESTED; nenhum reboot/boot target modificado. |
| SERVICE_PERSISTENCE | PASS restart sintético com mesmo armazenamento; logout/boot/user-manager reais NOT_TESTED. |
| PROCESS_CLEANUP | PASS observado nas fixtures/probes passivos sob FIX1; limite do debug isolado abaixo. |
| FINANCIAL_ADMISSION | BLOCKED, preservado; nenhum novo catálogo/quota/modelo consultado. |
| REAL_INFERENCE | NOT_RUN, zero SDK/send/sessões Copilot reais, marker ausente/não consumido. |

Nenhum PASS sintético concede autorização operacional ou comprova autenticação
pessoal. Sessões privadas, agentes e limites do A9_ISOLATED permanecem bloqueados.

## 6. Testes, comandos e falhas preservadas

[Verificação consolidada](../experiments/lr-10a-sdk-runtime/evidence/h2-verification.json):

- Compilação standalone de `fixtures/h2_stronghold.rs`, Edition2021, com rustc1.98.1
  instalado e rlibs **compatíveis previamente em cache**. [Argv/hashes](../experiments/lr-10a-sdk-runtime/evidence/h2-rust-fixture-build.json)
  e [build final](../experiments/lr-10a-sdk-runtime/evidence/h2-rust-fixture-final-build.json).
  Cache inicialmente selecionado1.94 retornou E0514; nenhum download/rebuild
  global para resolver. Sem alteração de toolchain, deps ou produção. Reprodução
  fica BLOCKED se o cache compatível faltar, não baixa/recompila Tauri automaticamente.
- `python3 experiments/lr-10a-sdk-runtime/h2_keyring.py`: PASS_SYNTHETIC_ONLY,
  binário/source SHA identificados no JSON. Evidência fixa O_EXCL já ocupada;
  não apagar, sobrescrever ou introduzir retry arbitrário.
- `python3 .../verify_a9_host.py --artifacts-dir .../evidence/h2-regressions`:
  **52 Rust/148 Python PASS**, incluindo20H2; [logs Rust](../experiments/lr-10a-sdk-runtime/evidence/h2-regressions/a9-host-rust-tests.txt),
  [Python](../experiments/lr-10a-sdk-runtime/evidence/h2-regressions/a9-host-python-tests.txt),
  [ownership](../experiments/lr-10a-sdk-runtime/evidence/h2-regressions/a9-host-owned-tests.json).
  Executa Cargo `test --offline --locked -- --test-threads=1`,
  COPILOT_SKIP_CLI_DOWNLOAD=1/CARGO_BUILD_JOBS=2, depois unittest discover sob FIX1.
- Testes novos: fixture fora do namespace negada; job/permissões/binário inválidos;
  sem HOME/bus/rede pessoal; GUI/contexto desconhecido/owner incompatível negados;
  sem senha em argumentos/pipeline/worker; crypto fallback negado; método sem
  criação/retry; eco/restauração/erro; PTY **real com senha pública sintética**.
- Revisão final: falha da consulta ps não pode ser interpretada como GUI ausente;
  exige exit1. [149 Python PASS sob FIX1](../experiments/lr-10a-sdk-runtime/evidence/h2-python-final-regressions.json),
  [log final](../experiments/lr-10a-sdk-runtime/evidence/h2-python-final-tests.txt),
  incluindo21H2; uma nova regressão testa erros/unknown. AST demonstra classe
  encrypted/leitor TTY idênticos à execução sintética no primeiro commit. Rust
  não mudou e não foi repetido gratuitamente depois desta revisão Python.
- AST de todos os Python novos, rustfmt --check Edition2021/skip_children,
  JSON/JSONL e git diff --cached --check PASS. Um newline redundante no novo log
  Rust foi removido; hashes original/publicado e bytes removidos estão no JSON.
- 215 arquivos experimentais históricos permaneceram byte a byte; só README
  recebeu adendo. Documentos permanentes preservam conteúdo anterior. Produção
  não mudou: suíte completa Tauri/UI NOT_RUN, justificada pelo escopo experimental.

Falhas de desenvolvimento não são reclassificadas como PASS: [D-Bus inicial](../experiments/lr-10a-sdk-runtime/evidence/h2-keyring-initial-setup-failure.json),
[segunda tentativa](../experiments/lr-10a-sdk-runtime/evidence/h2-keyring-second-setup-failure.json),
[NoReply](../experiments/lr-10a-sdk-runtime/evidence/h2-keyring-dbus-identity-failure.json),
[criação](../experiments/lr-10a-sdk-runtime/evidence/h2-keyring-create-failure.json),
[startup descoberto](../experiments/lr-10a-sdk-runtime/evidence/h2-keyring-startup-unlock-discovery.json),
[construtor libsecret abortado](../experiments/lr-10a-sdk-runtime/evidence/h2-keyring-libsecret-constructor-failure.json).
A infraestrutura privada passwd/group/machine-id/NSS corrigiu conexão, sem provar
isoladamente qual arquivo causou NoReply. Usar o construtor público open_sync da
libsecret eliminou o aborto na prova posterior. [Transporte plain exclusivamente
sintético](../experiments/lr-10a-sdk-runtime/evidence/h2-keyring-plain-transport-synthetic.json)
não foi promovido a método pessoal; foi substituído por encrypted libsecret.
[Desenvolvimento encrypted](../experiments/lr-10a-sdk-runtime/evidence/h2-keyring-encrypted-transport-development.json)
e [antes do check adicional dos bytes](../experiments/lr-10a-sdk-runtime/evidence/h2-keyring-encrypted-pre-snapshot-check.json)
identificam suas implementações separadas. Essas repetições foram fixtures locais,
nunca Copilot ou credenciais pessoais.

## 7. Recursos, cleanup e segurança

Ensaio final:5.180,09ms, cleanup32,80ms; RSS agregada amostrada640.958.464bytes
(~611,27MiB), CPU amostrada4,84s como limite inferior. Daemon Keyring amostrado
9.748.480–10.383.360bytes; readiness10,72–11,41ms em três starts. Não é benchmark
frio, RAM incremental de produção ou atribuição de consumo a uma KDF específica.
Binário debug162.290.248bytes ignorado em target, não publicado/downloadado.

13 registros sob harness têm ECHILD, identidades PID/start-time ausentes e cleanup
completo; não há sobrevivente conhecido de fixture. Nenhum processo externo
recebeu sinal. Daemons privados são encerrados somente por pidfd de filho
atribuído; timeout/recovery dos testes anteriores continuam separados de shutdown
SDK. H2 não iniciou SDK contra CLI real: sdk_shutdown_verified=false não é falha
de SDK novo. Testes Rust exercitam SDK somente contra fixtures controladas.

Uma execução adicional de desenvolvimento, somente no namespace sintético, usou
contenção PID do Bubblewrap fora do harness antes da correção D-Bus. Não há medida
pidfd/ECHILD para essa invocação; não se inventa essa verificação. Nenhum sinal
externo/credencial/serviço foi envolvido. Preservamos limites de morte do worker,
adversariais/reparenting/namespaces e tarefas kernel não interrompíveis; não é o
supervisor de produção nem alegação de contenção adversarial completa.

Nenhuma leitura de conteúdo de configurações/keyring/itens/sessões Copilot pessoais,
/proc/environ, tokens, PAM ou senha humana; apenas propriedades de contexto permitidas.
Nenhuma cópia de credenciais, plaintext fallback,
config global, keyring pessoal, serviços, GNOME/GDM ou boot alterados. Marker
verificado **só por existência**, ausente antes/depois/final; não criado/consumido.
Não se afirma saldo/cobrança externa medidos: **zero inferências enviadas pela POC**.

Helper humano é host-assisted, não sandbox. Desativa core/dumpability e restaura
TTY em interrupções tratáveis; Python mantém dados transitoriamente em RAM sem
zeroização garantida. SSH privado não gravado/capturado pelo Codex é obrigatório.
Senha não deve aparecer no chat, argv, env persistente, histórico, echo ou logs.
SIGKILL/host comprometido e protocolo Secret Service MODP1024 têm limitações
explícitas no contrato; não se promete segurança universal.

## 8. Etapa humana pendente e Stronghold

Foi solicitada preferência para coordenar a etapa pessoal ou publicar preparação;
**não recebemos confirmação específica de transição/desbloqueio nesta execução**.
Nenhuma ausência de resposta foi tratada como autorização. Publicação desta
preparação não encerra a GUI e não presume recusa do usuário.

Próxima etapa concreta está no [plano H2](../experiments/lr-10a-sdk-runtime/HEADLESS-MANUAL-UNLOCK.md):
confirmar recuperação física, dois SSH/tmux, trabalho salvo, efeitos sobre GUI e
aprovação naquele momento. Qualificar Secret Service sob user manager **sem
alterar** PAM/linger/credenciais, não pressupor transferência. Só depois, humano
em SSH separado poderá usar o helper, se aceitar a extensão experimental. A POC
não pode capturar essa digitação. Não executar logout/stop-display-manager/target
ou senha agora. Não há dispatcher de transição nesta entrega.

Stronghold pessoal não foi aberto: `secret_presence` passa por unlock_client e
pode chmod/inicializar chave/snapshot ou migrar legado. Não satisfaz diagnóstico
pessoal puramente read-only; exige escopo próprio antes de qualquer operação
pessoal. A fixture demonstra o mesmo backend com estado sintético.

Se necessário após qualificação, preparar um novo dispatcher/identidade fixa H2
one-shot para **uma** invocação SDK1.0.17/nativeCLI1.0.95
SHA`9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`:
start/getStatus/auth.getStatus/shutdown apenas, sem retry/model/quota/session/send.
Nenhuma reserva anterior será reutilizada. H2 não acrescentou esse dispatcher
antes das pré-condições reais; não executar os drivers GUI como se fossem headless.
Cold-start precisa janela posterior de boot pelo usuário, não realizado aqui.

## 9. Alternativa e release0.1

Não foi comprovado bloqueio estrutural do Keyring sem GNOME; o método sintético
é promissor e evita migração de credenciais. Limite material: extensão GNOME não
suportada, configuração user-manager/retorno e compatibilidade do cofre pessoal
não testadas. Primeiro qualificar setup humano versionado e esse único ensaio;
não abrir ciclos de FIX para repetir autenticação com GUI.

Se a extensão não for aceita, estudar uma interface suportada de unlock/PAM
humano sem alterar a pilha nesta H2. Alternativa Stronghold: UnlockKeyStore com
Argon2id/salt aleatório/parâmetros versionados, sem salvar senha/chave plaintext.
A chave aleatória atual exige migração criptografada, backup/rollback/recovery;
não pode ser substituída por uma KDF sem planejamento. **Proposta, não implementada**;
o sucesso sintético não justifica migração pessoal agora.

Prazo17/10/2026: estimativa de planejamento, não medição, uma janela assistida de
30–60min para qualificar serviço/transição/desbloqueio e auth limitada; cold-start
em janela separada. Narys0.1 pode priorizar provedores já aprovados e o contexto
GNOME/Keyring existente enquanto a validação pessoal está pendente. Copilot
permanece opcional/experimental; nenhum PASS completo LR10A ou avanço LR10B.

**Conclusão: preparação parcial demonstrada, HEADLESS_PREPARED_PENDING_USER_TRANSITION.
Auditoria independente da Luna pendente.**
