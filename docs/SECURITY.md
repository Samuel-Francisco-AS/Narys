# LR-3 — fronteira de segurança desktop

Esta etapa protege a fronteira antes de qualquer API key real. O React apresenta estado; comandos específicos no Rust decidem e executam ações. Não há provider, ferramenta de filesystem/shell, credencial real ou API genérica de segredos.

## CSP e capabilities

`src-tauri/tauri.conf.json` define CSP de produção com `default-src 'none'`, scripts/estilos/fontes locais, imagens locais e `data:`/`blob:` para texturas GLB, e `connect-src` limitado à própria origem e ao IPC Tauri. `worker-src`, `object-src`, `frame-src`, `base-uri` e `form-action` são bloqueados. Não há `unsafe-eval` nem `unsafe-inline` em produção. O Tauri injeta hashes/nonces nos assets empacotados.

`devCsp` acrescenta apenas WebSocket HMR do Vite em localhost e `style-src 'unsafe-inline'` para estilos injetados pelo Vite em desenvolvimento. Isso não é aplicado ao build distribuído. O navegador comum executa via Vite, sem IPC Tauri.

`main-window.json` limita a janela `main` a comandos específicos, sem API genérica de keyring, Stronghold, filesystem ou shell. Os comandos diagnósticos LR-3/LR-4/LR-5 ficam fora do `invoke_handler` release e da lista release no `AppManifest`. A capability estática ainda contém permissões declarativas diagnósticas; a separação dessa ACL por perfil exige configuração Tauri adicional e permanece registrada como superfície residual sem handler executável.

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
