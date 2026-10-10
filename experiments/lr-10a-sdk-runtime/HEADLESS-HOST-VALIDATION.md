# H3 — validação operacional no host pessoal

**HEADLESS_MANUAL_UNLOCK_PASS_REAL**, limitado a headless **após login gráfico**,
manual unlock e autenticação metadata-only. Cold-start/Stronghold pessoal não
foram testados. HOST_ASSISTED_HEADLESS é acesso sob o mesmo usuário, **não sandbox**.
Nenhuma inferência, sessão, ferramenta, modelo ou quota foi executada.

## Transição e recuperação

O humano confirmou trabalho gráfico salvo e SSH independente, autorizou parar
GDM e criou rollback root temporário. Codex verificou registro/waiting, unidade,
ação exata `/usr/bin/systemctl start gdm.service`, root/oneshot/transient e prazo
antes da transição. Tentativas não interativas de parada pelo agente retornaram
falha de autoridade; GDM continuou ativo. O humano parou-o em SSH privado e
substituiu o timer por **narys-h3-gui-rollback-2.timer**, desativando o anterior.
O novo vínculo exato e a mesma ação/autoridade/prazo foram verificados novamente.
[h3-after-manual-unlock.json](evidence/h3-after-manual-unlock.json) preserva os
campos efetivos do timer e cerca de1283s restantes antes do único ensaio SDK.

[h3_recovery.py](h3_recovery.py) é um classificador passivo do timer inicial.
Para o segundo timer, a inspeção H3 confirmou explicitamente o vínculo a
`narys-h3-gui-rollback-2.service` antes de aplicar o mesmo contrato de ação/root.
Essa adaptação está registrada nas evidências; não é uma autorização SDK ou
um parâmetro force. O retorno previsto é **GDM/tela de login**, sem login humano
automático. Não cobre falha do host/system manager. O timer não foi cancelado
pelo agente. O humano realizou retorno antecipado pelo SSH privado:

```sh
sudo systemctl start gdm.service
```

[Retorno observado](evidence/h3-gdm-recovery-confirmed.json): GDM active/running,
identidades SSH/tmux/Codex preservadas, mesmo dono do Secret Service no user
manager e metadados do cofre iguais. Não houve login gráfico ou prova visual da
tela física. Nenhum GNOME Shell foi observado nessa inspeção, incluindo o UID
do greeter; isso não garante que ele não apareça depois. Timer permaneceu ativo.

Não parar GUI/serviços nem rearmar ensaios SDK ao reproduzir documentação.
Nenhuma mudança permanente de target, PAM, linger ou enable foi feita. Ausência
real foi comprovada por logind, alvo gráfico inativo e ps exit1 para todos os
GNOME Shell, sem equiparar remoção de DISPLAY à ausência de GUI.

## Serviço de credenciais e intervenção humana

Após a parada, ambos os Keyrings anteriores saíram; não havia owner nem daemon.
Iniciamos a unidade **instalada** gnome-keyring-daemon.service e sua socket,
sob demanda, sem --replace, segundo daemon, cofre novo ou alteração de HOME.
O override **somente runtime**, em
`/run/user/1000/systemd/user/gnome-keyring-daemon.service.d/90-narys-h3-context.conf`,
contém:

```ini
[Service]
UnsetEnvironment=XDG_SESSION_ID DISPLAY WAYLAND_DISPLAY
LimitCORE=0
TimeoutStopFailureMode=terminate
```

Isso evita depender de contexto gráfico herdado e evita core dumps desta unidade;
não modifica o ambiente global do user manager. Nenhuma senha/token é transportado
por essas opções. Serviço/socket permanecem intencionalmente geridos por systemd;
não são processos órfãos da POC. Override desaparece com /run; Linger=no, portanto
não está comprovada continuidade após o último SSH ou cold-start sem configuração
humana. Não parar esse serviço pessoal para fins de cleanup de fixtures.

[Pré-condições](evidence/h3-before-manual-unlock.json): daemon50.0 pelo SHA/UID/
PID/start-time, owner no user manager, alias login existente, Locked=true,
GUI ausente, bus acessível, arquivo login.keyring preexistente e metadados seguros.
O humano usou o [helper H2 inalterado](h2_manual_unlock.py) em SSH privado,
fora do Codex, sem gravação, sem eco e sem senha em chat/argv/env/pipes:

```sh
cd /home/sam/Projetos/Narys
python3 experiments/lr-10a-sdk-runtime/h2_manual_unlock.py unlock-existing-login
```

O humano informou LOGIN_UNLOCKED; Locked=false foi observado antes do SDK.
A operação usa extensão GNOME **interna não suportada**, pin50.0 e libsecret
DH/AES, negando transporte plain e coleção ausente. Não transforma essa extensão
em API estável. A senha foi digitada pelo humano no terminal privado, sem captura pelo agente;
Python não promete zeroização/swap seguro. Mesmo UID/root/host comprometido não
estão isolados. Não houve leitura de itens, tokens ou conteúdos pessoais pelo
agente. Metadados do login.keyring ficaram iguais, sem afirmar igualdade de bytes.

## Único ensaio SDK — já consumido

[h3_metadata.py](h3_metadata.py), [h3_headless.py](h3_headless.py) e
[entrypoint Rust](src/bin/h3-metadata-confirm.rs) reutilizam o confirmer de
metadados, opções host e FIX1 sem modificá-los. O novo entrypoint corrige a
identificação de perfil no relatório compartilhado, após qualificar headless.

Evidência **h3-metadata-real.json** e reserva **h3-runtime-reservation.json** são
O_EXCL fixas e independentes das reservas anteriores/A9. Não apagar, sobrescrever
ou invocar novamente. Não existe parâmetro de retry/identidade/force. O contrato
exige hashes do código/binário testados offline, CLI1.0.95 SHA
`9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`, SDK1.0.17,
contexto real sem GUI, owner/coleção desbloqueada, marker ausente, estrutura de
config segura e inspeção proporcional de concorrência. Ambiente herdado é
filtrado para HOME/bus/runtime; nenhum token explícito/COPILOT_HOME alternativo.
Timeout65s, stdin null, stderr descartado, RPC sanitizado e cleanup por ownership.

Aplicação: **uma** inicialização, **um** getStatus, **um** auth.getStatus e shutdown.
Handshake/ping internos do SDK fazem parte do lifecycle, não são inferências.
Não se criou sessão: permission handler/DenyAll permanece no contrato anterior,
sem caminho que exponha ferramentas nesta operação. MCP builtin desabilitado.
Não se afirma cobertura operacional de hooks/ferramentas pelo teste de metadata.

[Resultado real](evidence/h3-metadata-real.json): authenticated=true, versão/
protocolo esperados, shutdown graceful, ECHILD e identidades ausentes, nenhum sinal
forçado. Drift de config inode/mtime/ctime no startup, estrutura aceita, autoria
INCONCLUSIVE e conteúdo NOT_VERIFIED. Não restauramos nem lemos config pessoal.
A política metadata não transmite PASS para sessões, financeiro ou autoridade.
O campo HEADLESS_AUTH da política genérica permanece NOT_PROVEN: essa política
não certifica contexto. O gate H3 específico usa contexto real antes/depois +SDK.
O sdk_shutdown_verified genérico do harness é false por schema legado; usamos
a observação Rust de shutdown e a prova independente de exaustão kernel.

## Evidências, recursos e limites

[Offline](evidence/h3-offline-verification.json):52Rust/158Python, nove testes novos
H3; [regressão ownership](evidence/h3-regressions/a9-host-owned-tests.json).
Compilador instalado1.98.1, SDK/lock/MSRV/Edition inalterados, jobs2, offline/locked;
formatação do novo entrypoint e reteste final H3. Nenhuma suíte Tauri/UI repetida:
não houve alteração de produção.

SDK: start2781ms, stop889ms, total4787,38ms, cleanup30,8ms; RSS máxima somada
267.878.400bytes (~255,47MiB), CPU amostrada como limite inferior1,92s.
[Daemon](evidence/h3-daemon-resource-sample.json): amostra RSS10.948.608bytes
(~10,44MiB), identidade verificada. Caches aquecidos; não são memória incremental
nem benchmark frio. Contagem global229→230 não prova orphan: ECHILD/identidades
possuídas são a evidência de cleanup. Nenhum processo externo sinalizado pelo
harness; morte do worker/adversariais/reparenting/namespaces/D-state continuam
limites conhecidos, sem supervisor de produção.

Stronghold pessoal NOT_TESTED: secret_presence tem efeitos incidentais e nenhum
acesso pessoal foi autorizado. Preservação baseada em ausência de operação sobre
o snapshot, não em leitura/hash de conteúdo. COLD_START_HEADLESS NOT_TESTED;
FINANCIAL/SESSION/AGENT_ACTION_ADMISSION BLOCKED; REAL_INFERENCE NOT_RUN;
marker A9 ausente/não reclamado. Sem saldo atual ou alegação de cobrança externa.

Para release0.1 em17/10/2026, manual unlock tem viabilidade pessoal demonstrada
para Copilot metadata sem GNOME Shell permanente. Validar boot humano headless e
acesso Stronghold com contrato sem efeitos incidentais em escopo futuro limitado.
Não migrar criptografia nem iniciar LR10B. Copilot continua opcional/experimental.

**LR-10A H3 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**
