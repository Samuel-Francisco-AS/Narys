# LR-10C — matriz de validação

## FIX-1 — Trusted Approval & Operational Boundary

**CANDIDATA PARA REAUDITORIA.** Resultados PASS abaixo são delimitados por camada;
nenhum concede PASS final à LR-10C. `[U]` unitário/Core fixture; `[I]` Core/CLI/IPC
reais; `[S]` subprocessos reais em sandbox; `[P]` peer sintético (Python ou SDK);
`[A]` Copilot autenticado real **não executado/NOT_VERIFIED**.

| # | Obrigação FIX-1 | Prova/limite |
|---|---|---|
| 1 | Aprovação operacional legítima pelo canal separado | I+S+P: CLI real mostra preview e recebe confirmação roteirizada; chain real write+sha256sum+SQLite. Humano físico/SSH real NOT_VERIFIED |
| 2 | Autoaprovação por subprocesso mesmo UID | S+P: UID igual; socket do operador, proc/Core/fds inacessíveis; tentativa real falha; intents approve/trusted/HumanLocal não são API de aprovação |
| 3 | Receipt reutilizado e duplicidade | I: oito clientes approve concorrentes, exatamente uma aprovação/claim; segunda aprovação recusada após consumo |
| 4 | Alteração de argumentos | I+U: digest divergente/fields adicionais recusados; mutações de contexto completas U; contexto operacional imutável e preview exato |
| 5 | Desconexão/reconexão durante pending | I: socket fecha com request parcial; outro cliente consulta/aprova sem herança. Transporte SSH específico NOT_VERIFIED |
| 6 | Ausência de operador/timeout/deny | I+S+P: TTL1 expira sem claim/arquivo; deny elimina efeito, sem consentimento implícito |
| 7 | Restart de aprovado não consumido | I + SQLite fault fixture com Core SIGSTOP: imagem approved→interrupted, sem arquivo/grant; não representa aprovação humana real nessa janela |
| 8 | Restart após claim e antes de resultado | I+S+P: sleep real started interrompido; uncertain/cleanup false, reentrada fechada. Imagem claimed-before-effect adicional por fixture SQLite |
| 9 | Cancel antes de claim | I+S+P: pending cancelado, zero execução/arquivo; ack cancel_requested distinto de cleanup |
| 10 | Cancel durante ferramenta longa | I+S+P: sleep30 real, cancel responsive e cleanup por pidfd; outro workspace conclui enquanto sleep está ativo |
| 11 | Cancel concorrente com término | I+S+P: oito corridas sleep0; estado terminal ordenado, cancel vencedor sem completed tardio. U: callback300ms libera mutex e recusa sucesso tardio |
| 12 | Descendente após cancelamento | S+P: fork/setsid e sleep descendente; PID1/pidfd readiness e wait; arquivo orphan não surge após1s |
| 13 | Efeito real permitido | I+S+P: arquivo contém bytes autorizados; sha256sum real exit0; Core verifica por openat do handle pinned e hash; ledger/events persistidos |
| 14 | Leitura/escrita fora do workspace | S+P: marker sintético externo intacto; leitura/escrita e proc host falham realmente, workspace do peer ro |
| 15 | Symlink/hardlink/traversal/TOCTOU | U+I+S: validação, substituição antes de approval bloqueada, mount por FD mantém inode original quando host troca o path; evidencia por nofollow/openat; sem executor geral de comandos |
| 16 | Socket Core/operador | I+S+P: autoapprove ordinária continua bloqueada; socket positivo excluído e criação AF_UNIX EPERM no peer |
| 17 | Credenciais/env do host | S+P: marker e env privados sintéticos inacessíveis; HOME/keyring/DB/SSH não montados. Nenhuma credencial real lida |
| 18 | Rede proibida | S+P: listener TCP host em preflight; socket AF_INET/AF_UNIX/AF_VSOCK real EPERM sob seccomp; nenhuma rede de provider liberada |
| 19 | Tool desconhecida/não mediada | I+S+P+U: bash/sh/plugin/MCP/perfil/origin/fields desconhecidos negados; peer não escreve workspace por rota alternativa; 15 tools CLI permanecem deny |
| 20 | Hook falha sem autorização | SDK peer P+U: malformed hooks.invoke produz output vazio; não entra em emitter/claim local; A NOT_VERIFIED e native BLOCKED |
| 21 | Resume sem grants herdados | I+U: recovery interrompe receipts, nova geração sem capacidades; sem API de replay/reset; reentrada incerta bloqueada |
| 22 | Supervisor LR-10B | Regressão Rust Core integral; leases/ownership/startup journals/recovery/cleanup preservados, nenhum lifecycle autenticado novo |
| 23 | Codex Planner read-only | Regressão Domain integral serial; dois gates externos ignorados, não PASS; A NOT_VERIFIED |
| 24 | Custos sem autorização | U+I+S: paid admission recusada; mode não compõe provider, rede proibida, status financeiro false, zero inference/auth tools; sem afirmação de quota externa |
| 25 | SQLite antes/depois do claim | I+S+P: trigger antes de claim faz rollback conjunto; antes de start gera failed/not_started/cleanup; pós-efeito falha resultado, fecha admission e preserva estado incerto, sem replay; falha na primeira revogação de shutdown não impede cancelar duas ferramentas sleep30 reais |
| 26 | Órfãos/cleanup inconclusivo | S+I: pidfd/wait, namespace PID1; crash/recovery mantém cleanup false e admission fechada; shutdown racing preflight não inicia depois de stop; oito admissions concorrentes reservam somente quatro slots antes do preflight; limite128 usa transação (imagem SQLite de127tasks, última admissão real). S: Core dentro de outer sandbox com userns disabled não emite aprovação, apesar de bwrap existir |

A suíte operacional é `narys-core/tests/test_boundary.py`, com 27 testes.
Eventos U de crash image são explicitamente fixtures controladas pelo operador,
nunca canais disponíveis ao agente. A cadeia positiva usa cliente/servidor e
programas reais; o emissor não vem de construção privada do módulo de testes.
Os testes medem falha de syscalls/ausência de arquivos/bytes/hash/exit status e
pidfd, não o retorno textual do peer. Afirmativas SSH/humano físico/autenticação
não são extrapoladas da confirmação roteirizada.

**PASS delimitado:** cadeia operacional offline sob boundary obrigatório.
**BLOCKED:** native Copilot, Autônomo isolado completo, YOLO real, aprovação
positiva na IPC host-assisted. **NOT_VERIFIED:** fluxo autenticado, gateway de
segredos/rede/entitlement, SSH real, MSRV exato, endurance/DoS agregado/cgroups.
**PARTIAL produto:** integração desse executor ao Copilot/conversa/TaskGraph não
é entregue; não atribuir a ela o PASS dos subprocessos locais.

Resultados/logs/tempos/checksums: [results.json](fix1/results.json) e
[operational-chain.json](fix1/operational-chain.json). Registro abaixo é histórico
anterior à FIX-1.

---

## Matriz histórica LR-10C inicial


**Candidata; nenhuma linha concede PASS definitivo da etapa.** PASS abaixo
significa resultado do teste delimitado, não approval humana real ou execução
autenticada. Fixtures positivas constroem provas dentro do módulo privado do
Core. Não são emissores operacionais. Logs e totais no
[relatório](../../LR-10C-DELIVERY-REPORT.md).

| # | Requisito / prova | Resultado e limite |
|---|---|---|
| 1 | Default deny; token aleatório falso; boundary/finance ausentes | PASS fixtures; produção sem emissores positivos |
| 2 | Aprovação legítima de uso único; arquivo positivo; segunda execução recusada | PASS fixture Core. Canal humano operacional BLOCKED |
| 3 | Negação, expiry e ausência de aprovador; callback nunca executado | PASS fixture, estado durável consultável |
| 4 | 8 aprovações concorrentes, 8 consumos; exatamente um efeito | PASS fixture/SQLite real |
| 5 | Troca de task/session/specialist/profile/workspace/tool/version; inode do workspace trocado | PASS fixture |
| 6 | Alteração de path/conteúdo após approval e digest falso | PASS fixture; sem consumo |
| 7 | Paths absolutos/traversal, symlinks/hardlinks/alias do workspace | PASS validação tipada; symlink criado no sandbox falha realmente |
| 8 | Leitura/escrita de marker externo ao workspace | PASS subprocesso real Bubblewrap, marker intacto |
| 9 | Shell aninhado, fork/setsid, namespace adicional | PASS negativas locais; descendente não escreve após kill/cancel |
| 10 | Marker privado, env sintético e /proc do Core, fds privados | PASS negativas reais; não acessa credenciais reais |
| 11 | TCP loopback no host negado por net namespace | PASS tentativa real. Transporte/rede autenticada Copilot NOT_VERIFIED e BLOCKED |
| 12 | 15 tools do CLI pinado + MCP/plugin/nomes desconhecidos | PASS inventário offline e deny do hook sintético. Nenhuma extensão habilitada |
| 13 | Hook e handler antes do efeito sintético; input de hook inválido | PASS para protocolo fixture; resposta vazia SDK confirmada. Cobertura nativa NOT_VERIFIED, efeitos nativos BLOCKED |
| 14 | HumanLocal/origin enviado pelo wire; regressão Broker specialist denial | PASS IPC real e testes ExecutionBroker/Domain |
| 15 | Subprocesso mesmo UID conecta ao IPC e tenta approve_once | PASS bloqueio real do servidor. Dentro da sandbox o socket do host nem é acessível |
| 16 | YOLO por wire sem canal, prompt/arquivo sem API de ativação, restart | PASS negação IPC/contratos; consentimento positivo apenas fixture; produção BLOCKED |
| 17 | Cancel pending/approved, impedir callback e nova emissão | PASS Core fixtures; não equivale a cancel de inferência real |
| 18 | 32 corridas cancel vs claim/efeito, nenhuma segunda execução | PASS fixture atômica; comandos nativos longos BLOCKED |
| 19 | Recovery idempotente pending/approved→interrupted, consumed preservado, nova geração sem grant | PASS fixture/SQLite; nenhum replay |
| 20 | Diagnóstico bem-sucedido não habilita perfil; nenhum fallback sem prova | PASS gate permanentemente fechado. Sandbox nativa completa BLOCKED |
| 21 | Política gerenciada exige approval; impede YOLO | PASS fixture e handler deny independentemente de flags de settings |
| 22 | Codex Planner read-only, fake app-server e dois gates autenticados ignorados | Regressão Domain registrada nos logs; gates reais NOT_VERIFIED |
| 23 | LR-10B leases, no-start-after-stop, ownership/journal, cancel, crash/recovery/cleanup | Regressão completa Core registrada nos logs; nenhuma operação autenticada nova |

As negativas de OS usam /usr/bin/bwrap e Python/shell reais, tempdirs próprios e
markers sintéticos. Não são apenas mensagens de política. As positivas da engine
usam callbacks de fixture dentro do Core, **não uma ferramenta chamada por LLM**.
O fake peer usa o SDK Rust oficial; não é o CLI Copilot autenticado. Inventário
usa o CLI real com ping/status/tools.list, sem sessão/send/exec/network/credentials.

**BLOCKED:** approve_once humano operacional, execução Assistido nativa,
Autônomo isolado Copilot, YOLO irrestrito. **PARTIAL:** integração de authority e
fluxo positivo de approval/YOLO com runtime operacional. **NOT_VERIFIED:** caminhos
de ferramentas autenticadas, transporte/segredos de provider, custos, inferência,
endurance, Windows/macOS e MSRV Rust1.94 exato. Nenhum desses itens recebe PASS por
um teste de fixture ou pelo número total de regressões.
