# LR-7D0.5 — Codex Agent Bridge

Estado: **D0.5A e D0.5B — PASS completo e integradas à `main` em 29/09/2026. D0.5C — PASS técnico após auditoria independente; pronta para integração.** Esta mini-trilha prepara a descoberta
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

## Explicitamente não implementado até D0.5C

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

A PR #5 foi integrada à `main` por squash no commit `c2262c4ba71aaf8cc5a4de3fcf4ef590a22c97ab`. A D0.5B está oficialmente encerrada em PASS completo. D0.5C é uma candidata à auditoria técnica, mantendo Codex separado dos `CognitiveProvider`s; D0.5D será a primeira integração real.
