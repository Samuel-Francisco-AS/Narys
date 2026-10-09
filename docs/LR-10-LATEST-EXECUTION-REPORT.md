# NARYS — LR-10A FIX-4 — Auth & Network Boundary Feasibility

## 1. Identificação, branch, base e commits

- Data: 09/10/2026; Fedora 44, SSH/headless, sem interface gráfica.
- Branch: `lr-10a-sdk-runtime-feasibility`.
- Base local/remota inicial: `b74cc2eeccbd4d859632a1a4453380fd6cf20bcb`; workspace limpo.
- Main local/remota verificada: `6603a78bd34cfffbd019ced8fa870d9bea02a7fb`, preservada.
- Implementação testada: **IMPL_COMMIT_TO_BE_LINKED**.
- HEAD documental final/remoto: [histórico verificável da branch](https://github.com/Samuel-Francisco-AS/Narys/commits/lr-10a-sdk-runtime-feasibility). O próprio SHA documental não é inserido circularmente no arquivo.
- Estado: **LR-10A FIX-4 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE**.
- Decisão: **FIX_AND_RETEST**, com **BLOCKED_AUTH_BOUNDARY**, **BLOCKED_NETWORK_BOUNDARY** e A9 bloqueado. Sem PASS definitivo da LR-10A.

Sem merge, PR, rebase, reset, force-push, alteração humana descartada ou mudança na main. Este relatório substitui exclusivamente o relatório reutilizável da FIX-3; documentos/evidências permanentes continuam preservados.

## 2. Objetivo e alterações por arquivo

Investigar autenticação e conectividade controlada mantendo o sandbox offline e a supervisão aprovada. Encontrou-se na crate efetivamente utilizada um ponto oficial de interceptação HTTP/WebSocket; não foi necessário construir um proxy genérico.

| Arquivo | Mudança / finalidade |
| --- | --- |
| [src/auth_network.rs](../experiments/lr-10a-sdk-runtime/src/auth_network.rs) | Handler de uma operação sintética fixa, credencial sintética host-only, limite de uma tentativa, I/O bounded, ambos defaults de rede substituídos |
| [src/bin/network-fixture.rs](../experiments/lr-10a-sdk-runtime/src/bin/network-fixture.rs) | Peer RPC nativo sintético; pedidos de metadata, tentativas TCP pai/filho, flags sanitizados e negação de métodos desconhecidos |
| [fix4_boundary.py](../experiments/lr-10a-sdk-runtime/fix4_boundary.py) | Reutiliza o plano FIX-3; substitui apenas o ELF sintético aprovado com a mesma closure de bibliotecas; valida flags SDK estritamente |
| [src/bin/auth-network-probe.rs](../experiments/lr-10a-sdk-runtime/src/bin/auth-network-probe.rs) | Registro do handler e metadata no CLI real offline, sem credenciais/endpoint; somente modo metadata e harness obrigatório |
| [run_fix4.py](../experiments/lr-10a-sdk-runtime/run_fix4.py) | Runner owned/subreaper para Cargo/testes e probe real; stat de configuração, hashes, cleanup e diretório de artefatos separado para reprodução |
| [tests/auth_network.rs](../experiments/lr-10a-sdk-runtime/tests/auth_network.rs) / [tests/test_fix4.py](../experiments/lr-10a-sdk-runtime/tests/test_fix4.py) | 12 testes Rust / 4 Python novos, determinísticos e sem inferência |
| [Cargo.toml](../experiments/lr-10a-sdk-runtime/Cargo.toml) / [Cargo.lock](../experiments/lr-10a-sdk-runtime/Cargo.lock) | bytes=1.12.1 e futures-util=0.3.34 tornam-se deps diretas da POC para o DTO; já eram transitivas/cacheadas; nenhuma versão de package foi alterada |
| [README](../experiments/lr-10a-sdk-runtime/README.md), [adendo permanente](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md), este relatório | Reprodução, contrato, limites, impacto no release e fechamento da execução |
| [evidence/fix-4-*](../experiments/lr-10a-sdk-runtime/evidence/fix-4-verification.json) | Inspeção, testes positivos/negativos, medições e verificação sanitizada; evidências finais e falhas exploratórias selecionadas |

Não foram implementados auth loader real, proxy de credenciais, sandbox/approval engine de produção, supervisor LR-10B, integração ao Broker, A9 ou release. measure.py, boundary.py, run_fix2.py/run_fix3.py, src/lib.rs, src/main.rs, src/persistence.rs e testes/evidências históricos permanecem iguais à base. Nenhum código/dependência de produção foi alterado.

## 3. Evidências de autenticação — FIX-4A

**Resultado: BLOCKED_AUTH_BOUNDARY. Prova com credencial real: AWAITING_HUMAN_AUTHORIZATION.**

A autoridade versionada é o conteúdo efetivamente consumido da crate 1.0.17, com checksum do archive cacheado e hashes das fontes. `.cargo_vcs_info.json` marca dirty=true; não se assume identidade com uma tag/árvore Git upstream. Os [trechos de contrato](../experiments/lr-10a-sdk-runtime/evidence/fix-4-upstream-contract-excerpts.txt) permitem auditar no GitHub o ponto de injeção, redaction e defaults sem depender do terminal. [Inspeção completa de provenance/versões](../experiments/lr-10a-sdk-runtime/evidence/fix-4-contract-inspection.json).

| Hipótese / mecanismo | Evidência efetiva | Resultado / limite |
| --- | --- | --- |
| A1 Token explícito oficial | ClientOptions.github_token injeta COPILOT_SDK_AUTH_TOKEN e flags --auth-token-env / --no-auto-login; teste com SDK real e peer sintético | PASS transporte SDK; NÃO autenticação no CLI/provedor real. Marker alcança runtime e filho, sem aparecer como valor no argv |
| A2 Permissões exigidas | Documentação atual: PAT fine-grained de usuário, permissão de conta Copilot Requests; página de PAT consultada apresenta copilot_requests=write | Requisito documental atual; permissões/entitlement aceitos pelo CLI instalado não comprovados. Não confundir com repo contents ou gestão de seats |
| A3 Fine-grained PAT no par instalado | SDK repassa String; não valida localmente escopo/formato/entitlement. Documentação atual inclui github_pat_ e OAuth gho_/ghu_; classic PAT não é caminho atual suportado | Compatibilidade autenticada 1.0.17/CLI 1.0.91 permanece BLOCKED. Nenhum token/PAT criado ou fornecido |
| A4 keyring/libsecret/D-Bus | Empty/keytar off, sem bus/home/config e seccomp nega APIs de keyring; regressões kernel passam | Não se investigou armazenamento pessoal. Expor bus/serviço pode ampliar acesso a credenciais/serviços; não montado |
| A5 Mediação host | GitHubTokenProvider retorna access_token por RPC ao runtime; aquisição no host não equivale a contenção. Handler HTTP oficial permite injeção apenas no conector host sintético | PASS operação finita host-only; auth GitHub/Copilot completa não implementada/comprovada |
| A6 Escopo e duração limitados | PAT tem política/expiração documentada; provider SDK representa lifetime, mas não emite nem reduz escopo de um token | Nenhuma credencial criada/renovada. Projeto futuro precisa aprovação de emissão, duração, revogação e billing |
| A7 Vazamentos | Token direto herdado por filho sintético; Debug do campo github_token redigido; gateway não exporta segredo ao runtime/filho/log/JSON, inclusive quando servidor o ecoa | Redaction não protege memória/env/RPC. Debug de ClientOptions imprime env; erros de provider podem incluir prose. Não colocar credenciais em env/erro/debug da aplicação |

Fontes atuais consultadas em 09/10, separadas da prova versionada: [autenticação CLI](https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli), [auth SDK](https://github.com/github/copilot-sdk/blob/main/docs/auth/authenticate.md), [permissões e expiração de PAT](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens). Nenhum link de criação/login foi executado. Escopo reduzido limita danos; não impede copiar/transmitir a credencial nem estabelece orçamento.

**Condições distintas:** indisponível no sandbox (CLI real desta FIX); fornecida ao runtime (controle negativo sintético); mediada pelo host (operação local finita); protegida contra exfiltração em geral (NÃO demonstrada). O gateway só aceita marker público sintético e tem endereço de loopback fixado pelo host, sem função de adquirir credenciais reais. Mantém-se intacta a rejeição de token/env/flags de auth na fronteira CLI FIX-3.

A inspeção de substrings do ELF instalado foi inconclusiva para vários contratos; ausência de literal não prova ausência de feature. O próprio literal setProvider não foi encontrado, embora seu registro tenha funcionado via RPC real. Não se utilizou grep do binário como prova de suporte a PAT.

## 4. Alternativas de rede — FIX-4B

**Resultado para Copilot real: BLOCKED_NETWORK_BOUNDARY.** Caminho finito sintético por stdio demonstrado; transporte/autenticação do provedor não demonstrados.

| Alternativa | Análise / prova permitida | Decisão e custo |
| --- | --- | --- |
| B1 Proxy host de saída | HTTP CONNECT deixa payload TLS opaco e pode ser utilizado por runtime e filhos com acesso ao proxy; não distingue intenção/processo. A POC não o implementa | Proxy genérico não satisfaz este boundary; acrescentaria política, transporte para namespace e auditoria |
| B2 Gateway allowlist | Domínio/IP/SNI isoladamente não restringe dados enviados ao destino permitido. URLs, redirects, DNS rebinding, CDNs e endpoints de conta precisam validação independente | Promissor somente com operações/payloads restritos e TLS host; não configurado para provedores reais |
| B3 IPC estreito | SDK 1.0.17 já fornece handler model-layer HTTP/WebSocket, registrado por llmInference.setProvider; stdio owned já existe, sem socket host exposto | Escolhido para fixture. Override explícito dos DOIS métodos; não é proxy genérico nem autoridade agentiva |
| B4 Encaminhamento rootless | pasta está instalado; slirp4netns não foi localizado. User-mode networking pode encaminhar a rede de um namespace sem sudo, mas não separa runtime/filhos por si | NOT_RUN encaminhamento; nenhum helper ativado/serviço/firewall alterado. Nova stack/política não justificadas frente ao seam existente |
| B5 Controles oficiais | Fonte da crate documenta escopo model-layer CAPI/BYOK. Defaults fazem pass-through. CLI real aceitou registro do handler offline | PASS registro; cobertura de auth/telemetria/downloads/ferramentas e tráfego autenticado NÃO comprovada; offline continua mandatory |

[Documentação oficial de proxy/certificados](https://docs.github.com/en/copilot/concepts/security-governance-and-network-settings/network-settings) descreve conectividade, não isolamento por ferramenta. A [allowlist oficial geral](https://docs.github.com/en/copilot/reference/copilot-allowlist-reference) inclui domínios de API/auth/telemetria e serviços/CDNs; não foi adotada como lista mínima do CLI 1.0.91. [pasta/passt](https://passt.top/passt/about/) documenta a tradução rootless; a conclusão de que isso não autentica a intenção de um filho é análise arquitetural, não benchmark executado.

Arquitetura testada: root vazio/namespace offline FIX-3 → peer RPC sintético → SDK host/handler finito → listener HTTP de loopback do teste. O namespace não recebe proxy, socket host, CA store, resolv.conf, D-Bus ou share-net. Pai e filho falham ao conectar diretamente mesmo ao listener permitido. O destino proibido é outro listener REAL ativo; após parar/join e drenar accepts pendentes, zero conexões aceitas — não apenas ausência de header autenticado.

A URL lógica https://fixture.invalid/metadata é reconhecida como operação fixa e não é resolvida/encaminhada. O host usa TCP/HTTP local, injetando marker sintético. Sem DNS, TLS ou SNI nesta fixture; **HTTP local não serve como transporte de credenciais reais**. CONNECT, mudança de método, host/path/query/traversal/header/body e WebSocket são negados. Resposta limitada e validada byte a byte para DTO {value:5}; 302/401/echo/timeout/unavailable não causam retries/fallback. Orçamentos de socket de 250 ms e deadline de leitura; código bloqueante e parser finito são somente experimentais.

Para transporte real faltam: HTTPS host com validação de certificado/hostname/SNI, origens/path/protocolos mínimos verificados, DNS controlado/rebinding, redirects negados ou auditados, política explícita de CDN e dados/custo. Não se mede nem alega CPU/RAM de alternativas não implementadas. Não houve contato da POC com destinos externos do Copilot.

## 5. Testes T1–T10 e resultados

| ID | Classe | Evidência observada e alcance |
| --- | --- | --- |
| T1 | PASS fixture; auth real BLOCKED | Sem auth/gateway e auth inválida: erro fixo/502, sem operação externa; CLI real auth_required; handler indisponível impede startup; ACK negativo aceito por Client::start é bloqueado pela POC antes de metadata, sem fallback |
| T2 | PASS sintético | Host-only marker ausente no runtime/filho e artefatos; echo negado. Controle negativo de token explícito prova exposição deliberada ao runtime/filho, não design aceito |
| T3 | PASS kernel/regressão | Canários/roots pessoais/symlinks/env da FIX-3 continuam inacessíveis; novo plano conserva exatamente mounts/ambiente/seccomp/network, alterando ELF aprovado com libs idênticas |
| T4 | PASS kernel + SDK/fixture | TCP direto pai/filho bloqueado; sem gateway genérico/share-net; URLs alternativas/CONNECT/body/headers/WS negados antes de conector autorizado |
| T5 | PASS sintético; provedor BLOCKED | Operação finita retorna 200/DTO pelo host; marker recebido somente pelo listener permitido e não pelo runtime; nenhum catálogo real inferido da fixture |
| T6 | PASS sintético | Destino local proibido ativo aceita zero conexões, assim como listener permitido no caso rejeitado; fila kernel drenada/join para prova independente da scheduling race |
| T7 | PASS fixture | Gateway ausente/unavailable/timeout/401/302/echo, auth inválida e segunda operação negados; no retry/open fallback; capacidades desconhecidas impedem startup |
| T8 | PASS controlado; contenção adversarial BLOCKED | Subreaper/pidfd/ECHILD/recovery/external-control retestados; owned runtime/filhos reclamados; não se simulou morte real do worker com árvore adversarial |
| T9 | PASS protocolo; cobertura interna real BLOCKED | DenyAll/zero tools/MCP/extensões/hooks/skills/ambiente negados nos testes create/resume/update herdados; nenhuma ferramenta agentiva real disparada |
| T10 | PASS | 38 Rust = 26 herdados + 12 novos; 44 Python = 40 herdados + 4 novos; syntax/rustfmt/JSON/diff/preservação verificados |

[38 testes Rust](../experiments/lr-10a-sdk-runtime/evidence/fix-4-rust-tests.txt), [execução owned](../experiments/lr-10a-sdk-runtime/evidence/fix-4-rust-owned-run.json), [23 observações do gateway](../experiments/lr-10a-sdk-runtime/evidence/fix-4-gateway-observations.jsonl), [44 Python](../experiments/lr-10a-sdk-runtime/evidence/fix-4-python-final-ack.json), [wire de permissões retestado](../experiments/lr-10a-sdk-runtime/evidence/fix-4-permission-regression.jsonl), [consolidação](../experiments/lr-10a-sdk-runtime/evidence/fix-4-verification.json).

PASS identifica somente o teste e superfície indicados. Não converte mocks em PASS operacional.

## 6. Fixtures versus SDK/CLI real

- **SDK real 1.0.17 / peer sintético:** despacho HTTP/WebSocket pelo protocolo oficial, flags/env de token, redaction, registros/acks, decisões de gateway e cleanup. O peer retorna catálogo vazio por construção; esse dado não descreve modelos da conta.
- **Kernel real Bubblewrap:** mesma política de mounts/namespace/seccomp; parent/child sem TCP externo. Servidores/providers são fixtures locais, não Copilot.
- **SDK + CLI local reais:** [probe final](../experiments/lr-10a-sdk-runtime/evidence/fix-4-real-offline-metadata.json) iniciou e registrou handler; RPC runtime 1.0.90/protocol 3, auth_required, catalog unavailable/rpc_error_unknown, quota_unknown/account.getQuota. Zero tentativas do conector host. Registro aceito com ACK positivo explícito não comprova request autenticado interceptado.
- **Inferência/persistência genuína:** BLOCKED_REAL; nenhum transcript de conversa foi criado. Resultados de FIX-2 não foram convertidos em prova de conversa real.

Outra descoberta versionada: `Client::start` da 1.0.17 descarta `setProvider.success`. A fixture devolveu false e o SDK iniciou mesmo assim. `require_registered` agora realiza **uma validação explícita com ACK positivo**, sem loop/retry/fallback; false impede as consultas de metadata e o Client é encerrado. O CLI real confirmou success=true no reteste final. Os probes intermediários anteriores a essa guarda inferiam registro pelo término de start e não são prova de ACK positivo. A guarda valida o caller antes das consultas; callbacks podem existir durante startup. No probe real o gateway permanece sem endpoint/credencial em todo o ciclo, incluindo esse intervalo. Uma contenção geral de startup/identidade adversarial não foi demonstrada.

Descoberta negativa relevante: o dispatcher SDK envia um head **101** antes de chamar o override WebSocket. A fixture observa 101 seguido de erro terminal, com zero conexão upstream. Não interpretar esse head como permissão, handshake real ou conectividade bem-sucedida.

Rodadas exploratórias preservadas: [initial](../experiments/lr-10a-sdk-runtime/evidence/fix-4-rust-tests-initial.txt) falhou em 5 testes por import local com Python -I; [diagnostic](../experiments/lr-10a-sdk-runtime/evidence/fix-4-rust-tests-diagnostic.txt) encontrou flags não repassados ao peer; corrigidos sem ampliar policy. [budget diagnostic](../experiments/lr-10a-sdk-runtime/evidence/fix-4-rust-tests-budget-diagnostic.txt) demonstrou cache de list_models: segunda chamada não fazia RPC. O teste passou a forçar somente o segundo models.list no peer sintético, sem inferência, para provar o budget. Todos os três harnesses recuperaram seus processos; falhas não foram relatadas como shutdown gracioso do SDK. Duplicatas internas de rodadas PASS foram arquivadas localmente fora do diff para reduzir a revisão; as três rodadas falhas e a rodada final permanecem auditáveis aqui. Não se reinterpretam resultados históricos de FIX-1–3 como retestes.

## 7. Segurança, processos, comandos e regressões

Comandos a partir da raiz; reprodução usa diretório novo para não sobrescrever observações publicadas:

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 RUSTC=/usr/bin/rustc RUSTDOC=/usr/bin/rustdoc \
  /usr/bin/cargo build --offline --locked --bins --manifest-path experiments/lr-10a-sdk-runtime/Cargo.toml
python3 experiments/lr-10a-sdk-runtime/run_fix4.py rust-tests --artifacts-dir /tmp/narys-fix4-retest --output /tmp/fix4-owned.json
python3 experiments/lr-10a-sdk-runtime/tests/test_measure.py --evidence /tmp/fix4-python.json
python3 experiments/lr-10a-sdk-runtime/run_fix4.py metadata --cli /absolute/path/to/pinned/native/copilot --output /tmp/fix4-metadata.json
```

Na execução final, os outputs foram evidence/fix-4-rust-owned-run.json, fix-4-python-final-ack.json e fix-4-real-offline-metadata.json. O runner Rust executou `/usr/bin/cargo test --offline --locked -- --test-threads=1` dentro do subreaper, com env de build explícito, sem copiar o ambiente pessoal para os testes. Opt-out de download em TODAS as invocações Cargo. Build inicialmente recompilou deps cacheadas por mudança do toolchain efetivo; sem downloads/install/update. Nenhum cargo bundle/preview ou benchmark prolongado.

Rust/Cargo **1.98.1 preinstalados**; /tmp toolchain 1.94 expirou. Crate POC mantém rust-version=1.94.0/Edition 2021; SDK 1.0.17/runtime/non-bundled. Não afirmar que os testes novos foram executados em 1.94. CLI --version 1.0.91 é referência histórica com SHA atual idêntico; RPC 1.0.90 reobservado. Referência runtime da crate 1.0.93 não substitui versão instalada nem prova incompatibilidade geral. Nenhuma versão atualizada.

Checks executados: rustfmt 1.9.0 --edition 2021 --check nos quatro arquivos Rust novos; py_compile nos três Python novos; parsing de todos JSON/JSONL da FIX-4; verificação de segredo sintético/encoded marker nos artefatos; git diff --check, hashes de fontes/arquivos históricos e identidades PID/start-time. Tentativa de rerun no mesmo diretório foi recusada ANTES de Cargo pelo guard de evidências existentes; não sobrescreveu resultados.

Não se repetiu a suíte Tauri de 1.107 testes: nenhum src-tauri/produção/contrato/Cargo de produção foi alterado. Regressões relevantes da POC cobrem protocolo, auth/errors/quota, lifecycle/persistência, permissões, FS/network, cancelamento/timeout/cleanup. Não alegar execução da suíte de produção.

| Medição final (uma amostra; não benchmark) | CLI real offline | Cargo + 38 testes Rust cacheados |
| --- | --- | --- |
| Startup SDK (ms) | 2429 | Não isolado |
| Shutdown SDK (ms) | 37 | Cada fixture verificada; não agregado como SDK único |
| Parede do harness (ms) | 3093.49 | 9757.8 |
| Pico RSS somado da árvore (bytes) | 438140928 | 725614592 |
| CPU amostrada, limite inferior (s) | 3.46 | 5.5 |
| Cleanup worker (ms) | 28.0 | 31.04 |
| Recovery signals / sobreviventes atribuídos finais | 0 / 0 | 0 / 0 |

Metodologia FIX-1: intervalos de 50 ms, RSS somado pode duplicar páginas compartilhadas e perder processos curtos; CPU é lower bound, não RAM incremental exata. A coluna de testes inclui Cargo, SDK/peers e threads de listeners. **Overhead isolado do gateway, TLS remoto e alternativas B1/B2/B4: NOT_RUN/inconclusivo**, sem atribuir os totais somente ao gateway. Artefatos/tamanhos e fontes estão na consolidação. Process counts globais não sustentam ownership; a prova usa kernel children, pidfd/start-time e ECHILD.

ExecutionAuthority/HumanLocal, planner read-only, Execution Broker, AgentRegistry, IPC release, OperationalTraceBus, TaskGraph/Scheduler/LR-8.5, UI e runtime 3D intactos. Nenhum shell genérico à WebView, executor Copilot de produção ou autoridade agentiva criada.

## 8. Credenciais e configuração preservadas

Somente stat de config.json antes/depois, inalterado nas execuções owned/metadata; nenhum conteúdo lido. Sem home/config/SSH/Git/keyring/D-Bus montados; nenhum token real extraído/copiado/exportado, sessão pessoal inspecionada, PAT/login/logout ou configuração global alterados. Sem sudo, firewall/serviço persistente/pacote/toolchain atualizado.

Marker sintético é public test data, usado exclusivamente nos controles locais. Nenhum valor/encoded marker apareceu nos logs/evidências; só flags/códigos/contagens. Servidores/threads/filhos encerrados, identidades conhecidas reclamadas, canários preservados. Não se enviaram sinais a Codex/tmux/SSH/GNOME ou processos apenas parecidos.

## 9. Inferência e quota

**Zero chamadas de inferência realizadas nesta FIX. Zero ferramentas agentivas reais.** Sem session.send/send_and_wait, prompts ao Copilot, CLI -p, YOLO/Autopilot, paid fallback ou A9. A chamada HTTP sintética é metadata local; identificadores llmInference.* pertencem ao protocolo mockado e não representam inferência enviada ao serviço.

Não se mediu saldo/delta autenticado da conta. quota_unknown não é zero nem ilimitada. Nenhum consumo por request de inferência da POC; não inventar cobrança/saldo/unidades a partir disso. A [spec A9 inerte](../experiments/lr-10a-sdk-runtime/fixtures/A9-COMMAND-NOT-AUTHORIZED.txt) foi preservada e não executada; o novo código continua sem caminho de inferência.

## 10. Bloqueios e riscos remanescentes para A9

| Gate | Causa / tentativas permitidas / evidência | Próxima prova e risco de prosseguir |
| --- | --- | --- |
| BLOCKED_AUTH_BOUNDARY | Transporte explícito/provider entrega token ao runtime; host-only demonstrado só em metadata sintética; real auth ausente | Aprovação humana futura para credencial dedicada/escopo/lifetime/entitlement e contrato que contenha auth no host. Expor bus/token para PASS permitiria leitura/exfiltração |
| BLOCKED_NETWORK_BOUNDARY | Registro real aceito, mas cobertura completa/auth/TLS/URLs não testadas; offline sem conectividade ao provedor | Conector HTTPS host estrito e rotas mínimas auditadas, sem generic tunnel/fallback; impedir exfiltração de bodies/headers mesmo em destino permitido |
| BLOCKED_SUPERVISOR_FAILURE_CONTAINMENT | Harness passa árvores controladas; worker-death real/adversarial não contido por prova desta FIX | Gate separado de arquitetura/auditoria: worker morto pode deixar root/descendentes vivos. Reparenting/namespaces adversariais e tarefas presas no kernel não cobertos |
| Identidade/canais e recursos | Context/session/agent IDs vêm do runtime; filho pode herdar stdio. SDK reassembla bodies antes da policy; não se implementou limite global pré-parser | Auditar canais/FDs/rate/memory e falha do host antes de aceitar payload de inferência. Seccomp denylist/root namespace não é isolamento completo |
| BLOCKED_REAL / enforcement / billing | Sem conversa genuína, tool interno real ou snapshot autenticado; A9 não autorizado | Nova autorização após todos gates/auditoria, orçamento/overage/Auto validados e uma única operação SDK; sem recriar sessão como resume nem aceitar session.idle como sucesso |

Nenhuma dessas lacunas foi escondida por retry, sleeps de persistência, acesso amplo ao host, credencial fake apresentada como auth real ou atualização do SDK/CLI.

## 11. Decisão técnica fundamentada

**FIX_AND_RETEST**. O caminho é promissor e tratável: seam oficial já existe na crate e seu registro tem ACK positivo verificado no CLI instalado; operação finita pelo host passa com sandbox offline e negativas efetivas. Isso evita desenhar um proxy genérico prematuramente.

Não se escolhe GO_CONDITIONAL: autenticação contida e transporte real ainda carecem de provas, e sucesso de mocks não supre esses gates. Não se conclui NO_GO_CURRENT_DESIGN global: registro real contradiz a hipótese de ausência do mecanismo; porém proxy CONNECT/domain allowlist genéricos não atendem aos requisitos. Não se conclui DEFER apenas pelo calendário, sem estimativa objetiva do trabalho restante.

Saídas: **BLOCKED_AUTH_BOUNDARY**, **BLOCKED_NETWORK_BOUNDARY**. Auth_path/network_path operacionais não recebem FEASIBLE com base somente na fixture. A POC finita é viável como experimento; não é READY_FOR_A9 nem sandbox/supervisor de produção.

## 12. Relação com o prazo de 17/10/2026

Restam oito dias de calendário desde esta execução. O custo de oportunidade inclui implementar/verificar HTTPS/auth de menor privilégio, fechar contenção do supervisor e obter auditoria/autorização antes de A9; sem estimativa validada de horas ou promessa de conclusão desses gates.

Manter o experimento separado reduz impacto sobre a estabilidade da Narys 0.1. Recomenda-se que a decisão humana de escopo/cronograma avalie gates separados e critérios de aceite, sem usar o prazo como autorização de credenciais/inferência. Esta FIX não implementou release nem condicionou a estabilidade do core a callbacks experimentais. [Adendo permanente](LR-10A-IMPLEMENTATION-AND-EVIDENCE.md) registra esse impacto.

## 13. Próximos passos recomendados

1. Luna auditar sources, evidências finais/negativas e limites do seam model-layer versus auth/outros endpoints.
2. Definir desenho de auth host/credential exposure aceitável, escopo e lifetime; qualquer credencial real depende de autorização humana separada, não solicitada aqui.
3. Planejar conector HTTPS mínimo com destinos/headers/payloads aprovados e testes TLS/DNS/SNI/redirects/bypass, mantendo runtime e filhos offline.
4. Tratar o gate de morte do worker/canais herdados/limites de recursos separadamente; não substituir auditoria por supervisor LR-10B improvisado.
5. Só considerar A9 após gates e autorização/budget independentes; persistência genuína continua dependente dessa futura chamada. Não avançar automaticamente.

## 14. Auditoria independente pendente

A conclusão pertence à execução FIX-4, preservando resultados históricos. Os PASS são preparatórios e limitados às superfícies testadas. Luna deverá auditar diretamente o GitHub antes de decisões de autorização/priorização. Nenhum PASS definitivo da LR-10A/LR-10, nenhuma autorização para A9 e nenhum avanço para LR-10B/LR-10C.

**LR-10A FIX-4 — IMPLEMENTAÇÃO CANDIDATA, AGUARDANDO AUDITORIA INDEPENDENTE**.
