# NARYS — LR-10A FIX-2: Session Persistence & SDK/CLI Compatibility

## 1. Identificação

- Projeto: Narys; fase LR-10A; execução exclusiva da FIX-2, em 2026-10-09 (America/Fortaleza).
- Branch: `lr-10a-sdk-runtime-feasibility`.
- Base verificada, inicialmente limpa e igual ao remoto: `21de782797a46cb81ef66229b07c4e65634a40d2`.
- `main` local e remota: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`, sem alterações.
- Implementação testada: o commit de implementação imediatamente anterior ao commit documental que finaliza este relatório; sua referência literal será registrada na finalização documental.
- HEAD documental final/remoto: [HEAD da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility). O histórico identifica o SHA do próprio relatório sem autorreferência impossível.
- Estado: **LR-10A FIX-2 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE**.
- Gate de retomada de histórico real: **BLOCKED_REAL / AWAITING_HUMAN_APPROVAL**. Nenhum PASS definitivo da FIX-2, LR-10A ou LR-10.

Este arquivo substitui somente o relatório reutilizável anterior. [Relatório da FIX-1 no histórico](https://github.com/Samuel-Francisco-AS/Narys/blob/21de782797a46cb81ef66229b07c4e65634a40d2/docs/LR-10-LATEST-EXECUTION-REPORT.md) e suas evidências continuam preservados.

## 2. Objetivo e escopo

Investigar `session_not_found` em sessões vazias, separar criação em memória de persistência em disco e caracterizar o par SDK/CLI efetivamente instalado. A correção modifica o contrato e a instrumentação da POC; não fabrica durabilidade para obter PASS.

Restrições respeitadas: nenhuma inferência, prompt, ferramenta do Copilot, atualização global, login/logout, credencial extraída, inspeção de sessão pessoal, escrita em configuração global, sudo, mudança em produção, alteração do Broker, UI, IPC ou autoridade agentiva. Sem YOLO/Autopilot, PR, merge, rebase, reset ou force-push. Não implementados FIX-3, A9, LR-10B ou um supervisor/sandbox de produção.

## 3. Causa investigada e evidências

A causa imediata do erro neste fluxo está comprovada: **a sessão vazia criada pelo SDK não possui transcript persistido reconhecido pelo CLI após detach**. O teste anterior pressupunha que `session.create`/`disconnect` garantiam esse transcript.

Nas duas execuções finais, quatro variantes (UUID explícito, UUID gerado pelo SDK, abort vazio e store desabilitado) apresentam:

1. Criação e detach reconhecidos; ID preservado e basename de workspace correspondente ao ID.
2. Um evento `session.start` em memória, zero mensagens de usuário/modelo.
3. Diretório privado com `workspace.yaml` de 202 bytes, **sem `events.jsonl`**, metadata persistida ausente.
4. Resume retorna `session_not_found`, tanto no mesmo Client quanto depois de encerrar e reiniciar efetivamente o CLI, preservando o diretório.
5. Abort e `enable_session_store` ligado/desligado não mudam essa observação.

Controles causais usam somente arquivos criados pela própria fixture em diretórios descartáveis:

- Um `events.jsonl` sintético com **apenas `session.start`**, sem mensagens ao modelo, é retomado pelo **SDK real + mesmo CLI local**, com o mesmo ID, metadata presente e bytes inalterados. Deletar esse ID torna a retomada `session_not_found`.
- Um arquivo sintético corrompido falha com RPC **-32603**, permanece byte a byte inalterado e não é recuperado automaticamente. Metadata pode estar presente mesmo nesse caso: sua presença isolada não prova transcript válido.

Portanto, ID gerado, mismatch do par SDK/CLI ou ausência de mensagem ao modelo não são explicações suficientes para toda retomada: o mesmo par retoma o transcript sintético sem inferência. **Isso não demonstra que o SDK persistiu histórico real**, nem justifica escrever transcripts artificialmente como correção. A causa interna de o CLI não gravar uma sessão vazia, o instante do primeiro flush e possível influência adicional da proteção read-only não foram demonstrados no código privado do CLI. Não afirmamos universalmente que toda sessão vazia exige inferência para resume.

Evidências definitivas: [estado com autenticação existente](../experiments/lr-10a-sdk-runtime/evidence/fix-2-real-existing-auth-final.json) e [COPILOT_HOME isolado](../experiments/lr-10a-sdk-runtime/evidence/fix-2-real-isolated-final.json). Ambas identificam hashes dos fontes, lockfile, executável e CLI usados.

### Contrato da versão publicada

A autoridade para Rust é a **crate 1.0.17 publicada**, checksum `c66d1375ffce624174ffab84f2781225ecd2c5c696ab412bcfabe01794cb0e97`, não uma suposição sobre tags ou documentação recente. Seu `.cargo_vcs_info.json` registra árvore dirty; os hashes dos arquivos publicados constam em [upstream/versionamento](../experiments/lr-10a-sdk-runtime/evidence/fix-2-upstream.json).

| Contrato | Fonte versionada e interpretação utilizada |
| --- | --- |
| ID omitido | [session.rs 1.0.17](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/session.rs), linhas 1697–1712: SDK Rust gera UUID localmente antes do RPC. ID explícito é encaminhado; retorno diferente produz erro tipado. IDs explícitos UUID funcionaram no CLI observado. |
| Store | [README 1.0.17](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/README.md), seção Infinite sessions: integração de busca/recuperação entre sessões; não é API de flush do transcript. |
| Estado e diretórios | [lib.rs 1.0.17](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/lib.rs), ClientOptions/build_command: `base_directory` define `COPILOT_HOME`. `cwd` define contexto; não é isolamento de filesystem. CLI usa session-state por ID. |
| Disconnect | [session.rs](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/session.rs), implementação a partir da linha 1275: `session.detach`, encerramento do roteamento local; não há flush explícito nem delete. Preservar um histórico existente não implica criar histórico ausente. |
| Abort | [session.rs](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/session.rs), `abort`: RPC para interromper turno; não é delete nem prova de conclusão/persistência. Abort vazio foi executado. |
| Delete | [lib.rs](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/lib.rs), linha 2884: `session.delete`. Usado somente para IDs próprios. |
| Stop | [lib.rs](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/lib.rs), a partir da linha 3054: detach de sessões registradas, shutdown do runtime/transporte e espera do filho. Não equivale a flush garantido de sessão vazia. Harness verifica árvore independentemente. |
| Resume e falhas | SDK envia `session.resume`; não tem operação separada para memória versus disco. Runtime decide. O SDK converte texto upstream contendo `Session not found` em SessionErrorKind::NotFound; a POC exporta somente tipo seguro e, quando mantido pelo SDK, código RPC numérico. |

A [documentação oficial atual de persistência](https://github.com/github/copilot-sdk/blob/main/docs/features/session-persistence.md) demonstra históricos após envio de mensagens, distingue sessão ativa de histórico em disco e descreve recuperação de transcript. **Não garante durabilidade de uma sessão vazia na crate utilizada.** Sua orientação sobre IDs gerados não substitui a implementação UUID do Rust 1.0.17. Também há divergência no README publicado: exemplo cita `InfiniteSessionConfig.workspace_path`, mas [types.rs 1.0.17](https://docs.rs/crate/github-copilot-sdk/1.0.17/source/src/types.rs), linhas 963–977, não oferece esse campo. A POC não adotou esse exemplo incompatível.

Nenhuma exigência universal de mensagem/transcript mínimo foi comprovada. Um evento start sintético bastou ao leitor de disco local. Para testar persistência **genuinamente produzida em uma conversa**, os exemplos oficiais dependem de send; esse caminho continua proibido e vinculado à autorização separada de A9.

## 4. Comparação SDK/CLI e decisões de versão

[Metadados oficiais e comparação de fontes](../experiments/lr-10a-sdk-runtime/evidence/fix-2-upstream.json) consultados em 2026-10-09; [instalação efetiva](../experiments/lr-10a-sdk-runtime/evidence/fix-2-installed-versions.json).

| Componente | Versão/observação |
| --- | --- |
| SDK mantido | `github-copilot-sdk = 1.0.17`, crates.io, default-features=false, runtime, MSRV 1.94.0 |
| Runtime de referência da crate 1.0.17 | `cli-version.txt`: **1.0.93**; isso não é pin imposto ao CLI explícito já instalado |
| CLI executado | `--version`: **1.0.91**; SHA256 `be17b42705ca17490098d7b87f293300d72a094d125b6bb2b2557dc4a0a4f8a8`, 178457408 bytes; mesmo hash histórico |
| Informação RPC do mesmo binário | `status.get.version`: **1.0.90**, protocolo 3 |
| Manifests npm locais | Loader e pacote nativo: **1.0.89**; discrepância registrada, não resolvida nem substituída por uma versão presumida |
| SDK 1.0.18 já em cache | Referência CLI 1.0.94; lib.rs idêntico a 1.0.17. session.rs contém mudanças em encerramento de subscriptions/tool handlers; não demonstram correção de flush vazio. Não executado outro par real. |
| Estáveis atuais consultadas | SDK **1.0.19** e CLI **1.0.95**; SDK release descreve atualização do snapshot CLI, sem correção de persistência vazia comprovada |
| Ferramentas efetivas | Rust/Cargo **1.94.0**, crate POC Edition 2021; Python 3.14.7, Fedora 44, kernel 7.2.8-200.fc44.x86_64; execução SSH/headless |

Handshake protocolo 3, lifecycle vazio, leitura de transcript sintético e shutdown funcionam nesse par. Isso caracteriza compatibilidade **desses caminhos**, não inferência, durabilidade completa nem suporte formal universal entre versões.

Não houve atualização do SDK/CLI, bundle, preview ou download de runtime. Arquivos oficiais linux-x64 seriam aproximadamente 112,3 MB (CLI 1.0.93), 114,4 MB (1.0.94) e 114,5 MB (1.0.95), comprimidos. Sem uma correção confirmada que exija outro runtime, não há justificativa para esse download. A matriz com esses pares permanece **NOT_RUN**. O MSRV/Edition da aplicação Narys não mudou.

## 5. Implementação realizada

Arquivos alterados/criados exclusivamente para FIX-2:

- [src/persistence.rs](../experiments/lr-10a-sdk-runtime/src/persistence.rs): matriz de observações, IDs UUID próprios, namespace de estado explícito, factory de ClientOptions para cada CLI novo, metadata de um ID próprio, verificação de arquivos por stat e controles sintéticos identificados como fixtures.
- [src/main.rs](../experiments/lr-10a-sdk-runtime/src/main.rs): modos de sessões encaminham à matriz protegida; remove fluxo antigo inacessível; gate de histórico real mantém exit 1. Metadata existente mantém proteção read-only e remove COPILOT_HOME ambiente ao usar auth existente.
- [src/lib.rs](../experiments/lr-10a-sdk-runtime/src/lib.rs): exporta módulo experimental e corrige comentário de enable_session_store; lifecycle anterior permanece para regressões.
- [Cargo.toml](../experiments/lr-10a-sdk-runtime/Cargo.toml) e [Cargo.lock](../experiments/lr-10a-sdk-runtime/Cargo.lock): UUID 1.27.0 torna-se dependência direta, default-features=false, v4; já existia transitivamente. Lockfile só acrescenta UUID à lista da POC; nenhuma versão de pacote mudou.
- [fixtures/persistence_cli.py](../experiments/lr-10a-sdk-runtime/fixtures/persistence_cli.py): protocolo determinístico separado da fixture antiga, storage persistido/vazio/falho, metadata indisponível, ID divergente, detach falho/tardio e corrupção moderna/legada. RPC inesperado registra violação; nenhum acesso a conta ou rede.
- [tests/persistence.rs](../experiments/lr-10a-sdk-runtime/tests/persistence.rs): dez testes, incluindo verificação independente de PID/start_ticks de todas as instâncias de fixture e ausência de RPC proibido.
- [run_fix2.py](../experiments/lr-10a-sdk-runtime/run_fix2.py): reprodução via harness FIX-1 inalterado, hashes em streaming, stat de config e JSON sanitizado.
- [README](../experiments/lr-10a-sdk-runtime/README.md), adendo ao [documento permanente](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md), este relatório e novas evidências `evidence/fix-2-*`.

Sem retries, sleeps para forçar persistência, recriação silenciosa ou tratamento de sessão recriada como retomada. `allow_transcript_recovery=false`; ID divergente, detach falho, storage inseguro e erro de metadata permanecem distintos. Factory é necessária porque ClientOptions 1.0.17 não é Clone. O guard exige que a origem do overlay esteja dentro do estado privado; stat recusa traversal e symlinks e nunca segue um workspace_path arbitrário retornado pelo CLI.

## 6. Matriz S1–S9

**PASS fixture** valida lógica/protocolo simulado. **SDK/CLI real** abaixo usa o binário local, sem inferência; transcripts sintéticos continuam identificados separadamente. FAIL observado não foi convertido em PASS de persistência.

| Caso | Fixture determinística | SDK real / CLI local | Limitação/gate |
| --- | --- | --- | --- |
| S1 ID explícito | PASS: ID UUID preservado, resume próprio | Create/detach PASS; resume vazio FAIL `session_not_found`, sem transcript | Durabilidade real BLOCKED_REAL |
| S2 ID gerado | PASS: UUID SDK, correlação preservada | Mesmo resultado de S1 | ID gerado não explica a falha sozinho |
| S3 reinício Client | PASS: persistido retoma; namespace novo não | Novo Client inicia novo CLI; vazio continua ausente/NotFound | Histórico genuíno BLOCKED_REAL |
| S4 reinício CLI | PASS: processos anteriores reclamados, fixture disk retoma | Encerramento real e restart executados; vazio NotFound. Novo CLI lê transcript sintético criado antes dele | Não comprova histórico SDK/provider |
| S5 estado preservado/novo | PASS: controle positivo preservado versus NotFound em novo estado | Vazio NotFound nos dois; store privado preservado até o teste de delete | Nenhum diretório pessoal recriado/limpo |
| S6 abort vazio | PASS: ack não é sucesso/persistência | Abort ack; continua sem events.jsonl e sem resume | Não houve operação de modelo a cancelar |
| S7 inexistente/excluída | PASS: negativos tipados, sem fallback | UUID nunca criado NotFound; delete próprio ack/NotFound. Controle sintético previamente resumível também vira NotFound após delete | Delete vazio sozinho não comprovaria destruição de histórico existente |
| S8 versões | PASS da caracterização estática; pin/protocolo registrados | Par instalado executado com resultados acima | Outros pares oficiais NOT_RUN; nenhuma atualização justificada |
| S9 falhas | PASS: storage/metadata indisponíveis, mismatch, detach tardio/falho, unsafe paths, corrupção -32075 e -32603 | Sintético válido retoma; corrompido falha -32603, sem rewrite; stat da configuração estável | Storage inacessível e detach tardio no CLI real NOT_RUN; não se arriscou estado pessoal |

## 7. Testes, comandos e evidências

Comandos executados a partir da raiz; variáveis aplicadas somente à invocação, sem instalação global:

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 \
RUSTC=/tmp/narys-lr10a-rust-1.94.0/bin/rustc \
RUSTDOC=/tmp/narys-lr10a-rust-1.94.0/bin/rustdoc \
/tmp/narys-lr10a-rust-1.94.0/bin/cargo test --offline --locked \
--manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml -- --test-threads=2

python3 experiments/lr-10a-sdk-runtime/tests/test_measure.py \
--evidence experiments/lr-10a-sdk-runtime/evidence/fix-2-python-regression.json

python3 experiments/lr-10a-sdk-runtime/run_fix2.py sessions-existing-auth \
/home/sam/.local/share/fnm/node-versions/v24.18.0/installation/lib/node_modules/@github/copilot/node_modules/@github/copilot-linux-x64/copilot \
--output experiments/lr-10a-sdk-runtime/evidence/fix-2-real-existing-auth-final.json

python3 experiments/lr-10a-sdk-runtime/run_fix2.py sessions \
/home/sam/.local/share/fnm/node-versions/v24.18.0/installation/lib/node_modules/@github/copilot/node_modules/@github/copilot-linux-x64/copilot \
--output experiments/lr-10a-sdk-runtime/evidence/fix-2-real-isolated-final.json
```

| Verificação | Resultado/evidência |
| --- | --- |
| Rust POC | **22/22 PASS**: 10 novos + 12 anteriores; zero doctests existentes. [Log completo por teste](../experiments/lr-10a-sdk-runtime/evidence/fix-2-rust-tests.txt). Timeout, handshake cancelado, shutdown e descendente continuam cobertos. |
| Python FIX-1 | **25/25 PASS**, 36 identidades de fixtures/controle verificadas no fim, zero sobreviventes. [JSON](../experiments/lr-10a-sdk-runtime/evidence/fix-2-python-regression.json), [log por teste](../experiments/lr-10a-sdk-runtime/evidence/fix-2-python-regression.txt). Nenhum CLI real chamado nessa suíte. |
| CLI local protegido | Dois probes finais executados. Exit **1 intencional**, porque real_history_resume_gate=BLOCKED_REAL; cleanup completo nos dois. Auth existente authenticated; home isolado authentication_required, que não representa estado da conta do usuário. |
| Sintaxe/formatação | py_compile dos dois Python novos e dos dois arquivos FIX-1; rustfmt --check de todos os Rust da POC; git diff --check, aprovados. Formatter host 1.9.0-stable, exclusivamente formatação; compilador/testes 1.94.0. |
| Consistência e escopo | [Verificações sanitizadas](../experiments/lr-10a-sdk-runtime/evidence/fix-2-verification.json): hashes, JSONs, branch/main, preservação dos arquivos FIX-1/históricos e ausência das identidades conhecidas. |

O writer Python histórico hardcodeia os rótulos FIX-1/comando antigo. Só os rótulos da **nova** evidência foram corrigidos após a execução, mantendo testes/observações originais; a alteração é identificada no JSON. O hash README naquela evidência corresponde ao instante do teste, anterior ao adendo documental; measure.py e test_measure.py permanecem idênticos.

Etapas intermediárias não representam o reteste final:

- [Baseline reproduzido](../experiments/lr-10a-sdk-runtime/evidence/fix-2-baseline-real-existing-auth.json) usou o executável original após a primeira tentativa de compilação falhar; rotulado explicitamente assim.
- [Matriz inicial](../experiments/lr-10a-sdk-runtime/evidence/fix-2-real-existing-auth-initial-matrix.json): IDs explícitos com prefixo não UUID falharam no create. UUID explícito resolveu **essa rejeição**, sem resolver a ausência de transcript. Não generalizamos que toda versão do CLI aceita somente UUID.
- [Diagnóstico UUID anterior à finalização](../experiments/lr-10a-sdk-runtime/evidence/fix-2-real-existing-auth-uuid-diagnostic.json) registrou o controle sintético, antes de adicionar código RPC numérico/delete negativo final.
- Compilações iniciais falharam por import de Session, tentativa de Clone de ClientOptions e API env dos testes; a primeira execução da fixture falhou por assert de nome de permission callback. Corrigidos de acordo com a API efetiva e retestados integralmente; não declarados PASS retrospectivo.
- Cargo foi executado offline sem --locked durante a inclusão/ajuste da dependência UUID já cacheada; o reteste final foi **offline + locked**, sem mudanças de versões nem runtime download.

Não repetida a suíte Tauri de 1.107 testes: diff restrito ao crate independente experimental, fixtures, runner e documentação. Nenhum fonte/manifest/lockfile de produção mudou. Recompilar toda a aplicação não validaria o contrato de persistência do CLI e consumiria recursos sem cobertura adicional pertinente.

### Recursos e processos (amostras únicas, sem benchmark prolongado)

| Probe final | Matriz completa | CPU amostrada, limite inferior | RSS agregado de pico | Cleanup harness | Processos sistema antes/depois |
| --- | --- | --- | --- | --- | --- |
| Auth existente | 19835,03 ms | 8,13 s | 286232576 bytes | 35,32 ms | 333 / 333 |
| Home isolado | 7202,20 ms | 7,93 s | 315076608 bytes | 28,18 ms | 334 / 334 |

Esses tempos são da **matriz inteira**, não latência individual de inicialização. RSS soma processos e pode duplicar páginas compartilhadas; CPU ignora atividade entre amostras de 50 ms. Contagens globais são contextuais e incluem processos externos/concomitantes; não são prova de ownership. Prova de recuperação: worker subreaper privado, kernel_children_exhausted=true, cleanup_complete=true, nenhum erro de atribuição/sinal de recuperação/sobrevivente. SDK reportou seis shutdowns graceful por matriz; harness manteve sdk_shutdown_verified=false, sem confundir responsabilidade do SDK com sua própria prova.

## 8. Segurança, credenciais e cleanup

Host montado read-only por bwrap, somente workspace/state privados writable. Auth existente usa o resolvedor do CLI, com session-state pessoal **ocultado por overlay vazio privado**, sem listar/copiar/ler seu conteúdo. No modo isolado, COPILOT_HOME aponta ao estado temporário; no existente, variável ambiente é removida para não desviar o store esperado. Variáveis de tokens/path SDK são removidas sem extrair valores.

Nenhum config.json lido em conteúdo; inode/tamanho/mtime/ctime coincidem antes/depois dos probes, além da proteção de escrita do mount. Apenas transcripts **sintéticos próprios** são lidos para verificar bytes; artifacts de sessões próprias criadas pelo SDK são inspecionados por stat. Não há dumps de environ/cmdline, RPC bruto, credenciais, event payloads reais ou mensagens upstream nas evidências.

FIX-1 preservada byte a byte em measure.py/test_measure.py e evidências históricas. Todas as fixtures novas verificam PID+start_ticks e ausência de RPC proibido; antigas mantêm testes de timeout, cancelamento, filhos e kernel adoption. Probes reais finais: nenhum recovery signal, nenhum processo órfão conhecido. Não se enviou sinal a processo externo.

ExecutionAuthority/HumanLocal, planner Codex read-only, AgentRegistry, IPC release, OperationalTraceBus, TaskGraph/Scheduler/LR-8.5 e ausência de shell WebView permanecem preservados por ausência de diff em produção. Não houve configuração global alterada ou serviço persistente criado.

**Inferências executadas: 0. Consumo de quota por inferência da POC: 0 esperado**, coerente com ausência de send/tool/prompt e zero mensagens nos timelines vazios. Não foi feita uma auditoria remota de cobrança nem inventada uma medição diferencial de quota. Metadata/auth e leitura local de transcript não demonstram capacidade de inferência.

A proteção bwrap continua experimental: host legível, dispositivos/rede disponíveis e extensões ambientes não provadas isoladas. Isso não concede um especialista em produção nem substitui a fronteira da LR-10C. Limitações do ownership worker da FIX-1 continuam aplicáveis.

## 9. Pendências e riscos residuais

- **BLOCKED_REAL**: persistência e retomada de histórico realmente produzido por SDK/provider, incluindo envio inicial e inferência após resume. Exige autorização separada de A9; nenhum comando de envio foi implementado/executado.
- Não comprovado o instante do primeiro flush de transcript vazio nem se sua ausência é política universal, bug do runtime instalado ou interação adicional com filesystem protegido. Não existe flush público identificado no SDK inspecionado que force durabilidade vazia sem mensagem.
- Discrepância --version/status.get/npm manifests mantida como risco de provenance. Binário/hash fixos e protocolo observado estão documentados.
- Pares SDK/CLI alternativos NOT_RUN; nenhuma correção confirmada de release justifica download de runtime nesta FIX.
- Storage verdadeiramente indisponível, eventos de operações de modelo e disconnect tardio do CLI real permanecem mockados ou não executados. Testes negativos locais não comprovam comportamento de serviço real.
- -32075 é um contrato documentado atualmente e simulado; CLI local devolveu -32603. Não usar mensagem genérica/RPC unknown para diagnosticar auth/quota ou corrupção automaticamente.

## 10. Recomendação técnica e conclusão

**BLOCKED_REAL para o gate de persistência operacional completa.** A investigação estabeleceu a ausência do transcript no fluxo vazio e eliminou a necessidade de inventar incompatibilidade SDK/CLI como causa exclusiva. A POC agora expressa esse contrato observado, possui controles positivos/negativos reproduzíveis e passou as regressões permitidas.

Recomendação: auditar a implementação candidata e a atribuição limitada dessa causa imediata. Manter bloqueado o aceite de persistência de conversa real; decidir separadamente a autorização de A9 e/ou um reteste futuro com runtime oficial compatível quando houver justificativa verificável. **Não avançar automaticamente para FIX-3 ou LR-10B.** Nenhum workaround de criação sintética de histórico foi integrado ao fluxo de sessões do SDK.

**LR-10A FIX-2 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE.**
