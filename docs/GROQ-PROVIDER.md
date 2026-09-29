# Groq Provider — LR-7B

Estado: **PASS técnico + PASS humano em 28/09/2026.** A LR-7B adiciona Groq como segundo provider real sem ativar ainda distribuição automática Gemini ↔ Groq.

## Contrato

- Provider ID: `groq`
- Endpoint: `POST https://api.groq.com/openai/v1/chat/completions`
- Modelo inicial: `openai/gpt-oss-20b`
- Autenticação: `Authorization: Bearer <secret>`
- Streaming: SSE `data:`, encerrado por `[DONE]`
- Reasoning: `low|medium|high`; `include_reasoning=false`
- Usage: solicitado via `stream_options.include_usage=true` e aceito somente quando retornado pelo provider
- Rate limit: 429 mapeia para `RateLimited`; `Retry-After` alimenta o cooldown compartilhado do Scheduler

Nenhuma quota comercial é hardcoded. LR-8 continuará responsável pelo Rate Limit Manager completo.

## Privacidade e contexto outbound

O adapter constrói uma fronteira outbound própria. Ele envia somente o nome canônico da assistente e idioma primário em uma system instruction mínima, o histórico explicitamente fornecido pela tarefa e a mensagem atual. O adapter não serializa automaticamente memórias globais, relationship, traits, source context nem outras sessões.

Campos de reasoning recebidos no stream são ignorados; somente `choices[0].delta.content` vira output do usuário.

## Credencial

`SecretKey::GroqApiKey` usa o mesmo Stronghold da aplicação. A UI recebe apenas `configured=true|false`; a chave salva nunca é devolvida ao React, SQLite, logs ou documentação.

## Isolamento da LR-7B

Gemini permanece prioridade 1 e Groq prioridade 2 no Registry real, mas Conversation e Summary continuam com policy `Fixed("gemini")`. A janela IA oferece um diagnóstico explícito `Fixed("groq")`, que não persiste conversa nem muda policies. LR-7C fará a primeira distribuição/fallback real entre os dois providers.

## Validação final — 28/09/2026

**Gate técnico: PASS.**

- `npm run typecheck`: PASS;
- `npm run build`: PASS, mantendo apenas o warning já conhecido de chunk acima de 500 kB;
- `cargo check --manifest-path src-tauri/Cargo.toml`: PASS;
- `cargo test --manifest-path src-tauri/Cargo.toml`: **89/89 testes PASS**;
- `cargo check --release --manifest-path src-tauri/Cargo.toml`: PASS;
- `git diff --check origin/main...HEAD`: PASS.

Os gates Rust foram executados no Fedora com `cargo 1.98.1` / `rustc 1.98.1` instalados pelo sistema. O MSRV declarado `rust-version = 1.77.2` não foi revalidado com a toolchain exata nesta rodada e permanece uma limitação documental não bloqueante.

**Gate humano: PASS.**

- chave Groq cadastrada pela janela “IA e modelos” e armazenada no SecretStore;
- diagnóstico real `Fixed(groq)` concluiu por streaming com `openai/gpt-oss-20b`;
- usage real observado: **126 input + 22 output = 148 total tokens**;
- após fechar/reabrir o Tauri, Groq permaneceu como “Configurado” sem reinserir a chave;
- Conversation e Summary permaneceram `Fixed(gemini)` com o modelo Gemini configurado;
- uma conversa normal após restart tentou Gemini, recebeu HTTP 429 e entrou em cooldown;
- apesar de Groq estar disponível, o Scheduler respeitou `Fixed(gemini)` e não fez fallback oculto.

A resposta Gemini pós-restart não concluiu por rate limit remoto; isso não bloqueia a LR-7B porque o objetivo desta etapa é provar o segundo provider real isolado e preservar a policy Fixed existente. A distribuição/fallback real Gemini ↔ Groq fica explicitamente para a LR-7C.

A implementação não faz request Groq no startup.

**LR-7B = PASS completo.**
