# LR-10B — auditoria independente final (10/10/2026)

**Veredito: PASS FINAL DELIMITADO — Copilot Adapter & On-Demand Supervisor, incluindo FIX-1.** A aprovação vale para os contratos de integração estrutural, lifecycle e segurança fail-closed da LR-10B. Não equivale a aprovação de ferramentas agentivas, inferências novas ou execução de engenharia.

## Base auditada e cadeia de evidências

- Repositório: `Samuel-Francisco-AS/Narys`, branch `lr-10b-copilot-adapter-supervisor`.
- Base inicial da main: `21be6382d146c99056d8dc99e6a13e2fa0dbe489`.
- Candidata inicial: `45bc5ad0cabfd49ccf1967a8ba7fda5a9d4fea75`.
- Falha encontrada na primeira auditoria: registro persistente de ownership anterior ao preparo dos diretórios, com retornos antecipados; supervisor inferia cleanup por lista negativa de códigos de erro.
- FIX-1 de implementação: `eaba03e98b54f5cde2b5968fbb1cbc4808f20576`.
- Candidata final reauditada: `13a75bebdac7ac1ce7f69a0b6fdfab861812d97a`, dois commits após a candidata inicial.
- Referências: [relatório de entrega](LR-10B-DELIVERY-REPORT.md), [arquitetura LR-10B](LR-10B-COPILOT-ADAPTER-SUPERVISOR.md), [matriz de validação](evidence/lr10b/VALIDATION-MATRIX.md), evidências em `docs/evidence/lr10b/fix1/`.

## Escopo da revisão independente

Inspeção do diff remoto e dos contratos em `narys-core/src/copilot/startup.rs`, `sdk.rs`, `supervisor.rs`, `store.rs`, `mod.rs`, testes `sdk/tests/startup_safety.rs`, relatórios e logs publicados. Verificação por GitHub dos commits e da ancestralidade da branch. A auditora **não executou os testes no Fedora do usuário**, não acessou seu serviço local e não reproduziu o lifecycle autenticado. Resultados de execução e estado do host são evidências registradas pelo implementador, não medições independentes.

### Resolução do bloqueio FIX-1

**Resolvido no código e coberto por testes direcionados:**
1. Journal autoritativo `preparing` anterior ao lançamento; `launch_intent` deve ser persistido antes de `Client::start`.
2. `StartupFailure` traz `StartupSafety` explícito (`NoProcessLaunched`, `CleanupVerified`, `CleanupUnverified`, `PersistenceUncertain`); classificação não depende do texto/código de erro.
3. Falhas na preparação de `workspace`, `logs` e `sdk-state` produzem certificado de ausência de launch, quando comprovável.
4. Falhas de persistência/ownership e cleanup não comprovado bloqueiam outra geração até reconciliação positiva; não se promove estado incerto para sucesso.
5. Recovery valida journal, vínculo de referência/diretório/boot, comprovantes e identidades; é idempotente e não cria sessão, não reenvia inferência nem sinaliza PID externo.
6. Testes negativos verificam SQLite e supervisor sob fault injection, corrupção/ausência de certificado, cancela­mento concorrente e crash com `launch_intent` durável.

Não foi encontrada regressão impeditiva demonstrável dentro deste escopo na reauditoria estática.

## Evidências de regressão registradas

- Core: **81 testes unitários + 11 integrações = 92 aprovados**.
- Domain: **1.059 testes + 2 doctests = 1.061 aprovados**; **2 gates reais ignorados**, não aprovados.
- Python: **17 testes aprovados**.
- Total registrado: **1.170 aprovados**, 17 testes adicionados pela FIX-1.
- Builds offline/locked, rustfmt e checksums registrados como aprovados.
- Host após FIX-1 segundo evidências: serviço Core ativo, Copilot `Dormant`, geração zero, nenhum subprocesso do especialista, ~25,86 MiB RSS, zero incremento de ticks de CPU em observação ociosa de ~10 s. SQLite schema021 com integrity_check OK, sem erro FK; backup antes/depois da atualização documentado.
- Não foi executado `agent new/resume` autenticado, inferência real, ferramenta agentiva ou consulta de faturamento nas validações da FIX-1, conforme relatório.

## Limites que **não** recebem PASS

- Lifecycle `create/resume` autenticado com CLI Copilot real e durabilidade de transcripts reais.
- Logout SSH durante execução ativa e cancelamento durante inferência real.
- Endurance e medição de recursos sob carga/inferência.
- MSRV Rust 1.94 exato (host comprovado Rust 1.98.1).
- Falha simultânea dos dois reapers fora de cgroup e escape adversarial.
- Sandbox de sistema operacional, hooks/permissões nativas comprovados, approvals, YOLO explícito, ferramentas reais e mediação de comandos pelo Broker.
- Quotas/faturamento externo e consumo por execução agentiva.

**LR-10C** deve estabelecer política/approval/boundary por ferramenta e prova negativa de sandbox ou falha segura; **LR-10D** deve integrar admission/quota/handoff/TaskGraph/trace; **LR-10E** deve comprovar tarefa real e controles de UX; **LR-10F** executará gates finais de segurança, concorrência, endurance e compatibilidade. **LR-11 Codex executor** tem aceite próprio. Nenhuma autorização encerrada da LR-10A é reutilizável.

## Decisão e transição

**PASS LR-10B com escopo estrito.** A FIX-1 é aceita sem uma segunda correção. A implementação pode ser integrada à `main` preservando histórico de auditoria e commits da branch. O marco `Narys 0.1 agentiva` segue **não implementado**, com alvo de produto em **17/10/2026**. Próxima subfase: **LR-10C**.

O merge GitHub não instala código no host; a evidência de deploy FIX-1 refere-se ao commit de código `eaba03e98b54f5cde2b5968fbb1cbc4808f20576`, não ao futuro merge nem a mudanças documentais subsequentes.
