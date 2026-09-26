# LR-5 — runtime cognitivo local

A Luna continua sendo a autoridade sobre tarefa, identidade, memória, orçamento, escolha do provider, cancelamento e resultado. O `MockProvider` é um recurso cognitivo substituível. Nenhuma API externa, segredo, HTTP client ou escrita de memória pelo provider participa desta etapa.

## Fluxo

`start_mock_cognition_task` (somente build debug) registra um `TaskId` no TaskRegistry da LR-2. O Context Builder lê a identidade atual, até três memórias ativas e, se houver, até seis mensagens da conversa mais recente do SQLite LR-4. O contexto fica estruturado em `ContextBundle` e é passado ao Scheduler como `ProviderRequest`. O Scheduler escolhe via Registry; o provider emite chunks pelo contrato Rust, e o Core projeta eventos no mesmo Tauri Channel da LR-2. O resultado contém texto mock, provider final, uso agregado e metadados de contagem, sem identidade ou memórias. Um `TaskRecord` terminal usa `kind=mock_cognition` e summary fixo seguro.

O Context Builder aceita `domain`, `kind`, `min_importance`, `memory_limit` e escolha explícita de conversa recente. O limite interno é cinco memórias e seis mensagens. A consulta LR-4 exige `state='active'` e ordena por importância decrescente, data de evento decrescente e ID. O diagnóstico usa limite três e nenhum domínio; não há inferência semântica. Sem identidade atual, retorna `identity_unavailable`; não reabre o bootstrap.

## Contrato Provider

O trait `Provider` recebe `ProviderRequest` com `attempt` por tarefa/provider, sinal de cancelamento do TaskRegistry e callback de `ProviderChunk`, e retorna `ProviderResponse` ou `ProviderError`. Ele usa `Pin<Box<dyn Future<...> + Send>>` para permitir `Arc<dyn Provider>` em Rust 1.77.2 sem dependência `async-trait`. `ProviderCapabilities` declara texto e streaming; campos futuros de visão, ferramentas e saída estruturada permanecem falsos no mock. `ProviderConfig` contém ID estável, `enabled`, prioridade e capabilities; nenhuma chave. `ProviderUsage` mede chamadas e tokens artificiais de entrada/saída; não estima dinheiro. Erros tipados incluem `RateLimited`, `Timeout`, `QuotaExceeded`, `Fatal`, `Cancelled`, `Unavailable` e `EventSinkClosed`.

O Registry mantém `mock-primary` e `mock-fallback` por cenário. Só retorna providers habilitados e com todas as capabilities exigidas. **Menor número de prioridade vence**; ID desempata. O Scheduler ignora cooldown ativo, impõe `TaskBudget` de chamadas e output tokens antes de cada tentativa, informa o restante ao provider e agrega chamadas, tokens, providers usados, retries e fallbacks. `Timeout`/`Unavailable` recebem no máximo um retry no mesmo provider após 80 ms canceláveis. `RateLimited` grava cooldown runtime pelo retry-after (padrão 3 s) e tenta fallback. Quota/fatal seguem para fallback sem retry. Cancelamento interrompe espera e chunks e impede fallback. Cooldown, Registry e budgets ficam em memória; não houve migration.

## Diagnóstico e limites

Os cenários da UI debug são `normal`, `streaming`, `rate_limit_fallback`, `timeout_retry`, `budget_exhausted` e `cancel`. A implementação configurável também cobre `transient_then_success`, `quota_exceeded` e `fatal` nos testes. Chunks são emitidos no Rust com esperas assíncronas; o React apenas os exibe. `cognition_provider_status` devolve somente ID, enabled, prioridade, capabilities e cooldown. Os comandos LR-5 ficam fora do `invoke_handler` e do AppManifest release. A capability estática ainda declara as permissões, sem handler executável. A ACL existente não foi ampliada para filesystem, shell, Stronghold IPC ou rede.

O contexto completo não é logado, auditado, enviado ao frontend nem à rede. Logs de falha de histórico contêm apenas código e TaskId. Testes usam SQLite temporário e dados sintéticos. O banco local real continua sem criptografia integral como registrado na LR-4. A LR-5 não oferece chat funcional, provider real, busca semântica, rate manager completo ou custo monetário.

## PRE-LR-6 hardening

O callback de eventos do Scheduler é falível. Qualquer falha do Tauri Channel em `TaskStarted`, `ContextBuilt`, seleção, retry, fallback, chunk, resultado ou evento terminal define `channel_closed`. O mesmo `AtomicBool` do TaskRegistry interrompe provider e backoff; `EventSinkClosed` é terminal e não dispara retry/fallback. Cancelamento pedido pelo usuário continua `cancelled`. O registry é limpo e o `TaskRecord` é `failed` com `error_code=channel_closed`, inclusive se resultado ou terminal não forem entregues. A entrega falha é tratada como falha operacional da tarefa; não há panic nem tarefa ativa indefinidamente.

O número da tentativa agora é recebido pelo provider na request da invocação. `Timeout` e `TransientThenSuccess` falham na tentativa 1 de cada tarefa e funcionam na tentativa 2, mesmo com o mesmo `CognitionRuntime`. Cooldown de rate limit permanece no Scheduler compartilhado entre tarefas. Testes exercitam falhas antes do provider, no segundo chunk e no evento de retry, além de duas tarefas consecutivas de timeout/transient e cooldown entre tarefas.

LR-6 permanece uma etapa futura. Não foi implementado provider real, cliente HTTP ou chave de API.
