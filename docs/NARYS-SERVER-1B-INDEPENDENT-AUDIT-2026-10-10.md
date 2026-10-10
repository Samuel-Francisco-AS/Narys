# NARYS-SERVER-1B — auditoria independente Luna (10/10/2026)

**Decisão: PASS FUNCIONAL da SERVER-1B em seu escopo específico.** Revisão independente e read-only de código, diff GitHub, relatórios, provas registradas de inferência real, testes e dados do host; **não** é reprodução pessoal no Fedora nem PASS da SERVER-1C/1D, do desktop, de ferramentas agentivas ou da Narys 0.1.

## 1. Identificação

- Base auditada: `narys-server-1-headless-runtime`, HEAD fornecido `9cd0981e9fc49951828cb08639d8d9d1e4226718`.
- Main verificada permanece `553b51182bb477d0b777093da5bfda93239fddf9`.
- Base da SERVER-1B: `fed52e32f77f117bd892da3458bd1bb5e6e9d3cc`; implementação da 1B `20f38596806af545224d4cb26fcba159b5cface2`. Nenhum merge ou alteração da main.
- Janela SERVER-1 permanece **10/10/2026 15:03:12 até 12/10/2026 15:03:12 America/Recife**. Não reiniciar por auditoria/etapa.

## 2. Provas específicas de funcionalidade

1. `narys-core/src/runtime.rs` compõe os quatro adapters originais no ProviderRuntime/Scheduler compartilhado do `narys-domain`, com config/timeouts e SecretStore existing. Sem segunda Conversation engine.
2. `narys-core/src/conversation.rs` conecta SessionCreate/Get/Resume/Close, Conversation, Providers/Configure, ConversationPolicy e TaskGet/Cancel ao estado autoritativo. IPC v1 continua Unix0600 + SO_PEERCRED, mensagens limitadas, sem autorização de tool/approval.
3. `narys-domain/src/persistence/conversation_runs.rs` implementa admissão de usuário/run/eventos em transação; conclusão com assistant/task/result/evento em transação; recuperação de pending/running como interrupted sem retry remoto; cancelamento e result duráveis. Schema020 aditiva para runs/permissões e detalhes sanitizados.
4. **Teste real Groq:** registro `docs/evidence/server-1b/live-conversation.json`, sessão38 e TaskIds 189/190 com duas chamadas reais ao modelo `openai/gpt-oss-20b`; primeira resposta devolveu marcador e **17+26=43**, confirmados exatamente por resposta `SERVER1B-479acc90 43`; segunda resposta depois de reinício recuperou marcador e 43 sem repetir conteúdo no prompt. Uso total reportado 795 tokens (236+225 e 278+56). Sem fallback/retry.
5. Task191 cancelada durante preflight sem terceira chamada de provider; conteúdo da entrada permanece no histórico como usuário, sem resposta fabricada. Teste `final-recovery.json`: dois completed e um cancelled recuperados após novo restart sem replay.
6. Históricos legados 37 sessões/94 mensagens verificados por consulta de IPC e hashes `history-ipc.json`, preservando 1 identidade, 4 memórias, 188 tarefas históricas/39 subtarefas. Total final 38 sessões/99 mensagens/191 task_records/3 runs.
7. Stronghold original não foi migrado/regravado; `SystemCredentialStore` no Linux passou a consultar coleção login existente, recusando bloqueio sem Unlock/Prompt/Create, e `SecretStore::existing` permanece read-only. Confirmação de uso Groq Free foi **declaração do operador**, não conferência de billing; outros provedores sem chamadas reais.
8. Evidências de regressão: domínio final 1058 PASS (2 manuais Codex ignorados), Core 32+1+6=39 PASS, Python 6 PASS, SecretStore 13 testes focados PASS (há sobreposição com suíte de domínio); desktop release/core check PASS. Sem testes reproduzidos pela auditora. Unit e dependências headless verificadas por logs.
9. `idle-isolated.json` reporta 30520 KiB de RSS (~29,8 MiB) e zero CPU incrementado por 10s no período ocioso. `live-conversation.json` reteve amostra contaminada por diagnóstico Stronghold com RSS ~554820 KiB, não é medida de idle; investigar custo transitório e headroom na 1D.

## 3. Ressalvas não bloqueantes e deveres seguintes

- **Copilot/Codex agentes com ferramentas:** continuam desabilitados e fora do PASS da 1B. Approval/ToolRequest devem seguir bloqueados até implementação autorizada e testes reais no marco 0.1 até 17/10.
- **Desktop legado:** segue intencionalmente recusado pelo takeover, ainda precisa de adapter IPC; não contabilizar GUI como recuperada.
- **Outros provedores:** integrados mas sem prova remota de disponibilidade. Manter desabilitados sem consentimento de uso gratuito/quotas aplicável; não supor autorização paga, overage ou retentativas de risco financeiro.
- **Carga transitória de memória:** verificar novamente no gate integrado e registrar picos separados de idle, distinguindo CPU/RSS de GNOME Keyring, Core e SDK/agents. Permanecem limites existentes da unit.
- **Ciclo de sessão/resultado após commit:** a engine encerra registro ativo antes do commit durável; erros posteriores de sink/evento precisam continuar não revertendo silenciosamente sucesso persistido. Há testes de atomicidade; considerar teste de falha pós-commit no trabalho futuro se este caminho for alterado, sem criar FIX artificiais na 1B.
- **Estado das credenciais após reboot:** `narys-core unlock` ainda chama helper experimental GNOME50 preso ao checkout. O leitor seguro da 1B não destrava automaticamente; planejar uma UX humana oficial em SERVER-1C.
- **Nenhuma nova evidência cold boot/Termux celular** é reivindicada nesta auditoria. Pertence à 1D. A prova do host vem de logs produzidos pelo Codex, não de execução independente.

## 4. Decisão e transição

**RATIFICADO: SERVER-1B PASS FUNCIONAL, sem necessidade de FIX.** SERVER-1C é a próxima etapa na **mesma branch** e mesmo relógio original. A prioridade é uma CLI coerente via SSH/Termux que permita conversa, status/credentials, sessions, tasks, eventos, providers/políticas, approvals quando integráveis e diagnósticos; incluir **Credential Unlock UX** com prompt exclusivamente em TTY privado, não via chat/LLM/IPC genérico e sem senha em argv/env/logs/files, reaproveitando o backend humano existente sob verificações de segurança. Não fazer migração de cofre nesta etapa. Validar desconectar/reconectar e estabilidade do serviço; gate real de cold boot/Termux pertence à 1D.

Luna não efetuou mutações de código funcional, migrações, alterações no Keyring/boot ou inferências remotas nesta auditoria.
