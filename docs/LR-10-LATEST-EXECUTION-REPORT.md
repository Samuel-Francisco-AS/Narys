# NARYS — LR-10A / A9-FIX-1

Headless Authentication Recovery & Financial Preflight — SEM INFERÊNCIA.

**AUTH_NOT_RECOVERED. A9 real NOT_RUN; BLOCKED_PRE_SEND mantido.**
**LR-10A A9-FIX-1 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE.**

## 1. Identificação e Git

- Execução: 09/10/2026 em America/Fortaleza; timestamps de evidências em UTC,
  já em 10/10. Repositório Samuel-Francisco-AS/Narys.
- Branch exclusiva: `lr-10a-sdk-runtime-feasibility`.
- HEAD inicial local/remoto confirmado: `938dc40ad3575435b97479fa14b8f3a2b1f7506b`.
  Workspace inicial limpo; nenhuma alteração humana sobrescrita.
- Main local/remota: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`, preservada.
- Implementação testada: IMPLEMENTATION_COMMIT_TO_REFERENCE.
- HEAD documental final: referência verificável ao
  [histórico desta branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility/).
  O SHA do próprio commit documental não é inserido circularmente no arquivo.
- Sem PR, merge, rebase, reset ou force-push. A publicação é somente nesta branch.

## 2. Objetivo, autorização e arquivos

Diagnosticar o contraste entre FIX-2 autenticada e A9 não autenticado, mediante
leitura estática e diferenças controladas de contexto. Esta execução é exclusivamente
metadata: a autorização de uma tentativa futura na franquia Student **não foi
consumida**. Não autoriza pagamentos, alteração de credenciais, GUI ou ferramentas.
HOST_ASSISTED_NOT_SANDBOX continua distinto de A9_ISOLATED.

| Arquivo | Mudança |
| --- | --- |
| [diagnose_a9_auth.py](../experiments/lr-10a-sdk-runtime/diagnose_a9_auth.py) | Matriz metadata-only, contexto não secreto allowlisted, clientes novos, pin CLI, stat de config, guard read-only, verificação de serviços/cleanup e interrupção fechada |
| [tests/test_a9_auth_diagnostic.py](../experiments/lr-10a-sdk-runtime/tests/test_a9_auth_diagnostic.py) | 10 testes novos de ambiente, diferenças unitárias, PATH, marker não consumido, bloqueio antes do CLI e identidade PID/start-time |
| [fixtures/auth_context_cli.py](../experiments/lr-10a-sdk-runtime/fixtures/auth_context_cli.py) | Peer local sintético de metadata, sem autenticação/provedor; conta chamadas indevidas |
| [tests/auth_diagnostic.rs](../experiments/lr-10a-sdk-runtime/tests/auth_diagnostic.rs) | 2 testes novos do contrato e metadata positivo/negativo, sem sessão ou send |
| [HOST-ASSISTED.md](../experiments/lr-10a-sdk-runtime/HOST-ASSISTED.md) | Reprodução e limites deste diagnóstico |
| [evidências novas](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-verification.json) | Matrizes inicial/final, inspeção, testes e verificação de preservação |
| [documento permanente](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md) | Adendo A9-FIX-1; histórico preservado |
| Este relatório | Substitui somente o relatório reutilizável anterior |

`run_a9_host.py`, binário Rust A9, `host_assisted.rs`, `claim_attempt`, `measure.py`,
FIXes 1–4, Cargo/lock e controles isolados permanecem byte a byte iguais à base.
Não foi aplicada uma correção de auth sem comprovação positiva. Não foi criado
modo send, retry, token explícito, proxy novo, sandbox ou supervisor de produção.

## 3. Comparação factual FIX-2 × A9

Fontes históricas: [FIX-2 final](../experiments/lr-10a-sdk-runtime/evidence/fix-2-real-existing-auth-final.json),
[metadata protegida](../experiments/lr-10a-sdk-runtime/evidence/runtime-existing-auth-protected.json),
[A9 final anterior](../experiments/lr-10a-sdk-runtime/evidence/a9-host-real-preflight.json),
[código FIX-2](https://github.com/Samuel-Francisco-AS/Narys/blob/323584f/experiments/lr-10a-sdk-runtime/src/persistence.rs)
e [contrato inspecionado nesta execução](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-contract.json).

| Dimensão | FIX-2 existente-auth | A9 / diagnóstico atual |
| --- | --- | --- |
| SDK/instalação | SDK 1.0.17, CLI hash pinado | Mesma crate/mesmos bytes CLI; RPC atual 1.0.90, protocolo 3 |
| Program | bwrap com CLI nativo em prefix_args | CliProgram::Path nativo explícito/canonicalizado |
| Mount/contexto | Host amplo RO, overlays privados de workspace/state/session-state | Host-assisted direto; nenhum mount amplo reproduzido |
| ClientMode | CopilotCli default | CopilotCli comprovado em teste; não Empty |
| use_logged_in_user | true | true; sem --no-auto-login, sem github_token |
| base_directory/COPILOT_HOME | None no existing-auth, COPILOT_HOME removido | None/removido; sem redirecionamento de conta por state privado |
| env/env_remove | Herança mais removals explícitos | Allowlist; token env não herdado; valores nunca adquiridos |
| PATH | Herdado; fingerprint histórico ausente | /usr/bin no baseline; variante com diretórios originais existentes validados |
| HOME/D-Bus/runtime | Herança histórica sem valores/fingerprint preservados | Presença atual e socket/owner verificados; valores somente em memória |
| cwd/logs | Paths privados da fixture | Paths privados /tmp; log-level None; --disable-builtin-mcps |
| Sessões | Matriz vazia/sintética histórica | Nenhuma operação real de sessão nesta FIX |
| Auth | authenticated na evidência histórica | false em todas as variantes reais desta execução |

A crate consumida foi inspecionada localmente, com hash comparado à FIX-4:
`build_command` herda ambiente, aplica env/env_remove, exporta base_directory
como COPILOT_HOME e desabilita keytar no modo Empty. `auth_args` usa
--no-auto-login quando o effective use_logged_in_user é false. Stdio inclui
--no-auto-update. Os números de linha e hash estão no JSON de contrato; a
proveniência publicada tem dirty=true e não é tratada como tag idêntica.
A [documentação oficial atual de auth](https://github.com/github/copilot-sdk/blob/main/docs/auth/authenticate.md)
descreve uso das credenciais existentes pelo SDK. Foi consultada em 10/10 UTC,
mas não substitui inspeção da versão 1.0.17 nem prova auth no host atual.

O erro inicial A9 com --no-auto-login já havia sido corrigido na execução anterior;
a evidência final anterior ainda era false. Não o reapresentamos como causa desta
falha. A identidade de instalação não comprova identidade de sessão/ambiente.
Os dados históricos não permitem reconstruir valores de PATH/XDG/bus, estado de
credenciais ou sessão gráfica da FIX-2. Não se buscou essa informação em arquivos
pessoais. Nenhuma causa de auth foi confirmada por controle positivo real.

## 4. Matriz real de contexto

[Matriz final sanitizada](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-real-auth-matrix.json):
quatro Client/CLI novos, sem cache entre variantes. Cada linha modifica uma única
dimensão do baseline. Somente auth.getStatus, models.list, account.getQuota e
lifecycle/status; não há método de sessão. Timeout de harness 65 s por variante;
limites SDK permanecem 15 s por RPC e shutdown limitado.

| Variante final / UTC 10/10 | Necessidade / diferença | Auth | Catálogo/quota | Wall ms / RSS bytes |
| --- | --- | --- | --- | --- |
| Baseline / 00:36:40 | Reproduzir contexto A9, PATH=/usr/bin | false | rpc_error_unknown / quota_unknown | 1407.74 / 301920256 |
| PATH validado / 00:36:41 | Testar resolução auxiliar herdada | false | Mesmo resultado | 1341.79 / 302870528 |
| Sem DBUS_SESSION_BUS_ADDRESS / 00:36:42 | Isolar contribuição da variável de bus | false | Mesmo resultado | 1312.57 / 303857664 |
| Sem XDG_RUNTIME_DIR / 00:36:44 | Isolar contribuição de runtime | false | Mesmo resultado | 1343.45 / 303218688 |

O PATH restaurado omite entradas inexistentes/duplicadas e rejeita entradas vazias,
relativas, de dono não confiável ou graváveis por grupo/outros. Isso é validação
de descoberta, não sandbox/integridade dos executáveis. Os valores não são publicados.
`gh` resolve ao mesmo executável /usr/bin tanto no PATH original quanto no baseline.
Restaurar PATH não foi suficiente para recuperar auth. Remover bus/runtime quando
baseline já falha não prova que sejam dispensáveis em um contexto autenticado.

XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME, GH_CONFIG_DIR e COPILOT_HOME estão
**ausentes** hoje: restauração dessas dimensões NOT_RUN, sem inventar valores.
Nomes conhecidos de token são todos ausentes nesta captura, registrados apenas
como booleans. HOME/bus/runtime presentes; DISPLAY/WAYLAND ausentes.

GNOME/GDM ausentes antes/depois; keyring daemon já existente e
org.freedesktop.secrets já owned. NameHasOwner consulta somente o bus daemon,
sem ativar/desbloquear o serviço. Socket existente/acessível não comprova conteúdo,
unlock ou disponibilidade de credencial. Não houve dump/env/proc-environ, gh auth
status bruto, keyring introspection, tracing ou inspeção de configuração pessoal.
`gh auth status` adicional não foi necessário: foi verificada a resolução auxiliar,
e os gates SDK separados já retornaram indisponibilidade; nenhuma informação de
conta extra foi extraída. Não se afirma que gh esteja autenticado.

[Matriz inicial](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-real-auth-matrix-initial.json)
fica preservada. Também teve quatro resultados false. Após fortalecer códigos de
erro/guard e adicionar teste de bloqueio pré-CLI, a matriz foi retestada com o
script final. A revisão final também removeu o timeout Python dos helpers de status, evitando
um mecanismo implícito de kill por PID; busctl mantém seu --timeout=2 próprio, e
ps é uma consulta local sem recovery. A matriz final foi retestada. A
[rodada intermediária](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-real-auth-matrix-guard-retest.json)
também fica preservada. São **12 preflights metadata nesta FIX, zero sends/sessões reais**.
A rodada inicial não é reinterpretada como execução da fonte final.

## 5. Gates atuais e segurança financeira

- AUTH_RECOVERED_HEADLESS: **não alcançado**; decisão AUTH_NOT_RECOVERED.
- CATALOG_OBSERVED: **BLOCKED**; models=null, sem origem de lista atual disponível.
- QUOTA_OBSERVED: **BLOCKED**; snapshots=null, source account.getQuota,
  quota_unknown / rpc_error_unknown. Unidade/saldo/entitlement atual desconhecidos.
- READY_FOR_NEXT_FINANCIAL_GATE: **não**; auth/catálogo/quota atuais insuficientes.
- A9 real: **NOT_RUN / BLOCKED_PRE_SEND**.

Não reutilizar histórico 200/52, Auto ou multiplicador sintético zero como admissão.
Não foi escolhido modelo. Mantidos, sem bypass:
`billing_units_and_maximum_cost_unverified`,
`no_paid_fallback_enforcement_unverified`,
`private_authenticated_session_state_unverified`.
Quota em requests não comprova unidades atuais de cobrança nem máximo de custo.
Zero inferências enviadas pela POC é o fato observado/estrutural; não se afirma
zero cobrança externa, saldo atual, delta de quota ou valor de fatura. Uma futura
chamada SDK pode representar múltiplas operações faturáveis internas.

## 6. Verificação D1–D12

| Gate | Resultado | Evidência / limite |
| --- | --- | --- |
| D1 Fontes/contextos históricos | PASS | Comparação factual acima, hashes/fontes; fingerprint histórico de ambiente não disponível |
| D2 CLI/SDK pinados | PASS | Hash CLI igual a FIX-2/A9; SDK 1.0.17/runtime, RPC atual; nenhuma atualização/download |
| D3 Diagnóstico sem credenciais | PASS | Allowlist/presence booleans, PATH metadata, bus owner; zero valores de tokens adquiridos/publicados |
| D4 Causa confirmada | INCONCLUSIVE | Nenhum controle positivo real; causa UNKNOWN, sem atribuir GNOME/logout/keyring |
| D5 SDK autenticado headless | BLOCKED | Real auth=false em 4 variantes finais; fixtures não substituem este gate |
| D6 Catálogo/quota atuais | BLOCKED | Erro RPC sanitizado; não há lista/snapshot real atual; ausência = unknown |
| D7 Sem inferência/sessão/claim | PASS | Sem live-send entry point/operações de sessão; marker ausente antes/depois; guard preservado |
| D8 Config/logs | PASS limitado | Config somente stat igual em todas as variantes; sem conteúdo/hash de config; logs privados descartados, evidências projetadas |
| D9 Ownership/cleanup | PASS no contrato experimental | ECHILD, kernel inventory vazio, PID/start-time ausentes, zero recovery signals; regressões externas/timeout/adoção aprovadas |
| D10 Regressões | PASS | 48 Rust = 46 anteriores + 2; 57 Python = 47 anteriores + 10; fonte final, logs/runner owned abaixo |
| D11 Headless | PASS | GNOME/GDM ausentes, serviços observados iguais; nenhuma UI/serviço/login/credencial alterado pela POC |
| D12 Separação de gates | PASS | Auth não recuperada, financeiro bloqueado, A9 real NOT_RUN; sem alegação operacional baseada em mocks |

## 7. Comandos e evidências executados

Executados a partir do root, sem downloads e com ferramentas preexistentes:

```sh
git status --short
git branch --show-current
git rev-parse HEAD
git ls-remote origin refs/heads/lr-10a-sdk-runtime-feasibility refs/heads/main
git show 323584f:experiments/lr-10a-sdk-runtime/src/persistence.rs
git show 323584f:experiments/lr-10a-sdk-runtime/run_fix2.py
git show 323584f:experiments/lr-10a-sdk-runtime/src/lib.rs
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc /usr/bin/cargo build --offline --locked --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml --bin a9-host-assisted
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc /usr/bin/cargo test --offline --locked --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml --test auth_diagnostic -- --test-threads=1
python3 -m unittest discover -s experiments/lr-10a-sdk-runtime/tests -p test_a9_auth_diagnostic.py -v
python3 experiments/lr-10a-sdk-runtime/diagnose_a9_auth.py /absolute/path/to/pinned/native/copilot --output /fresh/evidence.json
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-final-regressions
python3 -m py_compile experiments/lr-10a-sdk-runtime/diagnose_a9_auth.py experiments/lr-10a-sdk-runtime/fixtures/auth_context_cli.py experiments/lr-10a-sdk-runtime/tests/test_a9_auth_diagnostic.py
git diff --check
```

Paths `/absolute/path/...` e `/fresh/evidence.json` acima são placeholders para
reprodução; o resultado/hash/nomes reais de evidência estão nos artefatos linkados.
Rustfmt 1.9.0 preinstalado foi usado com --edition 2021 e --check no novo teste.
Rust/Cargo efetivos 1.98.1, Python 3.14.7, Fedora 44. Toolchain 1.94 temporário
anterior expirado; não reinstalado. Não declarar reteste específico de 1.94.
Nenhum MSRV/Edition/Cargo/dependência de produção ou POC foi alterado.

[Runner owned final](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-final-regressions/a9-host-owned-tests.json),
[48 Rust individualizados](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-final-regressions/a9-host-rust-tests.txt),
[57 Python individualizados](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-final-regressions/a9-host-python-tests.txt),
[gateway sintético](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-final-regressions/a9-host-test-gateway.jsonl),
[permissões sintéticas](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-final-regressions/a9-host-test-permissions.jsonl).
O runner histórico mantém rótulo A9_HOST_ASSISTED; esta pasta nova e a
[verificação da execução](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-1-verification.json)
identificam A9-FIX-1 e hashes dos arquivos novos. A primeira rodada de regressões,
48/56 antes do décimo teste Python, permanece em evidence/a9-fix-1-regressions/.
A rodada intermediária 48/57 permanece em evidence/a9-fix-1-guard-retest-regressions/.

O peer de metadata testa auth sintético ausente/presente e --no-auto-login;
identidade/prosa de erro não são exportadas, e positivo aparente nunca remove os
bloqueios financeiros. Guard, crash/rerun/race, SDK lifecycle, DenyAll, zero tools,
MCP/extensões, filesystem, rede offline, timeout/setsid/adoção e controle externo
continuam retestados. Sends dos testes anteriores são exclusivamente peers locais
sintéticos, jamais a instalação Copilot real. Nenhuma UI foi testada.

Suíte Tauri completa NOT_RUN: diff exclusivo da POC/documentação, sem código,
dependência ou inicialização de produção alterados. Repeti-la não verificaria a
falha de auth. Formatação/sintaxe, JSON/JSONL, diff e hashes históricos foram
verificados; resultados estão no JSON de verificação. Somente linhas vazias
finais dos logs Cargo novos foram normalizadas para diff --check; resultados
não foram alterados.

## 8. Configuração, marker, processos e limites

Config.json permaneceu com inode/tamanho/mtime/ctime iguais antes/depois dos
probes. Somente esses metadados foram lidos: não se afirma equivalência criptográfica
de conteúdo, não houve leitura de credenciais/configuração nem restauração automática.
Nenhum login/logout, exportação de conta/token, PAT, keyring daemon/UI ou serviço
iniciado. Conteúdo de sessões pessoais nunca acessado.

`~/.local/state/narys` existente foi verificado com opens de diretório ancorados,
no-follow, owner atual e 0700. Nenhum attempt file foi criado/lido; ausência
verificada antes/depois. `claim_attempt` segue intacto, create_new/0600/fsync e
resistente a concorrência, crash/corrupção/rerun nos testes sintéticos.

Todos os 12 probes metadata e runner de testes finais comprovaram cleanup completo,
ECHILD/inventory vazio e identidades conhecidas ausentes. SDK reportou shutdown
graceful nos probes, sem sinais do harness; são responsabilidades distintas.
A suíte mantém o teste de processo externo intocado e a checagem de ausência das
identidades próprias de fixtures. Nenhum killpg/sinal por PID numérico foi usado
pela recuperação. Variação no total de processos do sistema não atribui ownership.

A matriz final teve startup 978–1010 ms, shutdown SDK 28–33 ms, cleanup harness
13.86–25.12 ms. RSS agregado é amostrado e pode contar páginas compartilhadas duas
vezes; CPU é limite inferior. Uma amostra por variante/cache quente não é benchmark
nem prova de causalidade de desempenho. Valores detalhados constam no JSON.

O harness não contém adversário com morte inesperada do worker, nested subreaper,
escape de namespace/reparenting ou tarefa ininterruptível do kernel. Os testes de
worker failure retornam inconclusive; não há supervisor LR-10B novo. Perfil
host-assisted não isola dados/processos do mesmo UID ou egress arbitrário.
Bloqueios AUTH_BOUNDARY, NETWORK_BOUNDARY e SUPERVISOR_FAILURE_CONTAINMENT do modo
isolado permanecem; esta investigação não os resolve.

## 9. Pendências, recomendação e auditoria

**AUTH_NOT_RECOVERED / FIX-AND-RETEST.** Uma causa continua desconhecida; sem
controle positivo real, alterar PATH/XDG, forçar token ou restaurar montagens
amplas não seria correção comprovada. Não se declara GUI obrigatória ou logout.
Nenhum READY_FOR_NEXT_FINANCIAL_GATE/READY_FOR_A9/PASS definitivo LR-10A.

Luna deve revisar a evidência/contexto histórico e decidir se existe uma ação
humana proporcional para tornar a autenticação existente disponível no SSH/tmux.
Se ela exigir login, unlock, GUI ou mudança de credencial/serviço, deve ser um
procedimento separado; esta FIX não o executou nem solicita autorização de inferência.
Um novo probe metadata só deve seguir um contexto novo validado, sem repetir
indefinidamente a mesma matriz ou exportar segredos.

Após auth real positiva, ainda serão necessárias lista/modelos e quota atuais,
unidades/teto de custo, negação efetiva de paid fallback/overage e estado privado
autenticado. Somente depois desses gates/auditoria poderá ser preparado o único
request futuro autorizado, com guard antes do send, sem retry. Não contornar
bloqueios financeiros nem consumir a tentativa para diagnosticar autenticação.
Não avançar para A9, LR-10B ou outro gate por iniciativa própria.

**LR-10A A9-FIX-1 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE DA LUNA.**
