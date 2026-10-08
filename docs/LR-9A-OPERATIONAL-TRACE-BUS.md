# LR-9A — Operational Trace Contracts & Passive Event Bus

**Estado:** **LR-9A IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**
Não é PASS definitivo e não libera automaticamente LR-9B/C/D/E.

Base: `origin/main` em `dbfcbd4d1d426d79b59a20614753fe77d25e8f69`, workspace
limpo, referências atualizadas antes da criação de
`lr-9a-operational-trace-bus`. Nenhuma alteração direta de main, merge, rebase,
force-push ou PR faz parte desta entrega.

## Contrato nativo

`src-tauri/src/operational_trace/` contém `contract`, `bus` e `coalesce`.
Não depende de Presentation, WebView, IPC, providers, agentes, rede ou SQLite.
Reutiliza somente o tipo `TaskId` existente, sem converter `TaskEventKind`.

- `EventDraft`: provenance e kind; não recebe sequence, timestamp ou priority.
- `OperationalEvent`: identidade `u64` atribuída pelo bus, timestamp de observação
  em milissegundos UNIX, provenance e kind imutáveis, acessíveis por getters.
- `Provenance`: source type/id/instance; TaskId, subtask, correlation e coalescing
  key opcionais. Evento sem task é válido. TaskId presente mantém a faixa atual
  do Core: `1..=2^53-1`.
- `SourceType`: Core, Scheduler, TaskGraph, CognitiveProvider, Worker,
  SpecialistAgent, ExecutionBroker, TerminalProcess e Human. São categorias,
  sem integração com essas fontes nesta fase.
- `TraceId`: 1–64 bytes ASCII alfanuméricos ou `_ - . : /`; IDs de source,
  instance, subtask, correlation, coalescing e code usam o mesmo tipo validado.
  Não são comandos/capabilities, nem recebem autoridade por sua sintaxe.
- `TraceText`: até 8192 bytes UTF-8 exatos, incluindo texto vazio; excesso é
  rejeitado antes da cópia. Não existe truncation, chunking implícito ou JSON
  arbitrário. Campos internos privados e ausência de Deserialize impedem bypass
  de validação por construção/deserialização dos wrappers.

`OperationalKind` tem três famílias fechadas:

| Família | Tipos | Classe derivada |
| --- | --- | --- |
| Critical | ApprovalRequired, PolicyBlocked, Failed, Cancelled, Completed; code + message bounded | CRITICAL |
| State | Started, Planning, Routing, Fallback, Retry, Subtask/Command/ToolLifecycle, Checkpoint; code + detail bounded | STATE |
| TextDelta | Stdout, Stderr, ProviderText, AgentMessage, Progress, DisplayReasoningSummary; text bounded | STREAM |

Não existe flag livre para promover TextDelta a CRITICAL. Completed significa
conclusão publicada como necessária à integridade; adapters futuros deverão
respeitar essa semântica. DisplayReasoningSummary aceita apenas conteúdo que o
backend já disponibiliza explicitamente para exibição; não existe canal de
private/hidden reasoning. O bus não autentica fatos nem descobre segredos:
somente conteúdo previamente autorizado/sanitizado pode ser publicado, sob
responsabilidade do adapter que conhece a fonte.

## Instância e ordenação

`OperationalTraceBus::process_wide()` retorna o mesmo `Arc` via `OnceLock`.
Construtor de instâncias isoladas é privado e só é acessível a testes. A única
instância de produção é registrada como `Arc<OperationalTraceBus>` no Tauri
managed state antes do setup. Não existe comando Tauri novo, subscriber visual
ou publicação no fluxo de produto.

Mutex interno serializa assignment, retenção e tentativas de entrega live:
identidades únicas, crescentes, e ordem total nos eventos retidos e em cada
subscriber. O lock não atravessa callback, await, inferência, rede ou operação
de Presentation. Há contenção bounded entre publishers, com scans limitados a
1024 itens; não é uma promessa lock-free ou de latência em microssegundos.
Relógio de parede pode retroceder; sequence é a autoridade da ordem. Exaustão
`u64` retorna erro explícito, sem wrap/reutilização de identidade.

## Budgets

Todos os limites são constantes nomeadas; não há configuração capaz de removê-los.

| Constante | Default |
| --- | ---: |
| MAX_IDENTIFIER_BYTES | 64 bytes |
| MAX_TEXT_BYTES | 8192 bytes |
| EVENT_BASE_ESTIMATED_BYTES | 256 bytes |
| MAX_EVENT_ESTIMATED_BYTES | 8832 bytes |
| MAX_RETAINED_EVENTS | 1024 eventos |
| MAX_RETAINED_BYTES | 2 MiB estimados |
| CRITICAL_RESERVED_EVENTS / BYTES | 64 / 256 KiB |
| STATE_RESERVED_EVENTS / BYTES | 128 / 512 KiB |
| MAX_SUBSCRIBERS | 8 |
| SUBSCRIBER_QUEUE_EVENTS | 64 por subscriber |
| MAX_BATCH_EVENTS / BYTES | 128 / 256 KiB estimados |
| MAX_COALESCED_TEXT_BYTES / FRAGMENTS | 32 KiB / 32 |

Carga estimada de evento = 256 + comprimentos UTF-8 de **todos** os strings
de provenance/payload presentes. O pior caso tem seis IDs de 64 bytes e um texto
de 8192 bytes, totalizando 8832. A carga não mede JSON serializado, capacidade
do allocator, Arc/channel overhead ou RSS; esses números exigirão benchmark.
Os payloads usam Box<str> para não herdar capacidade excessiva do produtor.

Os defaults reservam uma janela recente pequena para o hardware atual, deixando
espaço para múltiplas fontes sem acumular minutos de deltas. São budgets de
engenharia conservadores, revisáveis após medição, não dados de benchmark.
Histórico e live usam Arc de eventos imutáveis. Filas live podem conservar
referências a eventos já evictados: no máximo `8 * 64` referências na fila mais
um lookahead por subscriber; carga lógica adicional de pior caso abaixo de
4,4 MiB. Nenhum acúmulo é proporcional ao tempo sem UI.

Os resultados de leitura pertencem ao caller. Cada resposta é bounded; um
caller que guarda indefinidamente respostas ou clones de Arc precisa impor
seu próprio budget. Isso não cria retenção indefinida dentro do bus.

## Retenção, reservas e overflow

Antes de inserir, verifica simultaneamente eventos **e** bytes:

1. STREAM ocupa no máximo 832 eventos / 1280 KiB.
2. STREAM + STATE ocupa no máximo 960 eventos / 1792 KiB.
3. Total ocupa no máximo 1024 eventos / 2048 KiB.

Reservas existem mesmo sem eventos de maior classe. CRITICAL pode usar a
capacidade inteira; a reserva é garantia contra classes menores, não um teto
independente de CRITICAL.

Ao atingir um teto, escolhe FIFO da menor classe elegível: STREAM primeiro,
depois STATE, depois CRITICAL. STREAM nunca evicta STATE/CRITICAL, e STATE nunca
evicta CRITICAL. Se só existir conteúdo de maior prioridade e o evento novo não
couber, descarta o novo histórico e retorna `retained=false`; publish continua
atribuindo identidade e tentando live delivery.

Uma inundação de CRITICAL eventualmente evicta o CRITICAL mais antigo, mantém
os caps e contabiliza perda. Nenhuma retenção infinita é prometida. Eviction
não é truncation e não fabrica um evento substituto ou resumo.

## Live best-effort

`subscribe` registra fila `std::sync::mpsc::sync_channel(64)` e retorna cursor
atual atomicamente com registration. Não envia histórico nessa operação.
`publish` usa exclusivamente `try_send`: Full descarta a entrega daquele
subscriber, conta lag e mantém a execução; Disconnected é removido e
contabilizado. Ausência de subscribers é válida.

`LiveSubscriber` tem leitura não bloqueante `drain_batch`, cursor e `status`
com count e última sequence perdida. Batches detectam descontinuidade em
sequence. Status revela também perdas no fim da fila, antes que chegue outro
evento. Os atomics de status são uma amostra concorrente, não snapshot
transacional entre os dois campos. Drop remove registration imediatamente,
libera slot e conta desconexão. Um subscriber lento não afeta outro saudável.

Subscriber possui no máximo um evento de lookahead quando um batch atinge o
cap de bytes, sem perder esse evento. Fila tem teto de eventos e teto derivado
de bytes `64 * 8832`; lookahead acrescenta até 8832 bytes. Nenhum sender espera
consumo. Todos os campos de eventos entregues permanecem imutáveis.

## Snapshot, replay, métricas e batching

`replay(after, BatchLimits)` é o snapshot/replay em lotes. BatchLimits rejeita
zero, limites superiores aos caps, ou bytes menores que 8832, garantindo
progresso mesmo para o maior evento. Cursor futuro é rejeitado.

Resposta contém eventos ordenados, bytes estimados, `has_more`, `next_after`,
`replay_complete` e snapshot de stats com sequence atual e janela disponível.
`replay_complete` diz se **todo** o sufixo `(after, latest]` ainda está retido;
paginações sem perda permanecem completas. `next_after` é o último item quando
há próxima página; na última página avança para latest mesmo diante de buracos.

Priority-aware retention produz buracos **internos**, inclusive quando o evento
mais antigo ainda é sequence 1. `highest_lost_sequence` mantém o maior evento
evictado/descartado: sufixo só é completo se `after >= highest_lost_sequence`.
Não existe coleção ilimitada de lost ranges. Stats são amostradas sob o mesmo
lock dos eventos do replay. Quem pagina durante novas publicações precisa
reavaliar completude em cada resposta; não há snapshot congelado entre chamadas.

`TraceStats`: published, evicted/dropped separados por classe, live delivery
dropped por tentativa/subscriber, disconnected, active subscribers, retained
events/bytes, latest, oldest retained e highest lost. Counters cumulativos são
saturating; sequence usa checked_add. Nenhuma métrica guarda conteúdo privado.
Published inclui eventos descartados do histórico; live losses não significam
necessariamente perda de replay. Rejeições de payload não são publications e
não consomem sequence. Não existe obrigação de IPC/render por evento.

## Coalescing real, separado do armazenamento

`coalesce_batch` aceita somente um batch capped e retorna uma view derivada.
Histórico/live mantêm as identidades originais. Só TextDelta adjacentes, com
sequences contíguas, mesmo canal, provenance inteira idêntica e coalescing key
presente podem concatenar. Diferenças de source type/id/instance, task, subtask,
correlation, key ou canal interrompem a concatenação. STATE/CRITICAL e buracos
de sequence também interrompem.

Concatenação preserva bytes, UTF-8, whitespace e ordem exatos, com range de
sequences, timestamps inicial/final e count de fragments. Não resume,
interpreta, normaliza, mistura fontes ou esconde perda. Ao atingir 32 KiB ou
32 fragments, inicia outro item. Não existe acumulador persistente.

## Relação com TaskEventBroker / Headless

`TaskEventBroker`/Sink/Registry continuam com seus contratos Conversation,
UiBound/HeadlessSafe, replay e lifecycle existentes. Não são o bus universal.
Nenhum desses arquivos foi modificado. Scheduler, TaskGraph, Safe Handoff,
Codex Planner, providers, rate/resilience e Conversation não publicam aqui.
PresentationMode não destrói nem condiciona o bus. Testes verificam utilização
durante suspend/resume de registry e detach da observação atual.

## Gates e evidências

Os 32 testes específicos passaram, sem falhas/ignorados. A última execução
integral concluiu com 1023 passando, zero falhas e dois ignorados herdados.
Testes sintéticos cobrem validação
UTF-8/provenance, publish taskless e sem UI,
classes derivadas, subscribers funcionais/desaparecidos/cheios, lag de sequência
e de fim de fila, replay/paginação/bytes, reservas/precedência/overflow extremo,
contabilidade, coalescing exato e isolamento arquitetural.

Gate reproduzível sem rede:

```sh
cargo test --manifest-path src-tauri/Cargo.toml operational_trace::tests::synthetic_multi_source_stress_gate -- --nocapture
```

Cinco threads: dois SpecialistAgents fictícios, dois workers fictícios e um
provider fictício; 2000 STREAM por fonte, STATE a cada 100 fragments, barrier
no meio do burst para CRITICAL próximo do pico, subscriber vivo nunca drenado
até o join de todos os producers. Confere caps em cada publicação, unicidade
de todas as identities, ordem por fonte/global, provenance, exatidão dos textos,
recuperação de todos os STATE/CRITICAL, drops e publicação após detach.
Sem assertions frágeis de timing, rede ou benchmark prolongado.

Resultado do stress na execução específica final: **10.105 publicações**, 937
eventos retidos, **1.152.230 bytes estimados**, 9168 STREAM evictados, 10.041
tentativas live descartadas, zero STATE/CRITICAL evictados e zero drops de
ingestão histórica. Todos os 100 STATE e cinco CRITICAL recuperáveis, sequence
`1..=10105` única/ordenável e provenance preservada. O valor de bytes varia com
o interleaving de fontes; os caps e as verificações permanecem determinísticos.

| Gate executado | Resultado |
| --- | --- |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | Diferenças preexistentes em build.rs, adaptive.rs, lib.rs e outros arquivos da base; nenhuma no módulo novo |
| `/home/sam/.cargo/bin/rustfmt --edition 2021 --check src-tauri/src/operational_trace/*.rs` | Limpo após formatação equivalente do escopo novo |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Concluído, 19 warnings de código existente |
| `cargo test --manifest-path src-tauri/Cargo.toml operational_trace -- --nocapture` | 32 passaram, 0 falhas, 0 ignorados; stress concluído em suíte de 2,35 s |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Primeira rodada: 1020 passaram, 0 falhas, 2 ignorados preexistentes, antes dos três testes adicionais |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | Árvore final: 1023 passaram, 0 falhas, 2 ignorados preexistentes (1025 registrados), em 205,77 s; binário e doc-tests sem testes adicionais |
| `cargo check --manifest-path src-tauri/Cargo.toml --release` | Concluído, 42 warnings de código existente |
| `npm run typecheck` | Concluído sem erros |
| `npm run build` | Concluído; aviso de chunk AvatarViewport de 634,27 kB (>500 kB), sem alteração frontend |
| `git diff --check` e `git diff --cached --check` | Limpos, incluindo arquivos novos staged antes dos commits |

Toolchain usada: Rust/Cargo Fedora 1.98.1; rustfmt disponível em
`/home/sam/.cargo/bin`, não no PATH inicial. O erro de formatação global foi
reproduzido também com `build.rs` extraído de `origin/main`; não houve
reformatação ampla de contratos aprovados. As duas linhas adicionadas ao
composition root seguem a formatação de seu contexto.

Nenhum warning do compilador aponta para `operational_trace`. Os warnings
incluem dead code existente e, em release, imports diagnósticos inutilizados em
`luna/runtime.rs` e `persistence/mod.rs`. Nenhum foi suprimido ou corrigido fora
de escopo. Nenhum teste foi removido/enfraquecido e nenhum `#[ignore]` foi
adicionado. Os dois ignorados herdados são `real_app_server_handshake`
(requer Codex local) e `manual_final_codex_agent_bridge_gate` (exige autenticação,
inferência e quota); esses gates manuais não foram executados pela LR-9A.

## Divergência documental encontrada

Na base remota, o fechamento da PERF-1 ainda mencionava a antiga LR-9
“Luna Voice / feedback natural”, embora o plano mestre e arquitetura já
registrassem sua redefinição em 08/10/2026. Corrigida apenas essa referência
de próxima fase; contratos PERF-1 e registros históricos continuam preservados.
Nenhuma divergência justifica ampliar o escopo autorizado.

## Fora de escopo e dívidas

- LR-9B: ExecutionRequest/Result, Broker, Structured Exec, PTY, sessão humana,
  permissões e authority; nenhum comando/processo é executado pelo bus.
- LR-9C: IPC, Terminal/Home, filtros, React, virtualização, cadence, reattach e
  budget das views; nenhum arquivo frontend foi alterado.
- LR-9D: adapters explícitos TaskEvent/Scheduler/TaskGraph/provider/agent,
  sanitização contextual e policy de exposição; nenhuma integração real feita.
- LR-9E: stress integrado, segurança, approvals, cancelamento de execução e
  gates de Presentation; nesta fase há apenas stress da observação sintética.
- Persistência/auditoria durável precisa de decisão futura de privacidade;
  nenhuma migration, log de conteúdo, stdout ou reasoning persistido.
- Medir RSS, latência/scan sob maior concorrência, budgets por fonte/task e
  serialização/limites reais de IPC antes de escalar; retenção atual é global.
  LR-9C deverá preservar a identidade u64 exata ao atravessar a fronteira JS.
- MSRV declarado e packaging cross-platform permanecem dívidas herdadas.
- Formatação global da base possui diferenças preexistentes; aplicar rustfmt
  ao módulo novo e verificar esse escopo sem reformatar código aprovado alheio.

Passive Observability é estrutural: nenhum prompt, provider call, agent request,
inferência, resumo por LLM, narração artificial ou private chain-of-thought
foi acrescentado no caminho da observação.
