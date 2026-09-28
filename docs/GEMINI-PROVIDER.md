# Gemini Provider · LR-6

**Estado:** implementação local e auditoria de protocolo concluídas; validação com API real pendente de chave inserida manualmente no painel Tauri. LR-6 ainda não é PASS completo. Nenhuma chave é solicitada no chat, terminal ou arquivo.

## Contrato e estado

O adapter Rust usa `POST https://generativelanguage.googleapis.com/v1beta/interactions`, modelo fixado por padrão em `gemini-3.8-flash` e autenticação exclusivamente no header `x-goog-api-key`. O `GeminiConfig` guarda modelo, endpoint e timeouts; nunca a chave. Toda request inclui atualmente `store:false`, `stream:true`, `max_output_tokens:4096`, `thinking_level:low` e `thinking_summaries:none`. **4096 e low são defaults temporários de protótipo, não a política final de produto.** O teto anterior de 512 produziu falhas `provider_incomplete` em conversa real. A UIP-6 tornará output e thinking configuráveis por papel/modelo: o usuário poderá escolher um máximo próprio ou “sem teto adicional da Luna”, limitado apenas pela capacidade real do provider/modelo. Não há `previous_interaction_id`, tools, grounding ou execução em background. O estado da Luna permanece no SQLite local. A [visão oficial da Interactions API](https://ai.google.dev/gemini-api/docs/interactions-overview), a [referência](https://ai.google.dev/api/interactions-api-v1), o [guia de streaming](https://ai.google.dev/gemini-api/docs/streaming) e a [página do modelo](https://ai.google.dev/gemini-api/docs/models/gemini-3.8-flash/) embasam este contrato.

## Privacidade de saída

O Context Builder continua criando um `ContextBundle` local; nesta tarefa ele já pede zero memórias e nenhum histórico. `MinimalOutboundContext` é uma fronteira adicional com allowlist: somente `canonical_name` e `primary_language` da identidade local entram na system instruction. Nome e idioma são limitados e validados; entrada inválida falha antes de acessar a rede. A mensagem atual digitada explicitamente pelo usuário vira `input`. Não saem `relationship`, `traits`, invariantes arbitrários, `MemoryRecord`, `source_context`, `retrieval_hint`, conversas LR-4 ou histórico geral. Nesta primeira versão, nem mesmo o histórico da própria sessão LR-6 é reenviado. A UI avisa sobre a [política do Free Tier](https://ai.google.dev/gemini-api/docs/pricing) antes do botão “Enviar ao Gemini”. `store:false` impede state da Interactions API, mas não altera os termos de tratamento de conteúdo do Free Tier.

## Segredo e fluxo

O campo `type=password` mantém a chave no state React apenas durante edição e limpa após sucesso. `gemini_set_api_key`, `gemini_delete_api_key` e `gemini_status` expõem somente status. O Rust valida tamanho de 1 a 512 bytes após trim e rejeita caracteres de controle. `SecretKey::GeminiApiKey` grava no Stronghold; a chave de unlock do snapshot vem do credential store do SO. A leitura síncrona usa `spawn_blocking` antes do HTTP. A chave não entra em SQLite, config, URL, body, log, TaskRecord ou resultado. A [documentação oficial de chaves](https://ai.google.dev/gemini-api/docs/api-key) confirma o header REST; novas chaves AI Studio podem ser auth keys, mas o header permanece o mesmo.

## Execução

`start_gemini_task` limita a mensagem a 4096 bytes, registra TaskId, emite `TaskStarted` e `ContextBuilt`, executa o Scheduler real isolado (somente Gemini), recebe `ProviderSelected` e `ProviderChunk`, e emite `TaskResultReady` e `TaskCompleted`. Mocks diagnósticos continuam separados. O bridge atual mantém `idle` durante a tarefa e usa `greeting` provisório na conclusão. O parser SSE aceita fronteiras HTTP arbitrárias, ignora eventos futuros e só emite `step.delta` de tipo `text` dentro de um `model_output`. `thought_signature`, thoughts e summaries não entram na UI ou no banco. `interaction.completed` só conclui com sucesso se `interaction.status` for `completed` e as contagens reais de input, output e total estiverem presentes; thought tokens são preservados quando disponíveis. `incomplete`, `requires_action`, `cancelled` remoto, `failed` e status ausente/desconhecido encerram com erro. Chunks já transmitidos podem permanecer como prévia, sem virar resposta final no SQLite. O adapter não estima uso.

O cliente `reqwest 0.11.27` usa rustls, timeout de conexão de 8 s, timeout total de 45 s e timeout idle de 15 s. Cancelamento local e `channel_closed` abandonam o future e o stream sem retry ou fallback. Erros HTTP leem no máximo 64 KiB e priorizam `error.code`; eventos SSE `error` usam o mesmo mapper. `rate_limit_exceeded` e `too_many_requests` criam `RateLimited` e cooldown, com `Retry-After` HTTP em segundos ou data (limitado a sete dias); `quota_exceeded` e `payment_required` não recebem retry. Autenticação/permissão, cancelamento remoto, erros de request/geração e códigos desconhecidos são terminais. `deadline_exceeded`, `gateway_timeout` (presente no exemplo oficial de streaming), `api_error` e `service_unavailable` permitem a única tentativa adicional do Scheduler somente antes do primeiro chunk. Sem código estruturado, HTTP usa fallback seguro: 401/403 autenticação, 408/504 timeout, 429 rate limit, 5xx indisponibilidade e demais 4xx fatal. Após um chunk, não há retry nem fallback. `error.message` e corpos brutos não entram em UI, TaskRecord ou audit.

Após sucesso, uma transação SQLite grava a mensagem do usuário e a resposta final na sessão `Luna · Gemini LR-6`. Chunks não são persistidos separadamente. Se a chamada falha ou é cancelada antes da fase de commit, nenhuma fala é adicionada. Ao entrar nessa fase, o TaskRegistry encerra a aceitação de cancelamento para impedir uma tarefa `cancelled` com resposta final persistida. O TaskRecord é gravado antes de `TaskCompleted`; falha nessa escrita produz erro operacional visível, sem reenviar a chamada Gemini. Se o Channel fechar ao emitir o evento terminal, o registro é marcado como `channel_closed`. A saída é texto não confiável e a UI a renderiza como texto.

## Validação e limites

Os testes usam servidor HTTP local e credential store fake, sem quota externa. Cobrem payload e marcadores privados, header, SSE partido, ordem dos chunks, usage, errors, Retry-After, cancelamento e Channel fechado. O gate de API real requer inserir a chave no painel Tauri, enviar pergunta neutra, observar chunks/usage/TaskRecord, cancelar outra chamada e reabrir o aplicativo. Nenhuma request é feita ao iniciar. A toolchain Rust 1.77.2 exata ainda deve ser verificada; `cargo check` em toolchain mais nova não prova MSRV. O SQLite local não tem criptografia integral.


## Auditoria de implementação — 26/09/2026

A arquitetura local passou na revisão de privacidade, SecretStore, streaming, cancelamento e persistência, mas a validação com API key real deve esperar dois ajustes:

1. O parser atual extrai `usage` de `interaction.completed`, porém ainda não valida `interaction.status`. A API pode terminar como `completed`, `incomplete`, `failed`, `cancelled` ou `requires_action`; somente sucesso completo deve seguir para persistência como resposta concluída. Em especial, `incomplete` pode ocorrer ao atingir `max_output_tokens`.
2. Eventos SSE `error` hoje são achatados para `Unavailable`, e erros HTTP são classificados principalmente pelo status. A API oficial fornece `error.code`; o adapter deve usar esse código para distinguir `quota_exceeded`, `rate_limit_exceeded`/`too_many_requests`, `authentication`/`permission_denied`, cancelamento, timeout e falhas transitórias, preservando fallback por status HTTP apenas quando o corpo não puder ser interpretado.

**LR-6 protocol FIX concluído em 26/09/2026:** os dois pontos acima foram corrigidos e cobertos por testes HTTP/SSE locais. A validação com API real está liberada como próxima ação manual, ainda pendente; não houve chave real nem request externa nesta rodada. O endpoint permanece `v1beta/interactions`; a migração para `v1` não faz parte desta correção. A referência oficial descreve `errors[]` no objeto completo da interação, mas não o garante no objeto parcial do evento terminal; `failed` sem código utilizável falha sem retry. A página oficial de erros recomenda, em alguns casos, modificar a entrada e tentar novamente; esta LR-6 encerra bloqueios e erros estruturais sem reenvio automático para preservar privacidade e custo.


## Auditoria da FIX — 26/09/2026

**PASS.** A revisão confirmou que somente `interaction.status=completed` com usage válido produz `ProviderResponse`; estados `incomplete`, `failed`, `cancelled`, `requires_action` e status ausente/desconhecido falham fechado. O mapper central de `error.code` é compartilhado por HTTP e SSE, preserva `Retry-After` para rate limit, distingue quota de limitação transitória e mantém erros terminais fora de retry/fallback. A regra pós-primeiro-chunk continua impedindo retry/fallback para evitar resposta/custo duplicado.

Com isso, o gate de protocolo está encerrado e a validação real com API key no painel Tauri está liberada. LR-6 permanece aberta até provar chamada real, SSE/usage, cancelamento e reinício.


## Validação real e fechamento — 26/09/2026

**LR-6 PASS.** A API key foi inserida exclusivamente no painel Tauri e permaneceu disponível após reinício, via Stronghold + credential store do SO. Uma chamada real ao Gemini concluiu com streaming e usage observado de 48 tokens de entrada, 78 de saída, 126 totais e 0 thinking; a resposta final foi persistida na conversa local.

Os testes manuais posteriores também exercitaram os caminhos de falha reais: uma tentativa terminou `unavailable`, outra terminou `provider_incomplete` com resposta parcial mantida apenas como preview, e uma terceira foi cancelada manualmente após receber chunks. O cancelamento terminou em `cancelled` antes da persistência da resposta final. Após fechar e reabrir a aplicação, Gemini permaneceu configurado e a conversa bem-sucedida anterior continuou disponível.

Com chamada real, SSE/usage, persistência, cancelamento e reinício comprovados, o gate LR-6 está encerrado. A próxima rodada planejada é de UI/performance antes de LR-7.


## Decisão pós-LR-6 — transparência de capacidade

Em 27/09/2026, durante o gate humano da UIP-5A, foi identificado que o chat de produto ainda herdava `max_output_tokens=512` e `thinking_level=low` como constantes internas da LR-6. O limite era invisível ao usuário e podia encerrar respostas como `provider_incomplete`.

Decisão arquitetural:

- esses valores passam a ser tratados apenas como defaults temporários de protótipo;
- a correção imediata pode elevar o teto de output para manter o chat utilizável;
- a UIP-6 deve substituir hardcodes de produto por política persistida e editável;
- o usuário poderá escolher provider/agente, modelo, thinking/reasoning, teto de output ou “sem limite adicional da Luna”, timeouts, retries, fallback, streaming e demais parâmetros suportados;
- limites reais do provider/modelo, quotas, segurança e permissões continuam válidos;
- Scheduler/adapters podem avisar ou rejeitar configuração incompatível, mas não degradar silenciosamente uma escolha explícita;
- erros causados por limites configurados devem apontar essa causa de forma inteligível, em vez de expor somente códigos genéricos.

O adapter permanece responsável por traduzir a política para a API do Gemini; não deve ser a origem permanente da política de capacidade.
