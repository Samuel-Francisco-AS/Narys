# LR-10C — relatório de entrega

**LR-10C — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE DA LUNA.**
Não autoriza fechamento/PASS definitivo ou efeitos autenticados.

Base limpa verificada e fetch de origin/main em
`e77c721beb091d6191b79f18b14cfa882b77babe`, sem divergência. Fast-forward sem
mudanças. Branch dedicada `lr-10c-authority-approval-sandbox`; main preservada,
sem PR ou merge. Documentos obrigatórios/arquitetura LR-10B, IPC, ExecutionBroker,
HumanLocal, trace, agentes, supervisor, journal e persistence examinados antes
das mudanças. Não foram aplicados consentimentos encerrados da LR-10A/B.

Commit de código validado: `5dd1cbcd6ca8f5fa7f369fd77a3c31e32fc2ee3f`. O commit posterior contém somente
documentação/evidências. Checksums dos binários e logs constam do JSON e de
[SHA256SUMS](evidence/lr10c/SHA256SUMS).

## Resultado concreto

Contratos agent-neutral; capacidades Core opacas não serializáveis; binding exato
e validade; provas independentes de boundary/finance; approvals pending/approve/
deny/expiry/consume/cancel/interrupted em SQLite transacional; recovery sem replay;
queries/deny/revoke CLI/IPC; SDK permissions/preToolUse deny em create e resume;
inventário nativo offline; testes de OS reais em diretórios descartáveis.

**A etapa não entrega execução agentiva real disponível.** Todos os perfis nativos
permanecem BLOCKED. O canal positivo de approval e de YOLO também fica BLOCKED:
socket0600/same UID não estabelece intenção humana; não há separação comprovada
para um runtime nativo autenticado. A engine positiva e seu HumanChannel são
exercitados somente por fixtures privadas. A consulta/negação é utilizável pela
CLI/SSH/Termux, mas não deve ser anunciada como fluxo positivo completo de approval.

O Bubblewrap funciona para o subprocesso offline testado; esse resultado não cobre
credenciais/provider networking/MCP/plugins/runtime Copilot completo. O SDK pinado
retorna output vazio em erro de parsing de hook e preMcpToolCall não possui veto.
Por isso, hooks não sustentam a liberação. Não existe mediação nativa pelo Broker.
Esse é um bloqueio fundamentado da candidata, não um PASS de isolamento parcial.
Detalhes no [threat model/arquitetura](LR-10C-AUTHORITY-APPROVAL-SANDBOX.md).

## Evidências e regressões

Resultados finais, comandos, durações, ignorados e checksums estão em
[validation-results.json](evidence/lr10c/validation-results.json), com logs de
Core, Domain, Python, release e higiene no mesmo diretório. A
[matriz de 23 cenários](evidence/lr10c/VALIDATION-MATRIX.md) delimita prova por cenário.

| Validação final | Resultado | Duração observada |
|---|---|---|
| Core: unitários + controle + protocolo + doctests | 117 PASS; 0 falhas/ignorados | 13,93s + 0,27s + 0,90s + 0,25s |
| Domain integral serial + doctests | 1.061 PASS; 0 falhas; 2 ignorados | 677,55s + 0,08s |
| CLI Python | 17 PASS; 0 falhas | 2,539s |
| Release offline/locked final | PASS | 1m52s incremental; build frio anterior 13m33s |
| Formatação Core/arquivos Domain alterados e diff-check | PASS | Sem erros |

**Total final: 1.195 testes PASS e 2 ignorados.** O Domain foi reexecutado
integralmente com `--test-threads=1`, sem compilações concorrentes. Nenhuma
assertion ou timeout foi relaxado. Frontend/GUI não alterados nem revalidados;
MSRV exato e caminhos autenticados permanecem NOT_VERIFIED.

As primeiras execuções detectaram expectativas de schema 021 ainda congeladas nos
testes e a faixa de schemas do importador. Foram corrigidas para schema 022,
preservando o teste de schema futuro (agora023), dados legados/WAL/conflitos e
backups. A fixture de hook inicialmente usava ID JSON-RPC string; SDK1.0.17 aceita
u64. Corrigido o harness para 900000, sem modificar protocolo/SDK para fazê-lo passar.
Erros de compilação iniciais das closures de fixture (E0282) foram resolvidos
com tipo de retorno explícito; uma substituição ampla atingiu temporariamente
`Connection::execute` (E0308) e foi corrigida antes das suítes. Falhas iniciais e
correções são registradas no histórico de validação, não contadas como PASS.

Uma reexecução Domain concorrente com builds sofreu quatro falhas de fixtures
HTTP/timeout. O log registra callbacks locais ausentes e prazo de 500ms excedido;
houve pressão de memória/swap durante compilação/linkedição. O reteste integral
serial, sem builds concorrentes, conserva os mesmos deadlines/assertions. A
revisão também corrigiu flags novos recusados pelo parser CLI e incluiu consulta
paginada/YOLO request pela CLI real. Não foram removidos testes ou relaxadas regras.

Dois gates Domain permanecem ignorados: real_app_server_handshake e
manual_final_codex_agent_bridge_gate, por exigirem Codex real/autenticado.
Não foram executados com --ignored. Não houve inferência, ferramenta autenticada,
consulta de quota/billing, consumo de AI Credits, YOLO real ou leitura de secrets.

Nenhum deploy/restart do serviço instalado, alteração de sudoers/SSH/firewall/boot,
instalação global ou mutation em repositório externo foi feita. O CLI pinado só
foi usado em sandbox offline para help e inventário, sem sessão ou provider.

## Desempenho e riscos residuais

Inventário pinado: **4,9866s** para três RPCs locais/start/cleanup no ambiente
offline. Durações de suítes e build constam do JSON/logs; não são latência de
produção nem benchmark de inferência. Engine bounded a 128 grants/4096 tombstones;
trace é passivo e mantém a retenção existente. Não foi medido consumo/endurance
de Copilot autenticado. Rust do host é 1.98.1; MSRV 1.94 exato não atestado.

Código preparatório privado sem emissores/executor operacional produz warnings de
dead_code; o Domain já possui warnings herdados. Não são suprimidos para aparentar
integração. Permanecem explicitamente PARTIAL os contratos positivos não conectados
e NOT_VERIFIED a contenção do runtime completo. Nenhum gatilho IPC/env/prompt libera
essas funções. A Narys 0.1 agentiva e a meta 17/10/2026 continuam sem aceite por esta
entrega; prazo não altera os bloqueios de segurança.

## Rollback e auditoria

Migration022 aditiva, única SQLite, backup online antes de upgrade. Não reduzir
user_version/apagar receipts, nem usar binário schema 021 em base022. Sem deploy
nesta sessão, o host instalado não necessita rollback. Para revisão, manter o
checkout/artefatos candidatos isolados. Uma instalação futura requer snapshot
verificável e seu próprio gate. Shutdown revoga admission; recovery não repete
efeitos. Ver procedimento completo na arquitetura.

A auditoria Luna deve distinguir prova algorítmica/fixtures, OS offline e runtime
nativo. Somente a auditoria independente pode conceder PASS definitivo e autorizar
fechamento. LR-10D/E/F e LR-11 mantêm seus gates próprios; nenhum bloqueio daqui é
resolvido por mera troca de status ou aprovação de outro marco.
