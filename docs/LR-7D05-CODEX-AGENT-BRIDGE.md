# LR-7D0.5 — Codex Agent Bridge

Estado: **D0.5A, D0.5B e D0.5C — PASS completo e integradas à `main` em 29/09/2026. D0.5D — CodexAgentBackend real + Planner read-only + PlanV1 — é o próximo checkpoint.** Esta mini-trilha prepara a descoberta
segura do runtime Codex sem transformá-lo em `CognitiveProvider`.

## Decisão arquitetural

Gemini e Groq continuam sendo `CognitiveProvider`s para Conversation e Summary.
Codex é um **AgentBackend futuro** porque sua integração prevista é orientada a
agente/app-server, threads, turns, ferramentas e aprovações, e não a uma chamada
cognitiva intercambiável. D0.5C cria somente a abstração genérica; nenhum
backend agentivo real é registrado ou executado nesta etapa.

## Checkpoints

- **D0.5A:** detectar executável, versão e estado seguro de autenticação.
- **D0.5B:** comunicação Rust ↔ codex app-server.
- **D0.5C:** contrato `AgentBackend` e registry genérico.
- **D0.5D:** planner read-only e `PlanV1`.
- **D0.5E:** cancelamento, recovery e eventos reais.
- **D0.5F:** gate real.

## Escopo e fronteira de segurança da D0.5A

O Luna Core executa diretamente `codex --version` e `codex login status`, sem
shell intermediário, com timeout de 5 segundos por processo. Cada stream é
drenado sem deadlock, mantendo no máximo 16 KiB por stream em memória; bytes
excedentes são descartados. A saída mantida é usada somente para extrair uma versão e classificar marcadores
públicos de autenticação (`ChatGPT`, API key, outro, desconhecido ou nenhum).
O frontend recebe apenas `CodexRuntimeStatus`, com `installed`, `version`,
`authenticated`, `authKind`, `available` e um código diagnóstico sanitizado.

`available` significa apenas “o executável foi encontrado e o próprio CLI
reportou autenticação”; não é prova de que uma inferência funcionará. D0.5A não
lê `~/.codex/auth.json`, tokens, IDs de conta ou variáveis de API key, não grava
credenciais e não persiste output bruto. O comando Tauri é somente leitura e a
interface oferece apenas consulta inicial e atualização manual.

## Implementado

- módulo Rust dedicado `agents::codex`, fora de providers, scheduler e registry;
- parsing/classificação fechados com falha segura para saída vazia ou inesperada;
- timeout e encerramento controlado em comandos externos;
- comando Tauri `get_codex_runtime_status` restrito à capability `settings-ai`;
- seção experimental em **IA e modelos**;
- testes unitários sem conta, internet, quota ou chamada de modelo.

## D0.5B — PASS completo

O comando read-only `probe_codex_app_server`, disponível apenas à capability
`settings-ai`, inicia diretamente `codex app-server --stdio` sem shell. Ele abre
stdin, stdout e stderr como pipes, envia uma linha JSON `initialize` com ID
único e `clientInfo` (`assistente-3d`, `Assistente-3D`, versão do aplicativo),
correlaciona a resposta pelo ID e valida o envelope de sucesso ou erro. Pode
ignorar até 32 notificações bem formadas intercaladas. Somente após uma
resposta válida envia `{"method":"initialized"}`. O formato foi conferido no
help e no JSON Schema gerado pelo CLI local `0.158.0`; o código não fixa essa
versão nem opta por `experimentalApi`.

O probe é efêmero: fecha stdin após o handshake, aguarda saída por até 2 s e,
se necessário, mata e recolhe o processo com `wait`. Ambos os caminhos são
cleanup bem-sucedido quando o processo é recolhido e os readers são joined.
O prazo do handshake é
8 s. Leitura incremental de stdout limita cada linha a 64 KiB e todo o probe
a 256 KiB, com canal de 33 mensagens (32 notificações permitidas mais a
resposta `initialize`); stderr é drenado em paralelo com retenção
máxima de 4 KiB. Stderr nunca é interpretado como protocolo, logado ou enviado
à UI. Erros de parse, EOF, timeout, ID divergente e rejeição encerram o
processo antes de retornar.

A resposta `initialize` é validada, mas `codexHome` e `userAgent` são
descartados. Somente plataformas reconhecidas por lista fechada chegam ao
frontend, junto com os booleanos `launched`/`initialized` e um código de
diagnóstico fechado. Não há raw stdout/stderr, caminho local, token, payload
de autenticação ou ID de conta no resultado. A UI oferece apenas um botão de
teste manual, sem polling nem inicialização automática no startup.

Não ocorre `thread/start`, `turn/start`, prompt, inferência, ferramenta,
alteração de login ou consumo deliberado de quota. D0.5C+ ficam responsáveis
por `AgentBackend`, registry, planner, cancelamento/recovery, eventos e gate
real; esta etapa não mantém processo residente.

Os testes automatizados usam mensagens e streams simulados, sem exigir Codex,
conta, rede ou quota. Um teste marcado `ignored` permite validar manualmente
o handshake local sem inferência.

## D0.5C — PASS técnico

D0.5C cria a fundação genérica para runtimes agentivos sem antecipar uma
operação concreta. `CognitiveProvider` continua sendo a família de inferência
de texto/stream, com Scheduler, retry, fallback e cooldown próprios. `AgentBackend`
é uma família separada para runtimes com futuro lifecycle de threads/turns,
ferramentas, filesystem, comandos, approvals, eventos e cancelamento. Portanto,
`AgentBackend` não implementa `Provider`, `AgentConfig` não reutiliza
`ProviderConfig` e `AgentRegistry` não reutiliza `ProviderRegistry`.

O `AgentRegistry` mantém `AgentEntry` com `AgentConfig` e
`Arc<dyn AgentBackend>`. O registro rejeita IDs duplicados; `eligible` exclui
entradas desabilitadas e exige todas as capabilities solicitadas. A seleção é
determinística por menor `priority` e, em empate, por ID. As capabilities
iniciais são `planning`, `repository_read`, `file_write`, `command_execution`,
`tool_use` e `structured_output`.

O contrato usa um future boxed para permanecer object-safe, `Send + Sync`, sem
`async-trait`. `AgentRequest` contém somente um objetivo e capabilities
necessárias; `AgentResult`, `AgentEvent` e um enum fechado `AgentError` são
deliberadamente pequenos. A flag `AtomicBool` e o event sink já aparecem na
assinatura como preparação estrutural, mas esta etapa não implementa
cancelamento remoto, lifecycle ou observabilidade real.

`MockAgentBackend` existe apenas em `#[cfg(test)]` para validar registro,
seleção, execução por trait object, cancelamento pré-marcado e propagação de
falha do sink. Não há mock em produção, exposição na UI ou gate humano nesta
etapa. Codex ainda não implementa `AgentBackend`; D0.5D será a primeira
integração concreta, com Planner read-only e `PlanV1`.

## Estado até D0.5C (histórico)

A ponte diagnóstica de app-server/stdio já existe desde D0.5B, mas ainda não há
backend agentivo real sobre ela. Não há login/logout, token ou API key na UI;
seleção de modelo; threads/turns; streaming agentivo de eventos; planner;
sandbox; aprovações; tool calling; MCP; execução de tarefas; leitura/escrita
real de repositório; `CodexAgentBackend`; `PlanV1`; novo provider; nem
alterações em Gemini/Groq, Conversation ou Summary.

## Auditoria independente da Luna — 29/09/2026

**PASS da auditoria independente e do gate humano.**

A revisão remota confirmou que:

- Codex permanece fora de `CognitiveProvider`, `ProviderRegistry` e `Scheduler`;
- o comando usa execução direta de `codex --version` e `codex login status`, sem shell intermediário;
- nenhum arquivo/token de autenticação é lido ou exposto ao frontend;
- autenticação positiva só é aceita quando `codex login status` termina com sucesso; falhas terminam fechadas;
- stdout/stderr são drenados em paralelo, com retenção máxima de 16 KiB por stream e descarte do excedente;
- timeout de 5 s encerra e recolhe o processo;
- o comando Tauri é somente leitura e restrito à capability `settings-ai`;
- a FIX posterior à primeira auditoria alterou somente a captura/classificação de status e esta documentação;
- nenhuma etapa D0.5B+ foi antecipada.

Os gates locais foram executados pelo Copilot e reportados como PASS, com 104 testes Rust. A auditoria da Luna foi revisão independente do código remoto, não uma segunda execução local desses comandos.

O gate humano foi concluído em 29/09/2026 com o runtime Codex real: a UI detectou `codex-cli 0.158.0`, autenticação `ChatGPT` e estado disponível; refresh manual e restart preservaram o diagnóstico correto. A referência externa `codex --version` / `codex login status` coincidiu com o estado mostrado pelo aplicativo.

## Auditoria independente da Luna — D0.5C — 29/09/2026

**PASS técnico. Não há gate humano nesta etapa.**

A revisão independente do código remoto confirmou que:

- `AgentBackend` é um contrato próprio, object-safe, `Send + Sync`, com future boxed e sem `async-trait`;
- `AgentBackendId`, `AgentCapabilities`, `AgentConfig`, `AgentRequest`, `AgentResult`, `AgentEvent` e `AgentError` são tipos próprios do domínio agentivo;
- `AgentRegistry` é independente de `ProviderRegistry` e não importa a família `cognition`;
- IDs duplicados são rejeitados; entradas desabilitadas e capabilities incompatíveis ficam fora de `eligible`;
- múltiplas capabilities requeridas precisam ser atendidas em conjunto;
- a ordenação é determinística por menor prioridade e depois ID;
- cancelamento por `AtomicBool` e event sink aparecem apenas como preparação estrutural do contrato;
- `MockAgentBackend` existe somente sob `#[cfg(test)]`;
- nenhum backend Codex real, composition state, UI, Tauri Channel, TaskRegistry ou integração com Scheduler foi criado;
- a ponte Codex D0.5B permaneceu intacta;
- D0.5D continua sendo a primeira integração concreta, com Planner read-only e `PlanV1`.

Os gates foram executados pelo Copilot e reportados como PASS: typecheck, build,
`cargo check`, **122 testes Rust PASS + 1 manual ignored**, release e
`git diff --check`. A auditoria da Luna foi revisão independente do código
remoto, não uma segunda execução local desses comandos.

Como D0.5C adiciona apenas infraestrutura interna sem comportamento de produto
observável, não há gate humano artificial. O checkpoint está pronto para
integração após esta auditoria.

## Auditoria independente da Luna — D0.5B — 29/09/2026

**PASS da auditoria independente e do gate humano. D0.5B = PASS completo.**

A revisão remota confirmou que:

- a ponte inicia diretamente `codex app-server --stdio`, sem shell intermediário;
- o handshake segue `initialize` → resposta correlacionada pelo mesmo ID → `initialized`;
- notificações válidas podem ser intercaladas antes da resposta, com limite explícito de 32;
- o canal limitado reserva `MAX_NOTIFICATIONS + 1` posições, permitindo 32 notificações mais a resposta de initialize mesmo em burst anterior ao consumo;
- 32 notificações + resposta passam e 33 notificações resultam em erro de protocolo;
- stdout é lido incrementalmente, com 64 KiB por mensagem e 256 KiB por probe; stderr é drenado em paralelo com retenção máxima de 4 KiB;
- `codexHome`, `userAgent`, stdout e stderr brutos não atravessam a fronteira pública;
- o processo é efêmero; EOF é tentado primeiro e `kill + wait` é fallback válido de cleanup, sem ser tratado por si só como falha;
- threads leitoras são joined antes do retorno;
- nenhuma inferência, thread, turn, ferramenta, AgentBackend, registry ou Planner foi antecipado.

Os gates locais foram executados pelo Codex e reportados como PASS, com 113 testes Rust no total (112 PASS + 1 teste manual ignorado). O teste manual real de handshake também foi reportado como PASS, sem processo remanescente. A auditoria da Luna foi revisão independente do código remoto, não uma segunda execução local desses comandos.

O gate humano foi concluído em 29/09/2026: três probes consecutivos pela UI retornaram `conectado` / `Linux`, e após cada execução `pgrep -af 'codex app-server --stdio'` não encontrou processo remanescente. Os processos persistentes `--managed-daemon` / `pid-update-loop` já existiam antes do gate e pertencem ao runtime gerenciado do próprio Codex; a ponte D0.5B cria e encerra apenas o subprocesso efêmero `--stdio`.

## Gates

O gate técnico desta candidata é `npm run typecheck`, `npm run build`,
`cargo check`, `cargo test`, `cargo check --release` e `git diff --check`.
O gate humano foi concluído: detecção, versão, autenticação ChatGPT, refresh e persistência após restart foram validados sem exposição de segredo. Os estados negativos permanecem cobertos por testes automatizados; não foi necessário desautenticar ou remover o runtime real do usuário.

## Fechamento de integração da D0.5B — 29/09/2026

A PR #5 foi integrada à `main` por squash no commit `c2262c4ba71aaf8cc5a4de3fcf4ef590a22c97ab`. A D0.5B está oficialmente encerrada em PASS completo. D0.5C foi posteriormente auditada e integrada; D0.5D será a primeira integração agentiva real.

## Fechamento de integração da D0.5C — 29/09/2026

A PR #6 foi integrada à `main` por squash no commit `663610cacff526558b353e22c6d6292ea7a5c0f4`. A D0.5C está oficialmente encerrada em PASS técnico, sem gate humano artificial. O próximo checkpoint é **D0.5D — primeiro `CodexAgentBackend` real + Planner read-only + `PlanV1`**, mantendo o Luna Core como autoridade sobre validação e execução.

## D0.5D — CANDIDATA AO GATE HUMANO

`CodexAgentBackend` é o primeiro backend agentivo de produção, registrado como `codex` em `AgentRegistry`. Suas únicas capabilities são `planning` e `structured_output`; leitura de repositório, escrita, execução de comandos e uso de ferramentas permanecem false. O módulo `planner` escolhe explicitamente esse ID, consome o contrato `AgentBackend` e valida o `PlanV1` antes de devolver uma estrutura sanitizada ao diagnóstico em **IA e modelos**. Não há integração com `CognitiveProvider`, `ProviderRegistry`, Scheduler ou TaskRegistry.

Cada chamada cria um diretório temporário fora do checkout e inicia um processo `codex app-server --stdio` nesse cwd. O transporte reutiliza framing, limites, drenagem de stderr e cleanup da D0.5B. O Planner declara `capabilities.experimentalApi=true` no initialize para os campos de isolamento presentes no schema local `--experimental`; o probe D0.5B mantém seu initialize original. O fluxo é `initialize` → `initialized` → `config/read` → `thread/start` → `turn/start` com `outputSchema` → notificações `item/completed` e `turn/completed` → `thread/unsubscribe` → EOF → wait ou kill+wait. IDs de request são únicos e correlacionados; request do servidor com ID, inclusive approval/tool, falha fechada. O processo nunca é daemon persistente. O frontend não recebe protocolo, caminhos, IDs, configuração, stdout ou stderr.

O `thread/start` usa `ephemeral=true`, `approvalPolicy=never`, `sandbox=read-only`, `environments=[]`, `dynamicTools=[]`, `runtimeWorkspaceRoots=[]` e `selectedCapabilityRoots=[]`. O `config/read` com o cwd isolado precede a criação da thread; os nomes de `mcp_servers` efetivos são sobrepostos com `enabled=false`. A configuração da thread também usa `default_permissions=:read-only`, `web_search=disabled`, `features.shell_tool=false`, `features.unified_exec=false` e desabilita code mode, apps, plugins, skills, multi-agent, ferramentas de permissões, request-user-input e update-plan, entre outras superfícies do padrão `temporary_structured_request` da tag `rust-v0.158.0`. Antes do turn, o Core rejeita sandbox efetivo que não seja `readOnly`, approval policy incompatível ou cwd divergente. O objetivo é enviado apenas como dado textual junto a instruções estáticas; não inclui repo, home, identidade, memória, histórico ou secrets.

**FIX de auditoria:** `environments=[]` não impede, por si só, que o Codex carregue instruções globais do `CODEX_HOME`. Antes de `turn/start`, a resposta de `thread/start` deve conter `instructionSources=[]`; campo ausente ou qualquer fonte herdada causa erro de protocolo sanitizado, sem leitura ou exposição do path. Também são obrigatórios na resposta `approvalPolicy=never`, `runtimeWorkspaceRoots=[]`, cwd temporário exato, sandbox `readOnly` sem `networkAccess=true` e `thread.id` não vazio. O diretório final é verificado contra a raiz canônica do checkout inteiro, derivada do pai de `CARGO_MANIFEST_DIR`. Um `TMPDIR` dentro do checkout é rejeitado. Não há alteração de `CODEX_HOME`.

**FIX de provenance do permission profile:** `activePermissionProfile` é metadata opcional neste fluxo; campo ausente ou `null` representam ausência de provenance. Quando não nulo, deve ser um objeto e seu `id`, se presente, deve ser string não vazia. O valor nominal não é exigido nem exposto/logado. O gate de segurança depende do estado efetivo da thread, incluindo sandbox `readOnly`, network ausente ou false, approval never e os demais requisitos de isolamento. Isso segue `temporary_structured_request` da tag `rust-v0.158.0` para o fluxo sem preservação de custom profile. A Luna não seleciona nem preserva custom profiles nesta etapa; `planner_permission_profile_rejected` indica somente metadata malformada. D0.5D permanece candidata ao gate humano.

Referência de protocolo e isolamento: [temporary_structured_request.rs na tag rust-v0.158.0](https://github.com/openai/codex/blob/rust-v0.158.0/codex-rs/tui/src/temporary_structured_request.rs). Os campos foram conferidos também no schema gerado pelo CLI local com `codex app-server generate-json-schema --experimental`; a versão do CLI não é codificada no backend.

`PlanV1` contém `version=1`, `objective`, `steps[{id,description,requiredCapabilities,dependsOn}]`, `risks`, `needsUserInput` e `questions`. O enum fechado de capabilities aceita `planning`, `repository_read`, `file_write`, `command_execution`, `tool_use` e `structured_output` apenas como requisitos propostos de passos futuros. A validação Rust rejeita campos desconhecidos, JSON inválido ou acima de 16 KiB, objetivo vazio ou acima de 2048 bytes, zero ou mais de 16 passos, IDs duplicados, dependências desconhecidas, próprias ou cíclicas, descrições ou IDs fora dos limites, mais de oito riscos/perguntas, texto longo e inconsistência entre `needsUserInput` e `questions`. Não há reparo automático de JSON e nenhum passo é executado.

O handshake e cada request preparatória têm limite de 8 s; o turno tem 60 s; shutdown espera 2 s antes de kill+wait. Mensagens do protocolo são limitadas a 64 KiB e tráfego total do Planner a 512 KiB. O produtor também limita frames a 8192 por sessão, derivados do budget existente dividido por 64 bytes de allowance de metadata por frame; até um marcador terminal adicional pode ser enfileirado. A fila pendente de RPC compartilha esse teto. Não há limite de 256 mensagens recebidas nem de 128 notificações intercaladas. Somente `agentMessage` da thread/turn correta é candidato a resposta; o último é aceito apenas após `turn/completed` com status `completed`. Itens de comando, alteração de arquivo, MCP, ferramenta ou qualquer tipo não permitido falham fechados. Não há chamada real ao modelo no gate automático. O teste real na UI está reservado para depois da auditoria independente.

Cancelamento remoto, `turn/interrupt`, observabilidade de progresso, eventos reais, persistência e recuperação ficam para D0.5E+. O Planner atual apenas rejeita cancelamento já marcado antes do início.

## Auditoria independente da Luna — D0.5D permission-profile FIX — 29/09/2026

**PASS técnico.**

A revisão remota confirmou que `activePermissionProfile` deixou de ser tratado como autoridade de segurança e passou a ser apenas provenance opcional. O gate efetivo continua baseado em sandbox `readOnly`, ausência de network access, `approvalPolicy=never`, cwd isolado, `runtimeWorkspaceRoots=[]`, `instructionSources=[]` e thread id válido.

Perfis ausentes, `:read-only` ou IDs customizados bem formados são aceitos somente quando todos os invariantes efetivos já passaram. Metadata malformada ou ID inválido continua falhando com `planner_permission_profile_rejected`, sem exposição do valor. Testes adicionais confirmam que profile arbitrário não autoriza workspace write, network access ou approval incompatível.

Os gates foram executados pelo Codex e reportados como PASS, com 151 testes Rust no total (150 PASS + 1 manual ignored). Nenhuma chamada ao modelo foi feita.

O próximo passo é repetir apenas o preflight humano **Testar isolamento**.

## Auditoria independente da Luna — D0.5D FIX diagnóstica — 29/09/2026

**PASS técnico para preflight humano sem inferência.**

A revisão remota confirmou que:

- `PreparedPlannerSession::prepare` é o caminho único de preparação usado tanto pelo preflight quanto pelo Planner real;
- a preparação executa diretório temporário, spawn, initialize, `config/read`, descoberta MCP, `thread/start` e validação efetiva;
- o preflight encerra a thread/processo após essa preparação e **não constrói nem envia `turn/start`**;
- somente o Planner real chama `run_turn` depois de uma preparação bem-sucedida;
- os diagnósticos públicos são um enum fechado por estágio e a resposta pública contém apenas `ready` e `diagnosticCode`;
- paths, payloads, stdout/stderr, Codex home, MCP names e IDs não aparecem na resposta pública;
- a UI mantém whitelist tanto dos códigos de preflight quanto dos `AgentError` do Planner real;
- cleanup continua obrigatório em sucesso e falha.

Os gates foram executados pelo Codex e reportados como PASS, com 148 testes Rust no total (147 PASS + 1 manual ignored). A auditoria da Luna foi revisão independente do código remoto, não uma segunda execução local desses comandos.

O próximo gate humano deve executar apenas **Testar isolamento**. Somente se o preflight retornar `ready=true` deve ser repetida a chamada real de Planner.

**FIX diagnóstica do gate humano:** a preparação foi centralizada em `PreparedPlannerSession::prepare`, que executa diretório temporário, spawn, initialize, `config/read`, descoberta MCP, `thread/start` e `effective_thread`. O preflight em **IA e modelos** usa essa mesma preparação e encerra a thread com `thread/unsubscribe` e shutdown sem construir ou enviar `turn/start`; só o Planner real inicia o turno após a preparação. A resposta pública do preflight contém exclusivamente `ready` e `diagnosticCode`, de enum fechado: `planner_spawn_failed`, `planner_initialize_failed`, `planner_config_read_failed`, `planner_mcp_config_invalid`, `planner_thread_start_failed`, `planner_sandbox_rejected`, `planner_approval_policy_rejected`, `planner_cwd_rejected`, `planner_workspace_roots_rejected`, `planner_instruction_sources_rejected`, `planner_permission_profile_rejected`, `planner_thread_id_invalid` ou `planner_cleanup_failed`. Nenhum path, ID, payload ou mensagem bruta atravessa essa resposta. O Planner real continua exibindo somente os códigos fechados de `AgentError`.

**FIX do caso nulo no gate humano:** o preflight humano revelou `activePermissionProfile: null`, que agora é aceito como ausência de provenance. Metadata malformada continua falhando fechada. Sandbox `readOnly`, rede desabilitada, approval `never`, cwd isolado, workspace roots vazias, instruction sources vazias e thread ID válido continuam sendo a autoridade de segurança, na mesma ordem de validação. O gate humano deve ser repetido somente em **IA e modelos → Planner experimental → Testar isolamento**; não executar **Testar Planner** antes de `Isolamento: pronto`. D0.5D permanece candidata ao gate humano.

### FIX diagnóstica do primeiro turno real

O gate humano de **Testar isolamento** passou (`Isolamento: pronto`). O primeiro **Testar Planner** real chegou ao caminho de inferência e retornou `protocol_error`. Segundo a verificação humana imediatamente após ambos os gates, `git status --short` e `pgrep -af 'codex app-server --stdio'` ficaram vazios: checkout limpo e nenhum app-server residual. O erro genérico não localizava o estágio da rejeição.

Esta FIX adiciona `PlannerTurnDiagnosticCode`, enum fechado, mantido na camada Planner/Codex e adaptado por `PlannerProbeError::code()` no comando Tauri até a whitelist do frontend. `AgentError` permanece backend-agnóstico. Os códigos são `planner_turn_start_failed`, `planner_turn_id_invalid`, `planner_turn_transport_failed`, `planner_turn_timeout`, `planner_turn_unexpected_notification`, `planner_turn_unexpected_item`, `planner_turn_failed`, `planner_response_missing`, `planner_plan_invalid` e `planner_cleanup_failed`. Falhas de preparação do Planner real preservam os códigos fechados já usados pelo preflight. O preflight mantém seu comportamento e usa a mesma preparação; não houve alteração de prompt, outputSchema, modelo ou controles de isolamento.

Um JSON-RPC error em `turn/start` resulta somente em `planner_turn_start_failed`, sem classificar ou expor message/data. Timeout durante notificações é distinguido pelo enum existente do transporte, sem ler stderr. Requests inesperadas do servidor continuam rejeitadas pelo transporte (`planner_turn_transport_failed` durante a coleta); notificações proibidas e itens proibidos continuam falhando fechados. Nenhum novo tipo de item é aceito. PlanV1 continua com o mesmo parse/validate, sem reparo; ausência de AgentMessage final é distinguida de plano inválido.

Cleanup sempre executa unsubscribe e shutdown, incluindo falhas de turno. A falha primária do turno prevalece se cleanup também falhar; sucesso do turno com cleanup falho retorna `planner_cleanup_failed`. Na preparação do Planner real, a falha primária de preparação/segurança prevalece sobre cleanup. A precedência anterior do preflight permanece intacta. Os diagnósticos não contêm payload, mensagens brutas, paths, IDs, configuração, dados de conta ou autenticação, e não são persistidos.

Não houve inferência real durante esta FIX nem tentativa de corrigir a causa do runtime. Os testes usam fakes e cobrem estágios de falha, parse inválido/válido, lifecycle, cleanup e precedência. D0.5D continua candidata ao gate humano. Após auditoria desta FIX, o próximo gate humano deve repetir exatamente o objetivo **“Planeje como investigar e corrigir um botão de uma aplicação Tauri que não responde ao clique. Não execute nenhuma alteração.”** em **Testar Planner**, observando o novo código sanitizado.

**FIX arquitetural dos diagnósticos:** removida a dependência de `agents::types` para `agents::codex::backend`. `AgentError`, `AgentBackend` e o registry de produção mantêm o contrato genérico. O probe cria um adaptador `AgentBackend` e um slot tipado de códigos fechados por chamada; o Planner consome esse adaptador via um registry restrito à operação, preservando a configuração/capabilities do registro `codex`. Adaptador e backend de produção usam a mesma função `execute_planner`, com a mesma preparação, turno e cleanup. O backend retorna somente `AgentError` genérico; a fronteira da operação recupera o código específico em `PlannerProbeError` e o comando retorna apenas seu código fechado. O slot não pertence ao estado gerenciado nem ao banco e é descartado ao final da chamada. Não há string arbitrária de erro, alteração da whitelist da UI, do preflight ou da precedência de cleanup. Os nove testes diagnósticos foram preservados; cobertura adicional compila os tipos genéricos isoladamente e verifica códigos de preparação/turno, erros genéricos, sucesso e gates do registry através da adaptação usada pelo comando. Nenhuma inferência real e nenhuma tentativa de corrigir a causa do runtime nesta FIX. O mesmo objetivo do gate anterior só deve ser repetido após nova auditoria.


### FIX do transporte de streaming

O preflight humano passou; `turn/start` real e seu `turn.id` também passaram. A falha humana observada depois disso foi `planner_turn_transport_failed`, com checkout limpo e nenhum processo residual após cleanup, conforme verificação humana.

A investigação demonstrou dois defeitos do cliente no HEAD `fc09a6b`: a sessão rejeitava a 257ª leitura, mesmo com tráfego legítimo abaixo do budget; e `try_send` no canal Planner de 257 posições encerrava silenciosamente o reader numa rajada. Antes da correção, os dois testes de regressão falharam: 1024 frames produzidos resultaram em somente 257 frames retidos, e 1024 notificações causaram `CodexAppServerProtocolError`. Esses mecanismos são causas prováveis do gate anterior; o payload daquela execução não foi capturado, portanto não se atribui retrospectivamente a ela um encerramento exato.

A correção remove a contagem artificial de leituras. O canal Planner usa `mpsc::channel`, cuja operação de enqueue não aguarda o consumidor, com limites obrigatórios no produtor: 512 KiB de wire bytes totais, 64 KiB por mensagem e 8192 frames de dados, mais no máximo um marcador terminal. O teto de frames deriva do mesmo budget de 512 KiB dividido por uma allowance mínima de 64 bytes de metadata por frame; é uma barreira independente contra uma quantidade excessiva de frames minúsculos. Isso limita tanto os buffers quanto a quantidade de alocações, sem afirmar que 512 KiB é o uso exato de RAM do parser/canal. O reader nunca espera espaço livre nem perde uma rajada por fila cheia; pode ser recolhido por EOF ou kill+wait mesmo sem drenar a fila. Receiver desconectado encerra o produtor deliberadamente. O probe D0.5B preserva canal de 33 posições, limite de 32 notificações e budget de 256 KiB. A fila RPC passa a `VecDeque`, preservando a ordem, sem remoção quadrática nem o antigo teto de 128 notificações; o teto é o mesmo budget de frames do transporte.

Requests válidas do servidor (`method` + ID) agora recebem classificação interna `CodexAppServerUnexpectedServerRequest`, distinguida de JSON/RPC corrompido. Durante a coleta, o Planner retorna somente `planner_turn_unexpected_notification`, sem responder à request ou expor seus campos. JSON inválido, EOF, read error, limites excedidos e responses RPC inesperadas continuam falhando fechados. `inspect_notification`, seus tipos de item permitidos, PlanV1, outputSchema, prompt, isolamento, registry e tipos genéricos de agentes não foram relaxados nem alterados.

A compatibilidade foi conferida na [fonte de métodos da tag rust-v0.158.0](https://github.com/openai/codex/blob/rust-v0.158.0/codex-rs/app-server-protocol/src/protocol/common.rs) e nos schemas oficiais dessa tag: `TurnStartResponse`, `TurnStartedNotification`, `ItemStartedNotification`, `ItemCompletedNotification`, `AgentMessageDeltaNotification`, `ReasoningTextDeltaNotification`, `ReasoningSummaryTextDeltaNotification`, `TurnCompletedNotification`, `ErrorNotification` e `ServerRequest`. Os envelopes dos eventos conferidos também coincidem com os schemas gerados pelo CLI local, que nesta rodada já estava em **0.159.0**. Não houve atualização/substituição do CLI ou seleção de modelo/provider pela FIX.

Integração manual separada, expressamente autorizada nesta rodada, executou o mesmo objetivo e o mesmo isolamento no runtime local 0.159.0: **PASS**, 721 frames e 217592 wire bytes agregados; PlanV1 válido e cleanup concluído. O git status permaneceu igual antes/depois da chamada e nenhum `codex app-server --stdio` residual foi encontrado. Nenhum payload real, resposta do modelo, ID, path privado, autenticação ou mensagem bruta foi registrado. A instrumentação temporária e o teste real temporário foram removidos antes dos gates finais; testes automáticos continuam sem inferência/quota.

Nove testes novos cobrem regressões de contagem/rajada, 2048 notificações antes de uma resposta RPC, budget de frames e reader sem consumidor drenando, budgets individual/total, EOF/read error/JSON inválido, request do servidor versus response inesperada, 3000 deltas benignos seguidos de PlanV1 válido e cleanup, e falhas no wire com cleanup sem responder à request. Os testes novos usam buffers em memória, sem alterar arquivos ou repositório. D0.5D continua candidata ao gate humano; o próximo teste humano só ocorre depois de auditoria independente deste commit.
