# LR-3 — fronteira de segurança desktop

Esta etapa protege a fronteira antes de qualquer API key real. O React apresenta estado; comandos específicos no Rust decidem e executam ações. Não há provider, ferramenta de filesystem/shell, credencial real ou API genérica de segredos.

## CSP e capabilities

`src-tauri/tauri.conf.json` define CSP de produção com `default-src 'none'`, scripts/estilos/fontes locais, imagens locais e `data:`/`blob:` para texturas GLB, e `connect-src` limitado à própria origem e ao IPC Tauri. `worker-src`, `object-src`, `frame-src`, `base-uri` e `form-action` são bloqueados. Não há `unsafe-eval` nem `unsafe-inline` em produção. O Tauri injeta hashes/nonces nos assets empacotados.

`devCsp` acrescenta apenas WebSocket HMR do Vite em localhost e `style-src 'unsafe-inline'` para estilos injetados pelo Vite em desenvolvimento. Isso não é aplicado ao build distribuído. O navegador comum executa via Vite, sem IPC Tauri.

`main-window.json` aplica-se somente à janela com label `main` em Linux/macOS/Windows. A lista inclui cinco comandos próprios (`start_mock_task`, `cancel_task`, `security_status`, `security_test_store_secret`, `security_test_delete_secret`) e nenhuma permissão de plugin/core. `build.rs` declara os mesmos comandos no `AppManifest` para que o ACL do Tauri os verifique. Os dois comandos de teste rejeitam chamadas em compilação release. Eles nunca recebem valor/chave do frontend nem devolvem bytes do segredo.

## Segredo artificial

`SecretStore` usa a implementação Stronghold do plugin oficial pelo Rust, sem registrar o plugin IPC ou instalar binding JavaScript. O snapshot fica no diretório de dados locais do aplicativo (`app_local_data_dir`), em `luna-lr3.stronghold`. A chave aleatória de desbloqueio de 32 bytes é criada uma vez em `luna-lr3.unlock`; no Unix, o diretório recebe `0700` e os dois arquivos recebem `0600`. Se houver snapshot e a chave faltar, ou se um arquivo for inválido/corrompido, o acesso falha e o status informa um código controlado. Nenhum caminho interno, chave ou valor é retornado ao frontend. O perfil dev otimiza apenas o pacote `scrypt`, conforme a recomendação do plugin, para evitar minutos de CPU por operação em debug.

Este POC demonstra criptografia do snapshot e persistência após reinício, mas **a chave de desbloqueio reside ao lado do snapshot, acessível ao mesmo usuário do sistema**. Isso não é proteção suficiente contra comprometimento dessa conta nem está aprovado para API keys reais. Antes da primeira chave real, decidir proteção da chave por mecanismo do sistema ou senha do usuário e rever backup/recuperação. A persistência não foi avaliada em Android/iOS.

O único identificador aceito nesta fase é `SecretKey::Lr3Test`, mapeado internamente no Rust. `ProviderConfig` futuro conterá apenas dados públicos como `enabled`, `priority` e `model`; `ProviderSecret` será uma referência tipada a um segredo no `SecretStore`, sem valor em React ou em configuração comum. O Provider Registry continua fora da LR-3.

## Audit e erros

O backend escreve linhas `security_audit timestamp_ms=... action=... result=... task_id=... detail_code=...` em stderr. A ação, resultado e códigos são enums/constantes internos; não há campo para valor de segredo, token, cabeçalho Authorization ou payload completo. O log é efêmero nesta etapa. Falhas do vault não são tratadas como “não configurado”: `storeAvailable=false` e `errorCode` expõem a falha sem detalhes sensíveis.

O `TaskId` recebido por `cancel_task` deve estar no intervalo `1..=Number.MAX_SAFE_INTEGER`; a desserialização de `u64` e a validação no Rust rejeitam valores inválidos. O registry continua removendo tarefas concluídas/canceladas.

## Validação manual

No Tauri dev, verificar Luna/Idle/Acenar, tarefa mock/Channel/cancelamento, ausência de violações CSP relevantes e os botões de storage de teste. Após gravar o segredo artificial, reiniciar a aplicação e confirmar apenas o indicador “armazenado”; então removê-lo. Em navegador comum, o painel deve dizer que SecretStore está indisponível e não invocar comandos Tauri.
