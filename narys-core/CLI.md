# Narys — terminal Linux, SSH e Termux (SERVER-1C)

No Termux, conecte ao host com `ssh sam@HOST`. Execute **no host**:

```sh
narys doctor
narys credentials status
narys credentials unlock
narys chat
```

O comando fica em `~/.local/bin/narys`. Se esse diretório ainda não estiver no
PATH do shell, use `~/.local/bin/narys`; a instalação não edita seus dotfiles.
Não há binário Android nesta etapa: Termux é o terminal do cliente SSH.

`credentials unlock` exige sessão logind SSH autenticada, shell interativo direto,
TTY de primeiro plano com stdin/stdout/stderr no mesmo terminal privado. Não use
pipe, redirecionamento, `ssh HOST comando`, scripts, tmux, screen ou agente. Abra
`ssh sam@HOST` e digite o comando no shell. A senha é solicitada sem eco apenas
quando o login keyring existente está bloqueado. Não forneça senha em argumentos,
variáveis, chat, JSON, arquivos ou comandos de LLM. O servidor e o Keyring são
serviços de usuário e não dependem de manter a sessão SSH aberta.

`narys help` lista comandos e opções. Consultas e erros suportam `--json`, com
versão v1, booleano `ok` e `data` ou `error_code`. Exit0 indica consulta/operação
aceita; exit1 indica erro. `task --wait` que devolve tarefa failed/cancelled continua
sendo uma consulta bem-sucedida: examine `data.state`. Unlock com `--json` ainda
exige TTY sem redirecionamento; o prompt vai ao mesmo terminal privado.

```sh
narys sessions --after 0 --limit 20
narys session 38 --after 0 --limit 50
narys session new
narys session resume 38
narys session close 38
narys tasks --after 0 --limit 50
narys tasks --namespace lr10a
narys task 189 --json
narys task 189 --wait --timeout 300
narys cancel 191
narys events --after 0 --limit 128 --json
narys models
narys providers --json
```

Páginas retornam `has_more` e cursor `next_session`, `next_message`, `next_task` ou
`next_sequence`. Passe esse cursor em `--after`; não interprete sessão como cursor
de evento. Eventos com `complete=false` indicam perda por retenção (4096 eventos).
`models` mostra catálogo local integrado e não faz consulta remota de disponibilidade.
`providers` também informa permissões, quotas/admission/rate/resilience/telemetry,
política Conversation e presença booleana de credenciais. Não valida billing.

Chat permite paginar (`more`) e escolher/criar sessão, enviar texto, obter recibo durável, acompanhar
a tarefa e imprimir seu resultado. `/exit`, `/history [cursor]`, `/task ID` e `/cancel ID`
consultam ou encerram o cliente. Ctrl-C durante acompanhamento preserva a tarefa.
Para submissão sem interface interativa, passe somente **texto de conversa** por
stdin a `narys send SESSION_ID`. O limite é 4096 bytes, e não há repetição automática.

Ao perder uma conexão durante envio, a admissão pode ter ocorrido. Reconecte e
consulte sessão, tasks e eventos antes de decidir sobre novo envio. RequestId não
é exactly-once. Reinício do servidor marca trabalho incerto interrupted; não
reenvia nem retoma inferência remota automaticamente. O Core possui todo o estado.

Configuração suportada:

```sh
narys policy show --json
narys provider gemini disable
narys provider groq enable --confirm-free
narys policy set < politica-conversation.json
narys approval RECEIPT_ID approve-once
narys approval RECEIPT_ID deny
```

Use `--confirm-free` apenas após confirmar pessoalmente plano sem cobrança e quota
para o provider. O Core não ativa overage, upgrade ou fallback pago. `policy set`
aceita o contrato `CognitiveRolePolicy` existente (camelCase), validado pelo Core,
com os limites IPC documentados. `policy show` devolve a visão de providers/política
completa. Approvals/ferramentas ainda retornam `capability_not_integrated`; não
executam shell nem transformam texto do modelo em autorização humana.

## Backend de desbloqueio

A CLI Rust incorpora o adapter existente-only; não executa scripts do checkout.
Usa Python3 isolado (`-I`, ambiente limpo), PyGObject/Gio e libsecret do sistema.
Status lê alias/Locked/metadados sem abrir sessão Secret Service nem ler itens.
Unlock é local, fora do IPC v1; usa a extensão GNOME50 `UnlockWithMasterPassword`
com Secret cifrado por libsecret, nome D-Bus único, UID/PID/executável/hash e
cgroup do serviço GNOME verificados antes da senha e antes do efeito. Mudança do
backend recusa com erro recuperável; exige revisão, não remoção casual do pin.

A senha é lida diretamente do terminal em buffer nativo mlock, sem String/bytes
Python contendo a senha, com core dumps desabilitados. Buffer é zerado; SecretValue
nativo é liberado; sessão criptografada é fechada; sinais INT/HUP/TERM restauram
termios. Não há password pipe, arquivo temporário, argv/env de senha, log ou
IPC genérico. Nenhuma coleção/snapshot/credencial é criada ou regravada.

Processos do mesmo UID continuam a fronteira de confiança Linux estabelecida na
1A/1B. As verificações recusam pipes, PTYs genéricos e ancestrais de agentes/scripts;
não provam humanidade contra um processo malicioso do mesmo UID que controle um
shell/TTY SSH legítimo. Não se declara isolamento adversarial adicional.

Instalação/atualização (sem sudo): construa `narys-core` e `narys` no target usado
pelo updater e execute `ops/install_cli.py CORE_SHA256 UNIT_SHA256` com hashes atuais
conferidos. Instalação desconhecida é preservada; trabalho ativo bloqueia update;
backups do Core/unit/CLI são privados. CLI/status/credentials não dependem do
checkout. O supervisor legado LR-10A/Copilot ainda mantém dependências históricas
do checkout e do CLI Copilot, fora da substituição criptográfica desta etapa.
