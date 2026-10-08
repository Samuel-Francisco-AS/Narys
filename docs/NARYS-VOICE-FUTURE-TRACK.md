# NARYS-VOICE — Unified Voice & Natural Feedback

**Estado:** TRILHA FUTURA — sem posição fixa no roadmap.  
**Origem:** antigo escopo da LR-9, adiado em 08/10/2026 para priorizar a
infraestrutura operacional pré-SpecialistAgents.  
**Relação com LR-9 atual:** independente; não bloqueia Operational Terminal,
Copilot ou Codex.

## Visão

Separar execução interna de apresentação ao usuário para preservar identidade
única mesmo quando o trabalho vier de providers, workers ou SpecialistAgents
heterogêneos.

## Escopo preservado

- resultado canônico adequado à apresentação;
- output model policy;
- fallback de apresentação;
- Identity + RelevantMemory;
- feedback local por templates quando suficiente;
- LLM somente quando feedback complexo justificar custo;
- apresentação nunca reescreve fatos de forma incompatível com execução.

Pacote conceitual:

~~~text
UserRequest
CanonicalResult
RelevantIdentity
RelevantMemory
RecentConversation
PresentationConstraints
~~~

Logs, prompts internos, private chain-of-thought e material bruto completo não
entram automaticamente.

## Regra econômica

Eventos simples não gastam LLM. A existência da camada nunca autoriza uma
segunda inferência obrigatória para toda resposta normal.

## Voice não significa necessariamente áudio

"Voice" significa identidade/apresentação unificada. VAD, ASR e TTS são
capabilities de speech separadas; podem convergir futuramente, mas não são
requisito automático.

## Motivo do adiamento

Antes de LR-10/LR-11 o ganho seria majoritariamente de apresentação. Já
Operational Terminal/Execution Broker cria infraestrutura necessária aos
SpecialistAgents e transparência operacional imediata.

A posição futura deve considerar quantidade real de agents/workers, divergência
observada de estilo, custo adicional, maturidade de memória/identidade e valor
de speech.

## Relação com observabilidade

NARYS-VOICE não fabrica mensagens para o Terminal. O Observation Plane da LR-9
é passivo e transporta somente eventos naturalmente expostos.

## Decisão

A trilha permanece preservada, sem número LR reservado e sem data. Pode ser
retomada em qualquer checkpoint estável quando o benefício funcional for
demonstrável.
