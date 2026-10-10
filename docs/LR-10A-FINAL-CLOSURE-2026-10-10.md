# LR-10A — Fechamento definitivo (10/10/2026)

**Decisão da auditoria independente Luna: PASS FINAL técnico, dentro do escopo experimental host-assisted de servidor headless + Copilot SpecialistAgent textual.**
**Trilha mãe LR-10: PAUSADA imediatamente após LR-10A.**
**Próxima prioridade: [NARYS-SERVER-1](NARYS-SERVER-1-HEADLESS-SERVER-RUNTIME.md), até 48 horas corridas após o início efetivo, no máximo quatro etapas; Narys 0.1 alvo 17/10/2026.**

## 1. O que a auditoria aceitou

Validação real em Fedora44 após cold boot `multi-user.target`, GNOME/GDM ausentes, Core independente de Tauri/GTK/WebKit e iniciado automaticamente em ~20s, `narys-core.service` ativo com linger, user manager/Keyring próprios e unlock humano privado por SSH. Stronghold pessoal preexistente aberto sem create/save/migração; metadados do snapshot e keyring permaneceram estáveis nos ensaios documentados.

SDK Rust `github-copilot-sdk=1.0.17` e CLI1.0.95/RPC3 com hash pinado; auth verdadeira, catálogo Auto/quota observados. A falha histórica da Task1 em `session.create` foi corrigida operacionalmente removendo o argumento opcional `sessionLimits.maxAiCredits=0.5`, que é soft e falhava com RPC -32603 categoria credits. O CLI recebeu `--no-custom-instructions` na configuração final; a causa interna única exata não é afirmada.

A Task2 enviou **uma** solicitação real ao Copilot em workspace descartável e recebeu resposta exata **5** para alpha2+beta3, sem ferramentas. Verificação separada criou/releu arquivo `result.txt` 0600 contendo um byte "5", TaskGraph concluiu, estado SQLite persistiu após restart do Core. Uma **sessão real** foi retomada pelo ID armazenado com dez eventos sem novo `send`/`session.create`, e houve stop/cleanup gracioso sem sobreviventes atribuídos.

**Autorização encerrada:** 1/3 envios efetivos; 2 oportunidades restantes foram revogadas por `closed.json`; zero retries, zero overage autorizado. Cota respondeu premium 200, used52 e 74,2% restante antes/depois (cache/atualização não comprovados); uso de sessão informou `totalPremiumRequests=1`, não se alegou valor USD faturado. O consentimento expirado/revogado **não** é autoridade para envios futuros.

Verificações: 35 unitários Rust, 1 integração Unix, 6 Python e 25 testes de ownership reportados, todos PASS (67 no total); rustfmt, schema/JSON, verificação da unidade systemd, diff check e build offline aprovados. Arquivos de evidência e logs: [relatório final](LR-10-LATEST-EXECUTION-REPORT.md), [task](../narys-core/evidence/final-integrated-operation.json), [resume](../narys-core/evidence/final-owned-session-resume.json), [service](../narys-core/evidence/final-service-state.json), [checks](../narys-core/evidence/final-checks.json), [logs Rust](../narys-core/evidence/rust-final-resume.txt) e [Python](../narys-core/evidence/python-final.txt). A matriz completa e as ressalvas sobre observações estão nos artefatos citados.

**Decisão:** GO/PASS_FINAL da **LR-10A**, não da LR-10 inteira, nem do agente Copilot com ferramentas de engenharia. Nenhuma nova FIX/teste ou complementação é necessária para *esta* entrega. Históricos H1/H2/H3/A9/FIX permanecem como registros da evolução, mesmo quando marcados candidatos à época.

## 2. Dívidas transferidas, com destino e sem bloquear o PASS da LR-10A

| Dívida/limite factual | Destino responsável | Regra de execução |
| --- | --- | --- |
| Headless Core funcional mas mínimo: ainda não serve Conversation/provedores/rotas da GUI; SQLite separado; administração somente SSH | **NARYS-SERVER-1A–1C** | Consolidar serviços atuais e garantir conversa/tarefa real e reentrada dentro do limite de 48h |
| Keyring50.0 pinado, helper GNOME com interface interna não suportada, desbloqueio humano após reboot | **SERVER-1B** para operação estável; manutenção posterior para compatibilidade upstream | Não substituir cofre, não automatizar senha em plaintext; falha de versão exige decisão explícita |
| Estado de sessão Copilot privado sob /tmp; retomada não garantida após limpeza/reboot | **LR-10B/D**, ou SERVER-1B somente se necessário às capacidades do release servidor | Persistência SQLite de resultado já é real; evitar confundir com durable session store |
| Core supervisor on-demand experimental; morte inesperada de supervisor/descendentes adversariais e execução com UID compartilhado não isolados | **LR-10B/F** | Supervisor e stress de produção não foram aceitos na LR-10A |
| Copilot textual zero ferramentas, DenyAll e sem HumanLocal; faltam edição/shell/approvals/sandbox/YOLO | **LR-10C/E/F** | Não habilitar execução agentiva sem authority e boundary provados |
| Uso "requests" vs AI Credits, soft maxAiCredits rejeitado, quota após operação sem variação observável; sem fatura USD independente | **LR-10D** | Budget/admission tipados por unidade real; não alegar teto hard nem overage seguro |
| Config Copilot apresentou drift de inode/mtime/ctime, autor não atribuído | **LR-10B/F**, apenas quando impactar operação/segurança | Não reescrever config pessoal nem concluir corrupção com base em metadata |
| Integração Copilot do Core separada da factory/Conversation Tauri; UI unificada e cliente Android inexistentes | **LR-10B/E** e trilha Android separada; SERVER-1C oferece CLI Termux | Não vender a demonstração textual como UI final |
| Rust1.98.1 executado; suporte ao MSRV declarado1.94 do Core e atualização do manifesto Tauri1.77.2 não medidos na mesma toolchain | **LR-10B/F** conforme mudança de dependências | Sem update de MSRV global baseado em suposição |
| DNS local do runtime Copilot usa `RES_OPTIONS=no-aaaa` após timeout IPv6 observado | **LR-10B/F** quando necessário | Reavaliar rede/portabilidade; não alterar rede global |

## 3. Procedimento de fechamento e Git

- Branch candidata `lr-10a-sdk-runtime-feasibility` auditada em `a413fc355db2d385be70371d0c49d792453d9c94`, com `main` base `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`; comparação: 40 commits à frente, zero atrás.
- Este fechamento **somente documental**, sem alteração de SDK/Cargo/Stronghold/host e sem reexecutar inferências. Incluir atualização de README, relatório final, plano LR-10A, plano LR-10B–F, plano operacional e nova trilha NARYS-SERVER-1.
- Integrar a branch à `main` por **fast-forward sem squash/rebase/merge commit** depois da validação dos documentos. Preservar branch remota até a sincronização do checkout local do usuário; limpeza de branches em passo posterior.
- Após publicação, confirmar SHA da `main`, branches e integridade dos documentos; não afirmar que o repositório local do PC foi sincronizado.

## 4. Nova ordem estratégica

```text
LR-9 PASS → LR-10A PASS FINAL → LR-10 PAUSADA
                                     ↓
                      NARYS-SERVER-1 / até 48h
                                     ↓
                  retorno à LR-10B–F por decisão explícita
                                     ↓
                          LR-11 e demais trilhas
```

**Regras de condução exigidas pelo usuário:** solução prática em vez de ciclos conservadores; diante de problema, Luna apresenta opções/riscos/tempo → usuário escolhe → Luna redige prompt → Codex implementa; correções ordinárias ficam na etapa; nenhuma subdivisão não essencial; risco crítico ainda exige decisão. O período de servidor possui limite máximo de quatro etapas, FIX somente por bloqueio de confiabilidade comprovado.

Aprovação independente registrada nesta seção como decisão documental; não declarar que outro processo de auditoria externo foi executado.
