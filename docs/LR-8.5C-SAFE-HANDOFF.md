# LR-8.5C — Safe Cross-Resource / Cross-Variant Handoff

Estado: **C1 = PASS técnico após auditoria independente; C2 é o próximo bloco interno.**

O PASS abaixo é exclusivo do C1. LR-8.5C não recebe PASS neste checkpoint;
C2/C3/C4 permanecem sem implementação iniciada.

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
