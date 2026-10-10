# LR-10A H2 — desbloqueio humano sem GNOME Shell

Estado candidato: **HEADLESS_PREPARED_PENDING_USER_TRANSITION**. A prova real de
componentes com **dados sintéticos** passou; o host pessoal continua com GUI.
Não há novo PASS de Copilot, Stronghold pessoal ou cold-start. Nenhuma inferência,
sessão Copilot ou reserva SDK real foi executada/criada. Auditoria pendente.

## O que foi comprovado

[h2-keyring-synthetic.json](evidence/h2-keyring-synthetic.json) executa GNOME
Keyring50.0 e D-Bus instalados, sem GNOME, rede, bus do host ou credenciais pessoais.
O daemon foi reiniciado mantendo apenas seus arquivos sintéticos criptografados.
A senha incorreta manteve a coleção bloqueada; a correta desbloqueou a coleção
existente. A libsecret0.21.8.2 negociou DH/AES e o helper rejeita fallback plain.

[h2_stronghold.rs](fixtures/h2_stronghold.rs) inclui **sem alterações** o backend
real `src-tauri/src/security/secrets.rs`. Criou um snapshot e sua chave somente
na fixture privada, depois consultou presença, reiniciou o daemon, desbloqueou e
reabriu o mesmo snapshot. Nenhum valor de item foi publicado. Os bytes do snapshot
e do login.keyring sintético ficaram iguais após desbloqueio/reabertura; não há
arquivo legacy plaintext. Isso não prova compatibilidade de credenciais pessoais
do Copilot nem valida que um snapshot pessoal poderia ser migrado sem riscos.

## Contrato de desbloqueio e limites

O [daemon50.0](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/daemon/gkd-main.c)
lê stdin com `--unlock` e aplica a senha na inicialização; esse caminho pode criar
login quando ausente. Não é um comando geral de desbloqueio do daemon já ativo.
O [login50.0](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/daemon/login/gkd-login.c)
implementa create-or-unlock e inicialização de slots. **Não executar --unlock no
cofre pessoal como tentativa cega, nem combinar --replace para substituir o dono.**

O contrato de [UnlockWithMasterPassword50.0](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/daemon/dbus/gkd-secret-service.c)
procura uma coleção específica e retorna erro se ausente, sem CreateCollection.
Pertence à [interface explicitamente não suportada](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/daemon/dbus/org.gnome.keyring.InternalUnsupportedGuiltRiddenInterface.xml).
É uma opção **experimental versionada**, não API pública portátil ou compromisso
de estabilidade do GNOME. O método público Secret Service Unlock pode exigir
prompt gráfico; não equivale a entrada de senha por SSH.

[h2_manual_unlock.py](h2_manual_unlock.py) reutiliza a
[libsecret para codificar o segredo](https://gnome.pages.gitlab.gnome.org/libsecret/method.Service.encode_dbus_secret.html)
e verifica o [algoritmo negociado](https://gnome.pages.gitlab.gnome.org/libsecret/method.Service.get_session_algorithms.html).
Usa nome D-Bus **único**, não ativável, com chamadas NO_AUTO_START. A coleção login
precisa existir; não chama CreateCollection/GetSecrets/Store/Prompt. Sem serviço,
coleção, contexto conhecido ou criptografia, bloqueia antes de solicitar senha.
Valida daemon50.0 pelo SHA
`c7c5ad270c98fc0c9466031a08a037d650a918786036579003d9bcca4844481e`, UID/PID/start-time,
dono no user manager e ausência de GNOME/sessão gráfica. Não inicia serviços.

Seu dispatcher aceita somente `unlock-existing-login`, stdin/stdout TTY e ausência
do worker automatizado. Lê `/dev/tty` com ECHO desativado, restaura o terminal em
erro/KeyboardInterrupt/SIGTERM/SIGHUP; não aceita senha em argv/env/stdin redirecionado.
Não grava arquivos/logs/senha. Desativa core dumps e dumpability do próprio helper.
Senha permanece transitoriamente na RAM; Python não garante zeroização/ausência
de swap. Usuário/host comprometido, root e processos do mesmo usuário não estão
isolados. DH/AES do Secret Service usa o protocolo legado MODP1024, não proteção
completa contra um host malicioso. Não criar um transporte criptográfico próprio.
SIGKILL/falha do terminal não permitem prometer restauração; recuperação humana
do eco: `stty echo`, em terminal próprio. Não executar o helper em terminal gravado
ou observado pelo Codex, nem pedir senha no chat.

## Plano concreto posterior — nenhuma transição executada

**Precisa de confirmação específica do usuário e revisão da extensão experimental.**
Antes de agir, confirmar acesso físico/local de recuperação, dois acessos SSH,
tmux/Codex acessíveis, trabalho gráfico salvo e autorização para interromper GUI.
O encerramento do display manager encerra sessão/aplicativos gráficos e o greeter;
logout simples pode deixar gnome-shell do greeter ativo e não satisfazer ausência.
Não usar terminate-user/session, killall ou parar serviços essenciais.

[h2-context.json](evidence/h2-context.json) encontrou sshd ativo, tmux/Codex fora
do scope gráfico, user manager ativo, Linger=no, dono do Secret Service no scope
gráfico e outro Keyring no user manager. Esses fatos permitem planejar, **não
garantem recuperação ou transferência do nome**. O [código50.0](https://raw.githubusercontent.com/GNOME/gnome-keyring/50.0/daemon/gkd-main.c)
observa logind quando há XDG_SESSION_ID e pode sair no fechamento daquela sessão;
PAM close_session no-op não garante que o dono sobreviva. Não ler /proc/environ
para tentar comprovar qual variável esse processo herdou.

1. Usuário/coordenador qualifica uma janela com retorno físico à tela de login.
   Confirma antecipadamente que parar somente a GUI não afeta sshd, user manager,
   tmux e Codex. Se isso não puder ser comprovado, não executar a transição.
2. Transição concreta e retorno exigem confirmação naquele momento. Não há
   dispatcher de logout/target/reboot nesta POC. Nesta execução não foram
   solicitadas senhas, encerrados serviços ou processos externos.
3. Sem GNOME/sessão Wayland/X11 de fato, verificar bus de usuário e dono do
   Secret Service. Não presumir transferência entre daemons. Se necessário,
   qualificar **sob demanda** as unidades já instaladas de GNOME Keyring no user
   manager, sem enable/linger. Não iniciar outro daemon enquanto houver dono
   desconhecido, nem alterar/copy/recriar keyrings. Os arquivos da coleção
   existente precisam permanecer sob o mesmo UID/home; não mudar COPILOT_HOME.
   Unidade sob user manager deve evitar XDG_SESSION_ID de sessão encerrada; essa
   configuração ainda não foi testada no host, não é uma ordem automática.
4. Somente depois da aprovação, em **outro SSH privado não capturado pelo Codex**,
   o humano poderá executar a operação abaixo. Digitar a senha somente no prompt;
   não enviar senha, stdout pessoal ou itens para a IA. O helper não inicia daemon
   nem cria coleção; se falhar, interromper e guardar apenas código seguro.

   ```sh
   python3 experiments/lr-10a-sdk-runtime/h2_manual_unlock.py unlock-existing-login
   ```

5. Verificar Locked=false por propriedade, sem itens/conteúdo. Isto não libera SDK.
   Só então preparar **nova identidade fixa one-shot H2** e dispatcher mínimo
   metadata-only reutilizando SDK1.0.17/CLI1.0.95 pinado e FIX1. Não usar reservas
   antigas nem driver GUI como se fosse headless. Start/getStatus/auth.getStatus/
   shutdown apenas; sem retry, modelos/quota/session/send. O dispatcher real não
   foi acrescentado enquanto as pré-condições de transição permanecem pendentes.
6. Preservar marker A9 por existência e FINANCIAL_ADMISSION=BLOCKED. Cleanup exige
   ECHILD, PID/start-time ausentes e nenhuma ação sobre processos externos.
7. Retorno da GUI pelo usuário/coordenador conforme plano físico aprovado. Nenhum
   reboot nesta H2. Cold-start requer janela independente com boot feito pelo
   usuário e desbloqueio manual; B após login gráfico não demonstra C.

## Stronghold pessoal e alternativa

`secret_presence` chama unlock_client: cria diretório, ajusta permissões, pode
gerar/guardar chave inicial ou migrar legado. Logo não é um diagnóstico pessoal
puramente read-only. **STRONGHOLD_ACCESS pessoal=NOT_TESTED**; não iniciar Narys,
usar cofre pessoal ou mudar snapshot/chave. A prova sintética usa o mesmo backend,
não uma cópia alternativa da implementação de criptografia.

O mecanismo preferencial tem viabilidade sintética suficiente; não há bloqueio
estrutural que justifique migrar o cofre agora. Se a extensão GNOME não for aceita,
primeiro qualificar uma interface suportada de desbloqueio existente ou seu fluxo
PAM humano, sem mudar a pilha PAM nesta H2. Alternativa futura: `UnlockKeyStore`
com chave derivada de senha, Argon2id, salt aleatório e parâmetros versionados.
A chave aleatória atual não pode ser substituída por uma KDF sem migração: exigir
unwrap/re-encrypt, backup criptografado, rollback e teste de recovery sob novo
escopo. Não salvar senha/chave plaintext; não implementado nem necessário nesta
entrega. Password unlock autentica o humano, não concede autoridade de tarefas.

## Reprodução e recursos

Sem instalar/download: o [comando exato e hashes dos rlibs](evidence/h2-rust-fixture-build.json)
e [build final](evidence/h2-rust-fixture-final-build.json) registram compilação
standalone com rustc1.98.1 instalado, Edition2021, bibliotecas **já em cache** do
backend. O cache inicialmente selecionado era1.94 e retornou E0514; selecionamos
artifacts1.98.1 existentes. Não alteramos toolchain, SDK, lockfile ou produção.
Reprodução do fixture Rust depende desse cache compatível; se ausente, BLOCKED,
não baixar/reconstruir Tauri automaticamente. Revisar o argv registrado e hashes
antes de repetir a compilação. Binário fica no target ignorado, não no Git.

```sh
python3 experiments/lr-10a-sdk-runtime/h2_keyring.py
python3 experiments/lr-10a-sdk-runtime/verify_a9_host.py --artifacts-dir /tmp/fresh-h2-regressions
```

O primeiro comando possui evidência fixa O_EXCL já existente: não sobrescrever
nem apagar para repetir. Nova execução sintética exige nova identidade revisada,
nunca um parâmetro para retry real. Build não executa o binário fora do namespace.
Bubblewrap começa vazio; monta /usr read-only para **software confiável do teste**,
etc sintético, /proc e /dev privados, estado0700; oculta home/run/etc pessoais,
rede e env. Esse mount de /usr não altera o boundary mínimo do Copilot da FIX3.

Medições estão no JSON final: whole-tree RSS somada, CPU amostrada como limite
inferior, duração/cleanup e amostras de RSS/readiness do daemon. Não representam
RAM incremental do servidor Narys, benchmark frio ou systemd --user real.
Regressões iniciais52Rust/148Python, incluindo20novos; [ownership](evidence/h2-regressions/a9-host-owned-tests.json).
Revisão final do dispatcher acrescentou negação de erro de ps (somente exit1
comprova ausência), sem alterar a operação encrypted testada na fixture inicial.
[Regressão Python final](evidence/h2-python-final-regressions.json):149PASS,
21novosH2, mesmo harness. Rust permanece byte a byte igual à versão já testada.
Não repetir suíte/UI Tauri: nenhum código de produção foi alterado.

## Release0.1 — 17/10/2026

Manual unlock sob demanda é plausível e não exige GNOME Shell por contrato dos
componentes testados. Keyring sintetizado usa cerca de10MiB nas amostras; o ensaio
combinado com Stronghold apresentou RSS somada de640.958.464bytes. Essa medição
não isola a causa do consumo nem demonstra custo de uma KDF específica. O binário
debug do backend tem162.290.248bytes, fica ignorado no target e não foi publicado.
Estimar **uma janela humana de30–60min**, não medida, para configuração user manager,
transição e um probe limitado; cold-start em janela posterior. Não prometer prazo
de integração completo. Narys0.1 pode priorizar provedores já aprovados e continuar
com GNOME/Keyring enquanto essa validação pessoal está pendente. Copilot não é
requisito para todas as funções; finanças, sessão privada, isolamento e LR10B
permanecem separados. Não iniciar nova infraestrutura/serviço/Android nesta H2.
