# LR-3 — fronteira de segurança desktop

Esta etapa protege a fronteira antes de qualquer API key real. O React apresenta estado; comandos específicos no Rust decidem e executam ações. Não há provider, ferramenta de filesystem/shell, credencial real ou API genérica de segredos.

## CSP e capabilities

`src-tauri/tauri.conf.json` define CSP de produção com `default-src 'none'`, scripts/estilos/fontes locais, imagens locais e `data:`/`blob:` para texturas GLB, e `connect-src` limitado à própria origem e ao IPC Tauri. `worker-src`, `object-src`, `frame-src`, `base-uri` e `form-action` são bloqueados. Não há `unsafe-eval` nem `unsafe-inline` em produção. O Tauri injeta hashes/nonces nos assets empacotados.

`devCsp` acrescenta apenas WebSocket HMR do Vite em localhost e `style-src 'unsafe-inline'` para estilos injetados pelo Vite em desenvolvimento. Isso não é aplicado ao build distribuído. O navegador comum executa via Vite, sem IPC Tauri.

`main-window.json` limita a janela `main` a comandos específicos, sem API genérica de keyring, Stronghold, filesystem ou shell. Na implementação LR-9E, commands exclusivamente DEV/fixture ficam fora do handler, do `AppManifest` declarado e da capability estática de release, incluindo `start_mock_task`. O setup `cfg(debug_assertions)` instala `debug-diagnostics.json` dinamicamente apenas para `main`; settings não recebem essa ACL. O relatório `perf1c_ui_report` existe somente no build opt-in de probe. Probes funcionais de configuração Codex/Groq continuam produto. A distinção handler/manifest/ACL/bundle é verificada por `scripts/test-lr9e-release-security.py`; registros históricos abaixo preservam o estado de suas datas.

## Chave de desbloqueio Stronghold

O snapshot `luna-lr3.stronghold` continua em `app_local_data_dir`. A chave aleatória de 32 bytes reside no credential store nativo: Secret Service persistente via D-Bus no Linux, Credential Manager no Windows e Keychain no macOS. O backend Rust usa `keyring` 3.6.3 com `sync-secret-service` e `crypto-rust` no Linux, sem `keyutils` de sessão. A crate declara MSRV 1.75; `dbus-secret-service` foi fixada em 4.0.1 (MSRV 1.70), pois 4.1.0 exige Rust 1.78. O `rust-version = 1.77.2` do projeto não foi alterado. A toolchain exata 1.77.2 não está instalada; o check de MSRV foi feito por metadata e o build local usa a toolchain disponível. React recebe apenas status e nunca acessa a chave.

No primeiro uso sem snapshot nem chave legada, o core gera 32 bytes, grava no cofre do SO, relê e verifica antes de abrir o Stronghold. Não cria novo `luna-lr3.unlock`. Se snapshot existir sem credencial, falha fechado. Se Secret Service/D-Bus estiver indisponível, bloqueado ou corrompido, falha fechado, sem fallback plaintext. O diretório é revalidado em `0700` e o snapshot em `0600` a cada abertura no Unix.

Para migrar um vault legado, o core valida arquivo regular e permissões, lê a chave antiga, confirma que ela abre o snapshot, grava no credential store, relê e compara a chave, abre novamente o snapshot com a credencial recuperada e só então remove `luna-lr3.unlock`. Se a gravação no cofre falhar, o legado e snapshot permanecem. Se a remoção falhar, o status aponta hardening incompleto e a próxima abertura tenta novamente; o cofre do SO permanece como fonte principal. Nenhum caminho ou byte da chave é enviado à UI ou ao audit.

A chave agora pertence ao ambiente do usuário no sistema operacional. A separação entre snapshot e chave melhora a proteção contra cópia do diretório de dados, sem prometer segurança absoluta contra comprometimento da sessão do usuário. Perda ou corrupção do credential store pode tornar o snapshot irrecuperável; credenciais futuras teriam de ser cadastradas novamente. Não há backup plaintext automático nem export/recovery nesta rodada.

O único segredo funcional permanece `SecretKey::Lr3Test`, artificial. No Fedora, o vault existente migrou em `tauri dev` com audit `unlock_key_migrated`; após encerrar e reabrir o app, `security_status` voltou sem erro. Uma leitura manual somente de status confirmou segredo artificial ausente, como esperado após a remoção na LR-3, e arquivo legado ausente. O snapshot foi preservado em `0600`, e o diretório em `0700`.

## Audit e erros

O backend escreve linhas `security_audit timestamp_ms=... action=... result=... task_id=... detail_code=...` em stderr. A ação, resultado e códigos são enums/constantes internos; não há campo para valor de segredo, token, cabeçalho Authorization ou payload completo. O log é efêmero nesta etapa. Falhas do vault não são tratadas como “não configurado”: `storeAvailable=false` e `errorCode` expõem a falha sem detalhes sensíveis.

O `TaskId` recebido por `cancel_task` deve estar no intervalo `1..=Number.MAX_SAFE_INTEGER`; a desserialização de `u64` e a validação no Rust rejeitam valores inválidos. O registry continua removendo tarefas concluídas/canceladas.


## Parecer histórico de auditoria LR-3 — 26/09/2026

Os itens abaixo descrevem o estado da LR-3 antes do PRE-LR-6 hardening. As mudanças atuais estão registradas acima.

**LR-3 aprovada para a fundação de segurança do protótipo desktop.** A auditoria confirmou:

- CSP de produção restritiva, sem `unsafe-eval` e sem `unsafe-inline`;
- `devCsp` separado para o ambiente Vite;
- capability da janela principal limitada aos cinco comandos próprios previstos;
- ausência de filesystem, shell, process ou Stronghold IPC na WebView;
- `SecretStore` acessível somente pelo Rust;
- comandos de teste sem parâmetro de segredo e sem retorno do valor armazenado;
- audit estruturado sem campo para conteúdo sensível;
- validação de `TaskId` no boundary Rust;
- preservação da LR-2, avatar e fluxo WebGL.

A aprovação **não remove o gate de credenciais reais** descrito acima.

Melhorias futuras não bloqueantes:

1. Os comandos `security_test_store_secret` e `security_test_delete_secret` ainda existem na ACL de release, embora rejeitem execução em release por `debug_assertions`. Em hardening futuro, preferir não compilá-los/não expô-los em builds distribuídos.
2. Os comandos diagnósticos LR-4 são removidos do `invoke_handler` em release, mas seus nomes/permissões ainda são declarados no `AppManifest`/capability. Eles não ficam executáveis sem handler, porém a superfície declarativa também deve ser removida em hardening futuro.
3. Antes de credenciais reais, além de mover/proteger a chave de desbloqueio, revalidar permissões dos arquivos e diretório existentes a cada abertura do vault, não apenas no momento de criação/escrita.

## Validação manual

No Tauri dev, verificar Luna/Idle/Acenar, tarefa mock/Channel/cancelamento, ausência de violações CSP relevantes e os botões de storage de teste. Após gravar o segredo artificial, reiniciar a aplicação e confirmar apenas o indicador “armazenado”; então removê-lo. Em navegador comum, o painel deve dizer que SecretStore está indisponível e não invocar comandos Tauri.


## Auditoria PRE-LR-6 — 26/09/2026

**PASS no desktop Fedora.** A revisão pós-implementação confirmou que a unlock key do Stronghold não possui mais fallback plaintext: o credential store do sistema é autoritativo, snapshot existente sem chave falha fechado e o arquivo legado só é removido depois da validação da chave recuperada e do vault. A migração real preservou o snapshot e `luna-lr3.unlock` permaneceu ausente após reabertura.

Limitações conhecidas que não bloqueiam LR-6 no desktop atual:

- a toolchain Rust 1.77.2 exata ainda não foi executada; o manifesto continua declarando esse MSRV;
- logout/reboot do sistema ainda não foi testado;
- Windows e macOS ainda não foram validados;
- o backend Linux `sync-secret-service` é síncrono/bloqueante; a leitura de credenciais no GeminiProvider usa `spawn_blocking`;
- permissions diagnósticas residuais ainda aparecem na capability estática, embora os handlers não existam em release.
# Extensão LR-6 · Gemini

O segredo tipado `GeminiApiKey` é gravado no Stronghold; o unlock continua no credential store do SO. Os comandos Tauri `gemini_set_api_key`, `gemini_delete_api_key` e `gemini_status` não devolvem o valor. A leitura no executor do provider ocorre em `spawn_blocking`. O header HTTP `x-goog-api-key` é o único local de autenticação. O frontend não envia requests ao Google, não expõe a chave nem amplia a CSP. O aviso do Free Tier antecede o envio manual; `store:false` e a política de saída mínima são aplicados no Rust. Veja [GEMINI-PROVIDER.md](GEMINI-PROVIDER.md).


# Extensão LR-7B · Groq

O segredo tipado `GroqApiKey` usa o mesmo Stronghold e a mesma chave de desbloqueio protegida pelo credential store do sistema. Os comandos `groq_set_api_key`, `groq_delete_api_key` e `groq_status` não retornam o valor da credencial. O adapter lê a chave no Rust via `spawn_blocking` e envia somente um header `Authorization: Bearer` marcado como sensível. A janela `settings-ai` recebe apenas o estado configurado/não configurado e não ganha filesystem, shell ou IPC genérico de segredos.

O diagnóstico `groq_probe` é explícito, usa `Fixed("groq")`, contexto sintético sem memória privada e não persiste conversa. Nenhuma chamada Groq ocorre no startup. Conversation e Summary continuam `Fixed("gemini")` durante a LR-7B; a distribuição real fica para LR-7C. Veja [GROQ-PROVIDER.md](GROQ-PROVIDER.md).

## LR-9C · terminal humano e trust boundary da main

A LR-9C amplia intencionalmente a confiança depositada na WebView `main`:
**dar input à PTY humana permite controlar aquele shell humano enquanto a
attachment estiver ativa**. Esse shell pode executar comandos humanos reais,
herdar o ambiente do usuário e acessar recursos permitidos pela conta do SO.
Uma main totalmente comprometida pode iniciar/anexar a sessão e operar esse
shell; esta UI não promete impedir comandos arbitrários nessa situação.
`Terminal is not Authority` significa que Presentation não define admission,
policy, ownership do processo ou authority agentiva. Não significa que uma
WebView com acesso ao input humano seja incapaz de controlar o shell.

Apenas `main-window.json` recebe as oito permissões específicas:
`terminal_session_status`, `open_human_terminal`, `attach_terminal_surface`,
`detach_terminal_surface`, `acknowledge_terminal_batch`, `send_terminal_input`,
`resize_terminal` e `close_human_terminal`. Todos os handlers também conferem
`window.label() == "main"`. Settings não recebe essas permissões.
A attachment nativa é exigida para input/resize e revogada no detach,
Close Presentation, substituição ou falha/timeout da bridge. Ela é um identificador
escopado de Presentation, nunca uma serialização de `ExecutionAuthority`.

Não há `execute_command`, escolha de executable/argv/environment/cwd/PID pela
WebView, shell/filesystem/process plugin ou token de authority no DTO. O registry
chama a boundary humana LR-9B, resolve o shell no backend e inicia em HOME
absoluto/canonical existente; fallback `/`. Isso é diretório inicial, não sandbox.
Conversation, providers e agents não importam a API de Terminal nem ganharam
um caminho nativo de execution authority. A separação de código não constitui
isolamento entre componentes que habitam a mesma main comprometida.

CSP de produção permanece `default-src 'none'`, scripts/estilos/fontes locais,
`connect-src` self/IPC, workers/frames/objects bloqueados, sem `unsafe-eval` ou
script remoto. Xterm e fit são assets locais lazy; não há addon de rede ou
plugin genérico. O probe verificou a renderização e IPC reais sob essa CSP.
O trace DTO projeta somente OperationalEvent autorizado, sem ExecutionRequest,
ExecutionResult ou projeção de environment/credenciais; não acrescenta private
reasoning ou summary inventado ao conteúdo já existente no bus.
PTY é stream humano bruto: o próprio usuário pode imprimir dados sensíveis no
shell; seu conteúdo não é encaminhado ao trace, Conversation ou provider.

Close Presentation preserva o processo; Quit cancela/reap pelo Broker existente.
Não há persistência da PTY após restart, execução agentiva, handoff ou sandbox.
Tauri Isolation Pattern permanece possibilidade de hardening futuro, sem ser
implementado nesta fase. Contratos, budgets, gates e limites:
[LR-9C — Terminal Surface & Streams](LR-9C-TERMINAL-SURFACE-STREAMS.md).

## LR-9E · release IPC e hygiene final

O hardening candidato consolida handler, AppManifest e ACL release separadamente.
Os nove commands exclusivamente DEV ficam somente no handler/manifest debug e
na capability dinâmica `debug-diagnostics-only`, instalada sob `cfg(debug_assertions)`
para `main`. `settings-general` e `settings-ai` não recebem essa ACL nem terminal
permissions. `perf1c_ui_report` é exclusivamente opt-in probe. As permissions
autogeradas podem descrever um command sem conceder acesso: definição conhecida,
registro do handler e permissão concedida são superfícies distintas.

Probes de configuração Codex/Groq, credenciais tipadas, operational status,
Conversation, cancelamento real e o terminal humano continuam produto. CSP,
remote origins e dependency graph não foram relaxados. O gate derivado
`scripts/test-lr9e-release-security.py --bundle` verifica as três superfícies,
settings, bundle production e contratos sensíveis frente à base autorizada.

HumanLocal é constructor nativo privado e não desserializável. Somente Human
é admitido; SpecialistAgent/Worker/CognitiveProvider com essa mesma authority
são negados. Origin declarada, TaskId, PID e correlation não cunham authority.
ExecutionRequest/Result internos continuam sem serde; nenhum DTO serializa
args, environment ou internals de workspace indiscriminadamente. WorkspaceScope
valida canonical cwd/raízes/symlinks e não é sandbox.

A fixture integrada usa markers distintos para input/context, Summary,
provider interno, environment, reasoning bruto, payload Codex proibido e PTY.
Conversation ProviderText permitido continua visível; payload interno não é
copiado pelo adapter. PTY impressa é uma saída humana bruta, não publicação de
OperationalTrace nem input de Conversation/provider. Evidências, matriz de
authority/faults e limites em
[LR-9E — Final Gate](LR-9E-CONCURRENCY-SECURITY-FINAL-GATE.md).

O fechamento formal da LR-9 depende da auditoria independente da Luna.
Approvals/grants agentivos e sandbox permanecem nas fases futuras; não foram
implementados para preparar esse gate. Os registros anteriores mantêm as
superfícies residuais existentes nas datas das respectivas auditorias.

O controle visual LR-9E `set_terminal_activity` é permitido somente em `main`,
com attachment vigente. Ele suspende/retoma a assinatura de OperationalTrace,
sem mudar a sessão humana ou sua authority. Epochs descartam ACKs de entregas
antigas; settings-general/settings-ai não recebem essa permission.
