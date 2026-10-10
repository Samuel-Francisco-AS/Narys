# LR-10A / A9-FIX-2 — Authenticated Host Recovery & SDK Metadata Verification

## 1. Identificação e decisão

Projeto Narys; execução de 10/10/2026; branch exclusiva `lr-10a-sdk-runtime-feasibility`.
Base local/remota conferida: `f0ae5787c7cde7b54a1970727e2a52b43f4612f8`.
Main local/remota: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`, intocada.
Workspace inicial limpo, sem divergência ou alteração humana descartada.

- Implementação efetivamente usada no probe real: [42756450fb8b942fdc2f7ec42b6ece364f3e5c73](https://github.com/Samuel-Francisco-AS/Narys/commit/42756450fb8b942fdc2f7ec42b6ece364f3e5c73).
- Implementação final testada sinteticamente e evidências: [8226149109b47ed3ec6dfa232f3013960e31336c](https://github.com/Samuel-Francisco-AS/Narys/commit/8226149109b47ed3ec6dfa232f3013960e31336c).
- Ambos os commits enviados e confirmados no remoto antes deste relatório.
- HEAD documental final/local/remoto: commit documental que publica este arquivo,
  verificável no [histórico da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility/docs/LR-10-LATEST-EXECUTION-REPORT.md). Referência sem SHA circular.

**SDK_AUTHENTICATED_WITH_GUI confirmado por SDK real. FIX_AND_RETEST da integridade;
FINANCIAL_ADMISSION=BLOCKED; A9_REAL_INFERENCE=NOT_RUN.** O sucesso de auth não
encobre a mudança de metadados de config.json durante o probe. A causa dessa
mudança e a causa individual da recuperação de autenticação permanecem desconhecidas.

**A9-FIX-2 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**
Nenhum PASS definitivo da LR-10A, nenhuma passagem à LR-10B.

## 2. Objetivo, autorização e escopo

Verificar reconhecimento da autenticação existente por SDK Rust 1.0.17, com
operações de metadata e lifecycle. A intervenção humana externa (reboot gráfico,
login físico, login keyring desbloqueado, CLI interativo signed-in v1.0.95 e MCP
conectado) é controle positivo informado pelo usuário; não foi repetida pela POC.
A autorização desta FIX não inclui inferência, sessão real ou alteração de credenciais.

O perfil novo **HOST_ASSISTED_WITH_GUI_NOT_SANDBOX** é independente. Executa pelo
SSH/terminal sem abrir janela, mas há GNOME/GDM ativos no host. Não prova execução
sem GNOME. Não amplia o boundary isolado/offline, não remove a guarda headless e
não concede autoridade agentiva. Não foi necessário gh auth status, token explícito,
proxy de credenciais, download ou instalação.

## 3. Alterações por arquivo

| Arquivo/grupo | Mudança |
| --- | --- |
| [diagnose_a9_gui_auth.py](../experiments/lr-10a-sdk-runtime/diagnose_a9_gui_auth.py) | Diagnóstico independente GUI-presente, manifesto/hash/ELF/versão explícitos, Locked booleano, ambiente allowlist, marker somente metadata, ownership/timeouts, stat de config e saída sanitizada. Sem send/session/claim. |
| [runtime-gui-candidate.json](../experiments/lr-10a-sdk-runtime/runtime-gui-candidate.json) | Pin independente da imagem nativa já instalada 1.0.95; não substitui o pin antigo. |
| [tests/test_a9_gui_auth.py](../experiments/lr-10a-sdk-runtime/tests/test_a9_gui_auth.py) | 11 testes sintéticos: contexto GUI/Locked, ambiente, hash/versão/flags, cleanup, headless intacto, marker, financeiros e falha de integridade. |
| [HOST-ASSISTED.md](../experiments/lr-10a-sdk-runtime/HOST-ASSISTED.md) | Adendo separado GUI-presente, reprodução condicionada à auditoria da configuração e limitações observadas. |
| [Documento permanente](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md) | Adendo factual desta FIX, sem alterar resultados históricos. |
| evidence/a9-fix-2-gui-sdk-metadata.json | Uma execução real de metadata, preservada integralmente. |
| evidence/a9-fix-2-contract-inventory.json | Identidades/proveniência, fatos do usuário e limites de causalidade. |
| evidence/a9-fix-2-as-run-regressions/* | Logs Rust/Python, ownership, permissões/gateway sintéticos da implementação executada. |
| evidence/a9-fix-2-final-regressions/* | Reteste da separação auth/integridade. |
| evidence/a9-fix-2-final-validation/* | Reteste final após restringir auto-start/autorização interativa do busctl e exigir flag no-auto-update. |
| evidence/a9-fix-2-verification.json | Hashes, preservação histórica, marker, contagens, sintaxe e revisão sanitizada. |
| Este arquivo | Substituição integral do relatório A9-FIX-1; versão anterior preservada no Git. |

Todos os caminhos de evidence pertencem a `experiments/lr-10a-sdk-runtime/`.
Nenhum arquivo Rust, Cargo.toml/lock, MSRV/Edition ou código de produção mudou.

## 4. Ambiente e identidade dos executáveis

[Inventário verificável](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-contract-inventory.json).
Fedora 44, Python 3.14.7, Rust/Cargo locais 1.98.1 já instalados; não se reinstalou
Rust 1.94 nem se atribuiu este reteste a essa versão. SDK pinado 1.0.17,
default-features=false, feature runtime; nenhuma atualização. A referência de
archive da crate é 1.0.93, não utilizada ou baixada.

| Identidade | Observação |
| --- | --- |
| CLI histórico nativo | v1.0.91/RPC1.0.90; SHA256 `be17b42705ca17490098d7b87f293300d72a094d125b6bb2b2557dc4a0a4f8a8`, preservado em run_a9_host.py. |
| CLI candidato atual | Native --version=1.0.95, RPC getStatus=1.0.95, protocolo3; SHA256 `9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`. |
| Caminho conhecido | `$HOME/.local/share/fnm/node-versions/v24.18.0/installation/lib/node_modules/@github/copilot/node_modules/@github/copilot-linux-x64/copilot`. Agora contém o novo hash. |
| Resolução atual do comando | Loader npm público via fnm; loader resolve o pacote nativo e repassa argumentos. Não se executou login ou o loader como substituto do SDK. |
| package.json loader/nativo | Declara 1.0.89; não equivale à versão efetiva do binário. |
| Instalação interativa do usuário | v1.0.95 informada pelo usuário; identidade do processo/PATH interativo exato não foi medida independentemente. |
| Contraste real 1.0.91 × 1.0.95 | NOT_RUN: nenhuma imagem antiga verificada no caminho conhecido. Sem busca em dados pessoais ou aquisição de outro runtime. |

A mudança de bytes ocorreu fora desta POC; autoria/momento não atribuídos.
Compatibility PASS observacional somente para startup, getStatus, auth.getStatus,
models.list, account.getQuota e shutdown com 1.0.95. Sessões, inferência e persistência
continuam NOT_RUN. Não se conclui que 1.0.91 seja incompatível ou que a atualização
corrigiu a autenticação.

A [release oficial 1.0.95](https://github.com/github/copilot-cli/releases/tag/v1.0.95)
foi consultada: suas notas não demonstram uma correção Linux/Keyring que explique
este resultado. A [documentação atual de autenticação SDK](https://github.com/github/copilot-sdk/blob/main/docs/auth/authenticate.md)
descreve resolução de autenticação armazenada; não é prova substituta do contrato
pinado. A chamada real com a versão identificada é a evidência deste gate.

## 5. Contexto de credenciais e comparação histórica

No probe, HOME/PATH/DBUS_SESSION_BUS_ADDRESS/XDG_RUNTIME_DIR estavam presentes;
XDG_CONFIG_HOME/DATA_HOME/CACHE_HOME, GH_CONFIG_DIR, COPILOT_HOME, DISPLAY e
WAYLAND_DISPLAY ausentes. Nomes conhecidos de tokens foram registrados somente
como booleans ausentes. Nenhum valor de token foi adquirido.

GNOME shell, GDM e keyring daemon estavam ativos; bus socket acessível, Secret
Service já owned, login collection Locked=false antes/depois. Somente a propriedade
Locked foi consultada; nenhum item, label, histórico ou GetSecret. A versão final
usa busctl auto-start=no e allow-interactive-authorization=no, com help local
confirmando essas opções. Esses flags finais foram retestados por fixtures, não
por nova consulta real após a falha de configuração.

| Variante/fonte | Contexto | Auth SDK | Limite |
| --- | --- | --- | --- |
| FIX-2 histórica | CLI1.0.91, wrapper/montagens protegidos diferentes | true histórico | Fingerprint ambiental exato não disponível; não reproduzido. |
| A9-HOST inicial | CLI1.0.91, allowlist reduzida | false histórico | Não era o mesmo contexto da FIX-2. |
| A9-FIX-1 | Headless; baseline/PATH validado/remoção individual de bus/runtime | false nos quatro probes históricos | Não reclassificados como retestes atuais. |
| Controle humano externo | GNOME físico, keyring desbloqueado, CLI interativo1.0.95 | Não medido pelo SDK nesse ato | CLI_INTERACTIVE_AUTHENTICATED=USER_REPORTED. |
| Esta FIX, identidade | HOME descartável, keytar desabilitado, --version/--help | NOT_RUN | Dois comandos de identidade, sem autenticação. |
| Esta FIX, SDK GUI-presente | HOME normal/bus/runtime, PATH=/usr/bin; CLI pin candidato1.0.95 | **true real** | Um Client, sem sessão; configuração mudou. |
| SDK sem GNOME atual | Não executado | NOT_PROVEN | GNOME não foi parado para comparação. |

ClientOptions e binário Rust de preflight não mudaram: CliProgram::Path explícito,
Stdio, use_logged_in_user=true, modo CopilotCli padrão, base_directory ausente,
COPILOT_HOME removido, sem github_token, LogLevel::None, cwd privado, built-in MCPs
negados e log-dir privado. O runtime usa resolução normal de credenciais do host.
Não se passou DISPLAY/token nem ambiente completo. PATH reduzido **foi suficiente
nesta execução**, mas isso não prova ausência de outras dependências do host.

GUI, desbloqueio/login e imagem nativa mudaram juntos. A recuperação é observada;
a dimensão causal individual é INCONCLUSIVE. Não se afirma GNOME obrigatório,
conta previamente deslogada ou correção causal do CLI1.0.95.

## 6. Metadata real e admissão financeira

[JSON original](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-gui-sdk-metadata.json),
coletado em **2026-10-10T03:34:13.229631Z** (10/10 00:34:13, Fortaleza).
`preflight.auth.authenticated=true` foi retornado pelo SDK real; identidade pessoal
omitida. `models.list` retornou **somente auto**, com multiplier=null e sem token
pricing, capabilities ou policy publicados no snapshot. Nenhum modelo selecionado.

| account.getQuota | premium_interactions | chat/completions (cada) |
| --- | --- | --- |
| entitlementRequests | 200 | 0 |
| usedRequests | 52 | 0 |
| remainingPercentage | 74.2 | 100 |
| isUnlimitedEntitlement | false | true |
| overage | 0.0 | 0.0 |
| overageAllowedWithExhaustedQuota | false | false |
| usageAllowedWithExhaustedQuota | false | false |
| Unidade exposta | requests_as_reported_by_runtime | requests_as_reported_by_runtime |

São respostas de **nova consulta** desta FIX, não transplante da quota histórica
200/52. A coincidência não prova frescor/idade do snapshot no servidor; só o horário
da coleta foi registrado. Os números expostos não reconciliam por si só preço do
modelo, unidades faturáveis, custo máximo ou franquia aplicável a uma inferência.
Unlimited é flag reportada para categorias específicas, não inferência de uso
ilimitado de qualquer modelo. Overage=false não prova ausência de todos os caminhos
pagos/fallbacks. Não se consultou saldo/fatura nem se mudou plano.

**FINANCIAL_ADMISSION=BLOCKED** por:
`auto_cost_unknown`, `billing_units_and_maximum_cost_unverified`,
`no_paid_fallback_enforcement_unverified`, `private_authenticated_session_state_unverified`.
Autenticação não desbloqueia envio. Uma chamada SDK não garante uma única requisição
interna/faturável. **Zero inferências enviadas pela POC**, sem afirmação inventada
de zero cobrança externa ou saldo da conta. Nenhuma autorização futura foi consumida.

## 7. Falha de preservação da configuração

Após identidade/help, stat permanecia igual. Após metadata SDK, config.json mudou:

| Metadata observável | Antes | Depois |
| --- | --- | --- |
| inode | 3768587 | 3768738 |
| bytes | 470 | 470 |
| mtime_ns/ctime_ns | 1791602000869867688 | 1791603250606145370 |

**Preservação desses metadados: FAIL. Autor/conteúdo da mudança: INCONCLUSIVE.**
A janela coincide com o probe; isso não atribui exclusivamente a escrita ao CLI
em vez de atividade concorrente do usuário. Nenhum conteúdo/hashes de config foi
lido, nenhum backup de credenciais foi feito e nenhuma restauração automática
ocorreu. Não houve outra consulta real ao runtime após a detecção. A descoberta
impede declarar verificação integral aprovada.

O source executado classificava auth e integridade juntos: o JSON original mantém
`sdk_authenticated_with_gui=BLOCKED`, `configuration_or_context_changed`, mas contém
authenticated=true dentro de sdk_metadata.sdk_report.preflight.auth. Não adulteramos
esse artefato. O código final separa `sdk_authenticated_with_gui` (observação) de
`verification_state` (bloqueado por integridade); o teste dessa distinção é sintético.
Não se declara que o source final tenha sido retestado com credenciais reais.

## 8. Verificação e comandos executados

Comandos de Git: status/branch/rev-parse/remotes/ls-remote, diff/check, hash-object
comparado à base, commit e push somente da branch. Sem operação sobre main.
Comandos públicos/status: resolução do loader e ELF/hash; ps -C com saída descartada;
NameHasOwner do bus daemon; get-property Locked; busctl --help local; --version e
--help do CLI com HOME descartável e stderr descartado. Sem gh auth/status bruto,
strace, environ de processos, secret inspection, debug de auth ou logs RPC brutos.

Probe real (executado uma vez; **não repetir automaticamente**):

```text
python3 experiments/lr-10a-sdk-runtime/diagnose_a9_gui_auth.py <native-pinado-1.0.95> --output experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-gui-sdk-metadata.json
```

Exit1 por configuração/contexto alterado. O preflight Rust também sai1 por design
financeiro BLOCKED_PRE_SEND; nenhum desses exits significa cleanup incompleto.
Timeouts do wrapper: identidade15s, metadata65s; nenhum timeout real ocorreu.

Regressões, executadas em três momentos com paths novos:

```sh
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-as-run-regressions
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-final-regressions
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-final-validation
python3 -m py_compile experiments/lr-10a-sdk-runtime/diagnose_a9_gui_auth.py experiments/lr-10a-sdk-runtime/tests/test_a9_gui_auth.py
```

O runner imutável usa `/usr/bin/cargo test --offline --locked -- --test-threads=1`,
COPILOT_SKIP_CLI_DOWNLOAD=1, CARGO_BUILD_JOBS=2, RUSTC/RUSTDOC locais explícitos;
Python unittest discover. Não inicia CLI real nesses testes. Saídas Rust/Python
persistidas são sintéticas, não stdout de credenciais.

| Etapa | Rust | Python | Resultado |
| --- | --- | --- | --- |
| Source do probe real | 48 | 67 | Todos PASS |
| Separação auth/integridade | 48 | 68 | Todos PASS |
| Source final, incluindo busctl/flags | 48 | 68 | Todos PASS |

[Logs finais Rust](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-final-validation/a9-host-rust-tests.txt),
[logs finais Python](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-final-validation/a9-host-python-tests.txt),
[ownership](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-final-validation/a9-host-owned-tests.json)
e [verificação](../experiments/lr-10a-sdk-runtime/evidence/a9-fix-2-verification.json).
São os 48 Rust e 57 Python anteriores, mais 11 testes GUI Python; cada resultado
individual consta nos logs. Incluem regressões FIX1–4/A9-FIX1: pidfd/ECHILD,
descendentes não amostrados/setsid, timeout, processo externo, worker failure
inconclusivo, guard/crash/race, negação de permissões e gateway/boundary sintéticos.

Sintaxe AST/py_compile PASS; revisão de whitespace/diff PASS. JSON/JSONL novos
parseados; padrões de tokens/private keys/Bearer não encontrados e revisão dos
campos sanitizados efetuada. Busca de padrões não é garantia absoluta de detectar
qualquer segredo. Logs txt novos tiveram somente linhas vazias finais normalizadas.
Preservação histórica por comparação de Git blobs: todo evidence anterior intacto;
dos arquivos antigos do experimento só HOST-ASSISTED.md recebeu adendo. Código e
contratos anteriores, SDK/Cargo pins e harness intactos.

Tauri/UI/testes sem GNOME: NOT_RUN. Não houve mudança de produção ou código Rust;
a suite direcionada da POC é pertinente, sem repetir os 1.107 testes Tauri nem
alegar UI testada. Não houve download/build redundante de runtime.

## 9. Matriz de gates e resultados

| Gate/verificação | Estado | Evidência/limite |
| --- | --- | --- |
| CLI_INTERACTIVE_AUTHENTICATED | INCONCLUSIVE para reprodução automática | Controle positivo informado pelo usuário; não repetido pela POC. |
| Identidade/pin candidato e SDK | PASS | Hashes independentes, versão native/RPC1.0.95; SDK1.0.17 intacto. |
| Compatibilidade metadata SDK1.0.17/CLI1.0.95 | PASS | Startup, auth, catálogo, quota, shutdown reais; não sessões/inferência. |
| Contraste com imagem antiga1.0.91 | NOT_RUN | Imagem antiga não verificada no caminho conhecido; nenhuma obtenção. |
| SDK_AUTHENTICATED_WITH_GUI | PASS observacional real | auth=true no JSON original; verificação geral bloqueada por configuração. |
| SDK_AUTHENTICATED_HEADLESS | NOT_RUN / NOT_PROVEN | GNOME presente; gate antigo preservado e teste de rejeição GUI PASS sintético. |
| Catálogo/quota atuais | PASS observacional real | Auto somente, preços ausentes; quota de nova chamada, unidade reportada sem reconciliação de custo. |
| FINANCIAL_ADMISSION | BLOCKED | Quatro bloqueios explícitos; nenhuma inferência liberada. |
| Zero send/sessão real/ferramenta agentiva | PASS | Binary preflight-only sem session/send; nenhuma operação real desse tipo. DenyAll/zero tools testados sinteticamente. |
| Marker intacto/não reclamado | PASS | Ausente antes/depois e verificação final; diretório existente seguro, somente metadata ancorada. |
| Configuração preservada | FAIL | inode/timestamps mudaram; autor/conteúdo INCONCLUSIVE; sem restore. |
| Cleanup real e sintético | PASS no escopo observado | ECHILD, identidades ausentes, survivors vazios; processo externo preservado em fixture. |
| Timeouts/versões incompatíveis/contexto/flags desconhecidos | PASS em fixture | Bloqueio sem fallback; não simula prova de compatibilidade com serviço real antigo. |
| Logs/artifacts/histórico | PASS nas verificações executadas | Campos sanitizados, parse/hashes/revisão; não prova detecção universal de segredos. |
| Serviços e credenciais manipulados pela POC | NOT_RUN (ações proibidas) | Nenhum start/stop/login/logout/unlock/export; serviços observados constantes no probe. |
| A9_REAL_INFERENCE e persistência genuína | NOT_RUN | Zero mensagens e sessões reais; marker não consumido. |
| A9_ISOLATED auth/rede/supervisor | BLOCKED | FIX3/4 mantidas; este perfil não é sandbox e não resolve esses gates. |

## 10. Processos e recursos

Um Client/CLI de metadata e dois comandos de identidade. Todos controlados pelo
harness FIX1 intacto, sem killpg ou sinal por PID numérico. Identidades PID/start-time
registradas com ownership; kernel child exhaustion é independente da amostragem.
No probe real, ECHILD e nenhum sobrevivente atribuído, nenhum erro de ownership,
zero sinais de recuperação, system-process count315 antes/depois.

| Medição real | Identidade/help | Metadata SDK |
| --- | --- | --- |
| Wall ms | 1604.18 | 2573.70 |
| Cleanup harness ms | 31.82 | 36.21 |
| Pico RSS árvore bytes | 276848640 | 255492096 |
| Pico processos owned | 3 | 2 |
| CPU amostrada s, limite inferior | 1.80 | 0.98 |

SDK reportou start569ms, stop810ms, shutdown=graceful. O harness reporta
cleanup_status=graceful_no_recovery; seu campo genérico sdk_shutdown_verified=false
não interpreta esse report. Não se transformou recuperação forçada em shutdown
SDK. `headless=true` genérico do harness significa invocação sem UI; não ausência
GNOME no host. Amostragem50ms, cache do SO quente, uma amostra por fase; RSS árvore
não é RAM incremental do sistema e CPU amostrada pode subestimar. Não são benchmarks
estatísticos. Nenhum processo externo foi sinalizado para verificar essas métricas.

Morte inesperada do worker, descendentes adversariais/subreapers, namespaces não
cobertos e processos ininterruptíveis permanecem limites. ECHILD observado não é
prova de contenção host contra runtime malicioso. Não se implementou supervisor
LR-10B/produção nem sandbox LR-10C.

## 11. Segurança, pendências e próximos passos proporcionais

Host-assisted executa sob o usuário e pode acessar recursos do host; não se promete
segredo protegido contra exfiltração nem controle de destinos de rede. Secret
Service desbloqueado permite resolução normal pelo aplicativo, não exportação.
A POC não leu configs de auth, tokens, cookies, keyring contents ou sessões pessoais.
MCP interativo conectado pelo usuário não é autorização MCP no experimento:
--disable-builtin-mcps permaneceu; não houve sessão/ferramenta real. A cobertura
integral de plugins/extensões do host não foi provada por metadata.

Próximos passos recomendados para decisão da Luna/usuário, **não executados**:

1. Auditar a mudança de stat e definir prevenção/atribuição proporcional sem leitura
   de segredos ou restauração automática. Não repetir probes autenticados para
   forçar PASS; verificar a estratégia de proteção antes de novo ensaio autorizado.
2. Se necessário, autorizar comparação controlada de versões com imagens previamente
   verificadas e regressões; não reinterpretar resultados antigos nem atualizar pin
   global/SDK silenciosamente. GUI atual não comprova disponibilidade sem GNOME.
3. Tratar custo/modelo/unidades/paid fallback e estado privado como gate financeiro
   próprio. Auto sem preço, quota e flags de overage não bastam para admissão.
4. Só após revisão dos gates materiais decidir uma execução real separada; nenhum
   sender foi acrescentado nesta FIX. A autorização prévia de uma tentativa não
   permite retry, cobrança adicional ou mudança de credenciais.

**FIX_AND_RETEST** é a recomendação desta execução apesar da recuperação observada
com GUI. Não declarar READY_FOR_A9 nem READY_FOR_NEXT_FINANCIAL_GATE aprovado.
Até 17/10/2026, esta evidência reduz a incerteza de metadata/auth, mas a integridade
pendente e os contratos financeiros impedem assumir integração pronta para Narys
0.1. Não estimamos prazo/custo sem medição nem adiamos apenas pelo calendário;
a priorização de release permanece decisão humana após auditoria.

**A9-FIX-2 IMPLEMENTAÇÃO CANDIDATA — aguardando auditoria independente da Luna.**
