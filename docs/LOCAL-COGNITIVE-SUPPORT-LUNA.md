# Luna + Local Cognitive Support

> **Status:** direção arquitetural / projeção registrada em 03/10/2026. Não implementada.
>
> Este documento descreve **como a Luna pode consumir cognição local auxiliar** sem transformar modelos pequenos em autoridade sobre a assistente e sem acoplar permanentemente o Luna Core ao hardware/modelos do host.

## 1. Decisão principal

Modelos locais pequenos e médios devem ser tratados como **coprocessadores cognitivos**.

Eles não substituem Cognitive Providers remotos nem Specialist Agents. Eles orbitam o trabalho desses recursos para reduzir:

- tokens de entrada;
- tokens de saída mecânicos;
- reenvio de contexto;
- chamadas remotas evitáveis;
- uso de cota;
- custo financeiro;
- trabalho de transformação que não exige raciocínio frontier.

A ideia central:

> **A LLM grande deve gastar sua capacidade pensando; modelos locais podem preparar o material que ela precisa pensar sobre.**

## 2. Fronteira com o AI-Native Runtime

A arquitetura-alvo coloca o núcleo da **Local Cognitive Support Layer** no AI-Native Runtime, não dentro da identidade da Luna.

Responsabilidades do Runtime:

- descobrir capacidades locais;
- registrar modelos/runtimes disponíveis;
- carregar e descarregar modelos;
- warm TTL;
- observar RAM/CPU/pressão/temperatura;
- admission control;
- escolher implementação local adequada;
- cache;
- telemetria;
- provenance;
- preservar evidência bruta.

Responsabilidades da Luna:

- entender a tarefa;
- definir criticidade;
- decidir quality floor;
- decidir se perda de informação é aceitável;
- decidir se pode esperar;
- escolher quando uma capability auxiliar é apropriada;
- rejeitar um resultado local insuficiente;
- escalar para Cognitive Provider ou Specialist Agent quando necessário.

Fronteira resumida:

> **O Runtime sabe o que pode ser feito localmente; a Luna sabe o que deve ser feito localmente.**

## 3. Situação de transição antes do Runtime existir

O AI-Native Runtime ainda não está implementado. Portanto, experimentos podem precisar acontecer primeiro dentro do repositório da Assistente-3D.

Isso é aceitável desde que seja tratado como **shim experimental**, não como contrato definitivo.

Direção recomendada:

~~~text
Luna Core
   │
   ├─ LocalSupportClient   ← interface semântica
   │        │
   │        └─ implementação provisória local
   │
   └─ futuramente:
            │
            ▼
      AI-Native Runtime
      cognition.*
~~~

O Luna Core não deve espalhar nomes de arquivos GGUF, quantizações ou detalhes de ONNX pela orquestração.

Quando o Runtime existir, o backend provisório pode ser substituído sem mudar os pedidos semânticos da Luna.

## 4. O que a Luna pode delegar

### 4.1 Context preparation

Antes de uma chamada remota:

- selecionar evidências;
- remover duplicação;
- normalizar formatos;
- extrair campos;
- montar listas;
- criar resumos factuais de baixa ambiguidade;
- transformar resultados brutos em evidence packets.

### 4.2 Tool-result distillation

Exemplo:

~~~text
cargo test / npm test / logs
        │
        ▼
resultado bruto preservado
        │
        ▼
modelo local
        │
        ├─ testes pass/fail
        ├─ falhas agrupadas
        ├─ stack frames relevantes
        ├─ arquivos citados
        └─ pendências
        │
        ▼
Codex / Planner / Verifier remoto
~~~

Se 15.000 tokens de output mecânico virarem 500–1.000 tokens de evidência útil, a economia acontece principalmente no **input remoto**.

### 4.3 Handoff entre papéis

~~~text
Planner remoto
→ output amplo
→ local handoff builder
→ Executor recebe pacote compacto

Executor + tools
→ logs/resultados
→ local evidence builder
→ Verifier recebe pacote compacto
~~~

O transform local não decide se o plano está correto. Ele organiza aquilo que o próximo agente precisa julgar.

### 4.4 Retrieval / memória

Pipeline candidato:

~~~text
pergunta
→ embedding local
→ recuperação semântica
→ top-N
→ reranker local
→ top-K
→ opcional distillation
→ Context Builder
→ provider remoto
~~~

Isso reduz a necessidade de enviar conversas, documentos ou memórias extensas por padrão.

### 4.5 Structured output e validação

Bom uso:

- texto → JSON;
- extração de entidades;
- schema validation;
- enum normalization;
- checklist;
- detectar campos ausentes;
- classificar resultado;
- marcar presente / ausente / inconclusivo.

Mau uso:

- "o código está correto";
- "esta decisão de segurança é aceitável";
- "a arquitetura é boa".

Validação estrutural não é julgamento semântico.

### 4.6 Routing simples

Com superfície bem conhecida, um modelo pequeno pode decidir:

~~~text
"abre o GitHub da Assistente"
→ open_url(...)

"coloque o volume em 30%"
→ set_volume(30)
~~~

Esse caminho exige confidence/guardrails e deve ser restrito a operações compatíveis com a política.

### 4.7 Voz

Possível cadeia local:

~~~text
microfone
→ VAD
→ ASR
→ Luna Core
→ resposta
→ TTS
~~~

Isso pode reduzir dependência externa para interação básica e permitir funcionamento parcial offline.

## 5. O que não delegar por economia

Não usar modelos locais pequenos como substituto barato para:

- edição autônoma de código em tarefas complexas;
- revisão final de segurança;
- autorização de side effects;
- decisões irreversíveis;
- auditoria independente que exija competência comparável ao autor;
- planejamento complexo;
- interpretação de requisito ambíguo com alto impacto;
- julgamento de qualidade final sem mecanismo de verificação adequado.

Para programação, a função local preferida é **orbitar** Codex/Copilot/LLMs grandes:

~~~text
local
→ encontrar / extrair / comprimir / estruturar

frontier ou specialist agent
→ entender / decidir / modificar / julgar
~~~

## 6. Integração com componentes atuais da Luna

### Context Builder

Pode futuramente consumir:

- embeddings;
- reranking;
- context distillation;
- evidence packets.

O Context Builder continua autoridade sobre **o que entra** na chamada. Um modelo auxiliar não ganha acesso irrestrito à memória.

### Orchestrator

Pode pedir capacidades auxiliares quando o benefício superar o custo local.

Exemplo:

~~~text
Orchestrator:
"preciso das falhas relevantes deste TestRun"

LocalSupport:
evidence_packet_v1
~~~

### Scheduler

O Scheduler pode passar a comparar classes diferentes de recurso cognitivo:

- local support;
- Cognitive Provider;
- Specialist Agent.

Mas não deve tratar todos como equivalentes. Uma capability local de distillation não concorre diretamente com Codex para editar código.

### TaskGraph

Nós do graph podem declarar transformações auxiliares:

~~~text
Worker output
→ distill node
→ dependent worker
~~~

Esses nós precisam de provenance e não podem esconder o Resource bruto.

### Shared Cognitive State

Resultados locais devem entrar como fatos derivados, sempre com origem:

~~~text
derived_fact
source = resource://...
transform = cognition.distill
model = ...
revision = ...
~~~

### Task/Event Stream

Eventos candidatos:

~~~text
LocalCognitionQueued
LocalModelLoading
LocalCognitionStarted
LocalCognitionCompleted
LocalCognitionRejected
LocalModelUnloaded
~~~

A UI não precisa expor todos por padrão, mas eles devem existir para observabilidade.

## 7. Economia / timing / distribuição

A decisão não é "local primeiro".

O Scheduler deve perguntar:

> **Qual recurso cognitivo mais barato satisfaz os requisitos da operação?**

Dimensões:

- qualidade mínima;
- latency budget;
- custo financeiro;
- cota;
- input/output remoto evitado;
- RAM;
- CPU;
- temperatura;
- modelo já quente;
- tamanho do contexto;
- interatividade;
- afinidade;
- necessidade de provenance.

Exemplo:

~~~text
operação: distill_tool_output
interactive: false
deadline: 15 s

Granite 350M
quality predicted: baixa

Qwen 2B
quality predicted: adequada
local cost: RAM/CPU
financial cost: 0

Groq
quality: alta
latency: baixa
quota cost: material

→ Qwen 2B pode vencer
~~~

Agora:

~~~text
Sam esperando resposta
deadline: 2 s
Groq saudável
→ não bloquear a interação para economizar alguns tokens
~~~

Economia não deve piorar Timing cegamente.

## 8. Lifecycle esperado

Modelos maiores entram apenas quando úteis.

~~~text
request
→ admission
→ load se necessário
→ execute
→ warm TTL
→ reuse se chegar trabalho compatível
→ unload
~~~

Para modelos de 1–2B, um TTL curto pode evitar reloads sucessivos. O valor deve vir de benchmark.

VAD e componentes minúsculos podem ter política diferente.

## 9. Hardware de referência atual

Plataforma de teste:

- Intel i7-3770;
- 8 GB RAM;
- sem GPU dedicada;
- Fedora Linux;
- CPU com AVX, sem AVX2.

Consequências:

- preferir runtimes compilados para a CPU real;
- evitar binários que assumam AVX2;
- limitar contexto;
- não manter múltiplos LLMs grandes residentes;
- admission precisa observar MemAvailable e pressão;
- 3–4B é teto experimental, não baseline;
- 1–2B quantizado é o principal espaço de interesse para coprocessamento geral.

O hardware é uma restrição do laboratório atual, não da arquitetura da Luna.

## 10. Candidatos registrados

### Sensory / voice

| Candidato | Papel | Observação |
|---|---|---|
| Silero VAD | fala/silêncio | muito barato; pode evitar ASR contínuo |
| Whisper tiny/base via whisper.cpp | ASR PT-BR | CPU-only; quantizável; baseline preferida ao Whistle por suportar português |
| Piper pt-BR | TTS | baseline leve |
| Kokoro-82M | TTS | candidato de maior qualidade a medir |
| CAMPPlus / sherpa-onnx | speaker context | não usar como autenticação forte |

### Retrieval

| Candidato | Papel |
|---|---|
| EmbeddingGemma 308M | embeddings / semantic retrieval |
| mMARCO MiniLM multilingual | reranking |

### Reflexos sub-1B

| Candidato | Papel sugerido |
|---|---|
| FunctionGemma 270M | function mapping com fine-tuning |
| Granite 4.0 350M | classify / extract / structure |
| LFM2.5-350M | transform / extract / summarize |

### Coprocessadores ~1–2B

| Candidato | Papel sugerido |
|---|---|
| Granite 4.0 1B | distill / RAG / function support |
| Qwen3-1.7B | baseline intermediária |
| Qwen3.5-2B | candidato principal a suporte geral |

### Teto experimental

| Candidato | Papel sugerido |
|---|---|
| Phi-4-mini-instruct 3.8B | análise local mais difícil, somente sob admission |

Nenhum modelo é escolhido definitivamente até benchmark no host real.

## 11. Por que Qwen3.5-2B é especialmente interessante

Ele ocupa um meio-termo importante:

- maior capacidade que os reflexos de 270–350M;
- muito menor que modelos locais de 7–8B;
- adequado para tarefas que podem tolerar segundos de latência;
- pode reduzir contexto mecânico antes de chamadas remotas;
- pode permanecer completamente fora do caminho crítico quando a interação exigir rapidez.

Papel projetado:

> **general-purpose local cognitive support**, não "LLM principal".

Usos:

- evidence building;
- distillation;
- normalization;
- handoff;
- análise leve de tool outputs;
- extração complexa;
- preparação de contexto;
- fallback offline limitado.

## 12. FunctionGemma e superfície de tools

FunctionGemma é interessante quando a superfície de ações estiver estável.

O Google posiciona o modelo de 270M especificamente para function calling local e recomenda fine-tuning específico.

Possível futuro:

~~~text
dataset Luna
"abra o Firefox no Reddit"
→ browser.open(...)

"silencie o computador"
→ audio.set_volume(0)

"mande isso para o Codex"
→ agent.start(...)
~~~

Fine-tuning só faz sentido depois que o contrato de capabilities estiver suficientemente estável.

Antes disso, Granite/LFM/Qwen podem ser melhores para exploração.

## 13. Memória local

EmbeddingGemma é uma candidata particularmente alinhada com a arquitetura da Luna.

Exemplo:

~~~text
"qual foi o problema do resumo da Cloudflare?"
        │
        ▼
embedding(query)
        │
        ▼
semantic retrieval
        │
        ▼
rerank
        │
        ▼
memórias relevantes
        │
        ▼
Context Builder
~~~

Benefícios:

- menos tokens;
- menos contexto irrelevante;
- recuperação independente de provider;
- funcionamento offline;
- privacidade melhor quando os dados não precisam sair da máquina.

A política de Memory continua no Luna Core. O modelo de embedding é mecanismo de indexação, não dono da memória.

## 14. Evidence packet candidato

Uma estrutura comum pode ser útil:

~~~yaml
kind: evidence_packet_v1

known_facts:
  - ...

observations:
  - ...

failures:
  - ...

changed_resources:
  - ...

unresolved:
  - ...

sources:
  - resource://...

transform:
  capability: cognition.distill
  model: qwen3.5-2b
~~~

Esse pacote deve ser pequeno e rastreável.

Qualquer item importante precisa manter referência para evidência original.

## 15. Benchmark antes de integrar

Corpus inicial deve usar atividades reais da Luna:

- logs de cargo/npm;
- outputs do TaskGraph;
- PlanV1;
- handoff Planner → Worker;
- handoff Worker → consolidação;
- resultados de tools;
- project memory;
- structured output;
- classificação de intenção.

Medir por modelo/capability:

- load time;
- TTFT;
- throughput;
- elapsed;
- RAM peak;
- CPU;
- temperatura quando observável;
- taxa de resposta válida;
- taxa de schema válido;
- completude;
- omissões;
- qualidade;
- tamanho antes/depois;
- input remoto evitado;
- output remoto evitado;
- chamadas evitadas;
- erro silencioso.

O teste importante não é "qual modelo conversa melhor".

É:

> **qual modelo executa esta capability com qualidade suficiente e menor custo total?**

## 16. Projeção de etapas

Estas etapas são **rótulos de pesquisa**, não novas fases LR já comprometidas.

### LCS-L0 — benchmark harness

- integrar um runner isolado;
- medir hardware;
- comparar 350M / 1B / 1.7–2B / 3.8B;
- corpus real;
- sem alterar routing de produção.

### LCS-L1 — voice primitives

- VAD;
- Whisper;
- TTS baseline;
- medir latência e CPU;
- pode convergir com objetivos futuros de LR-9.

### LCS-L2 — semantic retrieval

- embeddings locais;
- índice;
- reranker;
- Context Builder experimental;
- medir redução de contexto.

### LCS-L3 — distillation / evidence packets

- tool output;
- logs;
- handoff;
- preservação de raw evidence;
- remote-token savings.

### LCS-L4 — reflex routing

- classify/extract/tool mapping;
- apenas ações de baixo risco;
- fail closed;
- confidence/validation.

### LCS-L5 — Runtime migration

Quando o AI-Native Runtime possuir cognition capabilities:

- substituir backend interno;
- mover model registry;
- mover load/unload;
- mover hardware admission;
- preservar policy da Luna.

## 17. Relação com a trilha LR atual

**LR-8 não deve ser expandida por esta descoberta.**

LR-8 continua focada em quota, admission, accounting, cooldown/circuit breaker e painel para providers remotos conforme seu contrato atual.

A ideia local pode reutilizar princípios aprendidos em LR-8, mas não deve ser inserida silenciosamente na fase.

Possíveis pontos posteriores:

- **LR-9 / Voice:** candidatos VAD/ASR/TTS locais podem ser avaliados;
- **LR-14 / Economia-Timing-Distribuição:** incluir métricas de remote tokens avoided, local load time, local RAM/CPU e local-vs-remote decision;
- **AI-Native Runtime:** destino arquitetural do lifecycle/model registry local.

## 18. Segurança e confiança

Resultados locais são inputs derivados e precisam de provenance.

Políticas recomendadas:

- fail closed em parsing/schema;
- não promover "confidence" não calibrada a garantia;
- preservar raw evidence;
- limitar dados acessíveis ao processo local;
- revisar telemetria de runtimes terceiros;
- não tratar speaker ID como autenticação;
- não permitir que economia contorne permissões;
- ações sensíveis continuam passando pelos mesmos guardrails do Luna Core.

## 19. Fontes técnicas iniciais

Verificadas em 03/10/2026:

- whisper.cpp: https://github.com/ggml-org/whisper.cpp
- sherpa-onnx: https://github.com/k2-fsa/sherpa-onnx
- Piper voices: https://huggingface.co/rhasspy/piper-voices
- Kokoro-82M: https://huggingface.co/hexgrad/Kokoro-82M
- FunctionGemma: https://ai.google.dev/gemma/docs/functiongemma
- EmbeddingGemma: https://ai.google.dev/gemma/docs/embeddinggemma
- LFM2.5-350M: https://huggingface.co/LiquidAI/LFM2.5-350M
- Granite 4.0 1B: https://huggingface.co/ibm-granite/granite-4.0-1b
- Qwen3-1.7B: https://huggingface.co/Qwen/Qwen3-1.7B
- Qwen3.5-2B: https://huggingface.co/Qwen/Qwen3.5-2B
- Phi-4-mini-instruct: https://huggingface.co/microsoft/Phi-4-mini-instruct
- mMARCO MiniLM: https://huggingface.co/cross-encoder/mmarco-mMiniLMv2-L12-H384-v1

## 20. Princípio final

A Luna não fica mais inteligente por carregar mais modelos.

Ela fica mais eficiente quando consegue escolher **a menor capacidade suficiente para cada trabalho**, preservando modelos fortes para aquilo que realmente exige força.

> **Local prepara. Frontier julga. Specialist agents agem quando a tarefa exige especialização. Luna continua sendo a autoridade que coordena tudo isso.**
