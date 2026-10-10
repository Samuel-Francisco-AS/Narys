# Narys Core headless

Core residente independente de Tauri/WebKit, controlado por socket Unix 0600 em
`$XDG_RUNTIME_DIR/narys-core/control.sock`. O servidor valida SO_PEERCRED (mesmo
UID); não abre porta TCP. O usuário Linux é a fronteira de confiança. **Este
perfil HOST_ASSISTED não é sandbox e não protege contra processos do mesmo UID.**

Reutiliza os contratos `AgentBackend`, `AgentRegistry`, `PlanV1`, `TaskGraph`,
`TaskId`, o `OperationalTraceBus` limitado e as migrações SQLite existentes. A
factory da aplicação gráfica continua somente Codex. O banco do Core fica em
`~/.local/state/narys/core/db/luna.sqlite3`, separado do banco da GUI; resultados
sobrevivem a restart. Tarefas interrompidas nunca são reenviadas automaticamente.
O snapshot Stronghold existente continua no diretório original e é somente
aberto/validado: nenhum create_client, save, migração, chmod ou chave substituta.

O SDK 1.0.17 inicia o CLI 1.0.95 sob demanda. A identidade SHA-256 é verificada a
cada invocação. O harness FIX1 permanece inalterado: subreaper privado, pidfds,
PID/start-time e exaustão ECHILD. O cgroup da unidade contém Core e workers; o
Keyring está em outra unidade. Morte do supervisor ainda não equivale a cleanup
comprovado; systemd termina membros da unidade na parada. Não há supervisor
adversarial nem sandbox de produção implementado.

Neste Fedora, a conexão padrão aos endpoints públicos do Copilot excedeu o
timeout, enquanto IPv4 respondeu. O runtime recebe somente `RES_OPTIONS=no-aaaa`
para as consultas DNS glibc; o catálogo real passou com essa seleção. Não há
mudança global de rede, proxy ou desativação de TLS. É um ajuste local de
compatibilidade, sem restrição de destinos ou promessa de sandbox. A opção é
diagnóstica e incompatível com validação DNSSEC pela aplicação, conforme a
[documentação glibc](https://sourceware.org/glibc/manual/latest/html_node/Resolver-Options.html).

## Construção e instalação

```sh
COPILOT_SKIP_CLI_DOWNLOAD=1 CARGO_BUILD_JOBS=2 \
  CARGO_TARGET_DIR="$PWD/src-tauri/target" \
  /usr/bin/cargo test --offline --locked --manifest-path narys-core/Cargo.toml
python3 narys-core/ops/install_user.py
```

O instalador recusa sobrescrever unidades diferentes; não reinicia Keyring ou
GDM, não muda boot e não usa sudo. O binário instalado usa os scripts desta
checkout; manter este repositório e caminho. A unidade e o drop-in persistente
habilitam somente serviços de usuário. Linger e multi-user.target são condições
externas, não efeitos do instalador. Nunca digitar senha em sessão capturada
pelo Codex.

Atualizações do Core já instalado usam `ops/update_user.py SHA_BINARIO_ATUAL
SHA_UNIDADE_ATUAL`: os hashes precisam ser previamente revisados. Somente a
unidade `narys-core.service` é parada; Keyring e GDM não são reiniciados. Esta
entrega já instalou a unidade e o binário no host. Catálogo: timeout 30s, inferência:
120s, harness: 240s, parada systemd: 270s. Não há retry SDK pela Narys.

## Operação pelo SSH privado

```sh
~/.local/lib/narys/narys-core status
~/.local/lib/narys/narys-core credentials
~/.local/lib/narys/narys-core unlock
~/.local/lib/narys/narys-core stronghold
~/.local/lib/narys/narys-core copilot
```

`unlock` executa o helper humano H2/H3, fixado ao GNOME Keyring 50.0. A interface
GNOME utilizada é interna/não suportada. Exige ausência real de GNOME Shell,
serviço existente no user manager, coleção login preexistente e sessão libsecret
DH/AES. Senha somente via `/dev/tty`, sem eco, argumento ou persistência. Não há
unlock automático, criação de coleção, cópia de tokens ou leitura de itens.
`stronghold` resolve internamente a chave pelo credential store existente e
abre o client `luna-core`; retorna somente status, nunca valores. `copilot`
consulta status/auth/modelos/quota, encerra o runtime e guarda evidência privada.
Autenticação, quota e admissão financeira continuam estados independentes.

## Tarefa pequena e autorizada

```sh
~/.local/lib/narys/narys-core prepare \
  'Responda somente o número da soma de alpha=2 e beta=3. Não utilize ferramentas, não execute comandos, não acesse arquivos e não faça outras solicitações.' 5
```

Retorna ID e workspace descartável 0700 em `/tmp/narys-task-*`. Revisar tarefa e
caminho antes de submeter. Nenhum arquivo pessoal é incorporado ao prompt. Esta
versão oferece tarefas textuais de especialista com resultado independente
esperado, **zero ferramentas**. Ela não oferece execução agentiva de shell,
edição de repositórios ou ações externas. O Core grava o resultado na fixture
como efeito local controlado. O especialista não recebe `ExecutionAuthority` ou
`HumanLocal`; o Execution Broker original permanece inalterado.

Antes da primeira inferência há um gate financeiro humano obrigatório. A API
pinada informa requests, enquanto a documentação atual usa AI Credits; Auto e
multiplicador ausente não comprovam custo. O operador deve verificar a franquia
e ausência de orçamento adicional no GitHub e autorizar especificamente a
incerteza residual, sem autorizar pagamento. **Não executar o comando seguinte
sem essa decisão explícita.**

```sh
python3 narys-core/ops/review_financial.py ID
~/.local/lib/narys/narys-core submit ID
~/.local/lib/narys/narys-core result ID
~/.local/lib/narys/narys-core cancel ID
```

A revisão é exclusiva de uma tarefa, expira após 30 minutos e admite zero USD
adicional. A resposta atual do provedor ainda deve informar quota disponível e
ambas as flags de overage/uso após esgotamento false. Falta/erro bloqueia. O
limite de sessão de 0,5 AI Credits é **soft**, não garantia de custo máximo. Uma
operação SDK pode envolver várias requisições internas. Não há retry, fallback
ou compra. O guard `send-attempt.json` é O_EXCL + fsync antes do único
`send_and_wait`; erro/timeout consome a tentativa. Restart não libera reenvio.
O marker histórico A9 não é apagado ou reutilizado.

Para a validação desta entrega, o usuário já confirmou orçamento adicional
desativado e autorizou a única chamada apesar da diferença de unidades. Esse
consentimento deve ser associado à tarefa exata em um receipt privado; nenhuma
inferência foi enviada durante a preparação. O catálogo e as flags atuais do
provedor continuam obrigatórios antes do envio.

Cada sessão recebe diretório de configuração/estado privado, infinite sessions
(compaction automática) desabilitado, zero ferramentas, DenyAll, MCPs,
extensões, skills, hooks, descoberta de instruções/config e Git desabilitados.
Logs brutos do CLI ficam privados; journal recebe apenas labels de lifecycle.
`session.idle` indica término da operação, não sucesso: o Core compara o texto
final ao resultado esperado, verifica shutdown/cleanup e então conclui o grafo.
O arquivo `result.txt` precisa ser criado e relido com conteúdo idêntico antes
da conclusão. `events` retorna somente labels/correlações do trace local.
Uma mensagem do modelo não atesta efeito de shell/filesystem.

## Uma reinicialização, quando a entrega estiver preparada

Confirmar `loginctl show-user "$USER" -p Linger` e `systemctl get-default`
(`yes`, `multi-user.target`); unidades habilitadas. Reboot somente pelo usuário,
no SSH privado. Depois reconectar, executar status/credentials/unlock/stronghold.
O Core deve estar ativo mesmo com coleção bloqueada. Codex verifica ausência
real de GUI e executa o único teste integrado aprovado depois do gate financeiro.
Não iniciar GDM nem alterar permanentemente boot durante a validação.

O Core não é a aplicação gráfica completa: provedores cognitivos, renderização
3D e comandos Tauri continuam em seus módulos originais. É a composição mínima
para operar o especialista pelo servidor, sem antecipar toda a Narys Android.
