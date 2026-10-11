# LR-10C — matriz de validação

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
