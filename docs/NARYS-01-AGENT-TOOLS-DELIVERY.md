# Narys 0.1 — entrega agentiva com ferramentas reais (até 17/10/2026)

**Estado vigente (10/10/2026): SERVER-1A–D PASS DELIMITADO; LR-10B PASS DELIMITADO após FIX-1 e [auditoria independente](LR-10B-INDEPENDENT-AUDIT-2026-10-10.md).** Marco S encerrado. Marco A parcialmente preparado no lifecycle/supervisor, mas **ferramentas agentivas reais NÃO IMPLEMENTADAS**; próximo gate obrigatório LR-10C (authority/approvals/sandbox). LR-10D/E/F, LR-11 e Marco R permanecem pendentes. Exigir teste de desconexão durante tarefa ativa e cancelamento ativo no gate real. Desktop legado aguarda adapter IPC.

**Criado em 10/10/2026. Estado: META VINCULANTE DE PRODUTO / NÃO IMPLEMENTADA.** O usuário exige Narys operacional até **17/10/2026**, com agentes capazes de executar **trabalho real** e ferramentas disponíveis para LLMs/agentes; não apenas conversar, planejar ou simular ações.

**Dependência histórica já concluída:** [NARYS-SERVER-1](NARYS-SERVER-1-HEADLESS-SERVER-RUNTIME.md) (máximo quatro etapas e até 48h corridas após abertura). **Arquitetura aprovada:** [Core-First](NARYS-CORE-FIRST-ARCHITECTURE-2026-10-10.md). **LR-10A:** PASS FINAL restrito a Copilot textual sem ferramentas; duas inferências não usadas e autorização encerrada. LR-10B **concluída com PASS delimitado**, LR-10C–F em prioridade ativa depois do servidor; LR-11/Codex permanece trilha própria, mas deve ser planejada dentro do alvo 0.1.

## Resultado esperado em 17/10

- PC Fedora 44 liga em modo texto e inicia Narys Core persistente, independente de Tauri/GNOME/monitor.
- Telefone via Termux/SSH consegue conversar com provider remoto existente e solicitar tarefa, acompanhar progresso, aprovar/recusar ações elegíveis, consultar diff/resultado, cancelar e recuperar histórico/reentrada após desconexão ou restart.
- Orquestrador/Scheduler entrega tarefa a executor agentivo com **ferramentas reais**; o executor lê e modifica arquivo de workspace aprovado e executa pelo menos uma verificação real (teste/build/comando restrito), observando saídas. A ferramenta é invocada pelo agente sob política, não pelo operador fora da tarefa para simular sucesso.
- Ferramentas existentes do Copilot SpecialistAgent (LR-10B–F) devem ser integradas sem reimplementar seu agente; executor Codex (LR-11) é segundo objetivo explícito de engenharia dentro do marco. O objetivo é ter **agentes funcionais**, não chamar um backend textual de agentivo. Publicar o status de cada especialista separadamente; não mascarar bloqueio de um com sucesso de outro.
- Segurança: autorização por operação, workspace e perfil, aprovação humana, cancelamento com limpeza, quota/admission, sem repassar autoridade de shell humano. Isolamento somente se provado, sem YOLO implícito. Sem cobrança/overage ou inferência paga sem consentimento específico.
- Evidence real: TaskId, request→tool_call→autorizar→efeito→retorno→verificação, diff/artefato/teste, trace auditável, consumo, session/recovery, processos reaped. Falsos positivos e tarefas parcialmente realizadas não recebem PASS.

## Ordem de execução sem proliferação de etapas

**Marco S — Serviço:** finalizar NARYS-SERVER-1A/B/C/D dentro da janela própria de 48h, utilizando a estratégia de refatoração mais eficaz. Não transformar quatro etapas em dezenas de FIXes por polimento.

**Marco A — Autoridade e ferramentas:** LR-10B (adapter/supervisor) já foi concluída e integrada ao plano operacional; agora priorizar LR-10C (authority/approval/tool boundary), integrado ao Core servidor, para habilitar execução agentiva real. É autorizado reorganizar a divisão interna e antecipar pré-requisitos conforme necessidade, mas não eliminar gates de segurança. Workspace temporário e tarefa real de engenharia como prova funcional mínima. Registrar os limites do isolamento de forma factual.

**Marco B — Operação integrada e segundo agente:** completar LR-10D/E/F proporcionalmente aos contratos afetados e integrar Codex executor da LR-11 se operacionalmente possível até 17/10. Retomar as trilhas de implementação com branch e gate próprios quando SERVER-1 encerrar; nenhum antigo consentimento de inferência é transferido. O Core continua sendo a fonte de authority/recovery, e não o SDK/CLI.

**Marco R — Release 0.1:** gate ponta a ponta no Fedora headless e Termux: agente inicia tarefa, usa ferramentas, muda arquivo de teste permitido, executa teste, retorna diff verificável, mantém trilha após desconexão e respeita cancelamento/aprovação; benchmark de CPU/RAM realista; relatório com PASS/PARTIAL/BLOCKED por capability. Atualizar README, plano operacional e instruções de instalação/rollback.

## Decisões de prioridade

O compromisso com o dia 17 **prevalece sobre a preferência anterior por refatoração mínima**. Extração integral de Conversation/Scheduler, reorganização de crates, migração de estado, atualizações de protocolos e outras alterações estruturais podem ser realizadas se tornarem o caminho operacional melhor. Não criar bloqueio artificial em nome de compatibilidade histórica; preservar/migrar dados e provar regressões pertinentes.

O prazo é uma meta rígida de planejamento e esforço, **não licença para declarar PASS sem prova**. Qualquer risco material de credenciais, perda de dados, custos, permissão agentiva irrestrita ou mudança irreversível do host requer contenção e decisão humana.

## Estado inicial histórico e próximos checkpoints

- Em 10/10/2026, o commit de fechamento LR-10A já está na main; SERVER-1 ainda não foi aberta; portanto **não iniciar contagem retroativa das 48h**.
- A primeira execução é SERVER-1A, após sincronização do checkout local e abertura de branch.
- Após o fechamento SERVER-1D, checar o backlog agentivo real remanescente e iniciar imediatamente a próxima implementação dentro da janela até 17/10.
- Não afirmar que nenhum desses marcos futuros passou antes de evidência do host real.


## Checkpoint de fechamento LR-10B — 10/10/2026

[Auditoria independente PASS](LR-10B-INDEPENDENT-AUDIT-2026-10-10.md) e [relatório de implementação/FIX-1](LR-10B-DELIVERY-REPORT.md) delimitam o supervisor Copilot on-demand e o journal durável de ownership. **Próxima execução: LR-10C**, com autorização humana por operação, prova negativa de ferramentas/sandbox e sem herdar consentimento de inferência. O Marco A e a Narys 0.1 não ganham PASS apenas porque a LR-10B passou. A integração Git e a instalação do serviço são fatos distintos.
