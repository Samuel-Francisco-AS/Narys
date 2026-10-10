# NARYS-SERVER-1 — fechamento definitivo do escopo headless (10/10/2026)

**Estado: ENCERRADA — PASS FUNCIONAL DELIMITADO (SERVER-1A/B/C/D).** Fechamento registrado após auditorias independentes e integração da branch de implementação à **`main` via [PR #25](https://github.com/Samuel-Francisco-AS/Narys/pull/25)**. Não declarar `Narys 0.1 agentiva` pronta.

## Identidade e linha de tempo

- Repositório: `Samuel-Francisco-AS/Narys`.
- Branch de trabalho preservada até sincronização do checkout Fedora: `narys-server-1-headless-runtime`.
- Commit final da branch antes do merge: `39dafa8b045927e5da4609d8540b6208d4750e08`.
- **Merge confirmado no GitHub:** `16ea99cd35927292c258fa3dbba1f078cb9698e2` (PR #25).
- Base inicial da main `553b51182bb477d0b777093da5bfda93239fddf9`; PR preservou os commits de implementação e auditoria.
- Abertura da trilha: **2026-10-10 15:03:12 America/Recife**; prazo vinculante **2026-10-12 15:03:12**. Todas as quatro etapas e auditorias concluídas dentro da janela.
- Auditorias independentes: [SERVER-1A](NARYS-SERVER-1A-INDEPENDENT-AUDIT-2026-10-10.md), [SERVER-1B](NARYS-SERVER-1B-INDEPENDENT-AUDIT-2026-10-10.md), [SERVER-1C](NARYS-SERVER-1C-INDEPENDENT-AUDIT-2026-10-10.md), [SERVER-1D](NARYS-SERVER-1D-INDEPENDENT-AUDIT-2026-10-10.md). A auditora examinou GitHub/código/evidências e relatórios humanos; não reproduziu pessoalmente execução no Fedora.

## Entregas efetivamente verificadas

1. **Core Rust headless e authority única:** sistema operacional em multi-user.target, Core `systemd --user` + linger iniciado antes da primeira sessão SSH, sem GNOME/Tauri nem Copilot residente, com SQLite autoritativo, lease e backups. Migração preservou dados desktop/TaskIds.
2. **Conversation/Providers:** Groq com chamadas reais via Scheduler/Conversation compartilhados; session/message/run/task duráveis, contexto de sessão, status e cancelamento suportados pelo Core, eventos versionados e recovery sem replay remoto.
3. **Terminal/Termux:** CLI instalada `~/.local/bin/narys`, protocolo IPC Unix socket 0600 + peer UID, consultas/status/models/providers/sessions/events/tasks e chat/recibos/resultados; reconexão SSH após envio; sem API TCP pública Narys.
4. **Credenciais humanas:** GNOME Keyring login existente desbloqueado pelo operador via SSH privado, Stronghold original aberto sem write/migration/secret_values em output, nenhum segredo passado ao agente.
5. **Gate SERVER-1D:** task193/session39 responde `SERVER1D-371dd203 78` após uma chamada Groq Free, 318 tokens, 0 retries/fallbacks; histórico e resultado recuperados no Termux após logout/reconexão e após restart do Core. O serviço continua acessível e histórico anterior preservado. 8 testes IPC finais PASS.
6. **Performance observada:** aproximadamente 25,9MiB RSS ocioso no gate final; KDF Stronghold demanda transitoriamente ~512MiB e teve pico na faixa ~541MiB; observações sem OOM/pressão, sem transformar amostras curtas em garantia de endurance.

## Limitações que permanecem abertas (não reabrir a trilha por polimento)

- **Logout durante a execução real ativa não comprovado:** a task193 completou às 18:32:04.938 e o logout ocorreu às 18:32:17.667. Recuperação posterior está comprovada, execução remota durante desconexão não.
- **Cancelamento de inferência em andamento não comprovado:** testes de cancelamento efetivo envolveram tarefas preparadas `lr10a:3/4`; produto192 já estava terminal failed quando recebeu cancel, e não constitui PASS de cancelamento ativo.
- **Desktop Tauri legado não operacional após takeover:** aguarda cliente IPC em trilha de apresentação; não reabrir writer paralelo, não anunciar desktop recuperado.
- **Sem agentes com ferramentas reais:** approvals/tool requests continuam recusados `capability_not_integrated`, Copilot/Codex agentivos de engenharia ainda não demonstrados, nem isolamento forte. LR-10A Copilot textual não autoriza execução agentiva ou inferências adicionais.
- Providers Gemini/Cloudflare/Mistral registrados mas não revalidados remotamente no servidor; GNOME Keyring50 usa interface interna pinada; sessão SDK temporária e limite de memória KDF precisam ser respeitados.
- Integração de repositório não reinstala binários no Fedora: na evidência 1D, instalado era `06ef4076380ed2723b9c567f5323104ec596e846`; hashes no [relatório 1D](NARYS-SERVER-1D-REPORT.md). Não confundir HEAD Git `main` com release deploy instalada.

## Próxima ação e política de transição

**Próxima trilha prioritária: LR-10B–F**, utilizando o Core headless aprovado para integrar Copilot SpecialistAgent **com ferramentas efetivas, aprovações e execução limitada a workspace**. **LR-11 Codex executor** deve ser coordenada como segunda capacidade agentiva com gate próprio. **Meta vinculante Narys 0.1: 17/10/2026**, conforme [plano agentivo](NARYS-01-AGENT-TOOLS-DELIVERY.md). Os testes de desconexão *com tarefa ainda ativa* e cancelamento ativo tornam-se gates obrigatórios do trabalho agentivo, não PASS retroativo da SERVER-1D.

Manter autorização por ação, workspace e política de custos, separando humano de agente; nenhuma autorização de Groq/Copilot da prova anterior é herdada por novos tasks. Sem compra/overage implícita. O Core detém autoridade e a interface desktop/móvel deve ser cliente fino.

**Para sincronizar local:** após conferir `git status --short` limpo, usar `git fetch origin && git switch main && git pull --ff-only origin main`. Só eliminar branches locais/remotas em operação posterior deliberada, depois de assegurar base sincronizada. A branch remota continua disponível até limpeza autorizada.

**Critério de sucesso deste fechamento:** servidor disponível pelo telefone e sem GUI, comprovado; transparência sobre backlog de execução real e desktop. Não vender o PASS servidor como PASS de agentes.
