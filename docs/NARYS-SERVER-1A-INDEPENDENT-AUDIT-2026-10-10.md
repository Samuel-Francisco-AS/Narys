# NARYS-SERVER-1A — auditoria independente Luna (10/10/2026)

**Decisão: PASS TÉCNICO da SERVER-1A, dentro de seu escopo específico.** Revisão independente do código remoto, relatório, diff com a main e evidências versionadas de testes/host; **não** corresponde a execução pessoal dos testes ou acesso direto ao Fedora. Não constitui PASS das etapas 1B–1D nem da entrega Narys 0.1.

## Identificação

- Branch auditada: `narys-server-1-headless-runtime`.
- HEAD de implementação/documentação apresentado: `863683068319453acf2b804b8cc2303288ed51eb`.
- Base da main preservada no momento da auditoria: `553b51182bb477d0b777093da5bfda93239fddf9`.
- Comparação via GitHub: três commits à frente, zero atrás; migrações 001–018 transferidas para `narys-domain`, migration 019 aditiva.
- Horário de abertura da trilha conforme evidência: **10/10/2026 15:03:12 America/Recife**; prazo máximo absoluto **12/10/2026 15:03:12 America/Recife**. Auditoria não reinicia relógio.

## Evidências que sustentam o PASS

1. **Core-First real:** crate `narys-domain` recebe módulos operacionais compartilhados, sem permanecerem duplicados como cópias funcionais; `narys-core` torna-se composition root. `RuntimeServices` instancia TaskRegistry, ProviderRuntime, AgentRegistry, ExecutionBroker e SecretStore `existing` sem Tauri/GTK. Provider registry e agente de produto ainda não são conectados: isso é 1B/fase agentiva.
2. **Migrador SQLite:** `narys-core/src/storage.rs` valida ownership/versão/integridade, retém backups por SQLite online backup (incluindo WAL), refusa conflito de duas bases de domínio populadas, aplica schema019 e registra receipt. Publica candidato por rename e cerca desktop herdado. `WriterLease` usa flock com O_NOFOLLOW e mesmo UID. `headless_tasks` e `task_records` preservam namespaces e IDs. Evidência `host-final.json`: 18 tabelas desktop com contagem/hash iguais; 37 sessões, 94 mensagens, 188 tarefas, 39 subtarefas, duas LR-10A preservadas.
3. **IPC v1:** request/command tipados, deny unknown fields, validação de versão/TaskId/request_id; Unix0600/SO_PEERCRED, limites 16 KiB entrada/256 KiB saída/32 conexões, cursor de eventos persistidos. Contratos futuros de Conversation/Approval/Tools retornam `capability_not_integrated` antes de qualquer efeito. A API não carrega `HumanLocal`, e requests não são receipts de autorização.
4. **Lifecycle:** `Type=notify` com READY após migration/recovery/socket; serviço de usuário persistent sem display; nova instância não toma socket/lease; shutdown e cancelamento sem reenvio automático. Logs reportam restart real, estado persistente e serviço ativo; idle reportado aproximadamente 15,6 MiB RSS, CPU 0,0 s em amostra de 10 s.
5. **Credenciais/recursos:** Stronghold existente e autorização Copilot encerrada têm metadados estáveis nos snapshots de evidência; `SecretStore::existing` é somente leitura, sem gravação de snapshot/credenciais por padrão. Sem nova inferência real durante 1A. Evidência host documental, não nova reprodução da auditoria.
6. **Regressões comprovadas pelos logs:** 1.049 testes de domínio (2 ignorados), 12 focados em SecretStore, 35 Core, 10 desktop, 6 Python: **1.112 PASS, zero falhas** nos conjuntos reportados. Build desktop release, fmt e verificação da unidade reportados PASS. A suite ampla precede último ajuste de segredos, cuja validação focada está documentada.

## Limites aceitos e obrigações de continuidade

- **Desktop intencionalmente não utilizável** depois do takeover até adaptação IPC; apenas `cargo check` e testes direcionados são PASS, não smoke funcional da GUI. Manter esta regressão controlada **isolada da main** enquanto o ciclo servidor avança; planejar a restauração da GUI, sem dois escritores, antes de declará-la utilizável.
- **Conversation real, credenciais de providers, Scheduler em operação, tarefas de produto, approvals e ferramentas de agentes ainda não estão conectados ao IPC**. O Core expõe placeholders bloqueados; esse fato é correto para 1A. Não usar o PASS para comercializar funcionalidade não testada.
- **Segurança:** SO_PEERCRED controla mesmo UID, não isola processos desse UID nem constitui sandbox. Executores LLM continuam sem authority; tool call futura requer políticas e autorização humana. Autorização de inferências da LR-10A está encerrada.
- **Testes de hardware/host:** evidências reportam uma execução no Fedora, não reexecução independente por Luna; cold boot novo e operação Termux/SSH real são gates de 1D. MSRV 1.94 não foi testado; toolchain do host 1.98.1.
- **Sem bloqueador crítico demonstrado no escopo 1A.** A ressalva desktop é consequência explícita da migração arquitetural e destino de integração, não concessão para deixar a GUI permanentemente desativada.

## Decisão e encaminhamento

**Ratifico PASS técnico SERVER-1A.** Não exigir FIX ou nova suíte artificial para esta entrega. Iniciar **SERVER-1B** na mesma branch, após atualizar o checkout, para ligar Conversation, provedores reais utilizáveis (Groq primeiro), roteamento, TaskId, histórico/recovery e Stronghold existente ao Core/IPC, com pelo menos uma conversa real autorizada e persistente no servidor. Não iniciar ainda ferramentas agentivas com autoridade irrestrita; manter metas Narys 0.1 até 17/10/2026.

**Sem merge na main nesta auditoria:** etapa 1A isolada na branch, com desktop deliberadamente indisponível após takeover; integrar main apenas segundo política de fechamento e verificação de capacidade/compatibilidade. Não alterar boot/Keyring nem dados pessoais.
