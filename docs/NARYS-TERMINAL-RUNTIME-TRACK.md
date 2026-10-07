# NARYS-TERM — Terminal Runtime & Interactive Shell Surface

**Estado:** TRILHA FUTURA — registrada, sem posição definitiva no roadmap.
**Origem:** decisão de produto durante a PERF-1B em 07/10/2026.
**Relação com PERF:** fora de escopo da PERF-1B/1C/1D; não deve atrasar o
fechamento da trilha de performance.

## Visão

Transformar o workspace da Narys em uma superfície capaz de hospedar um
**terminal Linux real**, compartilhável entre usuário e agentes, sem confundir
Presentation com execução de processos.

A meta não é apenas embutir um prompt visual. A Narys deve oferecer uma sessão
de terminal interativa com PTY, ownership explícito, auditoria de origem,
políticas de autorização e integração futura com Luna/SpecialistAgents.

A superfície pode coexistir com Conversation, Tasks, Relays, System e outras
views do workspace central.

## Princípio

> o terminal é uma superfície operacional do Runtime, não um campo de texto que
> envia comandos cegamente.

Frontend renderiza a sessão; autoridade de processo, PTY, cwd, lifecycle e
políticas permanece no Core/nativo.

## Arquitetura alvo

```text
Economy / Narys Shell
        │
        ▼
Terminal Surface
(emulador visual)
        │
        │ IPC/event stream
        ▼
Terminal Runtime — Rust
        │
        ├── PTY session
        ├── process lifecycle
        ├── cwd / env permitidos
        ├── resize cols/rows
        ├── input/output
        └── audit / authorization
                │
                ▼
         bash / zsh / programas Linux
```

Uma biblioteca de emulação de terminal no frontend, como xterm.js ou equivalente,
é aceitável. Ela não deve possuir o processo real.

No backend, utilizar PTY real para preservar compatibilidade com ferramentas
interativas. Execução via simples `Command + stdin/stdout pipes` não substitui
o requisito principal para programas como shell interativo, vim, nano, ssh,
python REPL ou TUI.

## Sessões

Conceito inicial:

```text
TerminalSession
- session_id
- owner/origin
- cwd
- shell/process
- cols / rows
- state
- created_at
- process/pty handle
```

Capacidades mínimas futuras:

- criar sessão;
- enviar input;
- receber output incremental;
- redimensionar PTY;
- consultar estado;
- encerrar sessão;
- lidar com exit code/signal;
- recuperar UI sem fingir que processo inexistente continua vivo.

Persistência de processo entre restart do aplicativo **não é requisito inicial**.
Se desejada no futuro, deve ser uma decisão explícita.

## Origem e auditoria

Qualquer ação deve preservar autoria/origem.

Exemplo conceitual:

```text
CommandOrigin
- human
- luna
- specialist_agent
- automation
```

O Runtime deve conseguir registrar, conforme política de privacidade:

- quem originou a ação;
- sessão;
- cwd;
- comando/ação autorizada;
- timestamp;
- resultado/exit status;
- aprovações relevantes.

A implementação não deve depender de agentes "digitando teclas escondidas" na
mesma sessão humana sem ownership observável.

## Sessão humana e sessões agentivas

A direção desejada permite múltiplas sessões no workspace:

```text
Terminal
● Sam
  ~/Projetos/Narys

● Luna / Task #104
  cargo test

● Codex / Task #105
  ~/Projetos/Narys
```

Uma sessão humana pode ser interativa e visível. Sessões de agentes devem ser
identificáveis e auditáveis.

Compartilhar a mesma PTY entre humano e agente só deve acontecer por mecanismo
explícito, com regras claras de concorrência/controle.

## Segurança e autorização

Shell arbitrário é capability de alto impacto. O Runtime deve possuir fronteira
de autorização própria.

Comandos/ações sensíveis podem exigir aprovação conforme policy, por exemplo:

- `sudo`;
- alterações destrutivas de filesystem;
- instalação/remoção de pacotes;
- push/publicação;
- manipulação de credenciais;
- comandos que escapem do workspace autorizado;
- processos persistentes/daemons;
- rede quando restrita por policy.

A arquitetura deve distinguir pelo menos:

- leitura/observação;
- comando comum dentro de contexto autorizado;
- ação destrutiva ou privilegiada;
- delegação para agente.

Não implementar allowlist textual simplista como única barreira de segurança.

## Relação com agentes

A trilha deve poder evoluir para que Luna, Codex, Copilot e outros
SpecialistAgents usem sessões ou runtimes de terminal sem depender da UI.

Exemplo futuro:

```text
Usuário: "Luna, rode os testes."

Luna
→ solicita ação terminal
→ policy avalia
→ Terminal Runtime executa
→ output estruturado/eventos
→ UI mostra a mesma sessão
→ resultado retorna ao agente
```

A UI não deve ser requisito para a execução agentiva; isso preserva a direção
AI-Native do projeto.

## Relação com AI-Native Runtime

Essa trilha pode se tornar uma das primitivas operacionais do ecossistema:

- executar comando;
- abrir sessão;
- observar processo;
- enviar input;
- receber saída;
- manipular cwd;
- atribuir origem;
- pedir aprovação;
- gerar eventos auditáveis.

A API estruturada deve ser preferida quando houver primitive específica melhor
que shell. O terminal é ferramenta universal e fallback operacional, não
substituto de APIs nativas de filesystem, Git, navegador ou outras capabilities.

## UI futura

A Economy Shell pode ganhar uma entrada `Terminal` na navegação.

A view central pode suportar:

- tabs/sessões;
- terminal principal;
- indicação de cwd;
- owner/origin;
- estado do processo;
- sessão humana vs agentiva;
- approvals;
- exit status;
- reconexão visual.

Não adicionar gráficos/efeitos contínuos que contradigam a direção de performance
da Economy Shell.

A atual view `Shell / Home` continua sendo home/launcher e não precisa fingir
ser terminal até esta trilha existir.

## Não objetivos iniciais

A primeira entrega desta trilha não precisa:

- substituir o terminal do sistema inteiro;
- implementar multiplexer completo estilo tmux;
- persistir PTYs após reboot;
- suportar Windows/macOS no primeiro spike;
- conceder sudo automático;
- dar shell irrestrito a agentes;
- misturar terminal com Headless Runtime;
- implementar parser semântico completo de shell;
- substituir primitives estruturadas do Narys Runtime.

## Pré-requisitos sugeridos

Antes da implementação real, revisar:

- resultado da PERF-1;
- lifecycle do Headless Runtime;
- modelo de permissions/approvals;
- LR de ferramentas reais;
- fronteira de SpecialistAgents;
- política de workspace/filesystem;
- necessidades do AI-Native Runtime.

A trilha pode começar antes de todos esses itens somente como POC isolada de PTY,
sem expor execução agentiva irrestrita.

## POC sugerida

Primeiro spike:

1. criar uma única sessão PTY local;
2. abrir shell do usuário;
3. renderizar em uma view Terminal;
4. input/output bidirecional;
5. resize real;
6. `cd`, `ls`, `cargo test` e programa interativo simples;
7. encerrar/recriar sem processo órfão;
8. nenhuma capability de agente ainda.

Gate da POC:

> Narys hospeda uma sessão Linux interativa real, com lifecycle limpo e sem
> shell invisível/órfão.

Somente depois introduzir ownership de agentes e policies de autorização.

## Posição no roadmap

Sem posição definitiva.

Esta trilha é intencionalmente registrada agora para preservar a direção de
produto, mas **não deve interromper PERF-1**. Sua posição deverá ser escolhida
após PERF e considerando LR-13/tools reais, SpecialistAgents e o avanço do
AI-Native Runtime.
