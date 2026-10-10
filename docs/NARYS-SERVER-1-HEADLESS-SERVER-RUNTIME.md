# NARYS-SERVER-1 — Headless Server Runtime

**Estado: SERVER-1A PASS técnico ratificado por auditoria independente (código e evidências). SERVER-1B próxima; 1C/1D não iniciadas.** [Auditoria de Luna](NARYS-SERVER-1A-INDEPENDENT-AUDIT-2026-10-10.md).

Abertura real: **2026-10-10T15:03:12-03:00**, America/Recife. Deadline absoluto: **2026-10-12T15:03:12-03:00**. Base verificada `553b51182bb477d0b777093da5bfda93239fddf9`; checkpoint `2492285`. [Relatório único SERVER-1A](NARYS-SERVER-1A-REPORT.md). O planejamento abaixo conserva as etapas posteriores; não representa aprovação da trilha inteira.
**Direção arquitetural ratificada em 10/10/2026:** [Narys Core-First — monólito modular + thin clients](NARYS-CORE-FIRST-ARCHITECTURE-2026-10-10.md). **Meta integrada de produto:** [Narys 0.1 com agentes e ferramentas reais até 17/10/2026](NARYS-01-AGENT-TOOLS-DELIVERY.md). A decisão posterior do usuário permite extração ampla e refatoração de Conversation, Scheduler, persistência, provider runtime, protocolos e bootstrap Tauri quando técnica e operacionalmente favorável; invalida qualquer leitura desta trilha que imponha refatoração mínima como teto artificial.
**Decisão registrada:** 10/10/2026, após PASS FINAL da LR-10A.
**Prazo máximo absoluto de execução:** **48 horas corridas a partir da abertura efetiva da trilha** (abertura real registrada acima; sem contagem retroativa). **Máximo de quatro etapas**. **Meta externa:** Narys 0.1 utilizável até 17/10/2026.
**Ordem vinculante:** LR-10A PASS FINAL → **LR-10 PAUSADA** (B–F) → **NARYS-SERVER-1** → retomada explícita da LR-10 conforme decisão do usuário.

## 1. Missão de produto

Fazer a **Narys inteira nas capacidades operacionais da versão 0.1** funcionar como servidor Rust/headless no Fedora 44 sem depender de GNOME, GDM, Wayland, X11, WebKit, display, monitor ou WebView. Usar a partir do Termux/SSH como cliente inicial, com comandos para conversar, acompanhar/cancelar tarefas e administrar modelos/serviços, sem abrir desktop. Suportar funcionamento contínuo após saída do SSH e recuperar estado no restart, de acordo com a política de cada tarefa.

**Não confundir** Headless de janela Tauri (PERF-1C: Core vivo sem WebView dentro de processo Tauri) com servidor de inicialização em modo texto. A LR-10A provou **cold boot real**, `narys-core.service` sob systemd --user, socket Unix 0600/SO_PEERCRED, GNOME ausente, desbloqueio humano por SSH, Stronghold existente e **uma tarefa textual real Copilot**. Esta é a base obrigatória, não uma nova POC. [Evidência aprovada](LR-10-LATEST-EXECUTION-REPORT.md).

A aplicação gráfica desktop e o avatar continuam opcionais. O servidor não precisa renderizar personagem; clientes gráficos poderão fazê-lo. **"100% headless"** significa que todas as funcionalidades declaradas como suportadas **neste release servidor** operam sem GUI; não é alegação de que telas, avatar ou features futuras já funcionem em terminal.

## 2. Regras de execução / urgência

- Uma branch de implementação curta baseada na `main` atual verificada. A implementação SERVER-1A está rastreada no relatório vinculado acima.
- No máximo **quatro etapas numeradas** (abaixo). Não abrir H*, FIX* ou complementos para hipótese, polimento ou discussão. Corrigir dentro da etapa atual quando possível; FIX explícita **somente se absolutamente necessária** para funcionamento confiável (bloqueio funcional, perda de dados, regressão severa ou vulnerabilidade relevante).
- Metodologia: a Luna apresenta opções concretas com risco, tempo e trade-off; usuário decide; Luna escreve o prompt; Codex executa e comprova. O Codex resolve erros comuns autonomamente e só interrompe por criticidade real ou decisão que exceda autoridade.
- Escolher a **melhor arquitetura efetiva**, não a menor alteração de linhas. Codex está autorizado a extrair **integralmente** Conversation/Scheduler/Registry/State ou refatorar profundamente o Tauri, mover módulos para Rust reutilizável e migrar dados **quando necessário para um servidor unificado e operacional**. Reutilizar subsistemas quando vantajoso; não preservar acoplamentos por inércia. Exigir decisões de migração, ownership e regressões pertinentes. Não expandir para UI Android completa, avatar, AI-Native Runtime ou projetos não relacionados.
- Testes relevantes e **um gate integrado final**, sem repetir suites históricas não afetadas. 48 horas não podem virar prazo indefinido por refinamentos, mas a complexidade da refatoração **não é motivo para descartar o alvo de agentes com ferramentas reais até 17/10**. A SERVER-1 prepara a base e o ciclo de tarefas; o marco agentivo subsequente conclui LR-10/LR-11 conforme o plano 0.1. Registrar evidência honesta; não declarar PASS fictício quando uma função suportada falhar.
- Baixo consumo realista para PC Fedora i7-3770/8 GiB sem GPU dedicada; runtime Copilot **on-demand**, zero processo Copilot em idle. Sem assinatura ou serviços pagos adicionais.
- Uso de serviços remotos/credenciais sob decisões já concedidas apenas no escopo válido; **nova inferência ou gasto não herdam** a autorização de até três envios da LR-10A, que foi fechada/revogada.

## 3. Plano rígido — quatro etapas

### SERVER-1A — Núcleo de servidor e fronteira de runtime (início, janela 0–12h)

- Consolidar o processo `narys-core` como autoridade de tarefas independente de Tauri/WebView; inventariar dependências de Conversation/Scheduler/Registry/ExecutionBroker/SQLite, escolher e executar a extração/refatoração estrutural mais eficaz, mesmo que ampla, sem duplicar estado autoritativo. Registrar ADR sucinta, contrato das camadas e plano/mecanismo de migração.
- Garantir boot `multi-user.target`, systemd --user, linger, restart, logs privados e shutdown/cancelamento coerente. Sem reiniciar GDM/Keyring nem alterar boot sem motivo comprovado.
- Contrato de comunicação local tipado, versionado e seguro para clientes, incluindo fronteiras para tarefas, conversas, eventos, aprovação e ferramentas futuras; preservar `SO_PEERCRED` e não expor TCP público.

**Saída:** Core sobe/permanece ativo sem sessão gráfica, com contratos reais e nenhum fallback gráfico implícito.

### SERVER-1B — Cognição e persistência em modo servidor (janela 12–24h)

- Disponibilizar Conversation, TaskId, histórico/reentrada, Scheduler/roteamento e **os provedores atualmente utilizáveis** (priorizando Groq; integrar outros quando configuração/quotas existentes permitirem), sem duplicar chaves ou migrar snapshots por conveniência.
- Fortalecer o acesso ao Stronghold existente e ao GNOME Keyring pelo user manager após desbloqueio **humano privado via SSH**. Respeitar serviço existente/UID e ausência de GUI; sem copiar tokens, senha automática em texto puro ou nova coleção.
- Copilot textual aprovado na LR-10A como componente separado, sob demanda; conservar os seus controles de credenciais, estado, quota e lifecycle. As ações agentivas de edição/shell ainda são LR-10B–F, **não são requisito** deste marco.
- Persistência durable de conversas/tarefas suportadas, falhas tipadas, crash/restart sem replay de efeitos incertos; decidir explicitamente se a base SQLite do Core será unificada com a da GUI ou terá migração controlada.

**Saída:** conversa real com pelo menos um provider existente no servidor e estado recuperável, sem GUI.

### SERVER-1C — Administração e uso remoto real (janela 24–36h)

- CLI Termux/SSH utilizável para status/credenciais, conversa, tarefas/cancelamento, resultados/eventos, configuração suportada de modelos/provedores e diagnósticos sanitizados. Expor decisões humanas de aprovação/recusa em contrato tipado, preparando a execução de ferramentas reais por agentes após a etapa servidor; quando houver integração funcional antecipada, não a bloquear artificialmente. Tarefas exigindo aprovação devem aguardar/recusar com estado factual.
- Persistência e conexão após fechar Termux/SSH; não depender de tmux, terminal vivo, GNOME, monitor ou janela. Acesso remoto por SSH/túnel seguro por default; **não abrir API em 0.0.0.0**.
- Expor interfaces versionadas para cliente Android futuro, mas **não implementar o APK** nesta trilha.

**Saída:** operador gerencia Narys a partir do telefone via SSH e recebe resposta/tarefas reais do servidor.

### SERVER-1D — Gate integrado e fechamento (janela 36–48h)

- No host real: iniciar sem GDM; usuário desbloquear senha **só no SSH**; status do Core/Keyring/Stronghold; conversar com provider disponível; submeter/consultar/cancelar tarefa elegível; desconectar Termux e reconectar; confirmar estado/continuidade e shutdown/restart seguro. Copilot textual aprovado segue disponível sem processo permanente; evitar nova inferência real sem autorização específica.
- Evidência de efeitos, resultados e processo; testes direcionados a alterações, performance/RSS/CPU proporcional, segurança do socket, ausência GUI; verificar executor/tool gateway se já integrados. Documentar precisamente capacidades ainda necessárias ao marco agentivo de 17/10, sem confundir texto Copilot ou operação humana com uso de ferramenta pela LLM.
- Publicar relatório fechado e dívida objetiva com responsáveis, auditar uma vez e encerrar até o prazo. Se um impedimento crítico persistir, entregar status parcial **sem inventar PASS**, sem estender por conta própria.

**Saída:** release servidor utilizável, gate e fechamento rastreáveis.

## 4. Critério único de aceitação funcional

```text
PC liga sem ambiente gráfico
→ systemd inicia Core e serviços de usuário
→ Termux/SSH conecta
→ humano desbloqueia Keyring no terminal privado
→ Stronghold/provedores ficam disponíveis
→ usuário conversa com a Narys e envia tarefa suportada
→ recebe eventos e resposta factual persistida
→ fecha conexão SSH/Termux
→ Narys continua operando
→ reconecta, consulta/cancela ou retoma conforme policy
```

Se a Narys não conseguir **conversar ou executar uma tarefa suportada no servidor**, a trilha não poderá chamar esse caminho de 100% funcional.

## 5. Divisão de responsabilidades / dependências e exclusões

**SERVER-1:** hospeda serviços, providers existentes, conversas/tarefas e interface de administração remota, sem GUI, com contratos de aprovação e tool gateway preparatórios. **LR-10B–F:** Copilot agentivo completo (ferramentas/approvals/sandbox conforme evidência/UX), priorizado imediatamente após SERVER-1 para o marco 0.1. **LR-11:** executor Codex, também alvo do release de 17/10, com status próprio. **Android client:** UX no telefone/cliente de serviço, trilha distinta. **NARYS-NORM:** renomear identificadores legados com migração segura quando houver janela; não roubar o prazo.

Dívidas herdadas que não reabrem LR-10A: PIN Keyring50.0 e interface GNOME não suportada; requisitos de portabilidade; config Copilot drift com autoria não atribuída; quota em requests ≠ AI Credits; limites soft incompatíveis com CLI observado; isolamento kernel e supervisor adversarial ainda não disponíveis; dados da sessão Copilot em /tmp podem se perder no reboot; no baseline, Core mínimo tinha SQLite separado do desktop (consolidado com backups na SERVER-1A). Decidir no início o que bloqueia **as capacidades prometidas pelo SERVER-1** e o que continua dívida da LR-10.

## 6. Sequência de controle

**Estado atual: SERVER-1A PASS técnico; SERVER-1B é a próxima etapa.** A contagem de 48h começou na abertura real registrada acima; **não há início retroativo**. O prazo externo de 17/10 inclui **execução agentiva com ferramentas de engenharia reais**, planejada em [Narys 0.1](NARYS-01-AGENT-TOOLS-DELIVERY.md), não somente serviço de chat. Antes de iniciar, registrar hora e commit base; a cada etapa, registrar PASS ou impedimento direto. Não abrir mais de quatro etapas nem alongar prazo por upgrades estéticos. Prioridade de release 0.1: **17/10/2026**.

Referências: [fechamento LR-10A](LR-10A-FINAL-CLOSURE-2026-10-10.md), [trilha LR-10 pausada](LR-10-COPILOT-SPECIALIST-AGENT.md), [Core headless](../narys-core/README.md), [PERF-1C](PERF-1C-HEADLESS-RUNTIME.md).
