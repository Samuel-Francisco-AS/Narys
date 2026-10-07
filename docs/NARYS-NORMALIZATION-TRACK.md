# NARYS-NORM — Identity & Namespace Normalization

**Estado:** PLANEJADA / SEM POSIÇÃO FIXA  
**Criada em:** 07/10/2026  
**Execução:** somente em checkpoint estável; não bloqueia nem preempta LR-11.

## 1. Marco de identidade

Em 07/10/2026 o projeto anteriormente conhecido como **Assistente-3D** adotou
**Narys** como nome do produto/ecossistema.

A decisão separa duas identidades que não devem ser colapsadas:

- **Narys** — sistema, produto/ecossistema, runtime e superfícies técnicas;
- **Luna** — agente persistente, identidade/persona e autoridade cognitiva que
  opera dentro de Narys.

Portanto, a futura normalização **não é um search-and-replace de "Luna" para
"Narys"**. O objetivo é tornar cada nome semanticamente correto no lugar certo.

O nome do repositório remoto ainda pode permanecer temporariamente
`Assistente-3D` até a alteração de metadata do GitHub e a revisão de
compatibilidade. Isso não invalida a decisão de identidade.

## 2. Por que existe uma trilha própria

O projeto nasceu como uma aplicação de presença 3D e acumulou nomes antigos em
camadas que hoje têm responsabilidades muito maiores: Luna Core, cognition,
agents, persistence, security, TaskGraph, Scheduler, ResourceAllocator,
telemetry, continuations e interfaces.

Renomear somente a interface produziria inconsistência. Renomear tudo de uma
vez pode quebrar dados e credenciais.

Alguns identificadores podem influenciar:

- bundle/application identifier do Tauri;
- app-local-data directory;
- caminhos de SQLite e arquivos auxiliares;
- Stronghold / credential-store service names e chaves;
- nomes de package/crate e artefatos de build;
- labels de janelas, comandos IPC, logs e telemetry;
- variáveis de ambiente, scripts, CI e automações;
- namespaces, módulos, tipos e testes;
- referências em documentação, screenshots e material histórico;
- URLs/remotes Git após eventual rename do repositório.

A regra é: **branding pode mudar rápido; identidade persistida só muda com
migração provada.**

## 3. Princípios

1. **Narys é o sistema; Luna é a agente.**  
   Uma ocorrência de `Luna` não é dívida automaticamente.

2. **Compatibilidade antes de limpeza estética.**  
   Um nome legado pode permanecer internamente se removê-lo criar risco sem
   benefício operacional suficiente.

3. **Dados locais têm precedência.**  
   Nenhuma mudança pode fazer uma instalação existente parecer uma instalação
   nova e vazia sem migração explícita.

4. **Segredos não são recriados por conveniência.**  
   Mudanças de service/account/key no credential store devem preservar acesso
   ou possuir migração atômica e verificável.

5. **História não é reescrita.**  
   Commits, branches históricas, documentos de gates antigos e evidências podem
   manter o nome válido à época.

6. **Mudança de nome não é feature work.**  
   NARYS-NORM não deve introduzir novas capabilities, providers ou políticas
   cognitivas para justificar a refatoração.

7. **Sem posição fixa.**  
   A trilha entra quando houver espaço operacional e um checkpoint estável. O
   simples fato de ela existir não desloca LR-11 nem outras entregas em curso.

## 4. Escopo de auditoria

A primeira atividade é produzir um inventário mecânico e semântico, sem editar
nada. A busca deve cobrir pelo menos:

- `Assistente-3D`, `Assistente 3D`, variantes de casing e slugs;
- `Luna`, `luna` e derivados;
- nomes do crate/package/app;
- Tauri config, bundle identifiers, capabilities e window labels;
- app data, SQLite, Stronghold e credential-store identifiers;
- comandos IPC e nomes públicos de API;
- nomes de processos/subprocessos e agent backends;
- logs, audit events e telemetry;
- frontend routes, components e CSS/data attributes relevantes;
- scripts, GitHub workflows, assets e paths;
- documentação atual versus documentação histórica;
- URLs, remotes e links que dependam do nome do repositório.

Cada ocorrência deve receber uma classe:

`PUBLIC_NARYS`
: deve apresentar Narys ao usuário/desenvolvedor.

`AGENT_LUNA`
: pertence semanticamente à agente e deve continuar Luna.

`COMPATIBILITY_LEGACY`
: nome antigo preservado temporária ou permanentemente por dados/API/path.

`HISTORICAL`
: evidência factual de um checkpoint antigo; não reescrever sem necessidade.

`REMOVE_OR_RENAME`
: dívida segura de normalizar.

## 5. Subfases internas

### NORM-A — Inventory & Classification

Somente leitura e relatório.

Entregáveis:

- inventário por arquivo/identificador;
- classe atribuída;
- riscos e dependências;
- lista de nomes públicos que podem mudar sem migração;
- lista de identificadores que exigem estratégia de compatibilidade.

**Gate:** nenhuma alteração funcional; inventário revisado antes de qualquer
rename em massa.

### NORM-B — Public Identity Surface

Normalizar primeiro as superfícies de menor risco:

- README/documentação corrente;
- títulos e labels visíveis;
- package description quando seguro;
- assets/branding;
- mensagens de boot/status;
- referências correntes ao nome do produto.

**Gate:** Narys aparece como produto sem alterar storage, credenciais ou estado.

### NORM-C — Compatibility-Sensitive Identity

Tratar somente com plano de migração explícito:

- Tauri identifier/bundle metadata;
- app-local-data path;
- SQLite location;
- Stronghold/credential store;
- settings e caches;
- qualquer identificador usado como chave persistente.

Estratégias possíveis incluem alias temporário, leitura do path legado,
migração copy/verify/switch, dual-read por uma versão ou preservação deliberada
do identificador interno antigo.

**Gate:** instalação existente abre com os mesmos dados, sessões, memória,
settings e credenciais; rollback e falha parcial não apagam o estado anterior.

### NORM-D — Internal Namespace Cleanup

Depois que a fronteira semântica estiver clara:

- módulos e tipos cujo nome antigo não representa mais a responsabilidade;
- comentários e logs não históricos;
- helpers, constants e aliases;
- nomes de processo/artefato quando não forem contratos externos.

Não renomear `luna` apenas para maximizar uniformidade: `luna` continua
correto quando representa a agente.

**Gate:** diff revisável e sem mistura com feature work.

### NORM-E — Final Audit & Release Gate

Executar busca final e gates completos.

O fechamento exige:

- build/typecheck/testes verdes;
- release build quando aplicável;
- migração de dados testada a partir de uma instalação legada;
- credenciais recuperadas após restart;
- conversa, memória, TaskGraph/continuations e settings preservados;
- ausência de processo/path duplicado criado por rename acidental;
- documentação corrente consistente;
- toda ocorrência restante de nome legado classificada e justificada.

## 6. Riscos prioritários

### App identifier e diretório de dados

Alterar o identificador do aplicativo pode fazer o sistema operacional ou Tauri
usar outro diretório. Isso pode aparentar perda de banco/settings mesmo quando
os arquivos antigos continuam no disco.

### Credential store / Stronghold

Se service/account/key derivarem do nome antigo, uma troca direta pode tornar a
credencial existente invisível. A migração deve ler, verificar e só então
desativar o identificador legado.

### SQLite e migrations

Nome de arquivo/path pode mudar; nomes de tabelas ou migrations não precisam ser
renomeados apenas por estética. Schema histórico deve permanecer estável quando
não houver ganho real.

### IPC e contratos

Comandos Tauri, event names ou payload fields podem ser contratos entre Rust e
frontend. Renomear exige alteração coordenada e testes; aliases temporários são
preferíveis quando houver risco.

### Documentação histórica

Relatórios de LR/UIP/PERF registram o projeto como ele existia no momento.
Atualizar títulos históricos indiscriminadamente reduziria a qualidade da
evidência.

## 7. Relação com o roadmap

NARYS-NORM é **transversal**.

Ela não recebe número LR e não possui posição definitiva entre LR-9, LR-10,
LR-11, LR-12 ou PERF. Pode ser iniciada amanhã, após LR-11, ou em uma janela
posterior, desde que exista um checkpoint estável.

Regra atual:

> **não interromper nem atrasar a finalização de LR-11 apenas para executar
> normalização de naming.**

A exceção é um conflito real em que continuar LR-11 criaria novo contrato
público ou persistente com nomenclatura errada e custo de migração claramente
maior. Nesse caso, executar apenas o mínimo bloqueante e retornar à LR.

## 8. Fora de escopo

NARYS-NORM não inclui, por si só:

- redesenhar a arquitetura cognitiva;
- implementar Tool Runtime;
- adicionar providers/agentes;
- alterar política econômica;
- refazer UI da PERF;
- mudar identidade/persona da Luna;
- reescrever histórico Git;
- renomear schema ou storage apenas por estética;
- alterar formatos persistidos sem necessidade de compatibilidade.

## 9. Critério de sucesso

A trilha fecha quando um novo contribuidor consegue olhar o sistema e entender,
sem contexto oral adicional:

> **Narys é o sistema. Luna é a agente.**

E essa distinção é verdadeira não só na documentação e UI, mas também nos
namespaces e contratos em que a mudança é segura — enquanto identificadores
legados restantes são poucos, deliberados, documentados e cobertos por
compatibilidade.
