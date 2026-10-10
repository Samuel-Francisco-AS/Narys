# LR-10A H1 — autenticação headless

Decisão candidata: **HEADLESS_REQUIRES_USER_SETUP**. A preparação não disruptiva
foi concluída. Os cenários B e C continuam NOT_TESTED; não há demonstração de
impossibilidade técnica nem de prontidão headless. Nenhum SDK/CLI foi iniciado
neste H1. Não houve transição de sessão, reboot, inferência ou consumo do marker.

## Três cenários independentes

| Cenário | Evidência |
| --- | --- |
| A: SSH com GNOME ativo | SDK1.0.17/CLI1.0.95 autenticados no ensaio histórico [A9-FIX-4R](evidence/a9-fix-4r-metadata-confirmation.json). A observação H1 encontrou GNOME/Wayland ativos, barramento acessível e coleção login desbloqueada. Não repetimos auth. |
| B: GNOME ausente depois de login gráfico | NOT_TESTED. Um cofre que sobrevive ao logout não demonstra desbloqueio após reboot. |
| C: cold-start multi-user sem login gráfico prévio | NOT_TESTED. Exige boot real posterior e registro de ausência de login gráfico desde o boot; remover DISPLAY não reproduz C. |

## Contratos verificados e limites de versão

A crate instalada **1.0.17**, `src/lib.rs:2251–2304`, exporta COPILOT_HOME quando
base_directory é configurado; ClientMode::Empty desabilita keytar; token explícito
injeta COPILOT_SDK_AUTH_TOKEN; use_logged_in_user=false acrescenta --no-auto-login.
A POC mantém resolução normal, sem esses overrides. A identidade ELF/SHA do
CLI1.0.95 foi novamente verificada por leitura do binário público, sem executá-lo.
Hash da fonte instalada e versões Fedora estão em
[h1-static-lifecycle.json](evidence/h1-static-lifecycle.json).

A [documentação atual do GitHub](https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli)
suporta autenticação remota por device flow, keychain Linux/libsecret e tokens
de ambiente; também descreve fallback de armazenamento em configuração em texto
plano quando o keychain não está disponível. Device flow não resolve por si só o
desbloqueio/persistência segura do cofre. Tokens explícitos e fallback plaintext
não são opções admitidas no H1. A documentação atual não prova todas as regras
internas do CLI1.0.95 instalado; a prova operacional existente cobre A.

O [Secret Service](https://specifications.freedesktop.org/secret-service/latest/ch03.html)
separa serviço acessível e coleção Locked. O diagnóstico consulta somente
NameHasOwner, Locked e PID do dono do nome; nunca Items, GetSecrets ou Unlock.
DBUS_SESSION_BUS_ADDRESS, socket acessível ou daemon vivo não demonstram que a
coleção está desbloqueada, que o SDK autentica ou como cada credencial é armazenada.

A [interface GNOME Keyring50.0](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/docs/gnome-keyring-daemon.xml)
oferece --unlock por stdin, sem necessidade intrínseca de interface gráfica.
É uma alternativa para **setup humano posterior**, mantendo o cofre existente;
não foi executada, e a POC nunca deve capturar a senha. Pode criar uma coleção
se ela não existir, por isso requer revisão e confirmação do usuário antes do uso.
A automação sem interação após reboot exige outra fonte segura de desbloqueio ou
uma integração de login já suportada pelo sistema, ainda não validada aqui.

O [PAM GNOME50.0](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/pam/gkr-pam-module.c)
usa a senha de autenticação; seu pam_sm_close_session é um no-op. O arquivo
histórico GNOME descreve encerramento de daemon no fechamento da sessão, mas
essa descrição **não prova o comportamento da versão instalada**. Login SSH por
chave não fornece automaticamente uma senha de desbloqueio ao PAM; não se
inspecionou nem alterou a pilha PAM desta máquina para presumir seu comportamento.

[Linger/systemd259](https://raw.githubusercontent.com/systemd/systemd/v259/man/loginctl.xml)
mantém o user manager entre logouts/boot, não fornece a senha do cofre.
Linger=no foi observado; não o habilitamos. Unidades com
[PartOf=graphical-session.target](https://raw.githubusercontent.com/systemd/systemd/v258/man/systemd.special.xml)
podem acompanhar o ciclo gráfico. As unidades genéricas instaladas do Keyring
estão inativas e sem PartOf/BindsTo; isso não descreve o daemon efetivamente dono
do Secret Service, iniciado em outro contexto.

## Por que B não foi iniciado

Em [h1-context.json](evidence/h1-context.json), o dono de org.freedesktop.secrets
é PID2602/start_ticks3099, no scope do login Wayland; coleção login Locked=false.
Há também outro processo Keyring no user manager; não se presume que ele receberia
o nome ou conservaria o cofre desbloqueado. SSH/sshd e tmux estão presentes, com
sessão remota distinta, mas isso não garante a sobrevivência de todos os serviços
ou do Codex durante a transição. O user manager usa Linger=no.

KillUserProcesses=false foi observado separadamente. **Não se afirma que logout
normal certamente mataria o cofre**. Entretanto, terminate-session mata os
processos do scope, incluindo o dono observado. Parar display-manager, alvos
gráficos ou apenas gnome-shell também não foi qualificado como transição segura
e reversível preservando Keyring e trabalho humano. Nenhuma dessas ações ocorreu.

A condição de segurança anterior à solicitação de interrupção não foi satisfeita;
não solicitamos aprovação de uma transição sem proteção comprovada. Isto é
BLOCKED_TRANSITION_SAFETY, não recusa do usuário nem impossibilidade de headless.

## Procedimento posterior, inerte

1. Usuário/coordenador confirma acesso físico de recuperação, dois acessos SSH,
   tmux acessível e ausência de trabalho gráfico não salvo. Inventariar somente
   identidades e dependências dos serviços a preservar.
2. Definir o mecanismo específico de encerramento **da sessão gráfica**, suas
   dependências e retorno por login físico. Não usar terminate-user, kill-session,
   stop display-manager ou encerrar Keyring como atalhos. Qualificar sobrevivência
   do dono de Secret Service e dos processos necessários; se não for possível,
   replanejar setup humano separado, sem executar a transição.
3. Apresentar os processos/sessão afetados e obter autorização específica naquele
   momento. Aprovação antiga de inferência não autoriza logout ou reboot.
4. Estabelecido B de verdade: comprovar ausência de sessão gráfica/gnome-shell,
   user bus disponível e Locked=false, repinar CLI1.0.95, verificar marker somente
   por existência. Ambiente filtrado e resolução normal; sem tokens/copiar cofre.
5. Reservar **nova identidade fixa one-shot** de metadados; não reutilizar os
   binários/reservas consumidos da A9-FIX-4R. Reutilizar seu fluxo limitado e
   harness: start → getStatus → auth.getStatus → shutdown, no máximo uma invocação,
   deadline65s, sem retry. Adaptar explicitamente o rótulo GUI desse fluxo para B;
   não executar o driver GUI antigo como se fosse headless. H1 não adiciona esse
   dispatcher enquanto a transição está bloqueada.
6. Em falha estrutural/contexto/shutdown: encerrar somente filhos atribuídos;
   preservar resultados, sem novos auth probes ou credenciais de fallback.
   Verificar ECHILD/identidades ausentes, serviços e marker. Retorno pelo usuário
   no acesso físico combinado, sem restart de serviços pela POC.

Para C: uma janela futura com reboot **feito pelo usuário**, boot multi-user sem
login gráfico, e setup humano de Secret Service/desbloqueio via interface oficial
sem salvar senha/token. Registrar bus, Locked e inexistência de login gráfico
desde o boot. Depois uma reserva independente e o mesmo metadata-only limitado.
Essa proposta não certifica unlock automático, persistência at-rest ou SDK auth.
Não contém um comando executável de reboot/logout/desbloqueio.

## Reprodução não disruptiva e regressões

```sh
python3 experiments/lr-10a-sdk-runtime/h1_context.py inspect /absolute/pinned/copilot
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir /tmp/fresh-h1-regressions
```

`inspect` exige evidência h1-context.json inexistente (O_EXCL) e já foi concluído;
nunca apagar/sobrescrever seu resultado. Seu filho `observe` exige o harness.
Não existe modo SDK/send, serviço mutável, unlock, token, force ou transição.
São lidos somente presença de nomes de ambiente, PID/start-time, cgroup convertido
em booleans, propriedades whitelisted e hashes de código público. stdout bruto de
comandos não é persistido. Estado unknown bloqueia a preparação correspondente.

15 novos testes cobrem ausência real versus DISPLAY, target/sessões desconhecidos,
bus sem unlock, owner gráfico/ausente, remoto ausente, sem ativação, saída
sanitizada, PID reuse, scope e ausência de grants. Regressões: **52 Rust/128 Python**
offline/locked/jobs2/test-threads1, sem CLI real; veja
[verificação](evidence/h1-offline-verification.json) e
[ownership](evidence/h1-regressions/a9-host-owned-tests.json).
Produção não mudou; suíte Tauri/UI não repetida.

Cleanup PASS_REAL refere-se aos subprocessos passivos e fixtures sintéticas,
não a um novo SDK autenticado. ECHILD e identidades ausentes foram verificados;
nenhum processo externo recebeu sinal. Continuam os limites de morte do worker,
descendentes adversariais, namespaces e tarefas kernel não interrompíveis.

## Release0.1 em 17/10/2026

Copilot permanece opcional/experimental. A rota existente com GNOME/Keyring tem
metadata auth comprovada, mas sessão privada, finanças e inferência continuam
bloqueadas; não é um executor operacional aprovado. A Narys0.1 pode priorizar
os provedores/caminhos já aprovados e operar seu host com GNOME/Keyring, sem tornar
Copilot requisito de todas as funções. H1 não revalida esses outros provedores.

Próximo esforço proporcional: setup humano de desbloqueio suportado + uma janela
coordenada B, depois uma janela C se autenticação server for requisito do produto.
Estimativa de planejamento, não medição: uma janela assistida de 30–60min por
cenário, podendo aumentar se o gerenciamento de sessão exigir revisão. Não abrir
novas FIXes para repetir A. Não redesenhar PAM, vault ou supervisor antes do release
por conveniência. Auditoria independente pendente; LR-10B não iniciado.
