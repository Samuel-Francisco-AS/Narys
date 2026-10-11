# LR-10C — relatório de entrega

## FIX-1 — Trusted Approval & Operational Boundary

**CANDIDATA PARA REAUDITORIA**, sem PASS final. Corrige a candidata auditada
`e80943df2dd6c71ca927230167166c94da9281d2` na mesma branch. A seção inicial abaixo
fica preservada como histórico pré-FIX-1, não como disponibilidade vigente local.

### Resultado e alcance

A cadeia operacional **local/offline** foi executada através do Core/CLI reais:
peer sintético confinado → pedido tipado → preview/decisão no endpoint do operador
→ approval única → claim+ledger atômicos → ferramenta real confinada → arquivo
verificado por Core → resultado SQLite → cleanup/cancel/recovery verificáveis.
Não depende de fixtures privadas que fabriquem HumanChannel na engine: o emissor
operacional existe no modo explicitamente descartável `boundary-serve`.

O peer Python é um recurso de prova explicitamente selecionado, não um agente
Copilot reimplementado nem simulador permanente do produto. O SDK/CLI oficiais
permanecem pinados e sua política nativa restrita. **Copilot autenticado, Assistido
nativo, Autônomo isolado e YOLO real continuam BLOCKED; cobertura autenticada
NOT_VERIFIED.** Não equivale a entrega agentiva Narys0.1 ou integração de conversa/
TaskGraph de produção. Não houve deploy, restart do serviço instalado ou inferência.

A confiança do operador vem da **exclusão de todos os executáveis não confiáveis
dessa instância** do domínio do endpoint. Não vem de UID/TTY/challenge/flag. Não
protege host/conta comprometidos ou agentes lançados fora dessa contenção; por
isso não abre approve_once no servidor host-assisted. Confirmações de teste são
roteirizadas no domínio do operador: prova da interface/isolamento, não presença
física de humano ou transporte SSH autenticado real. Ver
[fronteiras e limitações](LR-10C-AUTHORITY-APPROVAL-SANDBOX.md#fix-1--trusted-approval--operational-boundary).

### Componentes operacionais

`agent_authority/local.rs` é executável no Core release e utiliza a AuthorityService
existente; não há segunda engine de approval. Usa a Database/writer lease SQLite
existente, migration023 aditiva, TaskId e sessão criados pelo Core, contextos
agent-neutral, proofs limitadas de boundary/zero-cost, capacidade somente em memória.

Canal de intents é pipe, workspace do peer read-only. Canal do operador fica
fora dos namespaces, sem secrets/grants em env/prompt/workspace. Preflight testa
negativas reais. Ferramenta é runner Python fixo, criação exclusiva de marker
ASCII e comandos estritamente sleep/sha256sum; não existe shell genérico pela IPC.
Preview mostra programa/argv/cwd/conteúdo/path/digest; falta de preview seguro
bloqueia. Todas as ações exigem approval. Timeout/deny/cancel/restart não aprovam.

Claim e journal são transacionais antes do efeito. Mutex não cobre execução longa.
PID1/pidfd+wait certificam cleanup; AtomicBool transmite cancelamento. Kernel
namespaces/cap-drop/userns-disabled, mounts por FD e seccomp formam o boundary.
Core verifica bytes/hash por openat/nofollow após cleanup. Resultado distingue
claimed, started, cancel_requested, completed, failed, cancelled e uncertain.
Falha de SQLite pós-efeito/recovery incerto fecha reentrada, sem replay/reset.
Shutdown também cobre a corrida com preflight anterior ao registro do peer.
Reserva atômica limita quatro peers incluindo admissions concorrentes em preflight.

### Validação final e evidências

Commit de código FIX-1: `05ea03dcab73839140dad2911282110941895183`; publicação de evidências em commit posterior.

| Validação final FIX-1 | Resultado | Duração observada |
|---|---|---|
| Core completo + protocolo + doctests | 119PASS,0fail | 38.58s |
| Domain completo + doctests | 1061PASS,0fail,2ignored | 671.45s |
| Python completo, com 27 testes operacionais | 44PASS,0fail | 12.703s |
| Repetição operacional com binários release | 27PASS,0fail | 8.376s |
| Build release offline/locked | PASS | 2m 07s |

Total da passagem Core/Domain/Python: **1224PASS,2ignored**. Os 27 testes release
são repetição dos mesmos casos, não aumento de cobertura. Prova exportada release:
startup0.024443s; cancel ack0.002358s; cleanup0.015234s; Core RSS/HWM13784KiB
(snapshot, não pico da suíte). Nenhuma dessas métricas representa provider/Copilot.

Comandos/totais/durações/checksums: [results.json](evidence/lr10c/fix1/results.json).
Prova exportada da cadeia: [operational-chain.json](evidence/lr10c/fix1/operational-chain.json).
Matriz26: [VALIDATION-MATRIX.md](evidence/lr10c/VALIDATION-MATRIX.md).
Logs, falhas iniciais e regressões estão em `docs/evidence/lr10c/fix1/`.

A primeira compilação detectou Transaction não-Send atravessando await; a conexão
passou a ser lexicalmente encerrada antes do preflight. Uma assinatura parcial
ao adicionar liveness do peer produziu E0425 e foi corrigida. Primeira suíte Core:
102pass/1fail, fixture de schema antigo mantendo tabelas novas; migration023 segue
o padrão idempotente IF NOT EXISTS. Primeiros testes operacionais:15pass/5fail;
corrigidos wait de readiness do pidfd (não basta poll instantâneo), status
not_started antes do payload e fechamento indevido por cleanup ainda pendente.
Primeira regressão Domain:1058pass/1fail/2ignored; sentinela de schema futuro
ainda era23 e passou para24 após a migration023. Nova execução integral serial
foi exigida. Também foram corrigidas reservas concorrentes de peers e o limite
transacional128tasks. Shutdown sinaliza todos os claims antes de qualquer
persistência; erro na primeira revogação não deixa ferramentas seguintes rodando.
Essas falhas não são contadas como PASS. Novo teste físico de mount por FD confirma
que trocar o path não redireciona o efeito. Nenhuma negativa de isolamento/timeout
operacional foi relaxada. A fixture
antiga de corrida foi ajustada ao novo contrato: cancelamento após início pode
ter efeito, mas não publicar sucesso; o novo teste callback300ms comprova esse
caso e os testes de processos certificam cleanup.

Execuções pesadas usam CARGO_BUILD_JOBS=1 e testes Rust serializados; Domain sem
build concorrente. Duas rodadas leves Core/Python tiveram concorrência limitada
após a compilação, não carga de linkedição junto das fixtures HTTP. Os dois gates
Domain reais continuam ignorados: Codex app-server real e bridge autenticado.
Não foram usados --ignored, providers, quota/billing ou credenciais reais.
Frontend/GUI e MSRV Rust1.94 exato não foram revalidados (sem alterações nesse
escopo; host1.98.1). SSH real permanece NOT_VERIFIED; sockets reais provam a
semântica de desconexão/reconexão sem replay.

### Recursos, custo, risco e rollback

Medições release locais no JSON de cadeia e logs; não são latência de inferência
ou benchmark de Copilot autenticado. Quatro peers simultâneos, oito intents/peer,
128tasks por instância, payload/saídas bounded; rlimits do filho CPU40s, AS256MiB,
file64KiB/fds64/core dumps0; tmpfs16MiB. Não certifica DoS agregado/endurance ou
contenção por cgroups. Perfis automáticos seguem bloqueados.

Zero autorização financeira, zero chamadas de inferência/autenticadas, zero
AI Credits/overage. Opção A foi selecionada para Copilot futuro, por impossibilidade
de provar ausência de rota nativa só com hooks/custom tools. Gateway de credenciais/
rede externo ao executor é arquitetura necessária, ainda não provada com o pin;
não foi implementado proxy fictício nem relaxada rede para autenticar.

Rollback: nenhuma instalação foi alterada. Encerrar a instância descartável,
observar certificados; preservar root bloqueado se cleanup incerto. Não apagar
receipts/reduzir user_version/reexecutar claims. Binário anterior exige snapshot
pré023 isolado e avaliação dos dados posteriores. `main` é preservada; publicação
somente nessa branch, sem PR/merge. Auditoria Luna decide fechamento/PASS final.

---

## Registro histórico da entrega inicial (pré-FIX-1)


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
