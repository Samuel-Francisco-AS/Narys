# LR-10C — Authority, Approval Policy, Sandbox & YOLO

**IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE DA LUNA.**
Base: `e77c721beb091d6191b79f18b14cfa882b77babe`; branch
`lr-10c-authority-approval-sandbox`. Não concede PASS definitivo ou libera
inferência, cobrança ou ferramentas autenticadas. Ver
[entrega](LR-10C-DELIVERY-REPORT.md) e [matriz](evidence/lr10c/VALIDATION-MATRIX.md).

## Decisão de segurança e disponibilidade

O CLI possui seu próprio loop de ferramentas. **Não existe mediação desse loop
pelo ExecutionBroker.** A decisão é conservar o SDK/lifecycle nativo da LR-10B,
negar ferramentas e preparar contratos neutros no Core. A sandbox offline de
diagnóstico não é instalada como executor do Copilot. Não foi construída uma
ponte fictícia de ferramentas nativas para o Broker.

| Capacidade | Estado demonstrado nesta candidata |
|---|---|
| Contratos tipados, grants opacos, consumo transacional, revogação | PASS em fixtures Core; integração de execução nativa PARTIAL |
| Consulta/histórico/negação de approvals por IPC/CLI | PASS com servidor e subprocessos locais sintéticos |
| Aprovação humana positiva operacional por CLI/SSH | BLOCKED: não existe canal com separação demonstrada para o runtime nativo |
| Assistido com efeitos nativos | BLOCKED: sem boundary/canal humano/financial admission operacional |
| Autônomo isolado Copilot | BLOCKED: a prova offline não cobre transporte autenticado, credenciais e todas as ferramentas |
| YOLO | Contrato PARTIAL, ativação positiva externa BLOCKED, execução irrestrita desabilitada |
| Bubblewrap offline, workspace descartável, Python/shell/descendentes | PASS nas tentativas reais locais especificadas na matriz; não equivale a sandbox Copilot completa |
| Hooks/permission handler SDK 1.0.17 | PASS em protocolo sintético; cobertura nativa antes de efeitos NOT_VERIFIED |

BLOCKED é uma indisponibilidade deliberada e efetiva, não um aviso seguido de
execução. Nenhum desses perfis pode ser habilitado por prompt, configuração,
retry, resume, fallback ou simples mudança de um campo IPC. Uma aprovação de
auditoria dos contratos tampouco habilita automaticamente o executor.

## Threat model

Adversário: texto/modelo/tool arguments, conteúdo do workspace, instruções de
skills/plugins/MCP, subprocessos e descendentes de ferramentas. Tenta modificar
escopo/identidade, ler secrets, escrever fora do workspace, fazer rede, escapar
por shell/daemon, tornar-se HumanLocal ou autoaprovar pelo socket de mesmo UID.
IDs de tarefa/sessão/correlação, origem declarada e recibos de auditoria são dados,
não credenciais. UID/0600 não são prova de intenção humana.

Trusted computing base: binário Core e suas dependências pinadas, composição de
admission do Core, SQLite sob writer lease, kernel/OS e ferramentas auxiliares
confiáveis. Código malicioso carregado dentro do próprio Core, root/kernel
comprometido e mutações concorrentes feitas por um operador hostil no host não
recebem garantia. **Não se assume que um processo agentivo host-assistido esteja
isolado de memória, banco, socket, keyring ou HOME do Core.** Por essa razão, ele
não recebe ferramentas ou grants nesta entrega.

```mermaid
flowchart TD
  H[Operador CLI / SSH / Termux] --> IPC[Unix IPC: consulta / deny / revoke]
  IPC --> C[Core confiável: política e lifecycle]
  IPC -. approve_once: BLOCKED .-> HC[Canal humano confiável ainda indisponível]
  C --> DB[Única SQLite autoritativa schema 022]
  C --> G[Grants opacos somente na memória do Core]
  C --> S[Supervisor LR-10B: leases / ownership / cleanup]
  S --> SDK[SDK 1.0.17 / CLI 1.0.95: lifecycle sem send]
  SDK --> D[availableTools vazio + deny permissions + preToolUse deny]
  D -. execução nativa BLOCKED .-> OS[Filesystem / rede / subprocessos do host]
  T[Testes sintéticos explícitos] --> B[Bubblewrap offline: namespaces + mounts mínimos]
  B --> W[Workspace descartável; sem HOME / credenciais / socket do host]
  G -. nenhum executor operacional conectado .-> OS
```

## Autoridade e aprovação

`narys-domain::agents::authority` contém somente intents serializáveis:
`AgentApprovalPolicy`, `AgentOperationContext`, `AgentOperation` e estados de
aprovação. Podem ser reutilizados por LR-11, sem alterar o Codex Planner.

`AuthorityService` mantém `AgentAuthority` de 256 bits aleatórios gerados pelo
Core (`/dev/urandom`, falha bloqueante). Campos são privados; não implementa
Serialize, Deserialize, Clone ou Debug. Não sai por IPC, prompt, env, filesystem,
SDK ou trace. `ap-HEX` é **outro** número aleatório: identifica um registro, nunca
um grant. Doctests verificam que código externo não constrói/deserializa o grant.

O binding SHA-256 cobre tarefa, sessão, especialista, perfil, workspace,
operação/tool, todos os argumentos/conteúdo, versão da política e identidade
`dev/ino` do workspace. O serviço compara também o contexto tipado completo.
`ExecutionBoundary` e `FinancialAdmission` são provas independentes e privadas,
vinculadas ao mesmo binding e com deadlines próprios. Não há emissor operacional
dessas provas nesta candidata. Os únicos emissores positivos usados nos testes
são fixtures dentro do módulo privado; **não provam admission financeira real**.
`paid_use_allowed` não é aceito nesta etapa, inclusive com consentimento YOLO.

Assistido é o default. Todo intent suportado nesta candidata necessita aprovação;
não há promoção automática de leituras nem autorização herdada de humanos.
Pedido válido tem TTL de no máximo 300 segundos e estado pending. Aprovação exige
`HumanChannel` privado da mesma geração do Core, digest exato e estado ainda
pending. Não há construtor operacional de HumanChannel. A resposta IPC
approve_once é sempre `human_approval_channel_unavailable`, independentemente do
UID, TTY, TaskId ou ID de aprovação. Negar apenas reduz permissões e é permitido
pela IPC. Falta de aprovador nunca aprova.

As transações `BEGIN IMMEDIATE` compare-and-swap ordenam pending→approved e
approved→consumed. A transação de claim é confirmada **antes** do callback de
efeito da fixture. Consumed significa reivindicação, não sucesso. Erro/crash após
claim não reexecuta a ação. Mutex do serviço ordena claim/efeito de fixture contra
cancel/deny. Para comandos longos nativos, interrupção e contenção permanecem
BLOCKED: não se extrapola esse teste para cancelamento de ferramentas reais.

Cancelamento remove grants e deixa tombstone de tarefa, impedindo nova emissão
na geração. Shutdown fecha admission e revoga. Recovery muda pending/approved
para interrupted, em transação e sem replay; grants/HumanChannel/YOLO não são
reconstituídos do banco. Leitura de histórico não produz efeitos. Capacidade
é limitada a 128 grants vivos; tombstones são limitadas a 4096 e saturação fecha
admission. Deadlines monotônicos protegem o grant contra retrocesso do relógio;
expiry UTC durável pode apenas antecipar bloqueio.

São contratos preparados de ferramentas do **Core**: narys.read/write/delete,
narys.command/git/network. Não são nomes presumidos do CLI. Rede está bloqueada.
Validação rejeita paths absolutos/traversal, symlinks, arquivos com hardlinks,
workspace alias/trocado, excesso de argumentos e tools incompatíveis. **Isso não
é executor de filesystem resistente a TOCTOU.** Nenhum executor usa esses paths
em produção; os callbacks positivos são fixtures. Uma futura liberação precisa
operar dentro de contenção efetiva e/ou usar handles de paths, não presumir que
canonicalização isola o OS.

Descrição de approval é gerada pelo Core com ação, tool, workspace sanitizado,
escopo, risco e digest. Não grava conteúdo, argv, URLs, prompts ou secrets. A
apresentação contextual completa de comandos/diffs não está liberada para uma
aprovação positiva operacional; o resumo mínimo não deve ser tratado como
consentimento humano de comando oculto.

## YOLO

Contrato vincula contexto completo, TTL até 300s e reconhecimento explícito do
risco sem isolamento. `activate_yolo` privado requer HumanChannel; política
gerenciada que exija approval impede ativação. Consentimento é volátil, expira,
é revogado por cancelamento/revoke/shutdown e não sobrevive restart/resume.
**Mesmo a fixture positiva não habilita execução.** Não há flags --yolo/--allow-all
no caminho operacional. YOLO não estende tarefa/Autopilot nem autoriza custos.

`agent-yolo-request` é um contrato estrito de solicitação humana, com TaskRef,
session_ref, TTL e acknowledgement obrigatórios. Sempre retorna
`human_yolo_channel_unavailable_execution_disabled`. `agent-yolo-revoke` remove
consentimento volátil; não habilita outro perfil.

## SDK/CLI e inventário efetivo

Versões preservadas: github-copilot-sdk **=1.0.17**, CLI **1.0.95**, RPC3, checksum
CLI `9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`.
Não houve atualização de providers/SDK/CLI. async-trait já estava no lock transitivo
e agora é dependência direta; somente essa ligação foi acrescentada ao Cargo.lock.

`scripts/lr10c-native-inventory.py` verifica checksum e inicia **somente** um CLI
offline em Bubblewrap, sem montar HOME/runtime do host, env vazio, sem credenciais,
sem sessão, sem tools.execute e sem send. RPC allowlist: ping, status.get,
tools.list. Retornou 15 descritores no modelo/default local da consulta:

| Ferramentas observadas | Política vigente |
|---|---|
| bash, read_bash, stop_bash, list_bash | Negar: subprocessos, shell aninhado e detach exigem boundary real |
| glob, grep, view | Negar: leitura de workspace não confere acesso ao host |
| create, edit | Negar: efeitos não são mediados pelo Broker |
| web_fetch | Negar: não há proxy/rede de provider e tools comprovados |
| skill | Negar: mecanismo extensível |
| task, read_agent, list_agents, write_agent | Negar: delegação/continuação não conferem authority |
| MCP, plugins e futuros nomes não observados | Default deny; discovery/loading/augmentation desativados |

Fonte efetiva e schemas: [native-tools.json](evidence/lr10c/native-tools.json).
Não é um catálogo universal para todos os modelos/opções/extensões. Política não
cria allowlist expansiva a partir desse resultado.

Permission handler instalado explicitamente em create **e resume**, sempre
reject, inclusive managed settings/desconhecidos. Não usar `deny_all_permissions`
junto com handler customizado: o SDK resolve a policy substituindo o handler.
`with_hooks` habilita hooks SDK; `enable_file_hooks=false` conserva scripts do
workspace desativados. PreToolUse sempre deny, com classificação tipada estrita
de view/create/edit/bash e bloqueio dos demais recursos. MCP/apps/config
discovery/plugins/skills/custom agents/additional directories seguem desativados,
availableTools continua vazio. Trace só publica códigos constantes sanitizados;
falha de observador não torna uma decisão deny em allow.

**Limitação comprovada do SDK pinado:** `session.rs` no despacho hooks.invoke
retorna `{"output":{}}` quando `dispatch_hook` falha. `preMcpToolCall` somente
transforma metadata e não oferece um veto tipado. A fixture sobre o SDK oficial
verifica um preToolUse deny antes do seu efeito sintético e confirma a resposta
vazia para input inválido. **Não há prova do caminho nativo CLI→hook→efeito real**;
uma ferramenta que ignore callback não está contida por ele. Não modificar o
SDK vendorizado nem atualizar versões para esconder o limite. O gate nativo fica
fechado. Referências contextuais oficiais:
[hooks](https://docs.github.com/en/copilot/how-tos/copilot-sdk/hooks/pre-tool-use),
[SDK Rust](https://github.com/github/copilot-sdk/tree/main/rust).
A evidência normativa desta etapa é o código local exato 1.0.17 e a fixture,
não a documentação online que pode descrever versões posteriores.

## Prova Linux delimitada

Fedora44, kernel7.2.8-200.fc44.x86_64, Bubblewrap0.12.0. User namespaces disponíveis
(`user.max_user_namespaces=31024`), Yama ptrace_scope=0: isolamento de PID/proc e
inacessibilidade de channels privados são indispensáveis; não confiar em Yama.
Um primeiro diagnóstico sem as bibliotecas falhou ao executar /usr/bin/true; a
presença do binário não foi aceita como sucesso. `--disable-userns` exige também
`--unshare-user` explícito nessa versão, mesmo com --unshare-all.

Diagnóstico final: user/PID/IPC/network/UTS namespaces, die-with-parent,
new-session, cap-drop ALL, disable/assert-userns, /usr somente leitura,
/lib e /lib64 para bibliotecas públicas, /proc interno, /dev mínimo, HOME/run/tmp
privados, env vazio, somente workspace descartável gravável, raiz readonly.
Árvore inicial rejeita symlinks, hardlinks, sockets/devices/FIFOs e outro device;
árvore é limitada. Não certifica submounts/binds de mesmo device; somente uma
árvore descartável criada pelo harness é admissível ao diagnóstico. Não monta
keyring, D-Bus, socket, database ou HOME reais.
Descriptors privados Rust são CLOEXEC, e a fixture verifica somente fds padrão.

Os testes constatam falha real de leitura/escrita externa, acesso ao /proc do
Core, socket do host, rede loopback do host, alteração de /usr, symlink criado
no sandbox, shell aninhado e unshare adicional. Um arquivo positivo é escrito
no workspace; marker privado externo é verificado intacto. Cancelamento mata o
processo externo Bubblewrap e a PID namespace elimina descendente que fez
fork/setsid, sem escrita tardia. Não usa sudo/SELinux override/instalações globais.

Não há prova para networking autenticado do Copilot, proxy de credenciais,
artefatos de builds arbitrários, host writers concorrentes, recursos de todos os
plugins, esgotamento adversarial/cgroups, Windows/macOS ou falhas de kernel.
Não fazer bind de HOME/credenciais, compartilhar rede ou flexibilizar namespaces
para promover o diagnóstico a Autônomo isolado. Nenhum certificado durável de
diagnóstico habilita perfil no boot. Referência de responsabilidade dos mounts:
[Bubblewrap upstream](https://github.com/containers/bubblewrap).

## Persistência e rollback

Migration022 aditiva na **única** luna.sqlite3: agent_approvals e
agent_approval_events, índices e constraints de estados. server_events mantém
decisões sanitizadas com retenção4096. Não armazena capacidades/HumanChannel ou
configuração YOLO. Backup online sob writer fence antecede upgrade de autoridade
existente. Importador aceita até schema022 e mantém conflitos/backup de WAL.
Testes cobrem upgrade021→022 idempotente, integrity_check e preservation de history.

Nenhum serviço instalado foi atualizado/reiniciado nesta entrega. Não usar um
binário antigo contra schema022: ele rejeita schema futuro. Rollback operacional
mantém ferramentas bloqueadas, fecha/revoga admission e usa agent stop quando
aplicável. Rollback de software exige snapshot consistente anterior em diretório
isolado e consideração de dados posteriores; não diminuir user_version nem apagar
histórico para fazer versão antiga aceitar a base. Sem replay automático.

LR-10D continua responsável por admission/quota/TaskGraph/handoff; LR-10E pela
UX completa e gate real explicitamente autorizado; LR-10F pela auditoria final.
O bloqueio de execução não é transferido como uma permissão futura implícita.
