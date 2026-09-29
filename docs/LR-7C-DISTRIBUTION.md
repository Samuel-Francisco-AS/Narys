# LR-7C — distribuição real Gemini ↔ Groq

Estado: **PASS técnico; gate humano pendente.**

A LR-7C ativa a primeira rota real multi-provider do produto sem antecipar o Rate Limit Manager da LR-8.

## Escopo

Apenas o papel `conversation` ganha roteamento configurável:

- `Fixed`: usa somente o provider primário Gemini;
- `Preferred`: prefere Gemini e permite Groq como fallback elegível antes do primeiro chunk.

O papel `summary` continua `Fixed(gemini)`.

Não entram nesta etapa:
- `Auto` no produto;
- score sofisticado;
- affinity;
- task graph;
- subtarefas;
- paralelismo;
- token buckets/fila/circuit breaker completo.

## Persistência

A migration 006 adiciona à policy cognitiva:
- `routing_mode`;
- `fallback_provider_id`;
- `fallback_model`;
- `fallback_thinking_level`.

A migration preserva o comportamento anterior: `conversation` continua `fixed` após upgrade. Ela apenas semeia o target Groq inicial (`openai/gpt-oss-20b`, thinking low), que só é usado depois de o usuário salvar `Preferred`.

## Runtime

Quando `Preferred` está ativo, cada tarefa de conversa carrega dois targets independentes:

1. Gemini com model/thinking/timeouts Gemini;
2. Groq com model/thinking/timeouts Groq.

O Scheduler não traduz ou reutiliza configuração entre providers.

Fallback continua permitido somente:
- antes do primeiro chunk;
- para `RateLimited`, `Unavailable` e `Timeout`;
- dentro de `max_provider_calls`.

Erros terminais como autenticação, quota terminal e request inválido não são mascarados por fallback.

Retry continua precedendo fallback para timeout/unavailable sem Retry-After. Portanto, se retries consumirem todo o orçamento de chamadas, pode não sobrar chamada para o Groq; a UI avisa sobre isso.

## Cooldown e preflight

O frontend não consulta mais somente `gemini_status` para decidir se pode enviar.

`conversation_routing_status` expõe:
- routing mode;
- provider primário configurado/cooldown;
- fallback configurado/cooldown.

Se Gemini já está em cooldown e Groq está elegível em `Preferred`, o envio segue para o Scheduler. O compositor não bloqueia o botão apenas por existir cooldown; o Core reavalia a rota no momento do envio.

## Observabilidade verdadeira

O evento de fallback agora contém:
- `from_provider_id`;
- `to_provider_id`;
- `reason_code`.

A conversa mostra seleção, fallback e provider final reais. Não há mensagem sintética de fallback sem evento correspondente.

O comando de produto foi generalizado de `start_gemini_task` para `start_conversation_task`, e novos históricos de tarefa usam kind `conversation`.

## Gate técnico — PASS

Validado em 28/09/2026 no Fedora:

- `npm run typecheck`: PASS;
- `npm run build`: PASS, mantendo apenas o warning conhecido de chunk acima de 500 kB;
- `cargo check --manifest-path src-tauri/Cargo.toml`: PASS;
- `cargo test --manifest-path src-tauri/Cargo.toml`: **90/90 PASS**;
- `cargo check --release --manifest-path src-tauri/Cargo.toml`: PASS;
- `git diff --check origin/main...HEAD`: PASS;
- `git status`: worktree clean.

## Gate humano pendente

1. Confirmar Gemini e Groq configurados.
2. Em Conversa, manter inicialmente `Fixed` e confirmar comportamento sem fallback.
3. Alterar Conversa para `Preferred · Gemini → Groq`; manter Summary `Fixed`.
4. Com Gemini saudável, confirmar que uma tarefa simples usa somente Gemini.
5. Obter um 429 real do Gemini antes do primeiro chunk e confirmar:
   - evento `Gemini → Groq · rate_limited`;
   - resposta concluída por Groq;
   - `providersUsed` registra ambos;
   - `fallbacks=1`;
   - mensagem final persiste uma única vez.
6. Se Gemini já estiver em cooldown no início, confirmar que Groq pode ser selecionado diretamente. Esse caso prova overflow por cooldown, mas não substitui o cenário anterior de dois providers na mesma tarefa.
7. Confirmar que falha depois do primeiro chunk não dispara troca de provider.
8. Reiniciar o app e confirmar persistência da policy.
9. Confirmar que Summary continua Fixed(Gemini).

O gate final da LR-7 exige ao menos uma tarefa real que use os dois providers sem perder sessão/identidade.
