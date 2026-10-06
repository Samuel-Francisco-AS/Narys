# LR-8.5C — Safe Cross-Resource / Cross-Variant Handoff

Estado: **C1 = PASS técnico após auditoria independente.**

**C2 = PASS TÉCNICO após auditoria independente em 06/10/2026.**

**C3 = PASS TÉCNICO após auditoria independente em 06/10/2026.**

**C4 FIX-2 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente.**

C4 continua **NÃO PASS**.

C1 PASS; C2 PASS; C3 PASS. LR-8.5C ainda **NÃO PASS**.
Merge ainda **NÃO autorizado**.

Esta subfase fecha a LR-8.5 provando continuidade segura entre decisões de alocação
independentes. Os blocos C1–C4 são **blocos internos da LR-8.5C**, não novas subfases
do roadmap.

## Objetivo

Permitir que uma tarefa continue em outro recurso, access path, modelo ou effort
**somente quando a unidade cognitiva atual alcançou uma fronteira segura e seu estado
necessário foi comprometido**, sem transformar handoff em fallback concorrente do
Scheduler.

Princípio central:

> **Uma alocação não muda durante uma unidade cognitiva já iniciada. O Luna Core só
> pode reconsiderar a alocação depois que a unidade atual alcançou uma fronteira
> segura e o estado necessário para continuidade foi confirmado.**

A LR-8.5B responde "quem deve executar esta unidade?". A LR-8.5C responde "quando e
com qual estado uma unidade posterior pode ser entregue a outro recurso sem repetir
trabalho, perder continuidade ou autorizar gasto indevido?".

## Baseline observada

A implementação atual já possui:

- ResourceAllocator provider-agnostic da LR-8.5B;
- policy econômica persistida por CognitiveRole;
- snapshot/configuração de alocação aplicada antes de execução;
- Scheduler como autoridade de admission, rate, retry, fallback e resilience;
- proteção existente contra fallback após output parcial;
- TaskGraph real com unidades independentes;
- lifecycle de subtarefas, cancelamento e terminal único;
- provenance persistida em `task_records` / `task_subtask_records`;
- resultados estruturados por subtarefa e consolidação determinística;
- alocação inicial de Worker pelo mesmo motor do Scheduler e pin da variante escolhida
  para a unidade já iniciada.

A C deve construir sobre essas garantias. Não deve desmontá-las para criar um segundo
sistema de retry/fallback.

## Invariantes

1. **Handoff não é fallback do Scheduler.**
2. Retry/fallback antes de output continua pertencendo ao Scheduler.
3. Uma unidade iniciada mantém sua allocation snapshot até terminar ou falhar.
4. Output parcial nunca autoriza reiniciar silenciosamente a mesma unidade em outro
   recurso.
5. Mudança de quota, scarcity ou ranking durante uma unidade não altera o executor
   daquela unidade.
6. Reallocation só ocorre para uma **nova unidade** em fronteira segura.
7. Unidade concluída não é repetida por causa de handoff.
8. Efeito externo confirmado como executado não é repetido.
9. Estado de efeito desconhecido/in-flight falha fechado para replay automático.
10. Cancelamento observado antes da próxima unidade impede a nova execução.
11. PaidUsePolicy e budget da tarefa continuam válidos no handoff; uma alternativa
    paga não ganha autorização por ser a única disponível.
12. Policy configurada pelo usuário não é silenciosamente reescrita no meio da tarefa.
13. Fatos operacionais/econômicos podem ser reavaliados entre unidades.
14. Checkpoints não armazenam prompts secretos, credenciais ou private chain-of-thought.
15. Nenhum provider ou Specialist Agent se torna dono exclusivo do estado da tarefa.

## Modelo de consistência

A consistência desejada é:

~~~text
Task
│
├── policy snapshot ───────────────────────── imutável durante a task
│
├── Unit A
│   ├── allocation snapshot: resource/model/effort A
│   ├── execução
│   └── CHECKPOINT COMMITTED
│
├── fatos operacionais/econômicos podem mudar
│
├── ResourceAllocator roda para a próxima unidade
│
├── HANDOFF opcional
│   A → B
│
└── Unit B
    ├── novo allocation snapshot
    ├── recebe somente Shared Cognitive State necessário
    └── execução
~~~

Assim, **policy da tarefa** e **alocação da unidade** têm ciclos de vida diferentes:

- policy snapshot: congelado para preservar intenção/configuração da tarefa;
- allocation snapshot: congelado apenas para a unidade atual;
- fatos LR-8/LR-8.5: podem ser reobservados entre unidades;
- próxima unidade: pode receber outra allocation se o Auto da B assim decidir.

## Fronteiras seguras

Fronteiras inicialmente elegíveis:

- antes de iniciar uma unidade ainda não executada;
- após conclusão confirmada de uma unidade;
- fronteira de nó/onda do TaskGraph;
- checkpoint persistido e validado;
- resume a partir de checkpoint comprometido.

Fronteiras proibidas:

- provider request em andamento;
- streaming/output parcial de unidade ainda não concluída;
- efeito externo em andamento;
- estado de efeito desconhecido;
- janela em que o Core ainda não sabe se a unidade anterior foi committed.

Não introduzir "best effort replay" para contornar fronteira incerta.

## Shared Cognitive State mínimo

Não criar nesta fase um estado cognitivo global e genérico da Luna.

O contrato deve ser mínimo e ligado à execução. Conceitos possíveis, nomes finais não
prescritivos:

~~~text
CognitiveCheckpoint
HandoffEnvelope
ExecutionUnitId
AllocationSnapshotRef
PolicySnapshotRef
HandoffDecision
HandoffReason
EffectState
CheckpointSequence
~~~

Um checkpoint deve carregar apenas o necessário para continuar de forma verificável,
por exemplo:

~~~text
checkpoint_id
root_task_id
unit_id / boundary
sequence
completed_dependencies
handoff_context
source_allocation
policy_snapshot_ref
effect_state
provenance
committed_at
~~~

### Handoff context

Pode conter:

- objetivo/requirements da próxima unidade;
- IDs e dependências concluídas;
- resultados concluídos necessários;
- referências a artefatos/estado persistido;
- fatos de provenance necessários;
- limites/requirements da próxima unidade.

Não deve conter por padrão:

- credenciais;
- secrets;
- headers;
- prompts privados completos sem necessidade;
- corpo remoto bruto;
- private chain-of-thought;
- estado interno não verificável de um provider.

## Effect fence / anti-duplicação

A C deve provar a semântica antes de Tool Runtime completo.

Modelo mínimo sugerido:

- `NotStarted`: pode iniciar;
- `Committed`: não pode repetir;
- `UnknownOrInFlight`: não pode repetir automaticamente; pausa/falha fechado.

Os nomes finais podem mudar. O ponto é separar "não ocorreu" de "ocorreu" e de "não
sei se ocorreu".

Uma nova allocation nunca deve ser usada como justificativa para repetir um efeito.

## Relação com Scheduler e ResourceAllocator

### Scheduler

Continua responsável por:

- compatibilidade técnica;
- admission;
- rate limits;
- retry;
- fallback;
- resilience;
- execução de uma unidade já selecionada.

### ResourceAllocator

Continua responsável por:

- hard gates econômicos/cognitivos;
- quality floor;
- scarcity;
- spend guard;
- ranking do Auto;
- escolha da variante da **próxima unidade**.

### Handoff layer

A C adiciona a responsabilidade de:

- validar fronteira segura;
- produzir/validar checkpoint;
- decidir se uma nova unidade pode nascer;
- pedir nova alocação quando permitido;
- preservar provenance;
- impedir replay;
- pausar quando continuidade automática não for autorizada.

Não duplicar score da B dentro da camada de handoff.

## Blocos internos de implementação

### C1 — Handoff Contract & Safe Boundaries

Objetivo: formalizar a máquina de estados e as fronteiras antes de tocar o runtime real.

Entregas:

- contratos provider-agnostic para checkpoint/handoff;
- identidade monotônica de unidade/checkpoint;
- enum/estado explícito de boundary;
- `can_handoff`/equivalente puro e testável;
- distinção entre unidade não iniciada, em execução, output observado, concluída,
  cancelada e estado incerto;
- EffectState mínimo para anti-replay;
- HandoffDecision explicável e sanitizada;
- nenhuma alteração no Scheduler ou TaskGraph real nesta etapa;
- nenhuma persistência/migration nesta etapa, salvo necessidade estrutural comprovada.

Gate C1 deve provar pelo menos:

1. nova unidade ainda não iniciada → handoff elegível;
2. unidade concluída + checkpoint committed → handoff elegível;
3. request em andamento → proibido;
4. output parcial → proibido;
5. efeito Committed → replay proibido;
6. efeito Unknown/InFlight → replay automático proibido;
7. cancelamento antes da próxima unidade → sucessor proibido;
8. decisão determinística para os mesmos fatos;
9. nenhuma decisão contém prompt/output/secrets;
10. contrato não depende de marcas/providers específicos.

### C2 — Checkpoint, Provenance & Shared Cognitive State

Objetivo: tornar o boundary verificável e persistível sem criar uma memória global.

Entregas:

- checkpoint estruturado mínimo;
- provenance por unidade e transição;
- vínculo com policy/allocation snapshot;
- handoff context limitado;
- persistência/restart quando necessário;
- anti-duplicação de unidade e efeito;
- comportamento fail-closed para checkpoint corrompido/incompleto;
- testes de resume sem reexecutar unidade committed.

C2 não deve ainda introduzir reallocation real no TaskGraph se a persistência não
estiver estabilizada/auditada.

### C3 — Boundary Reallocation & TaskGraph Bridge

Objetivo: aplicar a LR-8.5B novamente apenas nas fronteiras seguras de próximas
unidades.

Semântica alvo:

~~~text
unidade A termina
→ checkpoint committed
→ cancelamento?
   sim: parar
   não:
→ há próxima unidade pronta?
→ observar fatos atuais
→ aplicar mesma policy snapshot
→ ResourceAllocator escolhe nova allocation
→ pin da allocation para a unidade B
→ executar B
~~~

Entregas:

- reallocation por próxima unidade/nó;
- troca cross-provider;
- troca cross-model/effort no mesmo resource;
- nenhuma troca durante unidade em andamento;
- snapshots por unidade;
- provenance source → destination + reason;
- paralelismo preservando allocations independentes;
- regressões Scheduler/LR-8/LR-8.5B.

Fixed e Preferred preservam suas semânticas. Auto pode reavaliar fatos entre unidades;
isso não autoriza reordenar Preferred nem reescrever Fixed.

### C4 — Pause/Resume, Restart & Final Gate

Objetivo: fechar continuidade segura quando não existe sucessor automaticamente
autorizado.

Casos de pausa:

- única alternativa compatível é paga e PaidUsePolicy não autoriza;
- custo pago necessário é Unknown;
- budget conhecido é insuficiente;
- checkpoint/effect state é incerto;
- continuidade exige decisão humana/policy futura;
- nenhum candidato satisfaz capability/quality floor.

A pausa deve ser distinta de sucesso e de falha técnica sempre que o runtime suportar
essa distinção sem quebrar invariantes existentes. Se a representação persistida atual
não comportar um estado novo com segurança, desenhar primeiro a transição e usar um
estado conservador explicitamente documentado; não inventar semântica ambígua.

Gate final deve provar restart/resume sem replay, cancelamento nas fronteiras, zero
auto-spend indevido e compatibilidade com TaskGraph.

## Synthetic gate obrigatório da LR-8.5C

Sem gastar dinheiro real e sem depender de Codex/Copilot reais, provar pelo menos:

1. **Cross-provider boundary**
   - Unit A conclui em provider X;
   - fatos mudam;
   - Unit B ainda não começou;
   - Auto escolhe provider Y;
   - A não é repetida.

2. **Cross-variant same resource**
   - Unit A usa model/effort A;
   - próxima unidade exige ou favorece variant B;
   - handoff acontece apenas na fronteira.

3. **Scarcity transition**
   - recurso entra em Reserve após Unit A;
   - alternativa adequada recebe Unit B;
   - recurso anterior permanece válido para Fixed/única opção quando aplicável.

4. **Paid deny pause**
   - alternativa incluída indisponível;
   - única candidata factual é paga;
   - PaidUsePolicy::Deny;
   - nenhuma chamada paga ocorre;
   - execução pausa/reporta de forma explícita.

5. **Paid unknown fail-closed**
   - custo monetário desconhecido;
   - zero auto-spend.

6. **Cancellation at boundary**
   - Unit A committed;
   - cancelamento vence antes do dispatch de B;
   - B tem zero provider request.

7. **Partial output**
   - Unit A produz output e falha;
   - C não reinicia a mesma unidade em outro provider;
   - regras existentes do Scheduler permanecem autoridade.

8. **Committed replay protection**
   - restart após checkpoint;
   - Unit A não executa novamente.

9. **Unknown effect**
   - estado incerto;
   - nenhuma repetição automática.

10. **Parallel independence**
    - duas unidades prontas possuem allocations/checkpoints independentes;
    - handoff/reallocation de uma não reescreve a outra.

11. **Provenance**
    - source allocation, destination allocation, boundary e reason são persistíveis e
      sanitizados;
    - nenhum conteúdo privado desnecessário é gravado.

12. **Determinism**
    - mesmos snapshots/fatos → mesma HandoffDecision.

## Não objetivos

A LR-8.5C não deve:

- integrar GitHub Copilot real;
- ampliar permissões ou lifecycle do Codex;
- implementar LR-10/LR-11 antecipadamente;
- criar Tool Runtime completo;
- executar compra/refill;
- autorizar gasto automaticamente;
- implementar distributed transactions genéricas;
- criar memória global de raciocínio;
- persistir private chain-of-thought;
- reescrever o Scheduler;
- substituir retry/fallback;
- fazer handoff no meio de streaming;
- replay de efeitos para "tentar garantir";
- hardcodar comportamento por provider/modelo comercial.

## Relação com LR-10/LR-11

LR-10/LR-11 devem entrar como consumidores do protocolo, não como motivo para
reinventá-lo.

O gate real Codex ↔ Copilot continua condicionado às integrações posteriores. Nesta
fase, mocks/fixtures e Cognitive Providers existentes são suficientes para validar a
semântica.

## Ordem recomendada

1. C1 contracts + safe-boundary state machine;
2. auditoria independente / FIXes de boundary e anti-replay;
3. C2 checkpoint + provenance + persistence;
4. auditoria / restart fixtures;
5. C3 TaskGraph boundary reallocation;
6. regressões LR-7D3/LR-8/LR-8.5B;
7. C4 pause/resume + integrated synthetic gate;
8. auditoria final;
9. somente então fechamento da LR-8.5.

Não implementar C1–C4 em um patch gigante.

## Critério de fechamento da LR-8.5C

A C recebe PASS quando:

- existe contrato provider-agnostic de handoff;
- allocation é imutável durante unidade iniciada;
- reallocation só ocorre para próxima unidade em boundary seguro;
- checkpoint mínimo preserva continuidade sem chain-of-thought;
- unidade/effect committed não sofre replay;
- estado incerto falha fechado;
- cancelamento vence antes de nova unidade;
- Auto pode trocar resource/model/effort entre unidades;
- Fixed/Preferred preservam semântica;
- paid deny/unknown nunca produz auto-spend;
- restart/resume não repete trabalho committed;
- provenance é suficiente e sanitizada;
- Scheduler continua dono de retry/fallback e LR-8 continua dono de admission/rate;
- gate sintético passa sem dinheiro real;
- nenhuma integração completa de Codex/Copilot foi antecipada.

Com esse PASS, a LR-8.5 pode ser encerrada e LR-10/LR-11 recebem um protocolo de
continuidade já provado para Specialist Agents reais.

## C1 — Implementação candidata (06/10/2026)

**C1 = PASS TÉCNICO após auditoria independente em 06/10/2026.**

### Baseline e escopo verificados

Antes das alterações, fetch confirmou `main == origin/main ==
0024fec735e3f6cb2461dbdeba8fa6aa4be32d32`, workspace limpo e ausência da branch
local/remota `lr-8.5c-safe-handoff`. A branch foi criada dessa main sincronizada.
Os quatro documentos solicitados foram lidos integralmente; cognitive_resources,
Scheduler, TaskGraph/runtime/Worker, Task/Event lifecycle e allocation/runtime
policy da B foram inspecionados. A/B permanecem integradas e a B foi integrada
pela PR #24; nenhum contrato ou comportamento dessas camadas foi alterado.

O único código de produção adicionado é o módulo puro `handoff.rs` e seu export.
Nenhum consumidor runtime foi conectado. Não há persistência, migration, IO,
relógio implícito, retry/fallback novo, reallocation, pausa ou resume. C2/C3/C4
permanecem apenas planejadas.

### Contratos finais da candidata

- `ExecutionUnitId`: identidade local numérica `(root_task_id, sequence)`, com
  campos privados; ambos em `1..=9_007_199_254_740_991`. Não reutiliza o texto de
  um PlanStep nem depende do tipo interno TaskId do lifecycle Luna.
- `CheckpointId`: recibo mínimo `(unit_id, sequence)` vinculado a uma unidade;
  sequence tem o mesmo bound e é local à unidade. `next()` de ambos os IDs
  avança monotonicamente e retorna erro no limite, sem wrap/saturação.
- `ExecutionUnitState`: `NotStarted`, `Running`, `PartialOutputObserved`,
  `Completed`, `Cancelled`, `Failed`, `Unknown`. A observação de output parcial
  não se torna NotStarted por EOF/falha; nenhuma unidade iniciada é candidata
  a nova allocation neste protocolo.
- `EffectState`: `NotStarted`, `Committed`, `UnknownOrInFlight`. É um fence
  agregado conservador por unidade: qualquer efeito incerto/in-flight domina;
  caso contrário, qualquer efeito committed exige Committed; NotStarted exige
  que nenhum efeito tenha iniciado. Sem payload de efeito.
- `ExecutionUnitFacts`: somente ID, lifecycle e fence de efeitos.
- `HandoffBoundary`: `ConfirmedBeforeStart { unit_id }`,
  `ConfirmedCompletion { checkpoint }`, `Unconfirmed`, `Unknown`.
  Confirmação é fato explícito fornecido pelo Core, em memória nesta etapa.
- `HandoffRequest`: predecessor opcional, unidade solicitada, boundary e fato
  booleano de cancelamento observado.
- `HandoffDecision`: IDs, `HandoffStatus`, `HandoffReason` e avaliações separadas
  de replay da unidade solicitada e do predecessor. Campos privados, sem
  desserialização de aggregates e sem rationale livre.
- `ReplayDecision` / `ReplayReason`: `NotApplicable` somente quando unidade e
  efeitos nunca iniciaram; caso contrário `Forbidden` com reason enum. Não
  existe variante que autorize replay; NotApplicable não significa permissão.
- `HandoffContractError`: erros enum sanitizados para IDs/sequences inválidos,
  sem eco de valores rejeitados.
- `can_handoff(&HandoffRequest)`: função pura, síncrona, bounded e determinística.

### Semântica e validação

| Fatos | Elegibilidade de boundary | Replay |
|---|---|---|
| Unidade/efeitos NotStarted, sem predecessor, BeforeStart confirmado para seu ID | Eligible | NotApplicable |
| Predecessor Completed, efeitos NotStarted, recibo confirmado vinculado a ele; sucessor fresco | Eligible | Predecessor Forbidden/UnitCompleted |
| Mesmo caso, efeito anterior Committed | Eligible | Predecessor Forbidden/EffectCommitted |
| Unidade solicitada Running ou PartialOutputObserved | Blocked | Forbidden |
| Predecessor incompleto, cancelado, failed ou Unknown | Blocked | Forbidden se iniciado/incerto |
| Qualquer fence UnknownOrInFlight | Blocked | Forbidden/EffectUnknownOrInFlight |
| Cancelamento observado antes do sucessor | Blocked | Não concede replay |
| Boundary incerto, não confirmado ou contraditório | Blocked | Não concede replay |

A unidade solicitada deve ser NotStarted com efeitos NotStarted. Havendo
predecessor, IDs devem pertencer à mesma tarefa, a sequence do sucessor deve ser
estritamente maior e o predecessor deve estar Completed sem efeitos incertos.
Gaps são válidos para unidades independentes; não se exige `sequence + 1`.
ConfirmedCompletion exige predecessor e checkpoint ligado exatamente ao seu ID.
ConfirmedBeforeStart exige ausência de predecessor nesta transição e o ID exato
da unidade solicitada. Boundary não substitui lifecycle nem fence de efeitos.
NotStarted junto a Committed/UnknownOrInFlight é contradição e falha fechado.

A precedência da reason única é fixa: cancelamento, contradição lifecycle/efeitos,
identidade/task/ordem, lifecycle da unidade solicitada, efeitos/lifecycle anterior,
tipo/vínculo do boundary. Replay conserva os fences de efeitos mesmo quando a
reason principal é outro bloqueio. Se predecessor e unidade solicitada reutilizam
o mesmo ID, ambas as avaliações negam replay: fence incerto domina committed;
sem efeitos iniciados, a reason é UnitIdentityReused. Nenhuma das visões pode
declarar esse ID como fresco. Não há ordering de collection na decisão.

Eligible significa somente que **a fronteira permite considerar uma nova unidade**.
Não autoriza dispatch, gasto, quota, availability, capability ou seleção de recurso.
Não altera Fixed/Preferred/Auto nem duplica o score/ResourceAllocator da B.
Retry/fallback de uma execução existente permanece exclusivamente do Scheduler.

### Privacidade e arquivos

Nenhuma entrada/saída/diagnostic do C1 aceita strings, generic payloads, prompts,
output do modelo, chain-of-thought, credenciais, headers ou corpos remotos.
IDs são numéricos locais; states/reasons são enums. Há Serialize para inspeção
sintética, sem Deserialize que possa contornar os construtores de identidade.
O módulo depende apenas de serde e dos traits padrão; nenhuma marca/provider,
adapter, catálogo econômico, database ou manager é recebido.

Arquivos desta candidata:

- criado `src-tauri/src/cognitive_resources/handoff.rs`;
- criado `src-tauri/src/cognitive_resources/handoff_tests.rs`;
- alterado `src-tauri/src/cognitive_resources/mod.rs` (declaração/export/testes);
- alterado este documento (estado e registro exclusivo C1).

### Gate sintético C1

25 testes focados cobrem A–K, incluindo fences em todos os lifecycle states,
sucessor versus replay, source incompleto sem lavagem por novo ID, cancelamento,
contradições, cross-task, IDs iguais/anteriores, vínculo exato do checkpoint,
boundary sem predecessor/sem confirmação, limites zero/MAX/MAX+1/u64::MAX,
incremento sem overflow, repetição sem mutação, HashMaps auxiliares em ordens
distintas, JSON sanitizado e ausência de dependência comercial/runtime. O gate
de reutilização de ID também cobre 63 combinações de lifecycle/fences das duas
visões contraditórias, preservando o fence mais conservador em ambos os reports.
Não são introduzidas strings/IDs textuais; vazio ou caracteres inválidos não
possuem representação no contrato numérico.

A matriz exaustiva avalia **3.696 combinações**: predecessor ausente ou 21 estados
lifecycle/efeitos, 21 estados solicitados, quatro boundaries e cancelamento
true/false. Somente os dois casos seguros descritos acima recebem Eligible.
As regressões da B são executadas separadamente e na suíte completa (gate L).

### Gates locais da candidata

| Gate | Resultado |
|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources::handoff_tests` | 25 aprovados, 0 falhas, 0 ignorados no código final |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources` | 246 aprovados, 0 falhas: C1 25 + B1 46 + B2 96 + B3 41 + A 38 |
| `cargo test --manifest-path src-tauri/Cargo.toml b4_ -- --test-threads=4` | 54 aprovados, 0 falhas |
| `RUST_TEST_THREADS=4 cargo test --manifest-path src-tauri/Cargo.toml` | Exit 0; 841 aprovados, 0 falhas, 2 ignorados, 283,02 s; main/doc-tests sem falhas |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0, repetido após o refinamento de mesmo-ID |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Exit 1 por drift legado em 32 arquivos; log final byte a byte idêntico ao da main antes de editar |
| `/home/sam/.cargo/bin/rustfmt --edition 2021 --check src-tauri/src/cognitive_resources/*.rs` | Exit 0; módulo completo, inclusive C1 |
| `git diff --check` e `git diff --cached --check` | Exit 0 |

O drift global de formatação permanece dívida fora do C1. Não foram reformatados
arquivos legados fora do escopo para maquiar o gate. Warnings observados: 12 da
biblioteca e um de fixture, todos fora de C1, sem supressão. Nenhum teste focado
falhou. Não foram executados gates que requerem quota/API comercial real.

A suíte completa usa quatro threads, como os gates anteriores da B, para limitar
contenção do SecretStore. Não houve falha/flakiness observada nem alteração de
timeout/fixture legado. A primeira execução, antes do refinamento de mesmo-ID,
também passou (840 aprovados, 0 falhas, 2 ignorados); a suíte completa foi repetida
sobre o código final com o teste adicional. Os ignorados preexistentes são
`real_app_server_handshake` e `manual_final_codex_agent_bridge_gate`; não foram
habilitados e nenhum teste consumiu API/quota comercial real.

### Limitações para auditoria independente

Refinamento durante revisão local: mesmo-ID já bloqueava handoff, mas a visão
declarada NotStarted podia reportar replay NotApplicable. Corrigido antes do
commit para Forbidden nas duas visões, com teste dedicado e repetição dos gates.
Esta revisão local não é auditoria independente.

- O Core deverá fornecer fatos verdadeiros e completos. O evaluator valida
  coerência dos fatos recebidos; não prova histórico, unicidade global, freshness
  do recibo, dependências de um grafo ou atomicidade de dispatch/cancelamento.
  IDs não podem ser reciclados para esconder trabalho/efeitos anteriores.
- `CheckpointId` não é checkpoint persistido e ConfirmedCompletion não afirma
  SQLite commit. Sem ledger histórico nesta etapa, a monotonicidade de receipts
  é expressa pelo ID/next; detectar recibos históricos obsoletos depende de uma
  futura camada de continuidade. Nenhuma parte de C2 foi necessária ou iniciada.
- O fence de efeitos é conservador e agregado, sem mapa de efeitos nem autorização
  para executá-los. Um sucessor elegível não pode repetir efeitos do predecessor.
- A futura integração deverá preservar output observado, allocation fixada,
  autorização econômica e nova checagem de cancelamento antes de dispatch.
  O resultado puro não é um token de autorização durável.
- Nenhuma auditoria independente foi executada nesta rodada. Esta candidata não
  aprova C1 nem encerra LR-8.5C e não libera implementação de C2/C3/C4.


## Auditoria independente do C1 — 06/10/2026

**Resultado: PASS técnico. Nenhuma FIX obrigatória antes do C2.**

A auditoria comparou `main@0024fec735e3f6cb2461dbdeba8fa6aa4be32d32`
com a candidata `ed5f65a90618a93f30582ced343400ee2761e296`. A branch está
exatamente um commit à frente da baseline e altera somente os quatro arquivos
declarados: `handoff.rs`, `handoff_tests.rs`, o export de
`cognitive_resources/mod.rs` e este documento. Scheduler, TaskGraph,
persistência, migrations, adapters e UI não foram tocados.

### Achados

1. **Fronteira arquitetural preservada.** `can_handoff` é puro, síncrono,
   clock-free e não recebe ResourceAllocator, Scheduler, provider, database,
   rede ou policy econômica. C1 não criou um segundo sistema de fallback.

2. **Allocation de unidade iniciada permanece congelada por contrato.**
   `Running`, `PartialOutputObserved` e todos os estados terminais da unidade
   solicitada são bloqueados como nova unidade. Não existe caminho que converta
   output parcial em unidade fresca.

3. **Handoff e replay estão corretamente separados.** Um predecessor
   `Completed + Committed` pode liberar um sucessor distinto, enquanto o replay
   do predecessor continua explicitamente `Forbidden(EffectCommitted)`. Isso
   evita bloquear progresso legítimo sem enfraquecer anti-duplicação.

4. **Estado incerto falha fechado.** `UnknownOrInFlight` bloqueia continuidade
   automática a partir do predecessor e nunca concede replay. Reutilização do
   mesmo `ExecutionUnitId` preserva o fence mais conservador nas duas visões.

5. **Vínculo de boundary é explícito.** `ConfirmedBeforeStart` precisa apontar
   para a unidade solicitada e não aceita predecessor; `ConfirmedCompletion`
   exige predecessor e checkpoint ligado ao ID exato desse predecessor.
   Cross-task e sequence não crescente são rejeitados.

6. **Cancelamento tem precedência.** Um cancelamento observado bloqueia a próxima
   unidade mesmo quando os demais fatos seriam elegíveis. O próprio documento
   corretamente registra que uma nova checagem atômica/fresca antes do dispatch
   pertence à integração futura, não ao evaluator puro.

7. **Determinismo e privacidade estão adequados ao C1.** A decisão usa apenas
   tipos numéricos/enums, sem rationale textual livre, prompt, output, secret,
   header ou payload genérico. O teste de ordenação auxiliar não revela
   dependência de HashMap.

8. **Cobertura é suficiente para o contrato atual.** Além dos casos A–K, a matriz
   exaustiva percorre lifecycle, effect state, boundary, predecessor e
   cancellation; os testes específicos cobrem same-ID, cross-task, sequence,
   checkpoint mismatch, limits e reason precedence.

### Limitação aceita e transferida ao C2

`CheckpointId` e `ConfirmedCompletion` são, no C1, **fatos recebidos em
memória**, não prova de commit durável, freshness, unicidade histórica ou
atomicidade com dispatch. Isso é coerente com o escopo do C1 e está claramente
documentado. O C2 deverá transformar essa afirmação em checkpoint/provenance
persistível e verificável sem tratar o resultado puro do C1 como capability
token ou autorização durável.

O fence de efeitos também permanece agregado por unidade. C1 prova anti-replay da
unidade/estado recebido; identificação persistente de efeitos, resume/restart e
proteção contra repetição após crash pertencem ao C2/C4.

### Gates

Os resultados reportados são coerentes com o diff auditado: 25 testes focados,
246 em `cognitive_resources`, regressões B1/B2/B3/B4 verdes, suíte Rust com
841 aprovados / 0 falhas / 2 manuais ignorados, `cargo check` e
`git diff --check` aprovados. O `cargo fmt --check` global permanece
vermelho por drift preexistente em 32 arquivos; o módulo
`cognitive_resources` alterado passa `rustfmt --check`, portanto o drift
global não bloqueia este C1.

**Decisão:** C1 aprovado para servir de base ao C2. Este PASS não aprova C2/C3/C4,
não encerra LR-8.5C e não autoriza merge da branch neste checkpoint.


## C2 — implementação candidata (06/10/2026)

**C2 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente.**

C1 continua **PASS**, conforme auditoria do commit
`7407b0f8d2a530d8dcc5a13fcfd2e8faf9b357b0`. Esta implementação parte dessa
baseline limpa e sincronizada com `origin/lr-8.5c-safe-handoff`; `main` e
`origin/main` foram confirmadas em `0024fec735e3f6cb2461dbdeba8fa6aa4be32d32`.
Os registros anteriores de C1 são históricos; C3/C4 não foram iniciados e
LR-8.5C permanece sem PASS global.

### Contratos e fronteira de confiança

- `CognitiveCheckpoint`: proposta validada de gravação, ainda sem durabilidade.
  Recebe fatos confirmados pelo Core, exige `Completed`, identidade exata da
  unidade e `ConfirmedCompletion` identificando o mesmo `CheckpointId`.
  Não aceita `HandoffDecision` como autorização, nem promove output parcial,
  running, cancelamento, failure ou lifecycle incerto a conclusão.
- `CheckpointRecord`: receipt retornado apenas após commit bem-sucedido ou
  leitura verificada de registro committed. Contém a proposta e timestamp factual.
  Seu `replay()` reutiliza o evaluator C1 e sempre retorna `Forbidden`.
- `CheckpointRepository::commit/lookup`: armazenamento e consulta factuais,
  sem dispatch, reallocation, ranking, fallback ou execução do sucessor.
- `CheckpointLoadResult`: `Committed`, `Absent`, `HistoryWithoutCheckpoint`
  com estado terminal observado, ou `Invalid` com `CheckpointError` estruturado.
  Ausência/erro nunca equivalem a `NotStarted` ou autorização de replay.
- `TaskPolicySnapshot`: cópia imutável dos DTOs existentes
  `CognitiveRolePolicy` e, somente em Auto, `CognitiveRoleAllocationPolicy` da B4.
  Reutiliza os validadores da B. Fixed/Preferred permanecem independentes da
  policy econômica. Não há score ou novo allocator no checkpoint.
- Allocation: a identidade completa existente `AllocationVariant`, incluindo
  resource/access path/billing domain/model/effort efetivamente observados.
  DTO privado de leitura reutiliza os validadores de IDs da B, sem habilitar
  deserialização de agregados C1/B.
- `CheckpointProvenance`: fonte `RootTask` ou `TaskGraphSubtask` com ID público
  local e `RuntimeId`. Runtime deve pertencer à policy capturada; em modos
  explícitos model/effort devem corresponder ao target executado. Auto admite
  variante distinta, sem inferir disponibilidade, custo, quota ou autorização.

Não existia identidade persistida de snapshot: as rows de Settings são mutáveis,
e B4 captura snapshots de execução em memória. Por isso o ledger mantém uma
cópia por `(root_task_id, role)` e rejeita alterações dessa cópia durante a tarefa.
O chamador Core deve fornecer o snapshot capturado para a tarefa; esta API nunca
recaptura Settings durante commit/reload. A captura conjunta de papéis já existe
na B4; sua conexão à gravação fica para etapa posterior.

`UnknownOrInFlight` pode ser registrado para preservar a evidência incerta de
uma unidade cognitivamente concluída. **Committed do ledger significa gravação
confirmada, não sucesso dos efeitos.** Nesse caso `CheckpointRecord::boundary()`
retorna `Unknown`, replay continua `EffectUnknownOrInFlight` e o receipt não pode
ser predecessor/dependência segura. Não existe operação de resolver/substituir
esse fence. `Committed` de efeito sobrevive ao reload com replay proibido.

### Modelo persistido e migration 014

O schema inspecionado terminava em 013. A nova migration
`014_cognitive_checkpoints.sql` acrescenta somente duas tabelas, inicialmente
vazias, sem modificar tabelas/rows atuais:

| Tabela | Identidade e conteúdo |
| --- | --- |
| `checkpoint_task_policies` | PK `(root_task_id, role)`; JSON limitado do snapshot existente |
| `cognitive_checkpoints` | PK `(root_task_id, unit_sequence)`; checkpoint sequence, role/FK ao snapshot, source kind/key, effect fence, estado committed, envelope validado e timestamp SQLite |

Há um único checkpoint **final** por unidade: qualquer outro checkpoint sequence
nessa unidade, crescente ou regressivo, conflita. Não há atualizações de checkpoint
intermediário nesta etapa. Sequences de unidades independentes podem ter gaps e
chegar fora de ordem; vínculos de predecessor/dependência exigem mesma root e
sequence de unidade estritamente menor. O checkpoint sequence de cada referência
precisa coincidir exatamente com o receipt armazenado.

A constraint UNIQUE `(root_task_id, source_kind, source_key)` impede associar a
mesma fonte terminal a duas unidades. `RootTask` identifica a conclusão da tarefa
inteira (um receipt); `TaskGraphSubtask` identifica a conclusão de uma subtarefa
atual (um receipt por ID). Uma eventual granularidade diferente exigirá contrato
explícito; não foi inferida a partir de event sequences ou IDs textuais.

Não há FK para `task_records`: essa tabela só recebe tarefas terminais e uma
unidade pode concluir antes da root. Quando presentes, `task_records` e
`task_subtask_records` são consultados no mesmo snapshot e precisam concordar
com a fonte específica e seu runtime. Subtarefa completed permanece concluída
mesmo sob root cancelled/failed; esse fato não autoriza sucessor. Root terminal
sem a subtarefa referida é contradição. Histórico terminal sem checkpoint é
exposto como evidência, nunca convertido em unidade não iniciada.

Os campos JSON são DTOs fechados com enums/códigos estáveis (`completed`,
`confirmed_completion`, os três fences), não dumps ou reasons de texto livre.
O timestamp vem do `strftime` SQLite já usado pela infraestrutura e é validado
como UTC/RFC3339 com milissegundos. `committed_at` registra o instante da
inserção SQLite na transação confirmada, não o instante físico de fsync; não
implica freshness, monotonicidade ou clock global.

### Commit, idempotência, integridade e concorrência

1. Validar linkage/lifecycle/snapshots/contexto em construção, sem criar receipt.
2. Exigir conexão em arquivo, autocommit, FK habilitada, synchronous FULL/EXTRA e
   journal DELETE/TRUNCATE/PERSIST/WAL. Queries C2 qualificam explicitamente
   `main`, impedindo shadowing por tabelas TEMP ou outro schema. Modos não duráveis e transações externas
   são rejeitados, sem mudar as configurações do database existente.
3. Abrir `BEGIN IMMEDIATE`, verificar histórico e referências committed, comparar
   ou inserir snapshot imutável e comparar ou inserir checkpoint.
4. Recarregar e validar os dados gravados ainda dentro da transação.
5. Retornar receipt somente depois do `COMMIT` bem-sucedido. Erro desfaz ambas
   as gravações; há teste de falha real de commit por FK deferred.

Uma gravação semanticamente idêntica retorna o receipt original, inclusive
`committed_at`, sem atualização. Alteração de qualquer campo ou identidade
incompatível retorna conflito e não sobrescreve. Os writers são serializados pelo
SQLite; constraints acompanham a transação. Duas tentativas idênticas convergem;
em conflito, a primeira transação confirmada conserva seu conteúdo e a outra
falha. A ordem de chegada não confere autoridade nem permite dois conteúdos.

Lookup usa transação de leitura própria e valida JSON, IDs, enums, limites,
identidade indexada, snapshot, timestamp, histórico e cadeia de referências.
Registro inválido não se torna boundary segura; erros de infraestrutura também
falham fechado. Não há autenticação criptográfica contra edição maliciosa coerente
do arquivo SQLite ou garantia além da durabilidade fornecida por SQLite/OS.

### Shared Cognitive State e limites

`HandoffContext` contém **somente até 32 `CheckpointId`s de dependências concluídas**,
ordenados canonicamente, sem duplicatas. Predecessor é uma referência separada.
Todas as referências devem existir, estar verificadas e não possuir fence incerto.
A verificação transitiva é iterativa, com limite de 256 receipts distintos;
exceder o limite impede afirmar uma boundary segura.

Envelope serializado de checkpoint: máximo **8.192 bytes**; snapshot de policy:
máximo **16.384 bytes**. Há limites em construção/schema/leitura, inclusive antes
de deserializar JSON corrompido superdimensionado. IDs numéricos reutilizam o teto
C1 `9_007_199_254_740_991`, excluindo zero. Subtask ID: 1–64 bytes ASCII
alfanuméricos/`_`/`-`, igual ao contrato TaskGraph atual. IDs da allocation/runtime
reutilizam bounds e sintaxe da B. Model IDs no snapshot também precisam ser IDs
públicos válidos da B; policy legada com label fora dessa sintaxe é rejeitada nesta
API candidata, sem alterar sua execução atual.

Não há prompt, output textual, reasoning, secrets, credenciais, headers, cookies,
corpos HTTP, texto livre de contexto, caminhos arbitrários ou memória global.
Referências provam conclusão/provenance; não fingem conter resultados textuais que
TaskGraph hoje mantém apenas em memória. Não foi adicionado um storage de resultados
ou artefatos sem necessidade demonstrada.

### Arquivos alterados

- `src-tauri/migrations/014_cognitive_checkpoints.sql`.
- `src-tauri/src/persistence/checkpoints.rs`.
- `src-tauri/src/persistence/checkpoints/contracts.rs`.
- `src-tauri/src/persistence/checkpoints/tests.rs`.
- `src-tauri/src/persistence/migrations.rs`: aplicar 014 e ceiling de versão 14.
- `src-tauri/src/persistence/mod.rs`: declaração do módulo.
- `src-tauri/src/persistence/tests.rs`: versões esperadas de migration.
- `src-tauri/src/cognition/allocation_policy/tests.rs`: versões e fixture de upgrade
  compatíveis com 014; gates econômicos/routing preservados.
- `src-tauri/src/cognition/policy.rs`: somente versão esperada no teste de upgrade.
- Este documento.

Rustfmt foi aplicado aos arquivos Rust alterados, incluindo o drift prévio de
`persistence/mod.rs` e dois trechos de `persistence/tests.rs`. Essas alterações de
formatação não mudam comportamento.

### Gates C2

A primeira suíte completa registrou 878 aprovados, 1 falha e 2 ignorados: o teste
`cognition::policy::tests::v8_to_v11_preserves_existing_roles_and_adds_worker_defaults`
esperava schema 13 e recebeu 14 (`left: 14 / right: 13`). A FIX atualiza somente
a versão esperada nesse teste de upgrade. Não foi flakiness nem mudança de policy.

A revisão da durabilidade também fixou as queries do ledger/histórico no schema
`main`, com teste de shadowing por tabelas TEMP. Não há FIX conhecida pendente;
a implementação candidata ainda requer auditoria independente.

Todos os gates abaixo foram executados localmente, sem quota/API real. A suíte
completa usa quatro threads, como na baseline C1/B, para limitar contenção do
SecretStore. Após a FIX da expectativa de schema, nenhuma falha/flakiness foi
observada na rodada final. Os dois testes manuais de app-server/inferência real
continuam ignorados pela configuração existente; não foram executados.

| Gate / comando | Resultado final |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml c2_ -- --test-threads=4` | 39 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml c1_ -- --test-threads=4` | 25 aprovados, 0 falhas; C1 intacto |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources:: -- --test-threads=4` | 246 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources::allocation_tests:: -- --test-threads=4` | B1: 46 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml cognitive_resources::scoring_tests:: -- --test-threads=4` | B2: 96 aprovados, 0 falhas |
| `cargo test --manifest-path src-tauri/Cargo.toml b3_ -- --test-threads=4 --skip cognition::allocation_policy::` | B3: 41 aprovados, 0 falhas; exclui apenas nome de teste B4 que contém `b3_` |
| `cargo test --manifest-path src-tauri/Cargo.toml b4_ -- --test-threads=4 --skip persistence::checkpoints::` | B4: 54 aprovados, 0 falhas; exclui apenas nomes C2 que contêm `b4_` |
| `cargo test --manifest-path src-tauri/Cargo.toml persistence:: -- --test-threads=4` | 71 aprovados, 0 falhas; inclui migrations/ledger/histórico |
| `cargo test --manifest-path src-tauri/Cargo.toml cognition::policy::tests::v8_to_v11_preserves_existing_roles_and_adds_worker_defaults -- --test-threads=4` | Upgrade corrigido: 1 aprovado, 0 falhas |
| `RUST_TEST_THREADS=4 cargo test --manifest-path src-tauri/Cargo.toml` | 880 aprovados, 0 falhas, 2 ignorados; binário/doctests sem falhas |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Exit 0; 12 warnings preexistentes da lib |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Exit 1 pelo drift preexistente: baseline 32 arquivos, final 30; zero novo drift e diffs dos arquivos restantes idênticos à baseline |
| Rustfmt direto em todos os 8 arquivos Rust alterados | Exit 0 |
| `git diff --check` | Exit 0 |

Rustfmt direto: `/home/sam/.cargo/bin/rustfmt --edition 2021 --config
skip_children=true --check` com os oito arquivos Rust listados acima explicitamente.
`skip_children` evita percorrer módulos legados intactos; contracts e testes C2
foram incluídos separadamente. A suíte test também emite o warning preexistente
de campos não lidos em IdentitySnapshot.

Cobertura A–N: identidade exata; durabilidade somente após write/commit;
rollback das duas tabelas e falha real de commit; reopen sem replay; idempotência
com timestamp original; conflitos de conteúdo/source/policy; unidade/root
incompatíveis; limites e sequences regressivas; preservação de fences incertos e
committed; corrupção de JSON/IDs/enums/linkage/policy/timestamp; whitelist de
provenance; bounds de contexto/envelope; ausência de payload genérico e de binding
comercial. Também há gates de concorrência, evidência terminal contraditória,
dependências transitivas/limite de verificação, shadowing TEMP, upgrade 13→14 e
rollback/retry da migration. O/P são os gates C1 e B acima.

### Limitações e pontos para auditoria

- C2 é ledger candidato, sem conexão ao dispatch/lifecycle real. O Core continua
  responsável por confirmar os fatos fornecidos; persistência não prova por si só
  que um request ocorreu. C1 permaneceu intacto e sem dependência SQLite.
- Antes de futura integração, auditar captura estável da policy, identidade real
  da allocation, associação unidade/fonte e confirmação Core; verificar o limite
  de 256 receipts para os fluxos concretos. Nenhuma integração C3 foi iniciada.
- O seed atual de TaskRegistry considera apenas `MAX(task_records.task_id)`.
  Quando o ledger passar a receber unidades de tarefas ainda não terminais,
  a futura integração deverá impedir reutilização de root IDs após restart,
  considerando também essas identidades. O seed/runtime não foi alterado em C2.
- Lookup após reopen reconhece Unit A committed e proíbe replay de A; não despacha
  Unit B. Ausência/contradição/incerteza não iniciam unidade alguma.
- Não há resultados textuais persistidos de Worker, pausa/resume de produto,
  resolução de efeitos incertos, Tool Runtime, idempotency keys externas ou
  testes com queda física de energia. As provas usam arquivos SQLite locais,
  rollback e reopen; dependem das garantias usuais do SQLite/OS.
- Receipts não são capability tokens de spend/availability/cancelamento. A decisão
  futura exige novo fato de cancelamento e os gates do Core/Scheduler/LR-8/B.
- C1 continua PASS; C2 aguarda auditoria independente. C3/C4 não iniciados;
  LR-8.5C não encerrada.


## Auditoria independente do C2 — 06/10/2026

**Resultado: PASS técnico. Nenhuma FIX obrigatória no ledger C2.**

A auditoria comparou o C1 auditado em
`7407b0f8d2a530d8dcc5a13fcfd2e8faf9b357b0` com a candidata C2
`df2f86a0a4e00dad5c599018ce62e4ac85641325`. O diff contém um único
commit e os dez arquivos declarados. Não há mudança em Scheduler, TaskGraph
runtime, adapters, UI ou dispatch; C3 não foi antecipado.

### Achados

1. **Receipt somente após COMMIT.** `CheckpointRepository::commit` valida
   histórico/referências, abre `BEGIN IMMEDIATE`, insere snapshot/checkpoint,
   faz read-back ainda dentro da transação e retorna `CheckpointRecord`
   somente depois de `tx.commit()` bem-sucedido. O teste de FK deferred
   provoca falha real no COMMIT e confirma rollback das duas tabelas.

2. **Idempotência e conflito estão corretamente serializados.** A identidade
   de unidade possui um único checkpoint final. Retry semanticamente idêntico
   devolve o receipt original e preserva `committed_at`; conteúdo divergente
   não sobrescreve. PK/UNIQUE + transação `Immediate` evitam check-then-insert
   concorrente fora do SQLite. Os testes concorrentes confirmam uma única row
   e um único vencedor em conflito.

3. **Ausência nunca vira frescor.** `lookup` diferencia `Committed`,
   `Absent`, `HistoryWithoutCheckpoint` e `Invalid`. Todos os casos não
   comprovadamente committed continuam com replay bloqueado. Evidência terminal
   sem checkpoint não é convertida em `NotStarted`.

4. **Effect fence sobrevive ao restart.** `Committed` e
   `UnknownOrInFlight` são persistidos e revalidados. Um ledger committed com
   efeito incerto não produz boundary segura: `CheckpointRecord::boundary()`
   devolve `Unknown`, e referências a esse receipt não podem liberar
   predecessor/dependência.

5. **Shared Cognitive State ficou deliberadamente mínimo.** O contexto contém
   somente referências de checkpoints concluídos, com limite 32, ordenação
   canônica, rejeição de duplicata e verificação transitiva bounded em 256
   receipts. Não foi criado dump de prompt/output/reasoning nem storage
   especulativo de Worker results.

6. **Policy snapshot preserva a semântica da B4.** O ledger mantém cópia
   imutável por `(root_task_id, role)`, valida Fixed/Preferred sem allocation
   econômica e exige a policy B4 em Auto. Settings mutáveis posteriores não
   reescrevem o snapshot da tarefa.

7. **Provenance e allocation são persistíveis sem marcas comerciais.** O
   runtime precisa existir entre os targets autorizados da policy; em
   Fixed/Preferred model/effort precisam coincidir com o target. Em Auto, a
   variante efetiva pode diferir do target-base, como exige a expansão de
   variantes da B. A associação factual da variante ao dispatch real permanece
   responsabilidade da futura integração C3.

8. **Corrupção falha fechado.** JSON malformado/extra, IDs, lifecycle,
   boundary, effect indexado, policy, timestamp e referências inconsistentes
   não são promovidos. Queries críticas qualificam `main`, evitando shadowing
   por TEMP tables.

9. **Migration 014 é pequena e atomicamente aplicada.** Ela cria apenas
   `checkpoint_task_policies` e `cognitive_checkpoints`, parte vazia sobre
   schema 13, preserva histórico/policy B4 e mantém `user_version=13` quando a
   migration falha antes do commit.

10. **C1 continua preservado.** O C2 depende dos tipos/fences do C1, mas C1 não
    passou a depender de SQLite. Não há replay permitido, handoff mid-stream,
    ranking, auto-spend ou execução de sucessor.

### Pré-condições obrigatórias antes do C3 real

O C2 está aprovado como primitive de persistência, mas **C3 não pode conectar
este ledger ao dispatch sem resolver estes pontos de integração**:

1. **Identidade root após restart.** Hoje o startup semeia `TaskRegistry`
   somente com `MAX(task_records.task_id)`. Como o ledger C2 pode conter
   checkpoints de uma tarefa ainda sem row terminal em `task_records`, um
   crash/restart poderia reutilizar esse `root_task_id`. Antes de qualquer
   integração C3, o seed precisa considerar também o maior
   `cognitive_checkpoints.root_task_id` (ou uma fonte canônica equivalente),
   com teste de crash/restart. Um ID antigo jamais pode nomear uma nova tarefa.

2. **Fonte da allocation.** C3 deve construir `AllocationVariant` a partir da
   allocation efetivamente selecionada/pinada para a unidade, não de input
   arbitrário do caller. O ledger valida formato/coerência disponível, mas não
   prova sozinho que aquela variante foi realmente executada.

3. **Captura da policy.** O `TaskPolicySnapshot` persistido deve vir do
   snapshot imutável capturado para a tarefa, e não de uma releitura das
   Settings no momento do checkpoint.

4. **Atomicidade de cancelamento/dispatch.** Um receipt C2 não é token de
   autorização. C3 deve fazer nova checagem de cancelamento e gates
   Scheduler/LR-8/B imediatamente antes de despachar a próxima unidade.

Esses pontos são deliberadamente externos ao C2 e não justificam acoplar o
ledger ao lifecycle nesta rodada. Entretanto, são gates de integração, não
dívidas opcionais.

### Observação sobre continuidade após restart

O C2 prova "Unit A está committed e não pode ser automaticamente repetida" após
reopen. Ele **não prova ainda que todo o resultado cognitivo necessário para
executar Unit B sobrevive a um crash**: os outputs textuais de Worker continuam
fora deste ledger. Isso é coerente com o gate C2 solicitado, que não despacha B.
C3/C4 deverão demonstrar a disponibilidade do contexto realmente necessário
antes de prometer resume funcional após restart; ausência desse contexto deve
pausar/falhar fechado, nunca provocar replay de A.

### Gates auditados

A cobertura reportada é coerente com o código e o escopo: 39 testes C2, 25 C1,
regressões B1/B2/B3/B4, 71 testes de persistência/migrations e suíte Rust final
com 880 aprovados, 0 falhas e 2 manuais ignorados. `cargo check` e
`git diff --check` estão verdes. O `cargo fmt --check` global continua
afetado pelo drift legado; todos os oito arquivos Rust alterados passam
`rustfmt --check` direto e nenhum novo drift foi introduzido.

**Decisão:** C2 aprovado para servir de base ao C3, condicionado às
pré-condições de integração acima. Este PASS não aprova C3/C4, não encerra
LR-8.5C e não autoriza merge da branch neste checkpoint.


## C3 — Implementação candidata: Boundary Reallocation & TaskGraph Bridge

Data: **06/10/2026**. Branch única: `lr-8.5c-safe-handoff`.
Baseline local/remota confirmada, limpa, em
`8af97050712bd0b13679b3dfa2625185375ebe7b` (auditoria C2 PASS).
`main`/`origin/main` preservadas em
`0024fec735e3f6cb2461dbdeba8fa6aa4be32d32`.
C1/C2 continuam PASS; esta implementação não constitui auditoria independente.

### Recovery de identidade raiz

`persistence::task_history::max_id` passa a ser o high-water mark canônico:
consulta única em `main.task_records` **e** `main.cognitive_checkpoints`.
Inclui roots comprometidos antes do histórico terminal, sem inferir validade de
boundary a partir desse máximo. Falha de leitura/ID acima do limite impede a
inicialização. A consulta obrigatória ocorre no composition root, antes de
construir o runtime/iniciar Summary ou expor dispatch; não depende do sucesso de
uma abertura opcional anterior do banco.

`TaskRegistry::seed_next_id` usa `fetch_max`, conservando monotonicidade mesmo
com seed atrasado. O teto existente `2^53−1` permanece; seed além desse teto
fecha o allocator por esgotamento, sem overflow. O teste reabre SQLite com
checkpoint root=417 e histórico máximo=416 e obtém foreground=418,
background=419, incluindo tentativa de seed regressivo. TEMP tables não
substituem as fontes duráveis.

### Policy por tarefa e allocation por unidade

B4 `RoleRuntimePolicy` conserva também o DTO econômico original capturado na
mesma read transaction de routing/allocation. O preflight real de TaskGraph
continua capturando Orchestrator/Worker juntos e constrói `TaskPolicySnapshot`
Worker uma vez, antes do Planner/Workers. Fixed/Preferred mantêm allocation
None e independência da row econômica. Não há reload de Settings ao completar
uma unidade ou preparar a próxima; o runtime econômico deriva daquele mesmo
DTO validado, e cada checkpoint reutiliza o snapshot exato.

B3 `AutoRouteEntry` passa a conservar `ranked.variant` junto de target/score.
`Scheduler::ranked_provider_allocations` usa a mesma `resolve_provider_chain`
e devolve `PinnedProviderAllocation`, com campos privados e construção apenas
no Scheduler. Em Auto a identidade é a variante realmente selecionada pelo
ResourceAllocator, incluindo access path/billing domain/model/effort. Em
Fixed/Preferred é o binding local `provider_runtime` da rota explícita, sem
consulta ao catálogo/policy econômica nem mudança de ordem.

Worker recebe `PreparedUnit` e clona **somente seu target pinado** para o request
Fixed já usado pela D3. O checkpoint usa a variante do mesmo pin e verifica o
provider retornado/ledger de providers usados. Não escolhe a primeira variante
da config, não aceita allocation detached do caller e não reconstrói o ranking
após a resposta.

### Bridge e fronteiras duráveis

`cognition/task_graph_handoff.rs` mantém os claims de unidades e receipts desta
execução. IDs numéricos avançam na ordem de preparação, não na posição arbitrária
do plano. Cada step só pode ser preparado uma vez; a bridge não recria unidade
falhada/cancelada e não repete commit já acknowledged. Retries pertencem
exclusivamente ao Scheduler, dentro do mesmo target/model/effort.

Antes de preparar uma unidade pronta, a bridge verifica estado Pending/ready,
targets da policy original, resultados/dependências validados pela D3 e receipts
C2 associados exatamente aos steps. Revalida os receipts no SQLite por lookup,
incluindo identidade/sequence/conteúdo/policy/provenance. Ausência, corrupção ou
receipt diferente não liberam dependentes. Usa `can_handoff` C1 para **cada**
predecessor, com boundary do receipt e cancelamento atual. Uma unidade sem
dependências recebe ConfirmedBeforeStart; row no banco não substitui C1.

Auto executa uma nova decisão B3/B1/B2 por unidade pronta. Captura os fatos
operacionais LR-8 atuais na engine compartilhada; não cria catálogo novo nem
expande os targets autorizados. A primeira unidade de cada wave usa o winner
atual; peers independentes conservam a distribuição D3 na cadeia recém-avaliada.
Fixed permanece explícito; Preferred preserva ordem e cursor D3, sem reordenação
econômica. A seleção não é autorização de execução: Scheduler conserva todos
os gates de resilience/rate/admission/accounting/retry/cancellation.

Após `run_worker` retornar sucesso já validado pelo Core, a bridge constrói C2
com source TaskGraphSubtask, allocation efetiva, policy original e referências
exatas aos checkpoints das dependências. O fence é NotStarted porque este
runtime aceita somente trabalho cognitivo e não executa efeitos externos.
Somente após `CheckpointRepository::commit` devolver receipt committed a D3
marca Completed, disponibiliza o resultado em memória e emite SubtaskCompleted.
Falha de commit impede a boundary e bloqueia dependentes; rollback C2 inclui a
policy. Não há write intermediário por retry/chunk/token.

O fluxo Worker existente foi extraído em `execute_workers`, utilizado pelo
TaskGraph real e pelos gates sintéticos, para testar a integração sem Planner
remoto/vault. Compilação PlanV1, budget conservador, consolidação e formato dos
resultados cognitivos permanecem os da D3.

### Cancelamento, paralelismo e falhas

Cancelamento é checado no loop da wave, em prepare (antes/depois do lookup e da
alocação), no Worker e pelo Scheduler na invocation/admission/transporte.
Cancelar em SubtaskCompleted, já depois de COMMIT, impede B e conserva A.
Cancelar no callback SubtaskStarted de B também produz zero requests B e nenhum
checkpoint B. SubtaskStarted descreve intenção/lifecycle local, não HTTP factual.
A precedence channel failure > cancellation e o terminal único permanecem nos
caminhos D3/TaskRegistry existentes.

`tokio::join!` continua executando até duas Workers independentes. Cada uma tem
pin/claim/receipt próprios; nenhum lock/connection SQLite atravessa transporte
ou await. I/O de checkpoint/lookup usa spawn_blocking bounded pelo busy timeout
existente. Selecionar B enquanto A já aguarda no provider não muda A. As waves
preservam o fail-fast existente: uma falha pode encerrar a wave/tarefa mesmo que
outra branch não dependa dela; não foi introduzido um novo executor de branches.

Códigos locais distinguem `handoff_checkpoint_write_failed`,
`handoff_checkpoint_mismatch`, `handoff_boundary_unsafe`,
`handoff_allocation_unavailable`, identidade/contexto inválidos e cancellation.
Falha depois de partial output não produz checkpoint, retry/handoff alternativo
ou sucessor. UnknownOrInFlight permanece inseguro mesmo em receipt committed.

### Provenance e schema

Sem migration nova: schema continua **14**, usando exclusivamente as duas tabelas
C2. Receipts permitem reconstruir predecessor/source allocation e destination
allocation pelas referências de dependências, sem persistir Worker output.
SubtaskStarted recebe IDs/allocation/reason C1 e transitions por predecessor,
com flags estruturadas resource/accessPath/model/effort; todas false representa
allocation unchanged. SubtaskCompleted recebe CheckpointId somente após COMMIT.
Os tipos TypeScript foram estendidos para esses campos aditivos; nenhuma nova
UI, policy editor ou ação de produto foi criada.

Nenhuma metadata nova contém prompt, output, reasoning, secrets, HTTP payloads,
preços ou timestamps inventados. Model/resource/effort continuam sujeitos aos
limites e labels públicos do C2. O resultado cognitivo real continua no mecanismo
D3 em memória; checkpoints não o substituem nem inventam contexto pós-crash.

### Arquivos

Criados:

- `src-tauri/src/cognition/task_graph_handoff.rs`;
- `src-tauri/src/cognition/task_graph_handoff/tests.rs`;
- `src-tauri/src/cognition/task_graph_runtime/c3_tests.rs`.

Alterados:

- `src-tauri/src/cognition/allocation_policy.rs` e seu `runtime_tests.rs`;
- `src-tauri/src/cognition/mod.rs`, `scheduler.rs`, `task_graph_runtime.rs`,
  `task_graph_worker.rs`;
- `src-tauri/src/cognitive_resources/provider_bridge.rs`;
- `src-tauri/src/lib.rs`, `luna/runtime.rs`, `luna/task.rs`;
- `src-tauri/src/persistence/task_history.rs` e `checkpoints.rs` (somente comentário
  de integração neste último; contratos/schema C2 preservados);
- `src/luna/types.ts`;
- este documento.

### Gates locais

Gates locais/determinísticos, sem API/quota comercial:

| Gate/filtro | Resultado |
|---|---:|
| C3 `c3_` (A–R) | **25 aprovados** |
| C1 `cognitive_resources::handoff_tests::` | **25 aprovados** |
| C2 `c2_` | **39 aprovados** |
| `cognitive_resources::` | 246 aprovados |
| B1 `allocation_tests::` | 46 aprovados |
| B2 `scoring_tests::` | 96 aprovados |
| B3 `provider_bridge::tests::` | 41 aprovados |
| B4 `b4_` | 56 aprovados (54 B4 + 2 C2 com B4 no nome) |
| `persistence::` (inclui checkpoints/migrations) | 71 aprovados |
| `task_graph` (inclui C3) | 49 aprovados |
| D3 `task_graph_runtime_tests::` | 13 aprovados |
| `luna::runtime::tests::` | 6 aprovados |
| `admission_tests::` | 18 aprovados |
| `rate_tests::` | 69 aprovados |
| `resilience_tests::` | 76 aprovados |
| `telemetry_tests::` | 33 aprovados |
| `lr8e_gate_tests::` | 14 aprovados |
| `scheduler::tests::` | 2 aprovados |
| Suíte Rust completa | **905 aprovados, 0 falhas, 2 ignorados** |

A suíte integral concluiu em **261,08 s**, sem falhas nem flakiness observada.
Os dois testes ignorados são gates manuais legados que exigem app-server real
ou autenticação/quota; não foram ativados. Binário e doc-tests também concluíram
sem falhas.

Os filtros se sobrepõem; as contagens descrevem os módulos/nomes realmente
executados. Admission inclui ainda os dois gates D3 de cap 1/cap 2 no grupo
TaskGraph. C1 usa filtro de módulo para excluir o gate C3 que cita C1 no nome.

Comandos: `cargo test --lib --manifest-path src-tauri/Cargo.toml <filtro> --
--test-threads=4` para cada linha focada; `RUST_TEST_THREADS=4 cargo test
--manifest-path src-tauri/Cargo.toml`; `cargo check --manifest-path
src-tauri/Cargo.toml`; `npm run typecheck`; `git diff --check`.

`cargo check` aprovado, com os 12 warnings de biblioteca da baseline; testes
mantêm o warning legado de IdentitySnapshot. `npm run typecheck` e
`git diff --check` aprovados. `rustfmt --check --edition 2021 --config
skip_children=true` aprovado diretamente nos **15 arquivos Rust alterados**.
`cargo fmt --check --manifest-path src-tauri/Cargo.toml` ainda retorna 1 por
drift global conhecido: baseline auditada tinha **30 arquivos**, candidata tem
**27**, todos já presentes na baseline, sem drift novo. Formatação local de
mod.rs/Worker/task_history resolveu três desses arquivos; não se reformatou o
restante do workspace.

Durante desenvolvimento, a primeira fixture de recovery associou DTO econômico
a Worker Preferred e recebeu InvalidPolicy; a fixture foi corrigida para
Fixed/None, sem alterar C2. Um assert esperava `no_provider`, mas o código
Scheduler existente era `provider_unavailable`; C3 passou a expor o código local
`handoff_allocation_unavailable` para falha de seleção antes de execução,
separando-a de falha remota. Os gates finais C3 passaram após a correção.
Não foram classificados como flakiness nem ocultados.

### Limitações para auditoria e C4

Esta candidata só conecta checkpoints às unidades Worker da execução atual;
não implementa resume pós-crash, novo dispatch de outputs ausentes, checkpoint
do Planner, Tool Runtime, resolução de efeito incerto ou pausa de produto.
Após restart os receipts e a proteção contra reutilização de root sobrevivem;
ausência de output textual durável não vira contexto fictício nem replay.

O catálogo de produção continua minimalista/Unknown. C3 reobserva LR-8 entre
unidades, sem discovery comercial, refresh de catálogo, pricing, novo auto-spend
ou alteração de policy dentro da tarefa. Persistem as limitações aprovadas B/LR-8:
snapshots operacionais não são globalmente atômicos; race entre ranking e
reservation pode encerrar localmente sem replan; SQLite/OS e instância única
mantêm as premissas C2. A verificação de receipts é factual, não uma assinatura
contra edição maliciosa coerente do banco.

A auditoria deve revisar especialmente o binding variante→invocation→checkpoint,
policy original, recovery antes de startup do runtime, gates após cancelamento,
interpretação de eventos como intenção e distribuição Auto por wave/paralelismo.
C1/C2 permanecem PASS; C4 não iniciada; LR-8.5C não encerrada.

**C3 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente.**


## Auditoria independente do C3 — 06/10/2026

**Resultado: PASS técnico. Nenhuma FIX obrigatória antes do C4.**

A auditoria comparou o C2 auditado em
`8af97050712bd0b13679b3dfa2625185375ebe7b` com a candidata C3
`c02f7df28b318f548c3c97d91828bfb1ad4326c6`. O diff contém um único
commit e os 17 arquivos declarados. O schema permanece 14 e C4 não foi
antecipado.

### Achados

1. **Recovery de identidade root está correto.** `task_history::max_id` agora
   usa o high-water mark de `main.task_records` e
   `main.cognitive_checkpoints`. O composition root executa essa leitura
   obrigatoriamente antes de construir o ProviderRuntime/Summary worker e antes
   de qualquer dispatch acessível. `TaskRegistry::seed_next_id` usa
   `fetch_max`, portanto seed atrasado não regride a sequência. IDs fora do
   teto falham fechado.

2. **Binding allocation → execução → checkpoint está forte.**
   `PinnedProviderAllocation` possui campos privados e é criado pelo Scheduler.
   O Worker clona exatamente o target desse pin e executa a unidade como
   `ProviderSelection::Fixed` com um único target, impedindo re-score durante
   a unidade. O checkpoint usa a `AllocationVariant` do mesmo pin e ainda
   valida o `result.provider_id` e `providers_used`. Não há reconstrução
   retrospectiva da variante após a resposta.

3. **Policy snapshot está realmente congelada por tarefa.**
   Routing e DTO econômico Worker são capturados na mesma transação de leitura
   B4 antes do Planner/Workers. O `TaskPolicySnapshot` é construído uma vez e
   carregado pela bridge durante toda a execução. Mudanças posteriores em
   Settings não expandem o universo da tarefa.

4. **Auto reavalia somente novas unidades.** Cada `prepare` chama novamente a
   engine compartilhada do Scheduler/B para fatos atuais. Depois do
   `PreparedUnit`, target/model/effort ficam imutáveis. Fixed continua Fixed;
   Preferred conserva ordem/cursor D3; Auto pode mudar resource/model/effort em
   boundary posterior.

5. **C1 e C2 são usados como gates reais, não decorativos.** Dependências são
   resolvidas por receipt exato, reconsultadas no SQLite e comparadas com o
   receipt em memória. Depois disso `can_handoff` valida cada predecessor.
   Receipt ausente, alterado, corrompido ou com `UnknownOrInFlight` não libera
   sucessor.

6. **Checkpoint precede conclusão visível da unidade.** O TaskGraph só executa
   `mark_completed`, insere o resultado em memória e emite
   `SubtaskCompleted` depois de `CheckpointRepository::commit` retornar
   receipt. Falha de checkpoint marca a unidade como falha e dependentes não
   são despachados.

7. **Cancelamento preserva precedência.** Há checks antes/depois de preparação,
   no Worker e no Scheduler. O gate que cancela dentro de
   `SubtaskStarted` demonstra zero request para a nova unidade; cancelar após
   `SubtaskCompleted` preserva o checkpoint anterior e bloqueia a sucessora.
   O receipt continua não sendo capability token de dispatch.

8. **Partial output não vira handoff.** A unidade preparada é claimed uma única
   vez. Falha depois de output não cria checkpoint e a camada C3 não recria a
   mesma unit em outro provider. Retry continua interno ao Scheduler e ao pin
   atual.

9. **Paralelismo D3 foi preservado.** Até duas unidades independentes continuam
   em `tokio::join!`, cada uma com pin/receipt próprios. Nenhuma conexão SQLite
   ou lock global atravessa transporte/await. Reallocation de uma unidade ainda
   não iniciada não altera o pin de uma irmã já em execução.

10. **A persistência terminal continua coerente com C2.** Um root pode falhar ou
    ser cancelado mantendo subtarefas já committed; C2 já aceita essa
    combinação. Falha do write terminal não permite reutilização posterior do
    root porque os checkpoints agora entram no high-water mark.

### Observação não bloqueante — explicabilidade do Auto

A segurança/provenance necessária ao handoff está presente: allocation de origem,
allocation de destino, receipts, boundary C1 e flags estruturadas de mudança podem
ser reconstruídos. Contudo, `PinnedProviderAllocation` conserva apenas
`target + variant`; o `score` existente em `ProviderRouteEntry` é
descartado. Além disso, o Worker suprime o evento `SchedulerEvent::Selected`
porque a execução já está corretamente pinada como Fixed.

Consequência: o C3 permite provar **qual allocation foi escolhida e qual boundary
permitiu a troca**, mas não preserva integralmente **por que o Auto ranqueou aquela
allocation** (por exemplo, seu score econômico no momento da escolha).

Isso não viola o contrato de safe handoff e não justifica reabrir C3. Fica como
item do **C4/final gate de observabilidade**: decidir se a provenance final deve
carregar um código/score sanitizado da seleção Auto, sem persistir
`ScoreBreakdown` excessivo nem confundir `HandoffReason` com rationale
econômico.

### Gates

Os gates reportados são compatíveis com o diff e com os invariantes auditados:
25 C3; 25 C1; 39 C2; B1/B2/B3 46/96/41; B4 56; 246
`cognitive_resources`; 71 persistence; 49 TaskGraph; 13 regressões D3; 6
TaskRegistry; regressões LR-8 admission/rate/resilience/telemetry/LR-8E e
Scheduler verdes. Suíte Rust final: 905 aprovados, 0 falhas e 2 manuais
ignorados. `cargo check`, typecheck, rustfmt dos 15 arquivos alterados e
`git diff --check` passaram. O drift global de rustfmt permanece legado e caiu
de 30 para 27 arquivos sem novo drift.

**Decisão:** C3 aprovado para servir de base ao C4. Este PASS não aprova C4,
não encerra LR-8.5C e não autoriza merge da branch neste checkpoint.


## C4 — implementação candidata, 06/10/2026

**C4 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente.**

Baseline: `lr-8.5c-safe-handoff@c07e2a8398531fcd40823f22d4973a986d5706d7`,
sincronizada com o remoto, workspace limpo antes da implementação; `main` e
`origin/main` preservadas em `0024fec735e3f6cb2461dbdeba8fa6aa4be32d32`.
C1, C2 e C3 continuam PASS. Esta implementação não é auditoria independente,
não encerra LR-8.5C e não autoriza merge.

### Continuação durável e ordem dos fatos

Migration **015**, posterior à 014 efetivamente existente, cria somente:

- `cognitive_continuations`: root indexado, manifest JSON v1, lifecycle,
  reason estruturado de pausa e geração monotônica do claim;
- `cognitive_continuation_units`: uma row para cada step do manifest, lifecycle
  `not_started` / `started` / `completed`, sequência da unidade, allocation e
  seleção reais, grant de calls/output, resultado útil e sequência do checkpoint.

O manifest conserva apenas objetivo/steps validados do PlanV1, dependências,
capabilities/instrução bounded, o `TaskPolicySnapshot` original, timeouts capturados,
referência de versão da identidade e provenance/usage factual do planner.
Risks/questions narrativas não participam do Worker e não são copiadas. O plano
reconstruído no resultado de resume conserva objetivo/steps e tem essas listas
vazias. Limites do PlanV1 e do TaskGraph continuam aplicados; dependências
repetidas são rejeitadas antes do dispatch. Manifest: **48 KiB** de JSON;
identidade: referência de até **128 bytes**, sem cópia do contexto da identidade.

~~~text
manifest + todas as rows not_started COMMIT
→ seleção C3/B sob snapshot original
→ started + allocation/selection/grant COMMIT
→ Worker/Scheduler (sem transação SQLite durante await do provider)
→ checkpoint C2 + resultado útil + completed COMMIT atômico
→ evento subtask_completed
→ C1 + cancelamento + nova seleção para sucessores
→ lifecycle durável final
~~~

A marca `started` é um fence conservador de intenção de dispatch, anterior ao
request. Não afirma que HTTP ocorreu; admission/accounting factual continuam
exclusivos do Scheduler. Após crash, essa marca sem completion não volta a
`not_started`, mesmo se o crash antecedeu HTTP. É suficiente para também impedir
replay de qualquer unidade que produziu output parcial: não há gravação por
chunk nem promessa de reconstruir chunks/estado privado do provider.

### Resultados e integridade

O resultado persistido é o `TaskGraphSubtaskResult` útil, tratado como dado não
confiável, e seus contadores estruturados existentes. Texto não vazio: **16 KiB
UTF-8**, tanto em streaming textual normal quanto após validação do resultado
estruturado. JSON: **100 KiB**, permitindo escaping do texto bounded. IDs,
provider, usage, grant, policy e receipt exatos são verificados. Não se copia
request/prompt remoto, histórico, memórias, headers, HTTP body, credenciais,
secrets ou campos privados de reasoning do adapter. Nenhum estado global de
memória foi criado.

`CheckpointRepository::commit_with` mantém o mesmo writer/validação C2 e a mesma
exigência de conexão durável, incluindo proteção contra transação externa. O
callback local participa do seu COMMIT: erro no resultado/marker faz rollback do
checkpoint e da policy row também. O receipt somente retorna após COMMIT.
Result retry idêntico retorna o receipt original; conteúdo conflitante falha e
não sobrescreve. O JSON dos checkpoints C2/C3 não mudou; continuam legíveis sem
novos campos obrigatórios.

Rows ausentes, JSON/UTF-8 inválido, root/unit/receipt/source/policy divergentes,
sequence/grant inválidos, evidência terminal contraditória e contexto necessário
sem resultado impedem dispatch. Uma row `not_started` precisa existir explicitamente
no manifest e no ledger, sem evidência contrária em checkpoints/task history.
Não se infere pristine a partir de ausência.

### Pause, cancelamento e resume

Lifecycle raiz usa códigos distintos `running`, `paused`, `completed`,
`cancelled`, `failed`. Pausa tem enum estável `PauseReason`:
`EconomicAuthorization`, `RecoveryRequired`, `UncertainExecution`,
`InsufficientDurableContext`, `InvalidRecovery`. Pausa não cria row terminal
em `task_records`, nem evento de completed. Falha de persistência de pausa,
claim, marker, resultado, receipt ou lifecycle falha fechado.

A estratégia adotada é **A: resume com a mesma policy**. Nenhuma autorização
extra de gasto ou edição task-scoped/global da policy foi criada. Settings novas
não alteram o snapshot antigo. Quando a quota/fatos da alternativa autorizada
voltam a permitir continuação, resume explícito pode continuar aquela tarefa.

A pausa econômica só ocorre na seleção da próxima unidade, a partir dos
exclusions estruturados do mesmo motor B: candidato operacionalmente compatível
bloqueado exclusivamente por `PaidUseDenied` / `PaidBudgetExceeded`, com
resilience elegível. Não se repete scoring para diagnosticar o bloqueio. 429,
503, NoProvider, circuit open, capability ausente e custo UNKNOWN não viram
mensagem de compra/pausa econômica. `PaidCostUnknown` sob Allow continua excluído;
UNKNOWN permanece UNKNOWN. Deny nunca ganha autorização por falta de alternativas.
Allow originalmente capturado pode selecionar paid com custo conhecido dentro do
ceiling já definido na B; C4 não implementa billing nem ledger monetário novo.
Fixed/Preferred preservam o contrato explícito aprovado na B/C3.

A API Core real é `task_graph_runtime::resume_task_graph`: carrega/valida ledger,
obtém claim SQLite `BEGIN IMMEDIATE` por root/generation, registra o mesmo root
no TaskRegistry, reconstrói grafo/resultados, roda C1 antes de novos sucessores,
reavalia facts atuais e executa apenas rows comprovadamente nunca iniciadas.
Não chama planner outra vez nem relê Settings para policy/timeouts. Chamadas
concorrentes retornam busy/terminal, sem duplicar a unidade. O guard do registry
agora identifica sua própria registration por `Arc::ptr_eq`, evitando que Drop
de um guard antigo remova a registration de um resume do mesmo root (ABA).

`cancel_task` também compromete cancelamento no ledger de tarefas ativas/pausadas.
Cancelamento antes de novo dispatch vence; mesmo entre claim e registration, o
CAS de `started` exige root ainda running. Finalização reconhece cancelamento
persistido. Cancelled/completed/failed não aceitam resume; receipts anteriores
permanecem intactos. Não foi criada nova UI de autorização/resume. Tipos e
consumidores exaustivos existentes recebem somente compatibilidade com paused.

### Restart, dependências, identidade e paralelismo

Startup abre/aplica migrations, calcula high-water mark e recupera metadata
**antes de expor providers**. `task_history::max_id` considera history terminal,
checkpoints e agora continuations, inclusive root com payload inválido.
`recover` valida manifest/rows/receipts/results e mantém tudo paused; nunca
executa provider. Pausas econômicas válidas conservam o reason após reopen;
interrupções ficam RecoveryRequired/UncertainExecution e corrupção InvalidRecovery.

Units completed são restauradas com resultados/receipts exatos, sem executar A
ou recalcular retrospectivamente sua allocation. O metadata de finish restaurado
usa o timestamp factual do COMMIT C2; started_at desconhecido permanece ausente.
Task history do resume identifica o início factual da **sessão de resume**, sem
inventar o instante perdido de início da task original.

Uma unidade incerta bloqueia sua branch/dependentes. Unidades independentes
comprovadamente not_started podem continuar por resume explícito, sob budget
original: grants de calls/output das unidades iniciadas incertas são debitados
conservadoramente pelo teto, nunca restaurados como zero. O root permanece paused
enquanto houver incerteza. Efeito Committed permanece não replayable; efeito
UnknownOrInFlight nunca é normalizado para NotStarted nem resolvido automaticamente.

O paralelismo normal de waves foi preservado, com pins/results próprios. A ordem
de conclusão da preparação SQLite de unidades independentes pode variar; a ordem
de resultados consolidados continua sendo a ordem validada do plano.

### Observabilidade Auto

`PinnedProviderAllocation` conserva `AllocationSelection { mode, score }`:
score inteiro já calculado pela B para Auto, None para explícitos. O metadata vem
da mesma route entry que produziu o target/variant pinados; é emitido em
`subtask_started` e persistido na unidade. C1 `handoff_reason`/transitions continuam
separados dessa decisão econômica. Origem/destino/checkpoint, mudanças de
resource/access path/model/effort e seleção unchanged são verificáveis sem prompt,
output, secrets, preço inventado ou ScoreBreakdown completo em diagnostics.

### Gates e FIXes locais

Bateria final separada: `lr85c_final_*`, cobrindo A–Z do pedido. Bateria C4 local:
`c4_*`, incluindo persistência, bounds, corrupção, rollback, grants/claims,
cancelamento e ABA de TaskRegistry. Todos usam mocks/SQLite local; nenhum provider
comercial real foi chamado.

Durante a suíte completa, foram detectadas e corrigidas duas expectativas legadas:
assert de versão ainda 14 em migration de policy (agora 15), e primeiro admitido
fixo worker-1 entre independentes. O gate LR-8B agora verifica ambos os IDs únicos,
capacidade/overlap/provenance e a mesma consolidação na ordem do plano, sem timeout
relaxado. O teste de crash parcial foi tornado determinístico suspendendo o mock
após output de B, antes de completion. Também foram adicionados fences de budget
incerto e teste contra ABA de registration, sem duplicar Scheduler/accounting.

| Gate executado | Resultado final |
| --- | --- |
| `cargo test ... c4_` | **47 PASS**, zero falhas (inclui 24 integrados e ABA) |
| `cargo test ... lr85c_final` | **24 PASS**, cobre A–Z: A/D/T e M/X agrupados; G tem duas provas |
| C1 `cognitive_resources::handoff_tests` | **25 PASS**; contrato puro preservado |
| C2 `c2_` | **39 PASS**; wire/receipts legados preservados |
| C3 `c3_` | **25 PASS** |
| `cognitive_resources` | **246 PASS** |
| B1/B2/B3 (módulos allocation/scoring/provider_bridge) | **46 / 96 / 41 PASS** |
| B4 `b4_` | **57 PASS**: 54 testes B4 + 2 C2 + 1 C4; DTO/persistence policy isolado: 37 PASS |
| TaskGraph `task_graph` / D3 `cognition::task_graph_runtime_tests` | **95 / 13 PASS** |
| Scheduler / admission | **2 / 26 PASS** |
| rate / resilience / telemetry / LR-8E | **69 / 77 / 40 / 14 PASS** |
| TaskRegistry `luna::runtime::tests` | **7 PASS** |
| `persistence::` (inclui migrations/checkpoints) | **71 PASS** |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | **952 PASS, zero falhas, 2 ignorados**, 236,52 s; main/doc-tests sem falhas |
| `cargo check --manifest-path src-tauri/Cargo.toml` | **Exit 0** |
| `npm run typecheck` | **Exit 0** |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Exit 1 exclusivamente por drift legado: baseline **27 arquivos**, candidata **25**, zero novo arquivo com drift |
| Rustfmt direto nos **24 arquivos Rust alterados** | **Exit 0**, edition 2021, `skip_children=true --check` |
| `git diff --check` | **Exit 0** |

Os filtros se sobrepõem; seus totais não devem ser somados. Filtros amplos
`b1_`/`b2_`/`b3_` também passaram (51/99/42, incluindo referências cruzadas).
Os dois ignorados são gates manuais legados `real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate`; não foram habilitados. Nenhuma falha foi
classificada como flake: a primeira execução completa teve as duas expectativas
específicas descritas acima; ambas foram corrigidas e duas suítes completas
subsequentes passaram. A última inclui também a prova de cancelamento no caminho
real de resume, preservando filhos nunca iniciados como cancelled sem inventar
execução. `cargo check` conserva warnings de código não utilizado (15 na lib,
contra 12 na baseline; novas superfícies: ranking legado, leitura de seleção
restaurada e lookup factual do ledger); não há erro de compilação.

### Arquivos alterados nesta candidata

- `docs/LR-8.5C-SAFE-HANDOFF.md`
- `src-tauri/migrations/015_cognitive_continuations.sql`
- `src-tauri/src/cognition/allocation_policy/tests.rs`
- `src-tauri/src/cognition/policy.rs`
- `src-tauri/src/cognition/scheduler.rs`
- `src-tauri/src/cognition/task_graph.rs`
- `src-tauri/src/cognition/task_graph_handoff.rs`
- `src-tauri/src/cognition/task_graph_runtime.rs`
- `src-tauri/src/cognition/task_graph_runtime/c3_tests.rs`
- `src-tauri/src/cognition/task_graph_runtime/c4_tests.rs`
- `src-tauri/src/cognition/task_graph_runtime/c4_tests/storage_tests.rs`
- `src-tauri/src/cognition/task_graph_runtime_tests.rs`
- `src-tauri/src/cognition/task_graph_worker.rs`
- `src-tauri/src/cognition/types.rs`
- `src-tauri/src/cognitive_resources/provider_bridge.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/luna/mod.rs`
- `src-tauri/src/luna/runtime.rs`
- `src-tauri/src/luna/task.rs`
- `src-tauri/src/persistence/checkpoints.rs`
- `src-tauri/src/persistence/checkpoints/tests.rs`
- `src-tauri/src/persistence/continuations.rs`
- `src-tauri/src/persistence/migrations.rs`
- `src-tauri/src/persistence/mod.rs`
- `src-tauri/src/persistence/task_history.rs`
- `src-tauri/src/persistence/tests.rs`
- `src/luna/LunaCorePanel.tsx`
- `src/luna/taskAnimation.ts`
- `src/luna/types.ts`

### Limitações deliberadas para auditoria

- Resume é API Core interna, sem novo botão/command de resume na UI; cancelamento
  usa o command existente. Não há mudança silenciosa de policy ou aprovação paga.
- Antes de manifest validado/committed não há resume de planner; checkpoints
  C2/C3 legados sem manifest/result não são convertidos em planos resumíveis.
- A referência à identidade exige que a versão original ainda seja current e
  validável. Mudança/ausência pausa com InsufficientDurableContext; não se inventa
  contexto e não se consulta versão histórica nesta candidata. Worker com
  memórias/conversa adicionais não é aceito por esta ponte; o caminho normal
  atual usa apenas identidade + dependencies.
- Resultado acima do limite, corrupção ou contexto insuficiente não autorizam
  refazer a unidade. Fences uncertain não possuem API de resolução/replay.
- Grants incertos consomem teto conservador; isso pode impedir independentes
  mesmo com chamadas reais menores. O código não inventa accounting real para
  suprir essa lacuna e não declara o root completed com unidade incerta.
- Retenção/cleanup automático de resultados/ledger não foi acrescentado; estados
  terminais/receipts são preservados. Premissas locais C2/LR-8 de SQLite/OS
  confiáveis continuam; não há autenticação criptográfica contra edição maliciosa
  coerente do arquivo por terceiros, nem distributed lock.
- Não há Tool Runtime, efeito externo, billing, SpecialistAgent, provider discovery,
  handoff mid-stream ou replay de efeito incerto.

**C4 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente.**
C1 PASS; C2 PASS; C3 PASS; LR-8.5C ainda NÃO PASS; merge ainda NÃO autorizado.


## Auditoria independente do C4 — 06/10/2026

**Resultado: FIX obrigatória. C4 e LR-8.5C ainda NÃO PASS.**

A auditoria comparou o C3 auditado em
`c07e2a8398531fcd40823f22d4973a986d5706d7` com a candidata C4
`29ce2a618483e9923a36908855d2fb8fcdbec7f9`. O diff contém um único
commit e os 29 arquivos declarados. A maior parte dos invariantes C4 está
implementada de forma coerente: migration 015 atômica, manifest/result bounded,
marca `started` anterior ao dispatch, claim geracional, startup sem auto-dispatch,
resume explícito, fences incertos, policy congelada, spend denial e provenance
Auto sanitizada.

### Achado bloqueante — terminalização não é atômica com task history

O runtime atualmente terminaliza `cognitive_continuations` em
`finish_continuation` **antes** de persistir `task_records` e
`task_subtask_records`.

No caminho inicial:

1. Workers/checkpoints/results concluem;
2. `ContinuationRepository::finish(..., "completed" | "cancelled" | "failed")`
   pode COMMITAR o estado terminal da continuation;
3. somente depois `task_history::insert_with_subtasks` abre outra transação.

No caminho de resume ocorre a mesma separação: `finish_continuation` é chamado
antes de `task_history::insert_with_subtasks`.

Se o segundo write falhar (I/O, constraint, trigger, disk failure etc.), o Core
passa a reportar falha, mas a continuation pode permanecer duravelmente
`completed`, `cancelled` ou `failed` sem o histórico terminal
correspondente. Não existe rollback entre as duas transações.

Esse estado é especialmente problemático para `completed`: startup
`ContinuationRepository::recover` examina somente continuations
`running`/`paused`, portanto uma continuation `completed` sem
`task_records` não é automaticamente reconciliada. `claim` também a trata
como terminal. O root fica protegido contra reutilização, mas o lifecycle
durável fica contraditório e não existe caminho normal de reparo/resume.

O teste `c4_terminal_state_write_failure_never_reports_completed` cobre a
falha **do UPDATE da própria continuation**, o que é correto, mas não cobre a
janela inversa: continuation terminal COMMITADA seguida por falha no write de
`task_history`.

Isso viola os requisitos C4 de lifecycle persistente e de falha fechada para
terminal cleanup. Portanto o final gate A-Z ainda não é suficiente para fechar a
trilha.

### FIX exigida

A terminalização factual de TaskGraph deve tornar
**continuation terminal + task history terminal um único fato SQLite atômico**.

Direção preferida:

- extrair uma variante de `task_history::insert_with_subtasks` que possa
  escrever usando uma transação já existente, sem iniciar transaction aninhada;
- adicionar uma operação de finalização que use `BEGIN IMMEDIATE` e, na mesma
  transação:
  1. valide lease/generation/cancelamento durável;
  2. valide que `completed` só é permitido com todos receipts/results seguros;
  3. determine o estado terminal factual;
  4. grave `task_records` + `task_subtask_records`;
  5. grave o estado terminal correspondente em `cognitive_continuations`;
  6. COMMIT uma única vez.

A ordem interna dos writes é secundária desde que ambos compartilhem a mesma
transação e nenhum estado terminal escape se o COMMIT falhar.

`paused` continua não sendo histórico terminal e pode usar o mecanismo
persistente de pause separado.

Cancelamento concorrente deve continuar vencendo: se a continuation já estiver
duravelmente `cancelled`, a finalização deve produzir histórico
`cancelled`, nunca promover para `completed`.

### Gates mínimos da FIX

Adicionar testes determinísticos cobrindo pelo menos:

1. falha ao inserir `task_records` após todo trabalho cognitivo/checkpoints:
   nenhuma continuation terminal parcial;
2. falha ao inserir `task_subtask_records`: rollback de root history e estado
   terminal da continuation;
3. o mesmo cenário no caminho de `resume_task_graph`;
4. retry/recovery após remover a falha finaliza sem nova provider call;
5. cancelamento concorrente durante finalização continua terminal
   `cancelled`;
6. sucesso comprova `task_records.state == cognitive_continuations.state` para
   completed/cancelled/failed;
7. regressões C1/C2/C3/C4 e final A-Z continuam verdes.

Não corrigir com compensação do tipo “se history falhar, depois tente mudar
completed para failed”: isso ainda deixa janelas de crash e mistura estados
terminais. A propriedade necessária é atomicidade no mesmo SQLite transaction.

### Achados positivos preservados

- `mark_started` ocorre antes do request e é deliberadamente conservador;
- receipt + Worker result + completed unit são um único COMMIT;
- restart não executa providers automaticamente;
- claim por `generation` impede double-resume;
- result ausente/corrompido não autoriza replay;
- partial-output crash permanece `started`/incerto;
- roots de continuations entram no high-water mark mesmo com manifest inválido;
- Deny não salta para paid e UNKNOWN não vira custo zero;
- policy snapshot antiga não é reescrita por Settings;
- seleção Auto preserva `mode + score` separadamente de `HandoffReason`;
- migration 015 parte de v14 com rollback/retry;
- schema não adiciona campos de reasoning/credenciais.

**Decisão:** não abrir PR, não fazer merge e não marcar LR-8.5C PASS até a FIX
de atomicidade terminal passar por nova auditoria.


## C4 FIX-1 — atomicidade terminal, implementação candidata

**C4 FIX-1 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente.**

Baseline de auditoria `8f05dfca3408cbbb7cb4697e211ced140484575a`, posterior à
candidata C4 `29ce2a618483e9923a36908855d2fb8fcdbec7f9`. A branch estava limpa,
sincronizada com `origin/lr-8.5c-safe-handoff`; `main` permanece em
`0024fec735e3f6cb2461dbdeba8fa6aa4be32d32`. Esta FIX atende exclusivamente ao
achado bloqueante de terminalização/histórico. Não há migration/schema novo,
mudança de Scheduler/policy/allocation/UI nem redesign de C1/C2/C3.

### Um único fato terminal SQLite

`task_history::insert_with_subtasks_in_transaction` é o writer interno que recebe
uma `rusqlite::Transaction` já aberta. A API pública existente
`insert_with_subtasks` mantém assinatura e validações, abre sua própria transação
e delega a esse writer quando usada pelos demais consumidores.

`ContinuationRepository::finish_terminal` abre **BEGIN IMMEDIATE** e, antes do
único COMMIT:

1. valida root/kind/estado terminal, lease/generation e lifecycle durável;
2. carrega/valida o mesmo manifest, grants, checkpoints e results C4/C2;
3. observa cancelamento durável: `cancelled` nunca é promovido a `completed`;
4. exige todos os receipts/results e ausência de incerteza para `completed`;
5. valida IDs únicos/exatos de subtasks e provenance de cada completed;
6. escreve `task_records` e todas as `task_subtask_records`;
7. atualiza `cognitive_continuations` com o mesmo estado terminal factual;
8. COMMITA ambos os lados juntos.

Cancelamento conserva unidades já completed e marca as demais cancelled. Se um
cancelamento/falha ocorre antes da construção do grafo em memória, os IDs e
completions confirmados vêm do manifest/ledger; não se inventam outputs nem
started_at perdidos. `ContinuationRepository::finish` agora só aceita **paused**;
nenhum caller pode usá-lo para terminalizar sem history. `cancel` conserva sua
semântica de fence durável, inclusive quando já venceu antes da transação terminal;
um rollback nunca desfaz um cancelamento anteriormente committed.

### Runtime inicial, resume e falhas

A fase Worker devolve seu lease ao runtime e não terminaliza antecipadamente.
`finalize_execution` é a ponte única de persistência usada por `start_task` e
`resume_task_graph`, após reconciliar cancelamento e antes de publicar conclusão.
Os testes sintéticos de Worker também passam pela mesma ponte. Falhas de preflight
anteriores à criação de continuation conservam o writer de history existente.

~~~text
checkpoint/result de cada unit já COMMITTED
→ terminal transaction: history + continuation
→ COMMIT único
→ somente então TaskCompleted / resultado final
~~~

Falha de INSERT, UPDATE ou COMMIT faz rollback do novo fato terminal inteiro;
receipts/results anteriores não participam desse rollback e continuam íntegros.
Não há compensação de completed para failed. Depois do rollback, a ponte tenta
uma pausa separada **RecoveryRequired**, sem histórico terminal. Se essa pausa
também falhar, o ledger permanece running para startup recovery e o erro de
persistência é reportado em memória; não se grava um terminal failed para ocultar
o erro. Nenhum caminho dispara novamente providers automaticamente.

Após remover a falha, recovery/resume explicitamente carregam as units já
completed e finalizam usando seus receipts/results: zero novas calls, zero replay.
Pausa econômica/incerta/contextual continua separada de histórico terminal. Se
cancelamento vencer uma tentativa de pause, a mesma ponte terminal grava history
cancelled. A transação SQLite serializa writers: cancelamento committed antes
de adquirir o lock vence; um cancelamento posterior não reescreve um fato terminal
já committed, conforme o CAS existente de `cancel`.

Entrega de eventos ocorre depois do fato durável. O TaskGraph não chama mais
`task_history::mark_failed` isoladamente se o canal fecha depois do COMMIT: erro
de transporte não pode reescrever apenas um lado de continuation/history. Falha
de canal durante execução conserva o estado failed e sua terminalização atômica.

### Gates novos e regressões

O filtro `c4_fix1_` contém **8 testes novos**, com loops determinísticos onde
apropriado:

| Gate | Prova |
| --- | --- |
| A/B | `start_task` real com mocks: falha de root ou subtask history deixa ambos os históricos vazios, continuation paused/RecoveryRequired, checkpoints/results íntegros e nenhum TaskCompleted |
| C/D | A committed → crash → resume somente B → falha em cada history → rollback; remove fault/reopen/recovery/resume → zero novas calls, receipts idênticos e terminal agreement |
| E | cancel durável entre A/B, depois do último result no resume e contra economic pause; history cancelled, sem promoção para completed |
| F | completed/cancelled/failed normais concordam com history e IDs/estados de todas as subtasks |
| G | FK DEFERRABLE INITIALLY DEFERRED introduzida só na fixture falha no COMMIT após os writes: rollback de continuation/history e da row auxiliar; retry conclui sem provider replay |
| Falha adicional | terminal write + pause write falham: ledger permanece running, history vazio; recovery/resume finaliza sem novas calls |

A primeira bateria nova foi interrompida ao detectar um parâmetro incorreto de
fixture: barreira de paralelismo em plano sequencial. Corrigido o parâmetro, os
oito testes passaram. A primeira suíte integral teve **957 PASS, 3 falhas, 2
ignorados**: três assertions C3 ainda esperavam terminal/diagnóstico anteriores
em fixtures que mudam receipts duráveis. O writer terminal validado agora recusa
esse ledger: os gates foram atualizados para exigir paused/RecoveryRequired,
nenhuma row de history e B ainda Blocked, além de uma única provider call e dos
fences/IDs anteriormente testados. Os códigos finais são
`continuation_history_invalid`, `continuation_checkpoint_invalid` e
`continuation_checkpoint_mismatch`, respectivamente. A bridge C3 e seus guards
não foram alterados. Nenhuma falha foi atribuída a flakiness.

| Validação final sobre a FIX | Resultado |
| --- | --- |
| `c4_fix1_` | **8 PASS**, zero falhas; A–G e falha adicional de pause |
| `c4_` | **55 PASS** |
| `lr85c_final` A–Z | **24 PASS** |
| C1 `cognitive_resources::handoff_tests` / C2 `c2_` / C3 `c3_` | **25 / 39 / 25 PASS** |
| B1/B2/B3/B4, filtros `b1_`/`b2_`/`b3_`/`b4_` | **51 / 99 / 42 / 57 PASS** (referências cruzadas incluídas) |
| TaskGraph `task_graph` | **103 PASS**, incluindo D3, C3 e C4 |
| `persistence::` / TaskRegistry `luna::runtime::tests` | **71 / 7 PASS** |
| Scheduler / admission | **2 / 26 PASS** |
| rate / resilience / telemetry / LR-8E | **69 / 77 / 40 / 14 PASS** |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | **960 PASS, zero falhas, 2 ignorados**, 327,54 s; main/doc-tests sem falhas |
| `cargo check --manifest-path src-tauri/Cargo.toml` | **Exit 0** |
| `npm run typecheck` | **Exit 0** |
| Rustfmt direto nos **8 arquivos Rust alterados** | **Exit 0**, edition 2021, `skip_children=true --check` |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Exit 1 pelo drift legado: **25 arquivos antes/depois**, log byte a byte idêntico à baseline |
| `git diff --check` | **Exit 0** |

Os filtros se sobrepõem e não devem ser somados. Os dois ignorados continuam
sendo `real_app_server_handshake` e `manual_final_codex_agent_bridge_gate`;
não foram habilitados. Nenhum gate consumiu API/quota comercial real.
A rodada final completa e todos os filtros passaram após os FIXes/assertions
explicitamente descritos acima. Warnings de código não utilizado permanecem
(15 na lib / 1 na lib test); não há erro de compilação.


### Arquivos desta FIX

- `docs/LR-8.5C-SAFE-HANDOFF.md`
- `src-tauri/src/cognition/task_graph_runtime.rs`
- `src-tauri/src/cognition/task_graph_runtime/c3_tests.rs`
- `src-tauri/src/cognition/task_graph_runtime/c4_tests.rs`
- `src-tauri/src/cognition/task_graph_runtime/c4_tests/storage_tests.rs`
- `src-tauri/src/cognition/task_graph_runtime/c4_tests/terminal_fix1_tests.rs`
- `src-tauri/src/cognition/task_graph_runtime_tests.rs`
- `src-tauri/src/persistence/continuations.rs`
- `src-tauri/src/persistence/task_history.rs`

### Limites preservados

- Sem migration nova, reparação automática de terminais inconsistentes anteriores
  ou expansão do resume. Bases que já tenham sido alteradas externamente continuam
  fail-closed; esta FIX impede a nova janela no caminho real de terminalização.
- O fence autônomo de `cancel` é preservado como exigido; não se desfaz cancelamento
  durável diante de erro posterior de histórico. A materialização terminal feita
  pela ponte, inclusive após cancelamento, usa o writer atômico.
- Eventos não são transacionados com SQLite. Falha de entrega depois do COMMIT não
  muda o fato durável já coerente nem autoriza replay.
- As demais limitações da candidata C4 continuam válidas; não há nova feature.

**C4 FIX-1 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente.**
C1 PASS; C2 PASS; C3 PASS; C4 ainda NÃO PASS; LR-8.5C ainda NÃO PASS;
merge ainda NÃO autorizado.


## Auditoria independente do C4 FIX-1 — 06/10/2026

**Resultado: FIX-1 correta para terminalização normal, porém C4 ainda NÃO PASS.
FIX-2 obrigatória: cancelamento durável ainda cria estado terminal sem history
atômico.**

A auditoria comparou o finding documental
`8f05dfca3408cbbb7cb4697e211ced140484575a` com a candidata FIX-1
`e41fcc63be9640c3c39d9cf606a4a76ff7b5f150`.

### O que a FIX-1 corrigiu corretamente

- `task_history::insert_with_subtasks_in_transaction` permite que o caller
  componha history com outro fato SQLite sem transaction aninhada.
- `ContinuationRepository::finish_terminal` usa `BEGIN IMMEDIATE` e grava
  `task_records`, `task_subtask_records` e o estado terminal da continuation
  no mesmo COMMIT.
- A validação usa root/lease/generation, manifest, receipts/results e estado
  incerto antes de permitir `completed`.
- Cancelamento já committed antes do writer terminal é observado e ganha
  precedência sobre um `completed` solicitado.
- Falha em root history, subtask history ou no COMMIT deferred reverte tanto o
  history quanto o novo estado terminal.
- Após rollback, o runtime tenta persistir `RecoveryRequired`; se isso também
  falhar, mantém estado não terminal para startup recovery.
- O caminho inicial e `resume_task_graph` convergem em
  `finalize_execution`/`finish_terminal`.
- Retry após falha terminal reaproveita receipts/results e não chama providers
  novamente.
- Nenhuma migration nova foi necessária para a FIX-1.

### Achado bloqueante remanescente — `cancel()` ainda terminaliza isoladamente

`ContinuationRepository::cancel` continua executando, em uma operação
independente:

`UPDATE cognitive_continuations SET state='cancelled' ...`

sem gravar `task_records` e `task_subtask_records` na mesma transação.

O command real `luna::cancel_task` chama `registry.cancel(...)` e depois
essa operação durável; ele não cria history terminal.

Portanto ainda existe a sequência:

1. tarefa está `paused` ou `running`;
2. `ContinuationRepository::cancel` COMMITA `state='cancelled'`;
3. processo cai antes de qualquer `finish_terminal`;
4. SQLite fica com continuation terminal cancelled, mas sem history terminal.

Para tarefa pausada o caso é ainda mais direto: não existe worker ativo que vá
naturalmente chamar `finish_terminal` depois do command. O teste
`lr85c_final_i_cancel_paused_survives_restart_and_resume_is_terminal` valida
que resume é recusado, mas não exige a presença de `task_records`; portanto ele
aceita exatamente o estado parcial que a FIX-1 deveria eliminar.

Startup `ContinuationRepository::recover` consulta somente
`state IN ('running','paused')`. Logo uma continuation `cancelled` sem
history é ignorada no restart e permanece terminal/incompleta indefinidamente.

### Por que é bloqueante

A propriedade final não pode ser apenas “completion/history é atômico”. Deve
ser:

**qualquer transição durável para um estado terminal
(`completed`, `cancelled`, `failed`) precisa concordar atomicamente com o
history terminal correspondente.**

O cancelamento é uma transição terminal e hoje ainda foge desse writer.

Além disso, enquanto `cancel()` usa o próprio estado terminal como sinal de
cancel request, Core mistura dois conceitos distintos:

- pedido durável de cancelamento;
- fato terminal cancelled já materializado.

A FIX-2 deve separar essas semânticas ou fazer o próprio cancelamento produzir
history terminal no mesmo COMMIT, sem criar uma janela de crash.

### Requisito da FIX-2

É necessário garantir simultaneamente:

1. cancel de tarefa pausada produz, atomicamente, continuation cancelled +
   root/subtask history cancelled;
2. cancel de tarefa running não cria continuation terminal isolada antes de o
   lifecycle factual estar fechado;
3. crash imediatamente depois do pedido durável de cancelamento nunca deixa
   `cancelled` sem history;
4. restart não dispara provider e preserva a intenção de cancelamento;
5. provider/result tardio não pode transformar uma task cancelada em completed
   nem comprometer novo receipt após terminalização;
6. `resume_task_graph` de uma task realmente cancelled continua recusado;
7. history e continuation concordam em todos os caminhos de cancelamento.

Uma solução limpa pode introduzir um conceito persistente não terminal de
`cancel_requested` (campo/tabela/migration, se realmente necessário) e deixar
`finish_terminal` como a única autoridade que escreve `state='cancelled'`.
Outra solução é aceitável se provar os mesmos invariantes. Não mascarar o
problema apenas fazendo startup “reparar depois” um terminal parcial: a meta
continua sendo evitar que o fato terminal parcial seja committed.

### Gate mínimo adicional

Adicionar testes para:

- paused → cancel → crash imediato: history e continuation já concordam em
  cancelled antes do restart;
- running → cancel request → crash antes do worker finalizar: restart faz zero
  provider calls e conclui/retém cancelamento de forma segura sem replay;
- cancel concorrente com provider/result tardio: nenhum checkpoint/result novo
  após terminal cancelled;
- falha de history durante cancel: rollback do cancel terminal;
- falha de COMMIT durante cancel: rollback integral;
- sucesso: `task_records.state == cognitive_continuations.state == 'cancelled'`
  e subtasks coerentes;
- regressões FIX-1 + A-Z permanecem verdes.

**Decisão:** FIX-1 não deve ser revertida; ela resolve o finding original.
Entretanto C4 e LR-8.5C permanecem NÃO PASS até a FIX-2 eliminar a via de
terminalização isolada em `ContinuationRepository::cancel`.


## C4 FIX-2 — Durable Cancellation Atomicity — candidata de 06/10/2026

Baseline: branch `lr-8.5c-safe-handoff`, HEAD local/remoto
`bee611faeafb269720420a669714daa25f3289c4`; workspace limpo;
`main`/`origin/main` preservadas em `0024fec735e3f6cb2461dbdeba8fa6aa4be32d32`.
A finding da auditoria FIX-1 acima orienta exclusivamente esta correção.

### Intenção versus fato terminal

Migration **016**, após confirmar a versão **015** atual, adiciona somente
`cognitive_continuations.cancel_requested`: INTEGER, NOT NULL, DEFAULT 0,
CHECK de tipo/valores 0–1. O valor 1 só pode coexistir com `running`/`paused`.
O manifest, os checkpoints C2/C3, o result envelope e a policy não mudam.
O upgrade 15 → 16 usa `BEGIN IMMEDIATE`, ALTER e `user_version` dentro da
mesma transaction RAII; falha mantém a versão anterior e permite retry.

- **Running:** `cancel` valida a continuation/ledger sob `BEGIN IMMEDIATE` e
  grava apenas `cancel_requested=1`. Não cria history nem estado terminal.
  Repetir a intenção é idempotente. O command continua sinalizando o AtomicBool
  do TaskRegistry, mas só retorna sucesso depois da operação SQLite necessária.
  Erro durável é retornado mesmo que a flag em memória já tenha sido sinalizada.
- **Paused:** não há worker ativo que finalize depois. `cancel` usa a primitive
  compartilhada `finish_terminal_in_transaction`, derivada da FIX-1, na mesma
  transaction que carregou o manifest/ledger. Escreve root history, todas as
  subtasks e continuation cancelled em **um único COMMIT**. Units com receipt e
  result committed ficam completed; demais ficam cancelled. Receipts anteriores,
  resultados, allocation e policy são preservados. Um effect fence incerto não
  é normalizado: a unidade cognitivamente concluída conserva seu receipt/fence,
  sem autorização para replay ou continuação.
- **Terminal existente:** cancel de cancelled válido retorna true sem novos
  INSERTs; completed/failed retornam false e não são reescritos. O load verifica
  root history e todas as subtasks antes de aceitar essa idempotência.

A única primitive que materializa novo estado terminal é
`finish_terminal_in_transaction`, usada tanto por `finish_terminal` dos caminhos
normal/resume quanto pelo cancel paused. Não existe mais UPDATE autônomo de
`state='cancelled'`. O marcador é limpo dentro do COMMIT terminal; o próprio
estado cancelled + history passa a ser a autoridade durável.

### Corridas, crash e resultado tardio

A ordem factual é a aquisição/COMMIT da transaction SQLite:

- cancel request committed antes do writer terminal → o writer observa o marcador
  e materializa **cancelled**, inclusive se todas as units terminaram;
- completed terminal já committed antes do cancel → permanece completed;
- dois cancels concorrentes são serializados por `BEGIN IMMEDIATE`, sem history
  duplicado e sem depender de mutex de processo.

`mark_started` valida lease, generation, estado running e ausência de intenção.
Um cancel request observado nessa boundary retorna `cancelled`, antes de provider
request. `finish` de pausa devolve a precedência do pedido durável ao finalizador
compartilhado, evitando economic pause que perca a intenção. Scheduler/retry/
fallback/admission não mudam; não há polling por token/chunk.

`commit_result` valida também estado da continuation, generation e intenção na
mesma transaction C2 que grava receipt/result. Uma nova conclusão exige running
sem cancel request. Depois de cancelled/failed/paused ela é recusada, revertendo
inclusive qualquer INSERT provisório do checkpoint. A repetição exatamente
idêntica de resultado já committed de uma task completed continua idempotente;
ela não cria novo receipt. Resultado tardio não promove cancelled para completed.

Recovery mantém o pedido como **paused + cancel_requested=1**, preservando markers
started/uncertain e o high-water mark. Não executa providers e não resolve trabalho
restante. `claim`/resume retorna `continuation_cancel_requested`; o caminho Core
real de cancel fecha esse estado pausado atomicamente, sem provider calls. Se o
ledger estiver inválido, permanece fail-closed com root e intenção protegidos.
Não há auto-finalização em startup nem auto-resume. Essa retenção explícita é a
alternativa conservadora permitida pelo contrato da FIX.

Falha em root/subtask INSERT ou COMMIT do cancel paused reverte **todo** o novo
fato terminal. O command retorna erro. A tarefa continua não terminal, com ledger
anterior intacto, e o cancel pode ser tentado novamente. A recuperação/retry da
terminalização normal da FIX-1 continua usando os resultados já committed, sem
replay. SQLite transaction nunca atravessa provider await.

### Gates novos

Bateria local em `c4_tests/cancellation_fix2_tests.rs`, com mocks do Worker/
Scheduler real e SQLite em arquivo:

| Gate | Prova |
| --- | --- |
| A/G/H | cancel pelo Core usado no command, sem task ativa no registry: history/continuation cancelled imediatamente; A completed/B cancelled; reopen/resume terminal, zero novas calls |
| B/C | triggers em root/subtask history fazem o command retornar erro; rollback dos dois históricos e terminal continuation; retry conclui |
| D | trigger com FK deferred falha no COMMIT cancelled; rollback também da row auxiliar; retry atômico |
| E | B iniciou/emitiu output, pedido durável, crash: restart preserva pedido e started fence, recusa resume; cancel explícito fecha history sem replay |
| F | conclusão tardia de B é recusada antes e depois do terminal cancelled; nenhum receipt/result novo ou successor |
| I | dois cancels simultâneos em paused e running: idempotência, um único root history e duas subtasks |
| J | cancel request antes do writer completed termina cancelled; completed COMMIT antes do cancel permanece completed |
| Command | running sinaliza AtomicBool e persiste intenção; falha no write retorna erro, sem falso sucesso durável |
| Fence | cancel conserva receipt UnknownOrInFlight e history de unit já completed; nenhum replay/normalização |
| Migration | upgrade v15 com ledger/policy/history/rate preservados; conflito de ALTER mantém user_version=15/autocommit, correção/retry chega a 16 |

O gate final A–Z de cancel paused agora exige explicitamente agreement de root e
subtask history **antes** do reopen, além da recusa de resume.
A primeira compilação da bateria nova detectou a ausência de `#[path]` explícito
no módulo de testes; corrigida sem mudança de runtime. A rodada seguinte teve
9 PASS e uma falha da fixture de migration: a assertion reabria via Database uma
base ainda sob conflito intencional de ALTER. A assertion passou a usar a conexão
existente para verificar rollback. A rodada ampliada teve 12 PASS e uma falha
na fixture da corrida: a interrupção por Notify após o último result podia ocorrer
quando o writer terminal já havia sido agendado. O gate agora grava o cancel request
sincronamente no callback SubtaskCompleted de B, antes de o controle retornar ao
writer, provando a ordem factual pretendida no caminho real. Nenhuma falha foi
atribuída a flakiness. A primeira passagem pelas regressões C2 teve 38 PASS e
uma falha: a assertion de upgrade v13 esperava a versão final 15, embora o schema
atual já fosse 16. Atualizada somente a expectativa de versão, sem alteração dos
contratos de checkpoint. A primeira suíte integral teve **972 PASS, uma falha e
dois ignorados**: a assertion final do upgrade de policy v8 também esperava 15.
Atualizada somente essa expectativa para 16; revisadas as demais assertions de
schema. O gate v15 → v16 foi ainda fortalecido com history/rate **não vazios** e
comparação integral das rows de history, além de manifest, receipts e policy.
Repetida a validação final; os erros de versão não foram tratados como flakiness.

### Arquivos da FIX

- `src-tauri/migrations/016_cognitive_cancel_request.sql`
- `src-tauri/src/persistence/migrations.rs`
- `src-tauri/src/persistence/continuations.rs`
- `src-tauri/src/persistence/tests.rs` (assertions da versão atual)
- `src-tauri/src/persistence/checkpoints/tests.rs` (somente versão final do upgrade)
- `src-tauri/src/luna/mod.rs` (command e caminho Core compartilhado/testado)
- `src-tauri/src/cognition/allocation_policy/tests.rs` (versão atual/futura)
- `src-tauri/src/cognition/policy.rs` (somente versão final em teste de upgrade)
- `src-tauri/src/cognition/task_graph_runtime/c4_tests.rs`
- `src-tauri/src/cognition/task_graph_runtime/c4_tests/storage_tests.rs` (upgrade)
- `src-tauri/src/cognition/task_graph_runtime/c4_tests/terminal_fix1_tests.rs`
  (helpers compartilhados; gates FIX-1 preservados)
- `src-tauri/src/cognition/task_graph_runtime/c4_tests/cancellation_fix2_tests.rs`
- `docs/LR-8.5C-SAFE-HANDOFF.md`

### Validação final da FIX-2

| Gate / comando | Resultado final |
| --- | --- |
| `c4_fix2_` | **13 PASS**, A–J, fences, command e migration |
| `c4_fix1_` | **8 PASS**, atomic completion/failed, rollback, recovery retry sem replay |
| `c4_` / `lr85c_final` A–Z | **68 / 24 PASS** |
| C1 `cognitive_resources::handoff_tests` / C2 `c2_` / C3 `c3_` | **25 / 39 / 25 PASS** |
| B1/B2/B3/B4, filtros `b1_`/`b2_`/`b3_`/`b4_` | **51 / 99 / 42 / 57 PASS** |
| TaskGraph `task_graph` | **116 PASS**, incluindo D3 e gates C3/C4 |
| `persistence::` / `migration` / TaskRegistry `luna::runtime::tests` | **71 / 14 / 7 PASS** |
| Scheduler `scheduler::tests` / admission | **2 / 26 PASS** |
| rate (filtro amplo) / resilience / telemetry / LR-8E | **103 / 77 / 40 / 14 PASS** |
| `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=4` | **973 PASS, zero falhas, 2 ignorados**, 322,10 s; main/doc-tests sem falhas |
| `cargo check --manifest-path src-tauri/Cargo.toml` | **Exit 0** |
| `npm run typecheck` | **Exit 0** |
| Rustfmt direto nos **11 arquivos Rust alterados** | **Exit 0**, edition 2021, `skip_children=true --check` |
| `cargo fmt --check --manifest-path src-tauri/Cargo.toml` | Exit 1 pelo drift legado: **25 arquivos antes/depois**, log byte a byte idêntico à baseline |
| `git diff --check` | **Exit 0** |

Os filtros se sobrepõem e não devem ser somados. O filtro amplo `rate` inclui
referências cruzadas, além do Rate Limit Manager. A suíte integral final foi
repetida depois das correções descritas, sobre os arquivos Rust finais.
Os dois ignorados são `real_app_server_handshake` e
`manual_final_codex_agent_bridge_gate`; não foram habilitados. Nenhum gate usou
API/quota comercial real. Permanecem os warnings legados de código não utilizado
(15 na lib / 1 na lib test); não há erro de compilação.

### Limitações para auditoria

- Startup retém cancel request não terminal até cancel explícito; resume é
  recusado deterministicamente. Não se executa trabalho para resolver o pedido.
- A migration não repara terminais parciais previamente criados pela candidata
  antiga ou alterações externas. O load rejeita terminal sem history completo;
  o root continua protegido pelo high-water mark. Esta FIX impede novas janelas.
- O manifest legado não conserva o horário original de início da root. No cancel
  paused, os campos obrigatórios de timestamp da root history usam o horário
  factual da finalização; finished_at de units completed usa o receipt original.
  Não se inventa um horário de dispatch ausente.
- Antes de existir continuation (planner/preflight, mock e demais runtimes),
  mantém-se o cancel pelo TaskRegistry existente; não se expande esta FIX para
  um novo ledger desses caminhos.
- Eventos continuam sem transaction distribuída com SQLite. Não há nova UI,
  migration de checkpoints, alteração de C1/C2/C3/policy/Scheduler ou nova feature.

**C4 FIX-2 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente.**
C1 PASS; C2 PASS; C3 PASS; C4 ainda NÃO PASS; LR-8.5C ainda NÃO PASS;
merge ainda NÃO autorizado.
