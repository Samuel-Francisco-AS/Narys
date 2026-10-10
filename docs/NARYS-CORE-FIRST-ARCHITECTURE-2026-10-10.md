# Narys Core-First — arquitetura vinculante (10/10/2026)

**Decisão:** arquitetura **Core-First / monólito modular Rust / thin clients** aprovada pelo usuário em 10/10/2026. Este documento substitui qualquer orientação anterior de evitar refatorações extensas por princípio. **Estado: PLANEJAMENTO APROVADO; não equivale a código implementado ou gate PASS.**

**Metas distintas:** NARYS-SERVER-1 (até 48h corridas da abertura real, máximo quatro etapas) entrega o serviço headless operacional; **Narys 0.1 até 17/10/2026** deve oferecer agentes que realizem trabalho real por ferramentas controladas. Não reduzir a meta do produto a chat, POC textual, mock, terminal humano ou tarefas simuladas. [Trilha servidor](NARYS-SERVER-1-HEADLESS-SERVER-RUNTIME.md) · [Entrega agentiva 0.1](NARYS-01-AGENT-TOOLS-DELIVERY.md).

## 1. Topologia-alvo

~~~text
Termux/SSH CLI    Tauri desktop    Android futuro
       \               |                /
        \     contratos versionados      /
         +-------- Client API -----------+
                       |
       Unix socket local (0600 + SO_PEERCRED)
          ou túnel SSH; sem TCP público
                       |
           NARYS CORE (Rust, systemd --user)
  +--------------------+----------------------+
  | Conversa/Identidade| TaskGraph/Recovery   |
  | Contexto/Memória   | Scheduler/LR-8.5     |
  | Provider Registry  | Agent Registry       |
  | Approval/Authority | Tool Gateway/Broker  |
  | Trace/Telemetry    | Supervisor on-demand |
  +--------------------+----------------------+
        |                  |               |
  SQLite autoritativo  Stronghold       Providers/agents
   com migrations    + Keyring       por demanda e política
~~~

O **Core é autoridade** sobre tarefas, permissões, persistência, admissão e execução. Clientes solicitam ações, assinam eventos tipados e apresentam resultados; não possuem Scheduler, regras de aprovação ou executores independentes com mutações concorrentes. UI Tauri, avatar e experiência mobile são adaptadores de apresentação, não dependências de boot.

## 2. Liberdade de engenharia — decisão expressa do usuário

Está **autorizada a estratégia de maior eficácia técnica**, inclusive extrair módulos inteiros de src-tauri para crates Rust, refatorar interfaces e contratos, alterar ownership ou reorganizar o bootstrap Tauri. Não impor uma regra artificial de "extrair apenas o mínimo", "evitar mudanças grandes" ou "não tocar no Scheduler/Conversation". O Codex pode escolher extração integral, adaptação incremental ou refatoração dirigida por dependências conforme necessidade demonstrável.

Critério de escolha: reduzir estado duplicado, manter semântica e testes existentes, e produzir produto operacional com menor risco total **até 17/10/2026**. Reutilização é preferível quando poupa custo; não é obrigação se conservar acoplamentos for pior. Preservar compatibilidade do desktop ou documentar migração deliberada; não criar implementação paralela permanente de Conversation, Scheduler, Registry, Authority ou banco por conveniência.

Limite de engenharia não negociável: segurança real, nenhuma perda silenciosa de dados, rollback/migração reversível quando factível, sem duplicidade de execução incerta e sem PASS fictício. Refatoração estrutural não autoriza alterar credenciais, iniciar cobrança, rodar comandos privilegiados nem ampliar permissões agentivas sem gates explícitos.

## 3. Fronteiras dos módulos

- **narys-core:** processo headless proprietário das operações, state machine de tarefas, recuperação e IPC. Sem dependência obrigatória de Tauri, GTK, X11, Wayland, GNOME, WebKit ou monitor.
- **narys-domain / módulos reutilizáveis (nomes ilustrativos):** Conversation, memória/Context, Scheduler, provider/agent registry, políticas LR-8.5, TaskGraph, trace LR-9, approval e contratos tipados. Mover código quando necessário; não há exigência de criar crates para cada módulo.
- **narys-execution:** acesso às ferramentas mediado por Authority/ToolGateway/ExecutionBroker, com processo supervisionado e registro de efeitos. A existência prévia de ExecutionBroker não concede automaticamente autoridade a SpecialistAgent.
- **narys-client:** CLI acessível pelo Termux/SSH; no futuro Tauri e Android consumirão o mesmo protocolo, com adapters e eventos versionados.
- **storage/credentials:** fonte autoritativa de SQLite, migrations de dados atuais; SecretStore usa o snapshot Stronghold preexistente com Keyring humano desbloqueado via SSH. Sem duplicação de secrets, sem regravar snapshot por padrão e sem plaintext no CLI/logs.

## 4. Protocolo, dados e ciclo de vida

Contrato IPC local versionado, com tipagem e limites de tamanho/timeout, erros tipados, autenticação via credenciais do socket/peer UID, separação entre leitura e operação mutante, cancelamento idempotente e IDs duráveis. Termux conecta por SSH; **não** abrir 0.0.0.0 nem criar endpoint HTTP público. Separar sessão do cliente da vida da tarefa: desconexão não mata trabalho autorizado; reentrada consulta estado e eventos persistidos conforme retention/policy. Evento em memória não equivale a histórico durável.

Escolher **um SQLite operacional autoritativo**. Inventariar caminhos/dados das bases Core e Tauri e definir migração transacional, backup, ownership de escrita e transição do desktop; não apontar dois writers sem coordenação ao mesmo estado. Preservar dados de identidade/conversas, políticas, TaskId, trace relevante e recibos de autorização; não confundir sessão de Copilot sob /tmp com persistência durável. Schema novo somente com migration e teste de recovery.

Systemd --user + linger, readiness, restart e shutdown; nenhuma dependência de sessão SSH ou graphical target. Core ocioso leve no Fedora 44, i7-3770/8 GiB, sem GPU; SDK/CLI de especialistas ativados sob demanda, sem processos órfãos. Relatar RSS/CPU reais de maneira proporcional, sem benchmark fictício.

## 5. Ferramentas reais e segurança funcional

Até o marco 0.1, a Narys deverá possuir um **ciclo agentivo verificável**: usuário define objetivo e workspace → roteamento/plano → LLM ou SpecialistAgent solicita ferramenta → Core valida operação/autorização → execução real → resultado observado pelo agente → verificação de diff/teste/efeito → TaskGraph/trace/SQLite persistem o desfecho → cancelamento/reentrada funcionam.

Ferramentas prioritárias: inspecionar/listar/ler arquivos autorizados; criar/editar arquivos no workspace; executar comandos de build/teste com limites e observabilidade; obter diff/status e relatar resultados. Disponibilidade de ferramenta na UI não é suficiente: pelo menos uma tarefa de engenharia deve realmente **usar** ferramentas e concluir no host de validação. Priorizar ponte com agentes nativos Copilot/Codex conforme escopo LR-10/LR-11; não mascarar saída textual como execução.

**Autoridade não vem do texto da LLM.** Exigir esquema de ferramenta, escopo de workspace, política por operação, aprovação humana quando necessária, timeout/cancelamento, trilha auditável, tratamento de links simbólicos/path traversal, e limites de filesystem/rede/processo. Não promover a flag HumanLocal para agentes. Distinguir perfil assistido, isolado comprovado e modo expansivo somente com aprovação humana explícita para aquela tarefa; host-assisted sem sandbox não pode ser rotulado como sandbox. Comandos destrutivos, instalação global, segredos e escalada de privilégios não ficam autorizados por esta documentação.

Quota/preço: somente recursos existentes e já permitidos; execução real paga/inferência adicional não reutiliza o consentimento encerrado da LR-10A. Sem overage, compra, fallback pago, envio em massa ou retry financeiro implícito. O gate de custo é separado do gate de ferramentas.

## 6. Ordem e critérios de entrega

1. **SERVER-1A:** refatoração/extração do núcleo quando necessária, processo autoridade única, IPC, lifecycle e mapa/migração de dados. O Codex seleciona técnica após inventário e registra ADR curta.
2. **SERVER-1B:** Conversation/Context, provedores existentes (Groq prioritário), roteamento/TaskGraph, segredo e persistência durable sem Tauri.
3. **SERVER-1C:** CLI operacional via SSH/Termux, tarefas/consultas/cancelamento, aprovações tipadas, interfaces versionadas; preparar a ponte tool/agent para integração real.
4. **SERVER-1D:** gate no host: cold boot sem GNOME, conversa de provider, tarefa suportada, desconectar/reconectar, estado durável, cancelamento e segurança; se ferramenta agentiva já estiver integrada, verificar efeito real. Relatório factual.

**Após SERVER-1 e até 17/10:** concluir integração de agentes/ferramentas reais e gates de segurança/uso no [plano agentivo 0.1](NARYS-01-AGENT-TOOLS-DELIVERY.md). LR-10B–F seguem pausadas **enquanto SERVER-1 estiver ativa**; conclusão da SERVER-1 torna a retomada prioritária, sem tratar a LR-10A como autorização futura de inferências nem como prova de ferramentas.

**Aceitação global 17/10:** servidor sem GUI; conversa real; tarefas duráveis; agente orquestrador e executor(es) com ferramentas reais e efeito comprovado em workspace autorizado; usuário interage via Termux; logs/diff/resultados; aprovação e cancelamento; sem gastos extras inesperados. Quando qualquer requisito falhar, sinalizar BLOCKED/PARTIAL com evidência e responsabilidade, nunca reclassificar mock como PASS.

## 7. Baseline, responsabilidade e escopo

Estado observado em 10/10/2026: LR-10A **PASS FINAL somente textual e zero tools**; Core em narys-core separado do bootstrap do Tauri, DB separada, socket Unix e serviços systemd comprovados. O desktop já mantém Conversation, Scheduler, providers e ExecutionBroker que precisam ser integrados/extraídos. Esta decisão **não** altera código funcional, permissões ou boot do Fedora. A abertura real de SERVER-1 exigirá registro de hora/branch/HEAD verificados.

O usuário define a prioridade do produto; Luna desenha as alternativas e audita a evidência; Codex implementa autonomamente dentro do objetivo definido e sinaliza somente impedimentos críticos ou decisões de autoridade. A urgência permite refatoração ousada, **não** alegações técnicas sem testes.
