# Groq Provider — LR-7B

Estado: implementação candidata ao gate técnico/humano. A LR-7B adiciona Groq como segundo provider real sem ativar ainda distribuição automática Gemini ↔ Groq.

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

## Gate humano

1. Abrir “IA e modelos”.
2. Inserir a chave Groq e salvar.
3. Confirmar “Groq · Configurado”.
4. Clicar “Testar Groq”.
5. Confirmar stream textual, provider `groq` e usage.
6. Fechar/reabrir o app e confirmar que a credencial continua configurada.
7. Confirmar que a conversa normal continua usando Gemini.

A implementação não faz request Groq no startup.
