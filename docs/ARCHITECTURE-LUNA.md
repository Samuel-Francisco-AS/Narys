# Arquitetura-alvo — Luna Runtime

> Decisão arquitetural pós-M0. Última revisão factual: 25/09/2026.
>
> Este documento descreve a direção escolhida para a Assistente-3D. Ele não declara que os componentes abaixo já estão implementados. O estado implementado continua documentado em README.md, VALIDACAO.md e docs/M0-B-STATUS.md.

## 1. Visão do produto

A Luna não será uma interface 3D acoplada a uma única LLM. Ela será uma assistente/agente persistente com identidade, memória, estado, ferramentas e corpo 3D próprios. Modelos de linguagem são recursos cognitivos substituíveis usados pelo runtime quando necessário.

Princípio central:

> **A agente é a Luna. A LLM não é a Luna.**

Trocar Gemini, Groq, Mistral, OpenAI, Copilot ou Codex não deve trocar a identidade, a memória ou o estado da assistente.

## 2. Princípios operacionais

A ordem de prioridade definida para o runtime é:

1. **Economia** — minimizar custo financeiro, uso de cota, chamadas e reenvio de contexto.
2. **Timing** — manter latência proporcional à complexidade real; tarefas triviais não podem virar operações longas.
3. **Distribuição** — usar o conjunto de recursos cognitivos disponíveis sem concentrar trabalho desnecessariamente em um único provedor.

Guardrail complementar: **proporcionalidade**. O sistema não deve criar planejamento multiagente para uma operação que pode ser executada diretamente.

### Autonomia do usuário sobre política cognitiva

Configurações que alterem capacidade, qualidade, latência, custo ou comportamento cognitivo **não podem permanecer como limites invisíveis hardcoded do runtime** quando o provider/modelo permitir escolha.

O usuário deve poder configurar, por papel cognitivo e por integração:

- provider ou agente especialista;
- modelo;
- nível de reasoning/thinking, quando suportado;
- teto de saída;
- orçamento/contexto;
- timeouts;
- retries e fallback;
- streaming;
- parâmetros de geração expostos pelo provider;
- uso de ferramentas, grounding/web e capacidades opcionais;
- políticas de custo/cota.

Os defaults da Luna podem ser conservadores, mas devem ser **visíveis e substituíveis**. Um modo automático pode recomendar ou escolher valores, porém não deve substituir silenciosamente uma seleção explícita do usuário.

Escolher “sem limite adicional da Luna” significa não impor um teto artificial abaixo do máximo efetivamente aceito pelo provider/modelo. Permanecem válidos limites físicos da API, janela de contexto, quotas, segurança, permissões e restrições técnicas reais.

O Scheduler pode alertar sobre custo, latência, quota ou incompatibilidade, mas uma preferência explícita — por exemplo usar um agente especialista como Codex em `conversation`, se a integração suportar esse papel — não deve ser trocada silenciosamente por outra política. Fallback só ocorre segundo configuração conhecida do usuário.

Exemplo:

~~~text
"crie uma pasta teste"
→ interpretar
→ executar ferramenta
→ confirmar
~~~

e não:

~~~text
planner → três LLMs → revisor → ferramenta
~~~

## 3. Stack escolhida

### Manter

- **Tauri 2**: shell desktop/mobile, IPC, capabilities, integração com sistema.
- **Rust**: núcleo operacional da Luna.
- **React + TypeScript**: interface, configuração, conversa, observabilidade e painéis.
- **Three.js**: renderização do avatar.
- **Blender**: oficina principal de modelagem e animação.

### Mudar a responsabilidade

O documento de retomada M0 citava Python como possível base futura de agente/ferramentas. Essa direção é substituída por:

- **Rust = Luna Core / Agent Runtime**.
- **Python = ferramenta ou sidecar opcional**, útil para Blender, processamento, automações específicas e bibliotecas que justifiquem sua presença; não será o cérebro obrigatório do produto.

### Motivo

O runtime precisa coordenar estado persistente, concorrência, filas, rate limits, cancelamento, subprocessos, permissões, segredos, ferramentas e IPC. Rust já está dentro do Tauri e evita introduzir um segundo runtime obrigatório para o núcleo.

## 4. Arquitetura de alto nível

~~~text
                         LUNA

┌──────────────────────────────────────────────────────────────┐
│                    LUNA CORE · Rust                          │
│                                                              │
│ Identity     Memory       Task State        Permissions       │
│    │           │              │                 │             │
│    └───────────┴───── Orchestrator ─────────────┘             │
│                         │                                    │
│                ┌────────┴────────┐                           │
│             Scheduler        Tool Runtime                    │
│      economy/timing/distribution                             │
│                │                                             │
│         Provider Registry                                    │
│ Gemini · Groq · Mistral · ...                               │
│         Specialist Agents                                    │
│ Codex · GitHub Copilot · futuros                            │
│                │                                             │
│           Task/Event Stream                                  │
└──────────────────────────┬───────────────────────────────────┘
                           │ Tauri Commands / Channels
             ┌─────────────┴─────────────┐
             │                           │
             ▼                           ▼
┌──────────────────────┐       ┌────────────────────────┐
│      React UI        │       │     Avatar Runtime     │
│ conversation         │       │      TypeScript       │
│ configuration        │       │                        │
│ memory UI            │       │ Avatar Manager         │
│ provider UI          │       │ Animation Registry     │
│ task progress        │       │ Animation Director     │
└──────────────────────┘       │ Expression / LookAt     │
                               │ Three.js / three-vrm    │
                               └───────────┬────────────┘
                                           │
                                      Avatar Pack
                                    VRM + VRMA assets
~~~

O produto permanece um **monólito modular** no início. Não criar microsserviços por antecipação.

## 5. Luna Core

O Luna Core é a autoridade sobre a continuidade da assistente. Ele deve conter módulos independentes, mas executados no mesmo aplicativo:

### Identity Core

Representação estruturada de identidade e comportamento. Evitar depender de um único prompt gigante.

Campos conceituais:

- nome e identidade da assistente;
- estilo de comunicação;
- princípios comportamentais;
- relação e preferências do usuário quando autorizadas;
- limites e regras;
- exemplos de comportamento;
- versão do perfil de identidade.

### Memory

A memória não será apenas histórico infinito de chat. Separar pelo menos:

- **working memory**: estado efêmero da tarefa atual;
- **episodic memory**: acontecimentos relevantes;
- **semantic memory**: fatos duradouros e conhecimentos consolidados;
- **relational memory**: contexto de continuidade entre usuário e Luna;
- **project memory**: decisões e estado de projetos.

Persistência inicial recomendada: **SQLite local**. Embeddings ou banco vetorial não são requisito da primeira versão; só entram quando houver necessidade comprovada de recuperação semântica.

### Shared Cognitive State

Nenhuma LLM deve ser dona do estado da tarefa. Cada tarefa mantém um estado estruturado compartilhado:

- objetivo;
- fatos confirmados;
- resultados de ferramentas;
- hipóteses;
- decisões;
- subtarefas;
- dependências;
- pendências;
- erros;
- custo e cota consumidos;
- tempo gasto;
- próximos passos.

Isso permite trocar de modelo entre etapas sem perder continuidade.

### Context Builder

Antes de uma chamada cognitiva, o runtime monta apenas o contexto necessário:

~~~text
Identity relevante
+ memórias relevantes
+ estado da tarefa
+ conversa recente necessária
+ ferramentas permitidas
+ instrução da subtarefa
~~~

Evitar reenviar toda a memória ou toda a conversa por padrão.

## 6. Orchestrator e Scheduler

O Orchestrator decompõe trabalho quando necessário. O Scheduler escolhe onde executar cada unidade.

### Modos de execução

**Fast Path**

Para ações triviais e claras. Preferir ferramenta direta ou uma única chamada.

**Sequential Distributed**

Subtarefas dependentes podem passar por workers diferentes, usando Shared Cognitive State como handoff.

**Parallel**

Somente para subtarefas realmente independentes. Paralelismo reduz tempo, mas pode aumentar uso de cota.

**Fallback / Overflow**

Quando um provedor está indisponível, congestionado ou próximo do limite, uma subtarefa compatível pode ir para outro.

**Affinity**

Manter uma sequência no mesmo modelo quando trocar de worker exigiria reenviar contexto grande demais ou destruir continuidade útil.

### Score de seleção

Não implementar round-robin cego. O score deve considerar:

- capacidade exigida;
- compatibilidade com a subtarefa e quality floor;
- cota restante e escassez, quando factuais;
- RPM/TPM/RPD e outros limites;
- fila e concorrência;
- latência recente;
- custo monetário conhecido;
- custo de oportunidade de consumir uma quota escassa;
- caminho de acesso e billing domain;
- necessidade de reenvio de contexto e switching cost;
- erros e cooldown recentes;
- preferência configurada pelo usuário.

### Perfis de comportamento

Planejar três perfis globais:

- **Econômico**: prioriza gratuito, espera se necessário, evita pago.
- **Balanceado**: combina cota, latência e qualidade.
- **Rápido**: prioriza latência; recursos pagos só entram conforme permissão explícita.

## 7. Rate Limit Manager e orçamento

Todas as chamadas passam por uma camada comum de controle.

Requisitos:

- token bucket/leaky bucket ou algoritmo equivalente;
- RPM, TPM, RPD/TPD e limites específicos do provedor;
- limite de concorrência;
- fila ordenada e cancelável;
- leitura de headers de rate limit quando disponíveis;
- respeito a Retry-After;
- exponential backoff com jitter;
- cooldown de provedor;
- orçamento por tarefa;
- limite financeiro por tarefa/dia quando houver provedores pagos;
- telemetria de consumo.

O objetivo é **prevenir 429**, não apenas reagir depois.

### 7.1. Cognitive Resource Economy

Rate limit e orçamento operacional não bastam para decidir qual recurso deve ser
consumido. A arquitetura introduz uma camada de alocação econômica, planejada na
**LR-8.5 — Cognitive Resource Economy & Allocation**.

Ela distingue:

- ProviderFamily de AccessPath;
- free tier, allowance incluída, créditos estudantis, prepaid, metered e unknown;
- Cognitive Provider de Specialist Agent;
- disponibilidade de capacidade funcional;
- custo monetário de custo de oportunidade;
- quota abundante, escassa/reservada e esgotada quando esses estados puderem ser
  derivados sem inventar precisão.

Invariantes:

- unknown não significa grátis, pago, zero ou ilimitado;
- 429 transitório não autoriza escalada financeira;
- gastar dinheiro exige policy explícita e budget;
- recursos escassos podem ser preservados no modo Auto sem bloquear escolha Fixed;
- handoff automático ocorre apenas em fronteiras seguras da tarefa;
- Shared Cognitive State continua sendo a autoridade da continuidade.

A camada não compra créditos, não altera planos e não hardcode preços comerciais.
Ela consome fatos dos adapters/LR-8 e decide entre recursos elegíveis segundo policy.

Detalhes: [LR-8.5-COGNITIVE-RESOURCE-ECONOMY.md](LR-8.5-COGNITIVE-RESOURCE-ECONOMY.md).

## 8. LLMs como workers e agentes especialistas

Distinguir duas categorias.

### Cognitive Provider

Interface para inferência geral: texto, classificação, planejamento, síntese e revisão.

Exemplos candidatos: Gemini, Groq, Mistral, Cohere, OpenRouter, Cloudflare Workers AI, OpenAI API.

### Specialist Agent

Runtime com ferramentas e ciclo agentivo próprios. Não tratá-lo como simples completion API.

Candidatos iniciais:

- **OpenAI Codex SDK** para engenharia de software;
- **GitHub Copilot SDK** para tarefas de desenvolvimento e workflows agentivos.

Esses agentes devem obedecer às mesmas políticas de orçamento, permissões e observabilidade do Luna Core.

### 8.1. Planos cognitivo, de execução e de observação

A partir da LR-9, agentes e providers não confundem interface visual com
capacidade de agir.

~~~text
Cognitive Plane
    ↓
Execution Broker → Structured Exec / PTY → OS
    ↓
Operational Trace Bus
    ↓
Observation Plane / Terminal Surface
~~~

O **Execution Broker** é a fronteira de efeitos reais. Ele mantém owner/origin,
TaskId, workspace scope, cwd, lifecycle, cancelamento, provenance e ligação com
policy/approval.

A **Terminal Surface** é consumidora: fechar, ocultar ou atrasar a UI não pode
parar a execução.

SpecialistAgents não digitam silenciosamente na PTY humana. Sessões agentivas
possuem ownership próprio; compartilhamento exige handoff explícito.

Observabilidade segue passive trace: adapters transportam somente eventos já
produzidos naturalmente pelo backend. Nenhum prompt ou inferência adicional
existe apenas para gerar telemetria humana.

Plano: [LR-9 — Operational Terminal & Cognitive Trace Runtime](NARYS-TERMINAL-RUNTIME-TRACK.md).

## 9. LLM de saída / Luna Voice — trilha futura

O conceito continua válido, mas foi retirado da posição LR-9 em 08/10/2026.

Uma futura camada de saída pode receber resultado canônico resumido, identidade,
memórias relevantes e constraints de apresentação para produzir comunicação
coerente.

Responsabilidades preservadas:

- não ser fonte da memória;
- não alterar fatos executados;
- possuir fallback;
- evitar logs/tokens brutos;
- usar templates locais quando suficientes;
- não impor segunda inferência a toda resposta.

Essa camada não participa do Operational Trace e não fabrica "pensamentos" ou
progresso para preencher o Terminal.

Registro: [NARYS-VOICE](NARYS-VOICE-FUTURE-TRACK.md).

## 10. Feedback durante operações

A Luna não deve ficar silenciosa em tarefas longas.

O feedback deve nascer de **eventos reais do runtime**, não de uma LLM fingindo progresso.

Eventos previstos:

- TaskStarted
- TaskPlanned
- SubtaskStarted
- ProviderQueued
- ProviderStarted
- ProviderCooldown
- ToolStarted
- ToolCompleted
- FindingRegistered
- WaitingForDependency
- TaskCompleted
- TaskFailed
- TaskCancelled

Tauri **Channels** serão preferidos para fluxo ordenado e contínuo de progresso. Events ficam para notificações pequenas e desacopladas.

A UI e o Avatar Runtime consomem o mesmo stream, permitindo sincronizar texto e comportamento corporal.

## 11. Segurança

Antes de introduzir segredos ou ferramentas operacionais:

- substituir o CSP nulo atual por política restritiva;
- definir capabilities Tauri por janela/feature;
- manter chamadas privilegiadas no Rust;
- validar todo dado na fronteira WebView ↔ Rust;
- armazenar API keys/segredos fora de React/localStorage;
- avaliar **Tauri Stronghold** para segredos;
- exigir confirmação para ações destrutivas/sensíveis;
- registrar audit trail de ferramentas;
- aplicar allowlists de caminhos/comandos onde fizer sentido.

O estado atual com CSP nulo é aceitável somente como protótipo M0 sem segredos/agente operacional.

## 12. Critério de sucesso arquitetural

A arquitetura está cumprindo o objetivo quando:

1. trocar um provider não muda identidade/memória da Luna;
2. desligar uma LLM não destrói uma tarefa já em andamento;
3. uma tarefa trivial usa Fast Path;
4. uma tarefa complexa pode distribuir subtarefas;
5. limites de cota são conhecidos e dosados;
6. o usuário recebe progresso real;
7. animações e avatar podem mudar sem alterar o Luna Core;
8. a Luna permanece parcialmente funcional offline, ainda que com cognição limitada.

## 13. Local Cognitive Support e fronteira com AI-Native Runtime

A arquitetura passa a reconhecer uma terceira classe de recurso cognitivo além de Cognitive Providers e Specialist Agents: **Local Cognitive Support**.

Ela cobre capacidades auxiliares como:

~~~text
VAD / ASR / TTS
embeddings / reranking
classify / extract / structure
distill / evidence building
handoff preparation
~~~

Esses modelos não são "a LLM local da Luna". São coprocessadores de baixa autoridade usados para reduzir trabalho mecânico, tokens e cota dos recursos cognitivos principais.

A propriedade arquitetural pretendida é:

~~~text
Luna
  │
  │ política / criticidade / quality floor
  ▼
LocalSupportClient
  │
  ▼
AI-Native Runtime
  │
  └─ cognition.*
      model registry
      admission
      load/unload
      resource awareness
      telemetry
~~~

Enquanto o AI-Native Runtime ainda não existir, um backend experimental pode viver temporariamente no Luna Core, desde que permaneça atrás de interface semântica e não espalhe detalhes de GGUF/ONNX/modelos pela orquestração.

A fronteira é:

> **O Runtime sabe o que pode ser feito localmente; a Luna sabe o que deve ser feito localmente.**

A Luna continua sendo autoridade sobre tarefa, identidade, memória, permissões e escolha de escalada. Um modelo local não ganha autoridade para revisar código crítico, aprovar side effects ou substituir julgamento de um provider/agente especialista apenas por ser gratuito.

Detalhes, candidatos, benchmark e projeção de integração estão em [LOCAL-COGNITIVE-SUPPORT-LUNA.md](LOCAL-COGNITIVE-SUPPORT-LUNA.md).

## 13.1. Evolução cognitiva pós-LR-11

Após LR-11, a arquitetura reserva uma trilha experimental **NX — Narys Cognitive
Evolution**. Ela não substitui LR-8.5 nem Local Cognitive Support: parte dessas
fundações para investigar capacidade acumulativa no nível do sistema.

A hipótese central é que o runtime deve aprender a reduzir dependência futura de
raciocínio externo para famílias de problemas já compreendidas.

Primitives candidatas:

~~~text
Intent Field
    ↓
Cognitive IR
    ↓
Attention Market + Epistemic Ledger
    ↓
Cognitive Metabolism
    ↓
Shadow Cognition (quando o ganho esperado justificar)
    ↓
Action / Evidence Contract
    ↓
Proof-Carrying Action
    ↓
Experience
   ↙   ↘
Semantic   Skill Foundry
Immune         ↓
System     capacidade reutilizável

Local Reflex Mesh participa como camada subcognitiva de baixa autoridade.
~~~

Invariantes da pesquisa:

- memória não equivale a verdade;
- contexto/atenção possuem custo explícito;
- profundidade de raciocínio deve ser proporcional a risco e ganho esperado;
- executor não declara sucesso sem satisfazer o contrato de evidência;
- experiência repetida deve poder cristalizar em mecanismos mais baratos;
- defesas contra falhas aprendidas precisam de escopo e validade;
- modelos locais não recebem autoridade apenas por serem gratuitos;
- Narys mantém intenção, política, aprendizado e autorização;
- o futuro AI-Native Runtime mantém primitives de ambiente e execução
  determinística.

A fronteira pretendida permanece:

~~~text
Narys
  → Cognitive IR + Action Contract
  → AI-Native Runtime
  → Evidence Receipt
  → Narys
~~~

A trilha usa numeração NX separada das LRs de entrega e só é elegível para
execução após o fechamento da LR-11. Cada hipótese deve possuir baseline, métrica
e possibilidade real de rejeição antes de promoção ao núcleo.

Detalhes:
[NARYS-POST-LR11-COGNITIVE-EVOLUTION.md](NARYS-POST-LR11-COGNITIVE-EVOLUTION.md).

## 14. Fontes técnicas de referência

Verificadas em 25/09/2026:

- Tauri — Calling Rust / Channels: https://v2.tauri.app/develop/calling-rust/
- Tauri — Security: https://v2.tauri.app/security/
- Tauri — CSP: https://v2.tauri.app/security/csp/
- Tauri — Capabilities: https://v2.tauri.app/security/capabilities/
- Tauri — Stronghold: https://v2.tauri.app/plugin/stronghold/
- Three VRM: https://github.com/pixiv/three-vrm
- VRM Add-on for Blender: https://vrm-addon-for-blender.info/
