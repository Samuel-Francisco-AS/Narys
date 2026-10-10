# NARYS-SERVER-1B — Cognitive Runtime, Providers & Durable Conversation

**Resultado: PASS funcional da SERVER-1B**, comprovado por duas respostas reais do Groq no serviço instalado, persistência e recuperação após reinício. Não é aprovação da trilha inteira nem auditoria independente da 1B. SERVER-1C e SERVER-1D não foram iniciadas.

Repositório `Samuel-Francisco-AS/Narys`; única branch de entrega: `narys-server-1-headless-runtime`. HEAD inicial sincronizado e conferido: `fed52e32f77f117bd892da3458bd1bb5e6e9d3cc`. Commit de implementação e origem do binário instalado: **`20f38596806af545224d4cb26fcba159b5cface2`**. O commit posterior contém documentação/evidências; seu HEAD remoto integral é informado na entrega após o push. Nenhuma alteração ou merge na `main`.

Abertura da trilha preservada: **10/10/2026, 15:03:12, America/Recife**. Prazo absoluto preservado: **12/10/2026, 15:03:12, America/Recife**. Não houve reinício da contagem ou etapa suplementar.

## Arquitetura e capacidades entregues

Foram lidos o relatório/auditoria da SERVER-1A, a arquitetura Core-First, o planejamento SERVER-1 e o plano Narys 0.1. O Core continua autoridade única, com ownership/lease da base `~/.local/state/narys/core/db/luna.sqlite3`; `narys-domain` contém a implementação compartilhada. O desktop legado continua cercado pelo takeover da 1A.

`CoreRuntime` compõe os quatro adapters existentes no mesmo `ProviderRegistry` e `ProviderRuntime`, usando os timeouts/configurações persistidos e `SecretStore::existing`. A composição não abre o Stronghold nem faz chamadas remotas no boot. Groq é a prioridade operacional; Gemini, Cloudflare e Mistral permanecem integrados. Todas as permissões começaram desabilitadas e somente Groq foi habilitado após confirmação específica do usuário.

O módulo `narys-core/src/conversation.rs` adapta IPC ao **Conversation Engine existente**, sem outro Scheduler, engine ou cofre. São reutilizados TaskRegistry, broker de primeiro plano, Scheduler, admission, rate limiting, resilience, telemetria, allocation e políticas Fixed/Preferred/Auto. Há uma Conversation de primeiro plano por vez. A política é limitada e validada no Core; todos os candidatos, inclusive fallbacks, exigem permissão gratuita explícita.

O contexto usa identidade persistida, ContextBuilder e minimização existentes. A execução real registrou `identityVersion=continuity-bootstrap-2026-09-26`. O histórico pertence à sessão selecionada, respeita budgets e exclui a mensagem atual já admitida para não enviá-la duas vezes. Memórias não são automaticamente exportadas ao provider nesta capacidade; a contagem de mensagens em ContextMetadata não inclui esse histórico de sessão, montado separadamente.

O IPC v1 ativa criar/listar/consultar/selecionar/reabrir/fechar sessões, enviar Conversation, consultar/cancelar tarefas de produto, observar/configurar providers e política Conversation. Preserva Unix socket 0600, SO_PEERCRED do mesmo UID, validação tipada, campos estritos, erros sanitizados, limites 16 KiB/256 KiB e 32 conexões. Texto limitado a 4.096 bytes; páginas de histórico até 100 mensagens e 192 KiB. Os comandos mínimos e stdin tipado estão documentados em [IPC.md](../narys-core/IPC.md); não constituem a CLI completa da 1C.

Desconectar o cliente depois do recibo não cancela a tarefa: os workers pertencem ao serviço. TaskIds, provider/modelo, política aplicada, provenance, usage, timestamps e eventos de seleção/admission/roteamento/estado são recuperáveis. Eventos duráveis têm retenção de 4.096 e não contêm chunks, prompts, credenciais ou erros brutos; conteúdo e resultado ficam na conversa/run, acessíveis ao cliente local autenticado.

## Credenciais, autorização e segurança

O leitor Linux usa o mesmo backend Secret Service do keyring existente, com transporte criptografado, coleção login e correspondência exata dos atributos existentes. Recusa coleção/item bloqueado e duplicidades; não chama Unlock, Prompt ou Create. A leitura reutiliza exclusivamente o snapshot Stronghold existente, sem save, migração ou reconstrução. Falhas produzem códigos seguros, como `unlock_store_locked`.

O login keyring já estava desbloqueado pelo usuário; nenhum desbloqueio foi feito pelo agente. [Diagnóstico real](evidence/server-1b/stronghold-existing.json) confirma snapshot aberto, sem escrita/migração ou valores retornados. [Comparação de metadados](evidence/server-1b/metadata-comparison.json) confirma inode, tamanho, modo, mtime e ctime do cofre inalterados. Nenhuma chave foi impressa, exportada para evidência ou gravada em plaintext.

O usuário confirmou especificamente: **“Groq Free confirmado, sem overage e com quota disponível”**. O campo `free_tier_confirmed` registra declaração do operador, não verificação remota de billing. O Core não habilita overage nem faz upgrade. Presença de chave não promete validade/quota; estados locais deixam isso explícito. Foram consultadas as fontes oficiais de [rate limits Groq](https://console.groq.com/docs/rate-limits) e [billing Groq](https://console.groq.com/docs/billing-faqs). Gemini/Cloudflare/Mistral não receberam chamadas reais nesta etapa por falta dessa confirmação específica; Mistral não exige upgrade para permanecer integrado.

Não foram reutilizados consentimentos LR-10A: seu arquivo de autorização fechado e os dois resultados legados permanecem inalterados. Não houve inferência Copilot. Approval e ToolRequest continuam `capability_not_integrated`, sem ferramentas registradas, shell, autoridade agentiva ou equivalência com HumanLocal. O perfil continua `HOST_ASSISTED_NOT_SANDBOX`: processos do mesmo UID são a fronteira local de confiança, não um isolamento adicional.

## Persistência e migração

Schema **019 → 020** acrescenta `conversation_runs`, `server_provider_permissions` e detalhes sanitizados de `server_events`. O runner verifica a coluna antes de acrescentá-la; migrations anteriores não foram reescritas. O high-water de TaskId inclui runs duráveis.

Antes de migrar a autoridade existente, o Core faz backup SQLite online privado, incluindo WAL. [Evidência](evidence/server-1b/schema020-backup.json): `~/.local/state/narys/core/backups/server-1b-schema020-usKnag/authority-schema019.sqlite3`, SHA-256 `e634e611f49f7a8dc3bd199940877836ee40645d2b7076b5f7383c3b320ba0d2`, schema 19, 37 sessões e 94 mensagens. Não houve novo takeover nem recópia do desktop.

Admissão grava usuário/run/evento numa transação antes do recibo. Sucesso grava assistant, resultado completo, TaskRecord e evento terminal atomicamente. Falha/cancelamento preserva a entrada. Após crash, pending/running tornam-se interrupted com `restart_never_retries`; não há replay de efeitos incertos, summary automática ou retomada remota implícita. Cancelamento após o limite de commit é recusado como `commit_in_progress`, sem prometer desfazer efeito remoto.

As **37 sessões e 94 mensagens históricas** foram lidas pelo IPC e comparadas como subconjunto integralmente preservado, sem exportar conteúdo privado. Também foram preservados identidade, quatro memórias, 188 TaskRecords e 39 subtarefas anteriores. O total final é 38 sessões, 99 mensagens, 191 TaskRecords e três runs novos. [Histórico IPC](evidence/server-1b/history-ipc.json), [auditoria de dados/host](evidence/server-1b/host-final.json). A igualdade global das tabelas alteradas é naturalmente falsa por novas linhas e timestamps de política; a comparação das linhas históricas é verdadeira. Campos funcionais da política Conversation original foram restaurados após o gate; seu timestamp de atualização mudou.

## Gate real no serviço instalado

Executado em 10/10/2026, 16:23:23–16:23:46 (-03), usando Groq `openai/gpt-oss-20b`, thinking medium, sessão **38**, Fixed Groq, limite de saída 512, uma chamada por tarefa e retry desligado durante o gate. [Execução completa](evidence/server-1b/live-conversation.json).

| Tarefa | Resultado observado |
|---|---|
| 189 | Resposta real `SERVER1B-479acc90 43` ao marcador e soma 17 + 26; 236 tokens de entrada, 225 de saída |
| Reinício | PID 58145 → 58294; sessão e resultado recuperados iguais, sem replay |
| 190 | Pergunta sem repetir marcador/resultado; resposta recuperou `SERVER1B-479acc90` e `43` pelo histórico; 278 tokens de entrada, 56 de saída |
| 191 | Cancelada antes de seleção/admission do provider; usuário preservado, resultado nulo, nenhuma terceira chamada remota |

Foram **duas chamadas reais**, 795 tokens reportados ao todo, zero retry/fallback. Os clientes encerraram após os recibos, sem destruir a sessão. Um [segundo reinício](evidence/server-1b/final-recovery.json), PID 58294 → 59212, recuperou os três estados/resultados e cinco mensagens exatamente, sem nova chamada. Fechar/reabrir explicitamente a sessão também preservou as mensagens. Este é o fundamento do PASS; testes sintéticos apenas complementam a prova.

## Testes e regressões

| Gate final | Resultado / evidência |
|---|---|
| Domínio completo | 1.058 PASS, zero falhas, dois testes manuais de Codex appserver/auth ignorados; [log](evidence/server-1b/domain-final-tests.txt) |
| SecretStore direcionado | 13 PASS, incluindo novo leitor bloqueado sem reconstrução/escrita; [log](evidence/server-1b/secret-final-tests.txt) |
| Core | 32 unitários + 1 integração de controle + 6 integrações IPC = 39 PASS; [log](evidence/server-1b/core-complete-tests.txt) |
| Scripts operacionais | 6 PASS; [log](evidence/server-1b/python-tests.txt) |
| Compilação | Core check e desktop release check offline/locked PASS; [Core](evidence/server-1b/check-final.txt), [desktop](evidence/server-1b/desktop-check.txt) |
| Estrutura | Core fmt, diff sem erros de whitespace e unit systemd verificada; [fmt](evidence/server-1b/fmt-final.txt), [unit](evidence/server-1b/unit-verify.txt), [dependências](evidence/server-1b/core-dependencies.txt) |

A suíte completa do domínio precede apenas a adição do último teste do leitor bloqueado, coberto pela rodada direcionada; produção permaneceu igual. Cobertura inclui rollback atômico, recovery pendente/em execução e repetido, TaskId, eventos bounded/sanitizados, Fixed/Preferred/Auto, cancelamento, cliente desconectado, política antes de acesso a segredo, erros e recusa de ferramentas. Primeiras rodadas tiveram expectativas hardcoded de schema 19 e uma expectativa de ordem busy/sessão inválida corrigidas nesta etapa; logs intermediários são mantidos. Gates finais têm zero falhas. Persistem avisos de compilação; não houve smoke de GUI nem validação separada do MSRV em Rust 1.94.

## Instalação, serviço e consumo

O updater existente instalou o binário com verificação dos hashes anteriores e backup privado em `~/.local/state/narys/core/updates/server-1a-8ahgsssf` (prefixo herdado do updater). A unit não mudou. [Instalação](evidence/server-1b/install.txt), [artefatos](evidence/server-1b/installed-artifacts.json):

- Binário SHA-256: `53f0a3d31417675c7ede656be2fd1b89ffbead2146ee331a2d918de26f2fff4e`.
- Unit SHA-256: `df7b3205ebfcf5f673e9c72fd8aea4c0bba2c697d3f15a76feaaf4028b1a0810`.

`narys-core.service` permanece **active**, linger yes, `multi-user.target`, socket 0600, zero GNOME/Copilot, zero ferramenta/worker ativo. Nem dependências de produção nem linkage do binário incluem Tauri/GTK/WebKit/wry; não há ambiente gráfico no processo.

A [amostra ociosa isolada](evidence/server-1b/idle-isolated.json), sem probes de credenciais ou providers durante 10,018 s, mediu **30.520 KiB de RSS (29,8 MiB)** e **0,00 s de CPU adicional**. A primeira amostra coincidiu com diagnóstico Stronghold: RSS final 554.820 KiB (~542 MiB) e CPU +1,06 s; foi marcada `valid_for_idle=false` e conserva apenas observação de carga transitória, dentro dos limites existentes MemoryHigh 600M/MemoryMax 900M. A amostra correta demonstra consumo ocioso compatível; não é benchmark prolongado. Cold boot e experiência Termux completos ficam para o gate da 1D.

## Arquivos e limites do fechamento

O [manifesto completo](evidence/server-1b/changed-files.txt) relaciona arquivos desde o HEAD inicial. Grupos principais: Core runtime/conversation/server/ipc/main/storage/vault e testes; engine original e eventos em `narys-domain/src/luna`; persistência de runs/histórico/migration020; leitura SecretStore e dependência Linux; expectativas de schema em regressões; scripts de gate/auditoria; documentação e evidências. Cargo.lock dos três crates registra a dependência direta do backend já existente, sem upgrade geral de bibliotecas.

Não há impedimento ao critério central da SERVER-1B. Gemini/Cloudflare/Mistral estão compostos e configurados localmente, mas seu funcionamento remoto atual **não foi validado**; exigem confirmação específica de gratuidade/quota antes de habilitação. A declaração do operador não equivale a consulta de billing.

SERVER-1C ainda deve entregar a CLI de administração/uso remoto e UX de acompanhamento/aprovação; SERVER-1D deve executar o gate integrado de boot/SSH/Termux e fechamento. O cliente desktop deverá consumir o Core para voltar a operar após takeover. Agentes Narys 0.1 ainda precisam do executor/gateway de ferramentas, autorização por escopo, edição/build/test/diff e evidência de efeitos reais conforme LR-10B–F/plano 0.1. Conversation funcional não declara esses agentes concluídos. Não houve início automático dessas etapas.
