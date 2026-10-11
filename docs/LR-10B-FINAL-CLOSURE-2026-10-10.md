# LR-10B — fechamento e integração definitiva (10/10/2026, America/Recife)

**Estado: ENCERRADA — PASS FINAL DELIMITADO na auditoria independente.** A implementação Copilot Adapter & On-Demand Supervisor + FIX-1 foi integrada à `main` por **[PR #26](https://github.com/Samuel-Francisco-AS/Narys/pull/26)** em 10/10/2026 (America/Recife). Não equivale ao PASS de Narys 0.1 agentiva.

## Registro Git verificável

- Repositório: `Samuel-Francisco-AS/Narys`.
- Base original da implementação: `21be6382d146c99056d8dc99e6a13e2fa0dbe489`.
- Candidata inicial: `45bc5ad0cabfd49ccf1967a8ba7fda5a9d4fea75`.
- FIX-1 (código): `eaba03e98b54f5cde2b5968fbb1cbc4808f20576`.
- Candidata FIX-1 reauditada: `13a75bebdac7ac1ce7f69a0b6fdfab861812d97a`.
- Head final da branch antes do merge (incluindo auditoria e checkpoint documental): `700e3ff272c895f13af34dc0f19f22369d6156e7`.
- **PR #26 mergeada por merge commit**: `c91dbafe9f5f085b708acc07ca1b24169a0f13a5`, mantendo a história de sete commits da LR-10B.
- Verificação pós-merge: `main` apontava para o commit `c91dbafe9f5f085b708acc07ca1b24169a0f13a5`, PR `merged=true`, branch de implementação **ainda existente no remoto** após o merge.
- Este documento foi adicionado após a integração; seu commit documental posterior avança a `main`, sem modificar código do runtime.

## Auditoria, evidências e aprovação

- [Parecer independente PASS da LR-10B](LR-10B-INDEPENDENT-AUDIT-2026-10-10.md).
- [Relatório original e FIX-1](LR-10B-DELIVERY-REPORT.md).
- [Arquitetura permanente](LR-10B-COPILOT-ADAPTER-SUPERVISOR.md).
- [Matriz por cenários](evidence/lr10b/VALIDATION-MATRIX.md).
- Evidência publicada: **92 Core/IPC, 1.061 Domain/doctests e 17 Python = 1.170 testes aprovados**, 2 gates de provedor real ignorados. Não foram reexecutados no Fedora pela auditora; a auditoria examinou remotamente o código e os registros.
- Bloqueio FIX-1 solucionado: journal de ownership anterior ao launch, declaração tipada e positiva de cleanup, rejeição conservadora de ownership sem prova, recovery idempotente sem replay.
- Host observado pelo implementador depois da FIX-1: Core ativo e instalado no commit de código `eaba03e98b54f5cde2b5968fbb1cbc4808f20576`, Copilot Dormant/geração0, sem processos residentes, ~25,86 MiB RSS ocioso na amostra curta, SQLite schema021 íntegro. **Merge não significa deploy adicional no Fedora**.

## Fronteiras e dívidas preservadas

A LR-10B **não** habilita novos envios de inferência ou ferramentas agentivas. `HumanLocal`, isolamento, política financeira, IPC e Codex Planner read-only não recebem privilégios adicionais. Não existe autorização implícita para YOLO, comandos do agente, gastos, downloads ou reuso dos recibos da LR-10A.

**NOT_VERIFIED e transferido aos gates futuros:** lifecycle autenticado nativo e durabilidade real do transcript, logout SSH com tarefa Copilot em atividade, cancelamento durante inferência real, endurance e recursos sob carga, MSRV exato Rust 1.94, morte simultânea dos dois reapers fora do cgroup, escopo real de sandbox/permissões nativas, quota/faturamento externo.

**Próxima prioridade: LR-10C — Authority, Approval Policy, Sandbox & YOLO**; a seguir LR-10D/E/F, com LR-11 Codex executor em trilha independente. Meta de Narys 0.1 com tarefa real em **17/10/2026**, ainda não implementada. Validar efeitos reais em workspace descartável, approval, diff/testes verificáveis, cancelamento ativo e reentrada durante desconexão antes do release; autorização humana separada para uso real do modelo.

## Encerramento operacional e limpeza

- **GitHub:** PR integrada e `main` atualizada. A branch remota `lr-10b-copilot-adapter-supervisor` foi constatada como **ainda presente** após o merge; a configuração do repositório `delete_branch_on_merge=false` não a elimina automaticamente.
- **Fedora local:** sincronização e remoção da branch local **não executadas nesta sessão**: a integração GitHub não fornece acesso ao checkout do host.
- **Exclusão remota:** pendente de operação autenticada `git push origin --delete lr-10b-copilot-adapter-supervisor` ou botão `Delete branch` da PR mergeada. Não alegar remoção até comprovação.
- Procedimento seguro no checkout Fedora (executar com mudanças locais já preservadas e árvore limpa):
  ```sh
  git status --short
  git fetch origin
  git switch main
  git pull --ff-only origin main
  git branch -d lr-10b-copilot-adapter-supervisor
  git push origin --delete lr-10b-copilot-adapter-supervisor
  git fetch --prune
  ```
  Evitar `git branch -D`, force-push ou reset destrutivo. Se a branch já tiver sido apagada, tratar `git push --delete` como desnecessário e confirmar o remoto.
- Backups de autoridade schema021 e instalação anteriores devem ser preservados; não reverter para um binário incompatível com schema021 nem reiniciar o serviço sem necessidade.

**Critério de fechamento técnico atingido:** PASS auditado, registro documental, integração remota preservando provenance. **Pendência operacional explícita:** limpeza remota/local e sincronização do checkout Fedora.
