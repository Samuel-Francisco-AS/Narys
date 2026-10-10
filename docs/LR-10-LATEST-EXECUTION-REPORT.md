# Narys — servidor headless e validação integrada única do Copilot

**Entrega operacional candidata, aguardando auditoria independente da Luna.**

**Resultado: cold boot headless, Core, desbloqueio manual, Stronghold existente e autenticação SDK comprovados. A única tarefa integrada falhou no início da sessão, antes de qualquer send. Integração Copilot operacional completa NÃO comprovada; LR-10A NÃO recebe PASS definitivo.**

## Identificação e Git

- Fedora 44, execução em 10/10/2026 via SSH/tmux, após o único reboot realizado pelo usuário.
- Branch exclusiva `lr-10a-sdk-runtime-feasibility`; HEAD inicial local/remoto limpo `928f58c2915ebdf9a1885440760a65185404d8b3`.
- Implementação usada no ensaio: [`821bfd3e2cb6b09606ba0c5a88906f55f83399e3`](https://github.com/Samuel-Francisco-AS/Narys/commit/821bfd3e2cb6b09606ba0c5a88906f55f83399e3), sobre Core de [`284a31f`](https://github.com/Samuel-Francisco-AS/Narys/commit/284a31f00c7e5db3bdb728a83bc452ad641270b0).
- Commit técnico desta execução, testado/publicado: [`8bd5b625740f3f614b135aadf420786548f4986e`](https://github.com/Samuel-Francisco-AS/Narys/commit/8bd5b625740f3f614b135aadf420786548f4986e). Diagnóstico acrescentado **depois** do ensaio; sem reteste real.
- HEAD documental e publicação verificáveis no [histórico da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility), sem SHA autorreferencial.
- `main` local/remota preservada em `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`. Sem PR, merge, rebase, reset ou force-push. Evidências H1/H2/H3 e FIXes anteriores preservadas.

## Escopo e autorização

O usuário autorizou servidor sem GNOME, credenciais existentes desbloqueadas manualmente em SSH privado, Core residente e Copilot sob demanda. Confirmou orçamento adicional desativado e autorizou UMA chamada da franquia, Auto se elegível, aceitando explicitamente a diferença requests/AI Credits. Após reboot reafirmou a validação integrada única, sem tentativas adicionais automáticas.

Não foram autorizados pagamento, overage, compra, alteração de credenciais, YOLO, ferramentas perigosas, shell agentivo, push pelo especialista ou acesso irrestrito ao filesystem por ferramentas. HOST_ASSISTED continua **sem sandbox**. Não se enfraqueceu o modo isolado para obter sucesso.

## Implementação consolidada

[Narys Core](../narys-core/README.md) é um crate Edition2021 independente de Tauri/GTK/WebKit/X11/Wayland. Unidade systemd de usuário, socket Unix0600/SO_PEERCRED mesmo UID, sem listener TCP. Keyring está em unidade separada; Core ativo mesmo bloqueado e Copilot somente sob demanda. Reutiliza AgentBackend/Registry, PlanV1/TaskGraph, TaskId, OperationalTraceBus limitado e migrações SQLite em banco próprio. A factory gráfica continua Codex read-only. Não concede ExecutionAuthority/HumanLocal ao Copilot, nem altera Broker, Scheduler/LR-8.5, IPC release, UI ou runtime3D.

A composição mínima oferece especialista textual com **zero ferramentas** e resultado esperado independente; não oferece shell, edição de projetos, todos os provedores da GUI ou aplicativo Android. Tarefas e resultados sobrevivem a restart; tarefa interrompida/failed não é reenviada automaticamente.

Mudanças nesta continuação:

| Arquivo | Mudança |
|---|---|
| [worker.rs](../narys-core/src/worker.rs) | Fase SDK e código RPC numérico no erro de início, sem texto/payload; nenhuma mudança de permissões, versões, deadline ou retry |
| [sdk_peer.py](../narys-core/tests/sdk_peer.py) | Fixtures negativas de create e patch posterior, sem provedor/rede/credenciais |
| [postboot-single-submission.json](../narys-core/evidence/postboot-single-submission.json) | Contexto/metadata real, submissão única, falha, cleanup, inventário privado somente de metadados e persistência |
| [sdk-session-contract.json](../narys-core/evidence/sdk-session-contract.json) | Fonte efetivamente publicada1.0.17, checksum e contrato inspected |
| [logs Rust](../narys-core/evidence/rust-postboot.txt), [Python](../narys-core/evidence/python-postboot.txt) | Testes posteriores, separados dos históricos |
| [README](../narys-core/README.md), [evidências permanentes](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md) | Estado real e limitação operacional |
| Este relatório | Consolidação final desta entrega |

## Cold boot e credenciais: observações reais

[Evidência única pós-boot](../narys-core/evidence/postboot-single-submission.json): boot12:37:57-03; Core iniciou automaticamente12:38:17-03 (~20s de boot), sem restart automático. `multi-user.target`, linger=yes, GDM inactive e sem ExecMainStartTimestamp nesta inicialização, nenhum GNOME Shell, graphical-session.target inactive, sessões do usuário somente systemd-user/SSH tty. SSH/Codex/tmux continuaram acessíveis após reconexão. Não se simulou headless removendo DISPLAY.

Keyring PID1395 no serviço do user manager; coleção login preexistente e desbloqueada. O usuário informou LOGIN_UNLOCKED após digitar senha em SSH separado, e o Core confirmou o estado. Nenhuma senha foi solicitada/capturada pela IA. Helper H2/H3 pinado ao GNOME Keyring50.0, interface interna não suportada, transporte DH/AES; não cria coleção, copia armazenamento ou troca senha. Não há unlock automático/plaintext.

`narys-core stronghold` abriu o snapshot/client pessoal existente com a chave resolvida normalmente pelo credential store. Sem create_client, nova chave, save, migração, chmod ou retorno de valores. Metadados de login.keyring e luna-lr3.stronghold permaneceram iguais entre pré-boot e final; isso é observação de metadados, não equivalência criptográfica.

## Runtime, modelo e gate financeiro

SDK `github-copilot-sdk=1.0.17`, default-features=false/runtime, CLI nativo1.0.95/RPC3. SHA do CLI `9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`, validado em cada worker. Sem downloads/updates. Binário Core do ensaio: `bdf7b1939606d8de6bbbeb132ee3d956ccc860640b4a63d31e747e7c8f9ffba4`.

Duas invocações SDK reais após o reboot: (1) preflight metadata-only pela Narys; (2) execução da única task, com novo preflight e início de sessão. **Nenhuma terceira invocação/retry após a falha.** Ambas retornaram auth=true, catálogo somente Auto e quota por account.getQuota: premium entitlement200, usedRequests52, remainingPercentage74.2, overage0; flags overageAllowedWithExhaustedQuota e usageAllowedWithExhaustedQuota false. A quota é resposta RPC recebida agora, em **requests informadas pelo runtime**; não se comprova atualização do saldo no servidor/ausência de cache nem equivalência com AI Credits.

O receipt privado0600/O_EXCL foi criado a partir da confirmação humana já recebida e reafirmação pós-boot, vinculado à task1/objective SHA, uma chamada, zero USD adicional autorizado, expiração30min. Não se automatizou o input do helper humano nem derivou aprovação de auth=true. Modelo/quota/flags foram verificados novamente pelo worker. Nenhuma garantia financeira desconhecida foi convertida em PASS. O limite0,5 AI Credits de sessão é **soft**, não teto financeiro garantido; uma operação SDK pode envolver várias requisições internas.

A seleção DNS `RES_OPTIONS=no-aaaa` permanece somente no runtime, após contraste público sem autenticação pré-boot: padrão expirou, IPv4 respondeu e catálogo funcionou. Não houve mudança global de resolver/firewall, proxy ou desativação TLS. Não é controle de destinos; opção diagnóstica glibc com limitação DNSSEC pela aplicação.

## Única tarefa: falha antes do envio

Task1 foi preparada em `/tmp/narys-task-zs0Hbj/workspace`0700, objetivo exato autorizado: soma alpha2+beta3, somente resposta5, sem ferramentas/comandos/arquivos/outras solicitações. Uma chamada `narys-core submit 1` foi aceita como running.

Resultado final: **failed / backend_failed**. Report do SDK: **rpc_error_unknown**, preflight positivo, shutdown graceful, sdk_send_calls=0; nenhuma Session foi retornada por PreparedSession.start(). Não houve resposta, arquivo result.txt, eventos de resposta ou conclusão do TaskGraph. Trace registrou started/failed/failed, correspondendo às camadas backend/Core. Estado failed persistiu após restart somente do Core; nenhum segundo submit foi executado.

Contrato estático da [crate publicada1.0.17](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/session.rs): `PreparedSession.start()` inclui `session.create` e `finish_session_setup`, com `session.options.update` para skipCustomInstructions. Erro nesse patch dispara tentativa interna de disconnect e também impede retornar Session. Portanto **método exato e causa-raiz = INCONCLUSIVE**; não se atribui a versão, Keyring, billing ou configuração privada sem prova. É possível criação transitória seguida de erro no patch. O diretório privado session-state ficou vazio; isso não prova ausência absoluta de estado transitório interno.

A implementação executada descartava o código RPC numérico e não registrava subfase. Não se pode recuperá-lo da evidência sanitizada existente. A melhoria posterior conserva phase=session_start/código numérico sem mensagens, testada em create_error e options_error sintéticos. **-32602 nesses testes NÃO é o código real observado.** A crate declara VCS dirty=true; o SHA de repositório não é presumido equivalente ao pacote publicado, cuja identidade/checksum foram registrados.

## Matriz de aceite

| Gate | Estado | Evidência/limite |
|---|---|---|
| Cold-start sem GUI / Core automático | PASS_REAL | Contexto boot/systemd/ausência GUI |
| Unlock manual / D-Bus / Keyring existente | PASS_REAL | Informação humana + estado Locked confirmado, mesmo UID/user manager |
| Stronghold existente sem migração | PASS_REAL | Abertura status-only e metadados estáveis |
| H1 — Auth e entitlement reportado | PASS_REAL metadata | Auth=true/quota RPC; não prova billing atual inteiro |
| H2 — Modelo e admissão financeira | Condicional humana, não exercida | Auto observado, flags false, orçamento desativado confirmado pelo usuário; custo máximo não medido |
| H3 — Workspace, DenyAll, zero tools | PASS_CONFIG/FIXTURE | Privado0700, availableTools=[], MCP/extensions/hooks/skills/discovery negados; HOST_ASSISTED não sandbox |
| H4 — Single-attempt | PASS_FIXTURE + observação real | Uma submissão, zero sends, guard de envio ausente; estado failed persistido bloqueia reenvio do mesmo ID |
| H5 — Inferência real pelo SDK | BLOCKED_PRE_SEND | Falha no início de sessão, sem send |
| H6 — Resposta correta / arquivo / TaskGraph concluído | NOT_RUN | Nenhum5/arquivo de resultado; estado failed |
| H7 — Conversa persistida/resume genuíno | NOT_RUN | Não houve inferência nem Session retornada; SQLite failed persistido é prova diferente |
| H8 — Consumo/quota pós-operação | INCONCLUSIVE | Quota antes recebida; nenhum usage/consulta quota_after alcançado |
| H9 — Shutdown e ownership | PASS_REAL | SDK graceful + harness ECHILD/cleanup completo, zero sobreviventes atribuídos |
| H10 — Headless, segredos e produção | PASS escopo observado | Core/Keyring/SSH ativos, zero ferramentas/prompts, controles anteriores preservados |
| Sessões / ações agentivas / A9_ISOLATED | BLOCKED | Não herdam aprovação de autenticação ou cold boot |

**REAL_INFERENCE=NOT_RUN; zero SDK send/prompt; zero retry automático.** Guard send-attempt.json não foi criado; marker histórico lr10a-a9-host-attempt.json segue ausente/intacto. A autorização de uma inferência não foi exercida, mas não autoriza repetir automaticamente esta validação. Não se afirma zero cobrança externa medida nem saldo pós-operação; nenhum dado do provedor sobre cobrança foi recebido.

## Processos, configuração e recursos

Ensaio task: wall13737.51ms, CPU amostrada>=12.2s, peak tree RSS283672576 bytes, 3 processos observados, cleanup29.68ms, zero recovery signals/survivors, ECHILD=true, sem timeout. `exit_code=0` do worker NÃO foi considerado sucesso: Core avaliou state/error/shutdown/cleanup e marcou failed. SDK.shutdown=graceful é fonte distinta de measure.py.sdk_shutdown_verified=false (harness não atesta o SDK).

Config pessoal observado somente por metadados: estrutura/UID/permissões/nlink adequados antes/depois; inode3785475→3785482, tamanho470 mantido, timestamps mudaram. CONFIG_DRIFT_OBSERVED=true; WRITER_ATTRIBUTION=INCONCLUSIVE; conteúdo/legitimidade não verificados. Não houve restauração, lock, chmod ou leitura de conteúdo pela POC. Drift não é a causa atribuída do erro nem invalida automaticamente auth metadata.

O diagnóstico posterior foi instalado atualizando somente narys-core.service, sem reiniciar Keyring/GDM/SSH. Artefato final `354b33c13b1a8080c6f78ed6d09d28baeccaafac52c24baaa4b2e36849a74159`, RSS idle14972KiB; não foi retestado contra o provedor. MemoryCurrent inclui page cache de binário/CLI e não equivale ao RSS. Core segue ativo, Keyring PID inalterado, GDM inativo, nenhum Copilot remanescente observado.

Harness FIX1 intacto: subreaper privado, PID/start-time/pidfds e ECHILD; nenhum processo externo sinalizado. Limites de morte do supervisor, descendentes adversariais, processos do mesmo UID e host/network não isolados permanecem. Cgroup da unidade contém Core/workers e exclui Keyring. Timeout catálogo30s, send120s, harness240s, parada systemd270s. Sem supervisor de produção novo.

## Testes e comandos

Comandos históricos executados; **não repetir a submissão nem os probes reais desta validação**.

```sh
~/.local/lib/narys/narys-core status
~/.local/lib/narys/narys-core stronghold
~/.local/lib/narys/narys-core copilot
# Prepare objetivo autorizado e receipt privado; exatamente uma submissão:
~/.local/lib/narys/narys-core submit 1
~/.local/lib/narys/narys-core result 1
~/.local/lib/narys/narys-core events

COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 \
RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc \
CARGO_TARGET_DIR="$PWD/src-tauri/target" \
/usr/bin/cargo test --offline --locked --manifest-path narys-core/Cargo.toml
python3 -m unittest discover -s narys-core/tests -p 'test_*.py'
python3 -m py_compile narys-core/ops/*.py narys-core/tests/*.py
systemd-analyze --user verify ~/.config/systemd/user/narys-core.service
```

[28 unitários Rust + 1 integração Unix](../narys-core/evidence/rust-postboot.txt), [5 Python](../narys-core/evidence/python-postboot.txt) PASS após diagnóstico. Fixtures provam create/patch failure sem send/guard, código numérico sem segredo, shutdown/reap; send/timeout/cancel sintéticos também passaram. Rustfmt direcionado, sintaxe, JSON/invariantes e diff check PASS. Compiler Fedora1.98.1; SDK/CLI/lock/MSRV/Edition não atualizados.

[Preparação](../narys-core/evidence/preboot.json) preserva 26 Rust+1 integração, 5 Python, 25 ownership e 117 agentes Tauri PASS/2ignored. Não foram reapresentados como retestes pós-boot. Não se repetiu toda a suíte Tauri nem testes de UI: nesta continuação mudou só diagnóstico do worker/fixture e documentos, sem produção.

## Operação disponível e conclusão

Comandos SSH disponíveis: status, credentials, unlock humano privado, stronghold status-only, copilot metadata, prepare/submit/cancel/result/events. **A execução de tarefas Copilot não está operacionalmente aprovada**: task1 falhou e não deve ser resubmetida. Narys continua residente e resultados/erros persistem, com Copilot ausente quando idle.

Recomendação: **HEADLESS_MANUAL_UNLOCK_PASS_REAL; COPILOT_TASK_INTEGRATION=BLOCKED_BEFORE_SEND; FIX_AND_RETEST limitado à inicialização da sessão.** Próximo passo mínimo, somente após auditoria/autorização: um ensaio zero-send que registre código RPC/fase e delimite create versus patch, corrigindo o contrato sem remover DenyAll, private state, skip instructions ou controles financeiros. Não repetir inferência nem criar nova trilha H/FIX automaticamente.

Para a 0.1 de17/10, GNOME Shell não foi necessário para o fluxo de credenciais validado, e o Core headless foi entregue. O bloqueio concreto restante é o início da sessão do especialista, não o boot/Keyring. Menor funcionamento disponível agora é Core/Stronghold/administração/metadata, **não tarefas Copilot**. Não se afirma que os outros provedores da GUI já funcionam neste Core. Não integrar em produção ou avançar a LR-10B por iniciativa própria.

**LR-10A — ENTREGA OPERACIONAL CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE. Fechamento técnico parcial: headless comprovado, validação integrada bloqueada; sem PASS definitivo da LR-10A.**
