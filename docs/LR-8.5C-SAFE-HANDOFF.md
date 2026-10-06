# LR-8.5C — Safe Cross-Resource / Cross-Variant Handoff

Estado: **PLANEJADA — C1 é o próximo bloco de implementação.**

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
