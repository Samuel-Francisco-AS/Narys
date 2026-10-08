# Narys pós-LR-11 — Cognitive Evolution Track

**Estado:** PLANEJADA / TRILHA EXPERIMENTAL — registrada em 07/10/2026.  
**Posição:** elegível para execução somente após o fechamento da LR-11.  
**Natureza:** pesquisa arquitetural e prototipação; não altera o escopo de LR-9, LR-10 ou LR-11 e não declara implementação atual.

## 1. Motivação

A Narys já caminha para um sistema agentivo com identidade persistente, múltiplos
recursos cognitivos, orquestração, TaskGraph, rate/resilience, economia de quota,
safe handoff e Specialist Agents.

A trilha pós-LR-11 não existe para repetir o padrão do ecossistema de agentes com
"mais memória", "mais modelos", "mais tools" ou "mais agentes". O objetivo é
investigar propriedades arquiteturais que façam a capacidade da Narys emergir do
sistema como um todo, e não de uma LLM específica.

Tese de produto:

> **Narys transforma recursos cognitivos escassos em capacidade acumulativa.**

A propriedade desejada é que experiência útil possa reduzir, e não aumentar, a
dependência futura de raciocínio remoto caro:

~~~text
entender
→ estruturar
→ valorar
→ pensar
→ agir
→ provar
→ aprender
→ cristalizar
→ gastar menos da próxima vez
~~~

## 2. Critério de entrada

Uma proposta entra nesta trilha somente quando responder positivamente a pelo
menos uma destas perguntas:

1. muda uma propriedade estrutural do sistema, em vez de apenas adicionar uma feature;
2. reduz dependência de um provider/modelo específico;
3. converte experiência passada em capacidade operacional futura;
4. melhora simultaneamente qualidade e eficiência, ou cria mecanismo mensurável
   para negociar esse trade-off;
5. cria uma primitive reutilizável pela Narys e, futuramente, pelo ecossistema
   AI-Native.

Ter equivalente parcial em outros projetos **não invalida** uma ideia. Também
não será alegado ineditismo mundial sem pesquisa de prior art dedicada. O foco é
construir uma arquitetura autoral, coerente e verificável.

## 3. Princípios

- **Modelos são recursos, não identidade nem autoridade final.**
- **Raciocínio é recurso escasso.** Tokens, cota, dinheiro, latência, CPU, RAM e
  atenção competem por orçamento.
- **Conhecimento tem validade.** Lembrar não implica saber com confiança.
- **Execução precisa de prova.** Um worker não decide sozinho que terminou certo.
- **Experiência deve amortizar custo futuro.**
- **Autonomia nasce de contratos e permissões, não de liberdade implícita.**
- **Complexidade cognitiva deve ser proporcional ao risco e ao ganho esperado.**
- **Narys decide; o futuro AI-Native Runtime executa primitives de ambiente.**

## 4. Mecanismos candidatos

### 4.1. Cognitive IR — representação intermediária cognitiva

A intenção do usuário não precisa circular como prompt bruto entre todos os
componentes. A Narys poderá compilar uma solicitação para uma representação
tipada contendo, conforme necessário:

- objetivo;
- restrições;
- fatos e hipóteses;
- incógnitas;
- authority envelope;
- budget;
- risk class;
- evidence requirements;
- quality floor;
- success/stop conditions;
- dependências e estado da tarefa.

Planner, worker, memória, modelo local, Specialist Agent e Runtime receberiam
projeções diferentes da mesma representação.

**Benefício:** menos reenvio de contexto, menos drift entre modelos, handoffs mais
determinísticos e menor acoplamento a prompts/providers.

**Hipótese de inovação:** aproximar a cognição agentiva de uma pipeline de
compilação, dando à Narys uma representação interna independente da LLM que a
processa.

### 4.2. Cognitive Metabolism — metabolismo cognitivo

A Narys manterá um estado agregado dos recursos cognitivos e operacionais:
allowance, dinheiro autorizado, latência, CPU/RAM, contexto, urgência,
reversibilidade, risco e qualidade mínima.

A partir desse estado ela poderá escolher não apenas **qual recurso** usar, mas
**quanto raciocínio** a tarefa merece. Perfis candidatos:

- Reflex;
- Normal;
- Deliberative;
- Adversarial.

O perfil pode escalar ou reduzir durante a própria tarefa mediante evidência.

**Benefício:** transformar LR-8.5 de seleção econômica de recurso em gestão da
profundidade cognitiva do sistema.

**Hipótese de inovação:** inteligência passa a ser administrada como metabolismo,
com esforço proporcional ao ganho marginal esperado.

### 4.3. Attention Market — mercado de atenção

Memórias, documentos, histórico, resultados de tools, regras e hipóteses
competirão por espaço no contexto.

O valor de um item poderá considerar:

- relevância;
- confiabilidade;
- novidade;
- impacto potencial;
- dependências;
- custo em tokens;
- redundância com itens já selecionados.

**Benefício:** contextos menores e mais densos, menor custo e menor distração do
modelo.

**Hipótese de inovação:** tratar atenção/contexto como recurso econômico explícito,
não apenas como top-k similarity retrieval.

### 4.4. Epistemic Ledger — contabilidade de conhecimento

Fatos importantes poderão carregar:

- origem;
- momento da observação;
- confiança;
- volatilidade;
- evidências;
- dependências;
- contradições;
- última verificação.

A trilha investigará **dívida epistemológica**: conhecimento envelhece em ritmos
diferentes. Quando uma decisão depender de informação cuja dívida excedeu um
limite, a Narys deve revalidá-la antes de agir.

**Benefício:** reduz decisões baseadas em memória velha, frágil ou apenas
plausível.

**Hipótese de inovação:** separar memória de crença operacional e tornar a
deterioração da confiança uma propriedade mensurável do runtime.

### 4.5. Proof-Carrying Actions — ações acompanhadas de prova

Uma ação nasce com um contrato de evidência. O executor entrega resultado +
evidência, e não apenas uma declaração textual de sucesso.

Exemplos:

- código: diff + testes + estado esperado;
- arquivo: leitura/hash/metadata pós-operação;
- envio: receipt;
- mudança de configuração: leitura posterior confirmando o novo valor;
- pesquisa: provenance/evidência suficiente para as claims relevantes.

**Benefício:** reduz falsos "feito", melhora auditoria e permite composição segura
entre workers diferentes.

**Hipótese de inovação:** deslocar a autoridade de conclusão do agente para um
contrato verificável independente dele.

### 4.6. Skill Foundry — cristalização de raciocínio

Execuções bem-sucedidas poderão ser analisadas para detectar trabalho
repetitivo que não deveria continuar consumindo inteligência probabilística.

Uma solução madura pode ser cristalizada como:

- função;
- script;
- regra;
- template estruturado;
- workflow;
- mini-tool;
- sequência determinística validada.

Ciclo de maturidade candidato:

~~~text
experimental → validated → trusted → degraded → retired
~~~

A promoção depende de evidência e cobertura; falhas ou mudança de ambiente podem
rebaixar uma skill.

**Benefício:** amortização cognitiva. Inteligência cara já consumida vira
infraestrutura barata reutilizável.

**Hipótese de inovação:** integrar geração/maturação de skills à economia
cognitiva, evidência de sucesso e validade, em vez de manter apenas uma biblioteca
estática de habilidades.

### 4.7. Semantic Immune System — imunidade operacional

Falhas relevantes poderão gerar assinaturas semânticas compostas por:

- condições;
- sintomas;
- estratégia que falhou;
- causa conhecida ou hipótese;
- defesa/recuperação comprovada;
- validade e escopo da defesa.

Antes de uma nova execução, a Narys compara o cenário com assinaturas existentes.
Uma defesa compatível pode prevenir o erro em vez de repetir tentativa + retry.

**Benefício:** confiabilidade cresce com experiência real.

**Hipótese de inovação:** converter histórico de incidentes em defesas
operacionais executáveis e sujeitas a validade.

### 4.8. Shadow Cognition — cognição contrafactual

Tarefas de alta incerteza, alto custo ou baixa reversibilidade podem gerar
pequenos ramos cognitivos sem side effects. Esses ramos propõem estratégias
alternativas e são avaliados contra o Cognitive IR e os contratos da tarefa.

Somente o ramo escolhido recebe autoridade operacional.

**Benefício:** reduz compromisso prematuro com o primeiro plano plausível.

**Hipótese de inovação:** usar futuros cognitivos temporários como mecanismo
seletivo, evitando debates multiagente permanentes e caros.

### 4.9. Intent Field — campo persistente de intenções

Objetivos duráveis deixam de existir apenas como mensagens históricas e passam a
ser entidades com:

- prioridade;
- urgência;
- dependências;
- progresso;
- authority;
- decay;
- conflitos;
- relação com outros objetivos.

Novas tarefas podem reforçar, contrariar, suspender ou satisfazer intenções
anteriores.

**Benefício:** continuidade estratégica verdadeira. Decisões passam a ser
avaliadas contra objetivos persistentes, e não apenas contra o prompt corrente.

**Hipótese de inovação:** deslocar a unidade de continuidade de "conversa" para
um campo de objetivos concorrentes governado por authority envelopes.

### 4.10. Local Reflex Mesh — malha de reflexos locais

Modelos locais pequenos não precisam imitar um grande planner. Eles podem ocupar
papéis estreitos de baixa autoridade:

- classificação;
- triagem;
- compressão;
- extração;
- estimativa de dificuldade;
- reranking;
- detecção de anomalias;
- seleção de memória;
- primeira tentativa de skills conhecidas.

Recursos fortes/remotos continuam responsáveis por julgamento que exija maior
capacidade.

**Benefício:** menos chamadas remotas, melhor uso do hardware local e
funcionalidade parcial offline.

**Hipótese de inovação:** organizar modelos locais como camada subcognitiva de
reflexos especializados, em vez de tratá-los como "cérebro menor".

## 5. Composição pretendida

Os mecanismos devem poder operar como um ciclo coerente:

~~~text
Intent Field
    ↓
Cognitive IR
    ↓
Attention Market + Epistemic Ledger
    ↓
Cognitive Metabolism
    ↓
[Shadow Cognition quando justificável]
    ↓
Execution Contract
    ↓
Proof-Carrying Action
    ↓
Experience
   ↙   ↘
Immune   Skill Foundry
System       ↓
        capacidade reutilizável
~~~

O valor da trilha está mais nessa composição do que em qualquer primitive isolada.

## 6. Fronteira Narys ↔ AI-Native Runtime

A separação arquitetural desejada permanece:

> **Narys decide. O Runtime executa.**

Responsabilidade da Narys:

- intenção;
- contexto;
- estratégia;
- confiança;
- memória;
- budget cognitivo;
- escolha de recursos;
- contracts;
- aprendizado;
- autorização.

Responsabilidade futura do AI-Native Runtime:

- visão do ambiente;
- filesystem/processos;
- navegador;
- dispositivos;
- sandbox;
- primitives determinísticas de ação;
- execução local;
- receipts do ambiente.

Uma fronteira candidata é:

~~~text
usuário
  ↓
Narys
  ↓
Cognitive IR + Action Contract
  ↓
AI-Native Runtime
  ↓
Evidence Receipt
  ↓
Narys
~~~

A trilha não deve absorver o Runtime nem duplicar seu futuro papel.

## 7. Decomposição preliminar

A numeração **NX** é deliberadamente separada das LRs de entrega. Ela representa
pesquisa de evolução cognitiva e pode ser refinada antes de implementação.

1. **NX-0 — Cognitive Architecture Lab**  
   Especificar métricas, invariantes, threat model e experimentos antes de código
   de produção.

2. **NX-1 — Cognitive Contracts**  
   Cognitive IR, authority/evidence contracts e Proof-Carrying Actions.

3. **NX-2 — Epistemic Layer**  
   Epistemic Ledger, validade, contradição e dívida epistemológica.

4. **NX-3 — Attention Economy**  
   Mercado de atenção, budget de contexto e avaliação contra retrieval simples.

5. **NX-4 — Cognitive Metabolism**  
   Modos de profundidade, ganho marginal e integração com LR-8/8.5.

6. **NX-5 — Experience Engine**  
   Skill Foundry + Semantic Immune System.

7. **NX-6 — Adaptive Cognition**  
   Shadow Cognition e seleção dinâmica de topologia cognitiva.

8. **NX-7 — Persistent Intent**  
   Intent Field, conflitos, decay e authority envelopes.

9. **NX-8 — Reflex Layer**  
   Local Reflex Mesh e integração com Local Cognitive Support.

A ordem acima é uma hipótese de trabalho, não autorização de implementação.

## 8. Métricas de pesquisa

A trilha deverá tentar demonstrar propriedades, não apenas screenshots.

Métricas candidatas:

- qualidade útil por unidade de custo;
- tokens remotos evitados;
- chamadas remotas evitadas;
- percentual de tarefas resolvidas por skill/reflexo após maturação;
- taxa de revalidação epistemológica necessária;
- falsos "completed" prevenidos por contracts;
- incidentes recorrentes prevenidos pelo Immune System;
- context tokens por unidade de evidência útil;
- ganho de qualidade de Shadow Cognition por custo adicional;
- latência e custo antes/depois de uma capacidade cristalizada;
- dependência de provider específico;
- taxa de escalada Reflex → Normal → Deliberative → Adversarial.

Uma hipótese só merece promoção para produto quando seu ganho puder ser observado
sem esconder custo em outra camada.

## 9. Antiobjetivos

Esta trilha não deve:

- criar multi-agent theater;
- usar LLM quando uma função determinística basta;
- transformar número de providers/agentes em KPI;
- aumentar autonomia sem contracts/permissões;
- confundir memória com verdade;
- transformar modelos locais em planners fracos por obrigação;
- introduzir dependência prematura do futuro Runtime;
- perseguir "inovação" sacrificando testabilidade ou segurança;
- declarar ineditismo mundial sem evidência.

## 10. Gate para sair de pesquisa

Cada bloco NX deve começar com uma hipótese falsificável e um baseline.

Um bloco só entra no núcleo da Narys quando houver evidência de pelo menos uma
melhoria relevante em:

- economia;
- qualidade;
- confiabilidade;
- continuidade;
- independência de provider;
- segurança operacional;

sem regressão desproporcional nas demais.

Falha experimental é resultado válido. O propósito desta trilha é descobrir
mecanismos úteis, não defender previamente todas as ideias registradas.

## 11. Relação com o roadmap

~~~text
... → LR-9 → LR-10 → LR-11
                       ↓
            NARYS Cognitive Evolution
              NX-0 → ... → NX-8
~~~

A trilha fica **reservada ao pós-LR-11**. Seu registro não antecipa implementação,
não modifica os gates atuais e não obriga que todos os blocos sejam executados.

LR-12, LR-13, trilhas de avatar, NARYS-TERM e NARYS-NORM continuam independentes.
A posição relativa exata entre essas trilhas será decidida no checkpoint
pós-LR-11 segundo custo, dependências e valor experimental.

## 12. Decisão registrada

A direção estratégica é investigar uma Narys cuja inteligência útil não resida
integralmente em nenhum modelo individual.

A meta de longo prazo é tornar verdadeira, e mensurável, a relação:

> **mais experiência útil → menos inteligência externa necessária para obter a mesma capacidade.**
