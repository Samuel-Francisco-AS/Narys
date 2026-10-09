# NARYS — LR-10A A9-HOST-ASSISTED

## Identificação e conclusão

**BLOCKED_PRE_SEND. Zero tentativas reais de inferência.**

**LR-10A A9-HOST-ASSISTED — IMPLEMENTAÇÃO/EXECUÇÃO CANDIDATA,
AGUARDANDO AUDITORIA INDEPENDENTE.** Sem PASS definitivo da LR-10A.

- Data: 09/10/2026, Fedora 44 / SSH/headless.
- Branch: `lr-10a-sdk-runtime-feasibility`; base local/remota inicial limpa: `4216ba2a6a0e75822587bf9032782ddc454338fa`.
- Main local/remota: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`, preservada.
- Implementação testada: **IMPLEMENTATION_COMMIT_REFERENCE**.
- HEAD documental/remoto: [histórico verificável da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility). Referência fechada em commit documental, sem SHA circular.
- Nenhum merge, PR, rebase, reset, force-push ou avanço à LR-10B.

A autorização recebida permite uma tentativa real na cota Student, executada no
host sob o usuário Linux, aceitando ausência de isolamento completo. Não permite
cobrança adicional/overage/contratação, inferências adicionais ou retry, paid
fallback, YOLO/Autopilot, ferramentas agentivas perigosas/shell, alterações
pessoais ou credenciais. A condição expressa exige interromper antes do envio
se não for possível comprovar custo dentro da franquia e ausência de cobrança.
Essa condição foi aplicada; a autorização não foi convertida em tentativa.

## Objetivo e implementação

O perfil HOST_ASSISTED é separado do ISOLATED. A primeira prova deveria estabelecer
auth/entitlement/modelo/custo antes de permitir uma única mensagem pelo SDK.
A autenticação atual não ficou disponível e os dados de quota/modelos não foram
obtidos. A execução parou no preflight, sem criar sessão real.

| Arquivo | Implementação |
| --- | --- |
| [src/bin/a9-host-assisted.rs](../experiments/lr-10a-sdk-runtime/src/bin/a9-host-assisted.rs) | Executável independente, owned-harness obrigatório, somente metadata preflight; rejeita modos send/A9 antes de iniciar CLI |
| [src/host_assisted.rs](../experiments/lr-10a-sdk-runtime/src/host_assisted.rs) | Options de auth normal, projeção sanitizada, bloqueios financeiros explícitos, primitive atômica de claim/fsync |
| [run_a9_host.py](../experiments/lr-10a-sdk-runtime/run_a9_host.py) | CLI pinado, env de processo filtrado, diretório estável seguro, measure.py sem alteração, stat/hashes e evidência sem overwrite |
| [verify_a9_host.py](../experiments/lr-10a-sdk-runtime/verify_a9_host.py) | Cargo e regressões Python dentro do subreaper; paths novos para logs/observações |
| [fixtures/a9_host_cli.py](../experiments/lr-10a-sdk-runtime/fixtures/a9_host_cli.py) | Peer RPC sintético sem rede/auth/provedor, para envio e persistência de fixture |
| [tests/host_assisted.rs](../experiments/lr-10a-sdk-runtime/tests/host_assisted.rs), [tests/test_a9_host.py](../experiments/lr-10a-sdk-runtime/tests/test_a9_host.py) | 8 testes Rust e 3 Python novos |
| [HOST-ASSISTED.md](../experiments/lr-10a-sdk-runtime/HOST-ASSISTED.md) | Perfil, limites, comandos e diferença frente ao A9 isolado |
| [adendo permanente](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md), [evidências](../experiments/lr-10a-sdk-runtime/evidence/a9-host-verification.json), este relatório | Histórico preservado e resultados novos identificados |

Não há live-send entry point nesta candidata: ligá-lo com auth/custo desconhecidos
introduziria um caminho não admitido. Não há flag/booleano que force admissão,
retry/fallback, nem enumere/descarte sessões pessoais. SDK send está somente nos
testes contra o peer sintético. Isso é preparação bloqueada, não implementação
completa de inferência real. O procedimento A9 isolado TXT foi preservado, sem
executar ou reinterpretar sua autorização.

## Preflight real e modelo escolhido

[Resultado final SDK/CLI](../experiments/lr-10a-sdk-runtime/evidence/a9-host-real-preflight.json):

| Sinal | Observação real |
| --- | --- |
| SDK | 1.0.17, runtime não bundled, default-features=false; Cargo/lock intactos |
| CLI | Hash idêntico ao CLI 1.0.91 verificado antes; RPC 1.0.90 / protocol 3 reobservado |
| Auth | authenticated=false; identidade/statusMessage omitidos |
| Entitlement / quota | account.getQuota: quota_unknown / rpc_error_unknown; nenhum saldo ou overage atual conhecido |
| Modelos | models.list: rpc_error_unknown; catálogo indisponível |
| Modelo escolhido | Nenhum; Auto não escolhido nem presumido elegível/barato |
| Envios SDK reais | 0 |
| Sessões reais create/resume | 0; nunca apontou sessão pessoal para consulta |
| Shutdown | SDK reportou graceful; harness confirmou kernel children exhausted/cleanup completo, sem sinais |
| Config global | Stat antes/depois iguais; conteúdo nunca lido pela POC ou restaurado |

O [probe inicial](../experiments/lr-10a-sdk-runtime/evidence/a9-host-real-preflight-auth-disabled-initial.json)
incorretamente acrescentava --no-auto-login, que também desabilita fallback de
credenciais existentes. Foi removido; o probe final usa use_logged_in_user=true
sem token explícito/base_directory. Mesmo assim auth permaneceu false. Não se
atribui a causa ao keyring, conta deslogada, serviço, versão ou GUI sem prova.
**BLOCKED_GUI_REQUIRED não foi demonstrado**; não se abriu janela, desbloqueou
keyring, reiniciou GNOME/GDM ou fez login/logout.

Ambiente allowlisted do processo: HOME, PATH=/usr/bin, LANG, existentes
DBUS_SESSION_BUS_ADDRESS/XDG_RUNTIME_DIR para resolução normal do CLI e opt-out de
download. Sem valores de token lidos/copiados; overrides de token removidos;
DISPLAY/WAYLAND removidos. O host oferece filesystem/rede normais ao CLI; isso
**não é sandbox nem auth boundary de menor privilégio**. Há keyring daemon
existente e gh disponível, mas sua presença não prova auth acessível. Nenhuma
inspeção de armazenamento, configs ou sessões pessoais foi usada para diagnóstico.

Documentação atual consultada: [billing para indivíduos](https://docs.github.com/en/copilot/concepts/billing-and-usage/individuals/billing)
descreve AI credits/token pricing e modelos Student por Auto; [usage SDK](https://github.com/github/copilot-sdk/blob/main/docs/features/usage-and-billing.md)
expõe preços/quota do runtime. A crate 1.0.17 tem AccountQuotaSnapshot em requests,
flags de overage e ModelBilling com preços opcionais. Dados ausentes ou de outra
execução não demonstram custo atual. [Session limits](https://github.com/github/copilot-sdk/blob/main/docs/features/session-limits.md)
são soft/post-call e podem ultrapassar o limite em uma resposta. Não foram usados
como teto rígido. [Auth SDK](https://github.com/github/copilot-sdk/blob/main/docs/auth/authenticate.md)
distingue fallback de credenciais e tokens explícitos. Essas páginas atuais não
são prova de comportamento idêntico à versão instalada; [contrato/provenance](../experiments/lr-10a-sdk-runtime/evidence/a9-host-contract.json)
registra hashes das fontes efetivamente utilizadas e as referências.

Quota 200/used 52/Auto das FIXes anteriores permanece histórica, não snapshot
atual nem admissão financeira. A candidata falha fechada mesmo se um mock declarar
multiplier=0: unidades, máximo de gasto e paid fallback ainda exigem contrato
verificado. Não se tentou habilitar orçamento, comprar créditos ou atualizar CLI.

## Matriz H1–H10

| Gate | Estado | Evidência e limite |
| --- | --- | --- |
| H1 Auth e entitlement | BLOCKED | RPC auth executado: false; entitlement/quota indisponíveis. Não PASS com base na autenticação histórica |
| H2 Modelo/quota sem cobrança | BLOCKED | Catálogo e quota indisponíveis; nenhum modelo/custo escolhido; ausência de overage/fallback pago não comprovada |
| H3 Workspace, DenyAll, zero tools | INCONCLUSIVE | Workspace 0700/fixture 0400 preparada e intacta; configurações DenyAll/zero tools/MCP/hooks/skills/extensions testadas no peer. Nenhuma sessão real criada para atestar enforcement do CLI |
| H4 Single-attempt guard | PASS | 8 claims concorrentes: uma vence; arquivo 0600/fsync; entries existentes/corruptas/symlinks bloqueiam. Estado estável live preparado, sem ATTEMPTED porque não houve envio; PASS é da primitive/testes, não de inferência |
| H5 Inferência SDK real | BLOCKED | sdk_send_calls=0; nenhuma mensagem ao Copilot |
| H6 Resposta final correta | NOT_RUN | Nenhuma resposta real; "5" aparece somente como valor esperado e dado de teste |
| H7 Persistência/retomada genuína | BLOCKED | Depende de H5; nenhum transcript real criado/consultado ou events.jsonl fabricado. Persistência do peer não prova Copilot |
| H8 Quota/uso observados | INCONCLUSIVE | RPC foi executado, mas quota_unknown; nenhuma delta/fatura/assistant.usage real. Saldo/consumo informado pelo provedor desconhecidos |
| H9 Shutdown/processos | PASS | SDK graceful e exhaustion real no preflight; regressões de timeout/adoção/setsid/external-control passam; zero identidades conhecidas sobreviventes |
| H10 Segurança/headless/produção | PASS | Scope experimental, configs somente stat iguais, sem GUI/serviços/credenciais/produção alterados; ausência de isolamento do host continua declarada |

PASS descreve somente a propriedade observada na coluna. Não torna H5/H6/H7
PASS operacional a partir de mocks, nem aprova LR-10A ou A9_ISOLATED.

## Tentativa, resposta e persistência

- Envio aceito: **NOT_RUN**; zero operações SDK send reais.
- Resposta recebida: **NOT_RUN**; nenhuma resposta de modelo publicada.
- Estado terminal: **BLOCKED_PRE_SEND**, operação de metadata terminada; nenhum session.idle usado como sucesso de tarefa.
- Resultado validado: **NOT_RUN**; não atribuir a soma sintética a Copilot real.
- Consumo informado pelo provedor: **indisponível**; sem quota autenticada antes/depois ou billing delta.
- Duas execuções reais de **metadata**, inicial/final, sem mensagem; não são duas tentativas de inferência.

Diretório estável `~/.local/state/narys` preparado 0700. Nenhum arquivo ATTEMPTED
criado no host: a tentativa não foi consumida. A primitive create_new/O_EXCL abre
relativamente a um FD de diretório validado, grava ATTEMPTED, fsynca arquivo e
diretório ANTES do send. Qualquer entry preexistente bloqueia sem ler seu conteúdo;
crash/timeout/erro não autorizam nova claim. Os testes usam apenas diretórios
sintéticos privados; esse arquivo fica fora do repositório/Git.

Testes fizeram três SDK sends sintéticos por suíte (sucesso, erro, timeout), sem
rede/provedor/quota. A fixture verifica o prompt exato, DenyAll/zero tools e
options.update; restart/resume do mesmo ID ocorre sem create ou segundo send na
sessão retomada. O histórico é exclusivamente do peer. FIX-2 session_not_found
permanece observação real histórica sem reteste de conversa genuína.

**Uma operação SDK send não garante uma requisição interna/faturável única.**
Não há prova do número de chamadas internas de uma inferência real nesta execução.
Zero SDK sends reais não é um saldo/fatura medido nem afirma franquia ilimitada.

## Testes, comandos e evidências

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc \
  /usr/bin/cargo build --offline --locked --bin a9-host-assisted --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir /tmp/narys-a9-host-tests-release-candidate
python3 experiments/lr-10a-sdk-runtime/run_a9_host.py /absolute/path/to/pinned/native/copilot --output /new/path/preflight.json
```

Na execução, o caminho CLI foi o ELF existente sob a instalação fnm/npm validado
pelo SHA; driver emite códigos/booleans, não stderr/protocolo ou credenciais. Os
paths publicados de output são os JSONs acima. Verify executou Cargo test
--offline --locked -- --test-threads=1 e unittest discover de todos test_*.py.

- [Rust: 46 PASS / 0 FAIL](../experiments/lr-10a-sdk-runtime/evidence/a9-host-rust-tests.txt): 38 herdados, 8 novos.
- [Python: 47 PASS / 0 FAIL](../experiments/lr-10a-sdk-runtime/evidence/a9-host-python-tests.txt): 44 herdados, 3 novos.
- [Harness da suíte](../experiments/lr-10a-sdk-runtime/evidence/a9-host-owned-tests.json), [gateway retestado](../experiments/lr-10a-sdk-runtime/evidence/a9-host-test-gateway.jsonl), [permissões retestadas](../experiments/lr-10a-sdk-runtime/evidence/a9-host-test-permissions.jsonl).
- Rustfmt --edition 2021 --check nos três Rust novos; py_compile nos quatro Python novos; JSON/JSONL, hashes, sintaxe, diff e escopo verificados.
- SDK/CLI/deps/lock/MSRV/Edition intactos. Rust/Cargo **1.98.1 preinstalados**; não afirmar reteste destes arquivos em 1.94 exato. Sem bundle/download/update global.
- Suíte Tauri completa NOT_RUN: nenhuma alteração de produção; regressões dirigidas da POC cobrem FIXes 1–4. [Verificação de preservação](../experiments/lr-10a-sdk-runtime/evidence/a9-host-verification.json).

Antes da última suíte, duas rodadas sintéticas também passaram (44 Rust/47 Python)
na preparação da primeira versão e após a correção de auth options; a rodada
final acrescenta dois testes negativos e contém a candidata revisada. Não se
converte o probe inicial com auth desabilitada em evidência da configuração final.

| Medição (uma amostra/cache quente) | Preflight final | Suíte owned final |
| --- | ---: | ---: |
| Wall ms | 1946.52 | 14553.52 |
| Pico RSS somado bytes | 301416448 | 733794304 |
| CPU amostrada (s, lower bound) | 1.41 | 7.26 |
| Cleanup harness ms | 14.78 | 32.76 |
| Recovery signals / survivors finais | 0 / 0 | 0 / 0 |

Startup SDK final: 1630 ms; stop: 29 ms. Métricas /proc a 50 ms podem perder
processos curtos e contar páginas compartilhadas várias vezes; não são PSS/RAM
incremental física ou benchmark. Exhaustion de children e PID/start-time, não
contagem global, sustentam cleanup. Recursos reais de inferência não medidos.

## Segurança e limites residuais

94 arquivos históricos do experimento permanecem byte a byte iguais à base,
inclusive README, lib.rs/persistence.rs/measure.py/boundary.py, A9 TXT e evidências
FIX1–4. Documento permanente apenas recebeu adendo; este relatório foi substituído.
Nenhum src-tauri, Cargo de produção, authority/Broker/registry, IPC/UI, TaskGraph,
LR-8.5 ou serviço Fedora foi modificado. Não se usa a POC como executor Narys.

O CLI normal pode resolver credenciais internamente e possui acesso do usuário ao
host. A POC não lê/exporta tokens/keyring/config ou sessões particulares; não
promete impedir que um runtime comprometido acesse esses recursos. Diretório de
trabalho e fixture RO não são isolamento. Raw logs são privados/temporários e não
publicados; somente projeções/códigos e evidências de testes sintéticos são entregues.
Config stat igual não é prova por comparação de conteúdo, que não foi permitida.

Timeout/startup/shutdown/cancel/cleanup são experimentais. Morte inesperada do
worker, descendentes adversariais/reparenting/namespaces e tarefas presas no kernel
não receberam contenção definitiva. Sinais ficam restritos aos filhos atribuídos
por pidfd; processos externos são preservados. Não foi implementado supervisor
LR-10B ou approval engine LR-10C.

A9_ISOLATED continua com BLOCKED_AUTH_BOUNDARY, BLOCKED_NETWORK_BOUNDARY e
BLOCKED_SUPERVISOR_FAILURE_CONTAINMENT; inferência/persistência isoladas não
executadas. A aceitação humana do host-assisted não resolve esses gates.

## Recomendação e auditoria

**Não avançar automaticamente à LR-10B.** Entregar BLOCKED_PRE_SEND para auditoria:
verificar por que a auth normal não está disponível em headless, sem ler/copy
credenciais ou forçar login; estabelecer metadata atual de modelo/quota/unidades,
limite de custo/no overage/paid fallback e estado de sessão privado autenticado.
Só depois desses gates poderá existir um sender real guardado pela claim persistente.
A tentativa real permanece não consumida; este relatório não amplia a autorização.

O prazo 17/10 não muda a condição financeira nem permite publicar um PASS de
resposta/persistência sintéticas como Copilot real. O experimento permanece separado
da produção, sem implementação de release. Luna auditará os resultados pelo GitHub
antes da decisão de próximos passos. Nenhum PASS definitivo da LR-10A.

**LR-10A A9-HOST-ASSISTED — BLOCKED_PRE_SEND; IMPLEMENTAÇÃO/EXECUÇÃO CANDIDATA,
AGUARDANDO AUDITORIA INDEPENDENTE.**
