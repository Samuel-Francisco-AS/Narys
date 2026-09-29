# LR-7D0.5 — Codex Agent Bridge

Estado: **D0.5A — PASS completo e integrada à `main` em 29/09/2026. D0.5B é o próximo checkpoint.** Esta mini-trilha prepara a descoberta
segura do runtime Codex sem transformá-lo em `CognitiveProvider`.

## Decisão arquitetural

Gemini e Groq continuam sendo `CognitiveProvider`s para Conversation e Summary.
Codex é um **AgentBackend futuro** porque sua integração prevista é orientada a
agente/app-server, threads, turns, ferramentas e aprovações, e não a uma chamada
cognitiva intercambiável. Nenhuma abstração `AgentBackend` ou registry de agentes
é criada em D0.5A.

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

## Explicitamente não implementado

Não há login/logout, token ou API key na UI; seleção de modelo; app-server,
JSON-RPC/JSONL, threads, turns, streaming de eventos, planner, sandbox,
aprovações, tool calling, MCP, execução de tarefas, leitura/escrita de
repositório, `AgentBackend`, `AgentRegistry`, novo provider ou alterações em
Gemini/Groq, Conversation e Summary.

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

## Gates

O gate técnico desta candidata é `npm run typecheck`, `npm run build`,
`cargo check`, `cargo test`, `cargo check --release` e `git diff --check`.
O gate humano foi concluído: detecção, versão, autenticação ChatGPT, refresh e persistência após restart foram validados sem exposição de segredo. Os estados negativos permanecem cobertos por testes automatizados; não foi necessário desautenticar ou remover o runtime real do usuário.
