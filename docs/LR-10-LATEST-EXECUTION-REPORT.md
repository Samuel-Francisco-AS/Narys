# Narys — Core headless + Copilot SpecialistAgent

**Estado: implementação operacional preparada; aceite final pendente do único reboot humano e da tarefa real. Auditoria independente pendente.** Nenhuma nova trilha H/FIX foi criada.

## Identificação

- Execução: 10/10/2026, Fedora 44, SSH/tmux, prioridade operacional determinada pelo usuário.
- Branch exclusiva: `lr-10a-sdk-runtime-feasibility`. Base local/remota verificada: `7a336649e3a359463cecaad4232d6ccb4084c4ce`.
- Implementação testada/publicada: [`284a31f00c7e5db3bdb728a83bc452ad641270b0`](https://github.com/Samuel-Francisco-AS/Narys/commit/284a31f00c7e5db3bdb728a83bc452ad641270b0).
- HEAD documental verificável no [histórico da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility). Este documento será consolidado com a validação pós-boot; não contém SHA autorreferencial.
- `main` local/remota preservada: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`. Sem PR, merge, rebase, reset ou force-push. H1/H2/H3 e experimentos históricos preservados.

## Implementação e arquivos

[Narys Core](../narys-core/README.md) é um crate Edition 2021 independente de Tauri, com processo residente de uma thread Tokio e controle por Unix socket 0600, SO_PEERCRED do mesmo UID, sem porta TCP. Unidade [narys-core.service](../narys-core/ops/narys-core.service) habilitada e ativa; Copilot inicia somente por solicitação explícita. Restart do Core recupera resultados SQLite e marca tarefas interrompidas sem reenviá-las.

- [server.rs](../narys-core/src/server.rs): administração SSH, registry real do especialista, TaskGraph, trace limitado, persistência, cancelamento e verificação independente do resultado/arquivo.
- [worker.rs](../narys-core/src/worker.rs): SDK autenticado, preflight, sessão privada, único send guarded, eventos sanitizados, abort/disconnect/shutdown.
- [policy.rs](../narys-core/src/policy.rs): quota/modelo, admissão humana separada, receipt privado por tarefa, expiração 30 minutos e zero pagamento adicional autorizado.
- [vault.rs](../narys-core/src/vault.rs): abre snapshot/client existentes com a chave resolvida normalmente no credential store; não cria key/client, salva, migra ou retorna segredos.
- [ops](../narys-core/ops/): instalação conservadora, atualização somente com hashes revisados, status de credenciais, harness FIX1 reutilizado, revisão financeira humana e coleta segura.
- [testes](../narys-core/tests/): socket/persistência/restart reais com HOME sintético, peer JSON-RPC sintético, bloqueios e políticas.
- `src-tauri/src/agents/{registry_storage,plan_contract}.rs`, `cognition/scheduler_usage.rs`, `luna/task_id.rs`: extrações dos contratos puros. Os arquivos originais reexportam os mesmos tipos/comportamentos. `SchedulerUsage.providers_used` continua `Vec<String>` (o alias ProviderId já era String). A factory gráfica continua somente Codex.
- Cargo/lock próprios, README, evidências e adendo permanente LR-10A. Cargo.toml/lock/MSRV/Edition da aplicação original não foram alterados.

O especialista oferece **tarefas textuais com zero ferramentas**; não recebe HumanLocal/ExecutionAuthority. Execution Broker, regras LR-8.5, Scheduler, IPC release, UI e runtime 3D permanecem inalterados. Não há engine de approvals de produção nem execução agentiva de shell/edição. O Core grava apenas seu resultado em workspace explicitamente preparado. Este Core mínimo não substitui todos os provedores e fluxos cognitivos da GUI.

## Configuração persistente e credenciais

Foi instalado [drop-in mínimo](../narys-core/ops/keyring-headless.conf) no user manager, sem reiniciar/substituir o daemon existente. Keyring service/socket e Core habilitados. `Linger=yes` e `multi-user.target` já estavam configurados no host quando esta entrega começou; a implementação não ativou linger nem mudou boot. GDM permanece ativo nesta preparação.

Desbloqueio: comando `narys-core unlock` executa o helper H2/H3 em terminal humano privado. Ele exige daemon GNOME Keyring 50.0 pinado, mesmo UID/user manager, coleção login existente, ausência real de GUI e transporte DH/AES. Interface GNOME interna/não suportada: pin/condições inválidos bloqueiam. Não solicita senha em chat, argumento, env ou logs; não cria coleção. Senha somente no SSH privado, sem eco. Python não oferece garantia de apagamento físico de memória.

O Stronghold **pessoal existente foi aberto com sucesso nesta preparação**, status-only, sem save/migração/chave substituta. Metadados do snapshot original e de login.keyring estão na [evidência](../narys-core/evidence/preboot.json). Não se leu config.json pessoal, enumeraram itens pessoais ou copiaram/exportaram tokens. A chave do Stronghold foi resolvida internamente como operação normal da aplicação e mantida fora da resposta.

## SDK, rede e preflight real

SDK exato `github-copilot-sdk=1.0.17`, runtime não bundled. CLI nativo 1.0.95, protocolo RPC 3, SHA-256 `9cf62455c0fef57658c976b737f57ddc4b87c2f513a17864846f2d0e16a18a99`, validado por invocação. Sem download/update/login/logout. Construção offline com rustc/cargo Fedora 1.98.1; MSRV declarado Core 1.94.0. Cargo metadata não encontrou pacote com MSRV declarado superior a 1.94; não foi executado compilador 1.94 nesta entrega.

Primeiro preflight integrado: auth=true/quota recebida, catálogo expirou em 15s, shutdown/cleanup completos. Diagnóstico público sem autenticação: endpoint Copilot com conexão padrão expirou; IPv4 e `RES_OPTIONS=no-aaaa` retornaram HTTP404 com TLS normal. Aplicado somente ao ambiente do CLI, com timeout catálogo 30s e sem retry pela Narys. Segundo preflight integrado: **auth=true, catálogo real somente Auto e quota recebida**.

A falha é compatível com o [relato upstream de IPv6/Node SEA](https://github.com/github/copilot-cli/issues/2361), mas essa issue trata outra versão/OS e não comprova por si a causa no Fedora. A prova local é o contraste observado e a consulta posterior bem-sucedida. `no-aaaa` é uma opção diagnóstica glibc, afeta DNS/NSS desse processo, não protege destinos nem credenciais; é incompatível com DNSSEC feito pela aplicação. Não se alteraram resolver/firewall/rede global, nem desabilitou TLS. Consulte também o manpage instalado `resolv.conf(5)`.

Quota recebida por `account.getQuota`: premium entitlement 200, usedRequests 52, remainingPercentage 74.2, overage 0; ambas as flags `overageAllowedWithExhaustedQuota`/`usageAllowedWithExhaustedQuota` false. **Unidades: requests informadas pelo runtime**, não AI Credits. A resposta RPC foi recebida agora; não se comprova a atualização do saldo no servidor nem ausência de cache. Não se reutilizou apenas o histórico H3/FIX2.

## Autorização financeira e tentativa

O usuário confirmou explicitamente orçamento adicional desativado e autorizou **uma única chamada da franquia**, Auto somente se elegível, apesar da diferença de unidades e custo máximo em AI Credits não comprovado pelo SDK pinado. Nenhum pagamento adicional, fallback pago ou retry foi autorizado. Essa decisão será associada ao objetivo exato e task ID no receipt privado antes do envio após boot.

Autenticação não aprova gastos: quota disponível, catálogo real Auto e flags atuais false continuam obrigatórios. Sessão recebe limite 0,5 AI Credits **soft**, não um teto financeiro garantido. [Limites oficiais](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/session-limits) e [billing atual](https://docs.github.com/en/copilot/concepts/billing-and-usage/individuals/billing) distinguem esses conceitos. Uma operação SDK não garante uma única requisição/unidade interna faturável.

Guard `send-attempt.json`: O_EXCL0600, fsync do arquivo/diretório antes do único send; timeout/crash não liberam reenvio. Receipt exclusivo de uma tarefa; alterações do objetivo invalidam admissão. Não há retry, resumo automático, compaction infinita ou fallback de modelo. Marker histórico A9 permanece ausente/intacto, sem reutilização das reservas antigas. **Até esta preparação: zero inferências e zero sessões reais novas.** Não se afirma zero cobrança externa medida do conjunto da conta.

## Testes e comandos executados

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 \
RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc \
CARGO_TARGET_DIR="$PWD/src-tauri/target" \
/usr/bin/cargo test --offline --locked --manifest-path narys-core/Cargo.toml

# Regressão da composição original afetada:
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 \
RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc \
/usr/bin/cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib agents::

python3 -m unittest discover -s narys-core/tests -p 'test_*.py'
python3 -m unittest discover -s experiments/lr-10a-sdk-runtime/tests -p test_measure.py
python3 -m py_compile narys-core/ops/*.py narys-core/tests/*.py
systemd-analyze --user verify ~/.config/systemd/user/narys-core.service
```

| Verificação | Resultado | Evidência |
|---|---|---|
| Core Rust: TaskGraph/trace/vault/policy/SDK fixture | 26 PASS | [log](../narys-core/evidence/rust-core.txt) |
| Core real Unix/SQLite/restart em HOME sintético | 1 PASS | mesmo log; submit não autorizado bloqueado sem CLI/send |
| Core Python/unidades/segurança local | 5 PASS | [log](../narys-core/evidence/python-core.txt) |
| FIX1: ownership/subreaper/pidfd/external preservation | 25 PASS | [log](../narys-core/evidence/python-ownership.txt) |
| Aplicação: agentes/registry/planner/Codex | 117 PASS, 2 ignored | [log](../narys-core/evidence/rust-shared-agents.txt) |
| Rustfmt direcionado, sintaxe Python, JSON, diff check | PASS | comandos executados; sem alterações de formatação em módulos não afetados |
| Unidade systemd / dependências binárias | PASS | unidade instalada verificada; ldd sem GTK/WebKit/X11/Wayland |
| Sessão/send/cancel/timeout | PASS_FIXTURE | peer sintético; uma chamada, abort/detach, guard e PID reclamado |
| Sessão/send pelo provedor real | NOT_RUN | reservado à validação final após boot |

Falhas iniciais locais de fixtures (permissão temporária, wire `configDir`/resposta `success`) foram corrigidas; não foram usadas para afirmar sucesso real. Otimização scrypt igual à produção evita teste de snapshot sintético prolongado. Logs finais registram somente testes efetivamente aprovados. Não houve teste gráfico. A aplicação completa compilou para os 117 testes direcionados; não se repetiu a suíte inteira de 1107, porque Broker/IPC/UI/Scheduler não mudaram e os contratos extraídos têm regressão proporcional.

## Lifecycle, recursos e limites

[Preflight real](../narys-core/evidence/preboot.json): wall15.395s, peak tree RSS270757888 bytes, CPU amostrada >=12.62s, cleanup32.33ms, 3 processos observados, ECHILD=true, zero sinais de recuperação, zero sobreviventes atribuídos. `cleanup_status=graceful_no_recovery`; SDK report.shutdown=graceful. `measure.py.sdk_shutdown_verified=false` permanece por desenho: o harness não atesta o SDK; as duas fontes não devem ser confundidas. GUI ainda presente nesse probe.

Binário instalado final: SHA `1dc0a2c939b1ae38120cad05db292e9ce320465051f3ebb06bb5a35c65978ace`, 34711008 bytes (debug stripped). RSS idle final observado14716KiB/uma thread. MemoryCurrent systemd inclui page cache de binário/CLI, não equivale ao RSS; não apresentar como heap. O probe usou o artefato `eb63090...`; depois houve somente proteção adicional de zeroização de chave no caminho de erro e formatação de assertion sintética. Os hashes/source manifest distinguem as implementações.

Timeouts: catálogo30s, send120s, harness240s, parada systemd270s. Core TERM solicita cancelamento e aguarda o harness. FIX1 reutilizado intacto; sinais somente em processos atribuídos com identidade kernel/pidfds. Cgroup limita a unidade Core/workers; Keyring está separado. Morte inesperada do supervisor/descendentes adversariais continuam limites conhecidos; não há supervisor de produção nem proteção contra processos do mesmo UID. Logs de runtime/estado ficam privados, não versionados; journal somente labels fixos.

Config drift não implica corrupção nem escrita legítima: observação estrutural/metadados antes/depois, autoria INCONCLUSIVE. Não houve restauração/lock/chmod de config pessoal. O perfil continua HOST_ASSISTED_NOT_SANDBOX, com rede do host. O modo isolado e seus bloqueios auth/rede/supervisão não foram resolvidos por essa entrega.

## Aceite operacional e único reboot pendente

| Requisito | Estado atual |
|---|---|
| SSH/user manager/tmux preservados | PASS observado nesta preparação |
| Core systemd residente / Copilot sob demanda | PASS_REAL pré-boot |
| Keyring existente + desbloqueio sem GUI | PASS histórico H3 pós-login; cold boot NOT_TESTED |
| Stronghold pessoal existente aberto sem migração | PASS_REAL pré-boot; pós-boot NOT_TESTED |
| Auth SDK + catálogo/quota pela Narys | PASS_REAL pré-boot com GUI presente |
| Boot realmente sem GNOME | NOT_TESTED nesta entrega |
| Tarefa alpha2+beta3 pela Narys | NOT_RUN, única tentativa autorizada preservada |
| Resultado/arquivo/task graph/trace/shutdown dessa tarefa | NOT_RUN |
| Integração operacional concluída | **NÃO declarada** |

Checklist curto, **somente no SSH privado do usuário**:

```sh
# Uma reinicialização humana; nunca pedir senha administrativa no chat.
sudo systemctl reboot
# Após reconectar por SSH/Termux:
~/.local/lib/narys/narys-core status
~/.local/lib/narys/narys-core credentials
~/.local/lib/narys/narys-core unlock
```

Informar apenas `BOOT_HEADLESS / CORE_ATIVO / LOGIN_UNLOCKED` ou código sanitizado. Senha somente no terminal privado, não compartilhar saída com dados pessoais. Depois Codex confirma contexto/Stronghold, prepara o workspace autorizado, registra a autorização financeira já recebida e executa **uma** submissão pela Narys, sem retry. Resultado esperado5, arquivo relido, eventos/shutdown/ownership verificados. GDM não será iniciado nem boot alterado durante essa validação.

## Decisão e release

Implementação funcional preparada e publicada; aceite integrado pendente da ação humana acima. Não avançar à produção nem declarar LR-10A PASS definitivo. O Core mínimo atende a operação local de especialista textual e baixo custo idle; UI Android, acesso de escrita/shell e composição de todos os provedores da GUI ficam fora desta entrega. Para 17/10, concluir essa validação única oferece decisão operacional concreta sem novos ciclos investigativos. Auditoria independente continua pendente após o resultado consolidado.
