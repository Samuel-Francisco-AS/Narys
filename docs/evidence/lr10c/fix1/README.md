# FIX-1 — evidências locais

Candidata para reauditoria. Nenhum resultado deste diretório concede PASS final
ou autorização para sessão/inferência/provider/ferramenta autenticada.

- `results.json`: commit de código, comandos, totais finais, gates ignorados e
  falhas/intermediários preservados. `environment.json`: host diagnosticado.
- `core.log`, `domain.log`, `python.log`: últimas passagens integrais. Os logs
  numerados/initial/check são anteriores; não equivalem a validação final.
- `release.log`, `release-boundary.log`: build otimizado e repetição dos casos
  operacionais com os binários release. A repetição não aumenta cobertura.
- `operational-chain.json`: observação de Core/CLI/socket/ferramentas/SQLite reais,
  arquivo/hash independente, cancelamento/cleanup e restart sem reaplicação.
- `native-tools.json`: apenas ping/status.get/tools.list offline do CLI pinado,
  sem sessão/send/inferência/execução de ferramenta.
- `SHA256SUMS`: integridade dos arquivos deste diretório, excluindo o próprio
  manifesto. O manifesto pai foi atualizado para a matriz expandida; os demais
  artefatos da implementação inicial permanecem preservados.

Reproduzir a cadeia a partir da raiz do checkout após build release:

```sh
NARYS_TEST_CORE="$PWD/src-tauri/target/release/narys-core" \
NARYS_TEST_CLI="$PWD/src-tauri/target/release/narys" \
python3 -W error::ResourceWarning docs/evidence/lr10c/fix1/export-chain.py
```

Esse comando sobrescreve o JSON de observação, pois novos IDs/tempos/hash de
binários são observados. Use checkout descartável para preservar as evidências
publicadas; não reutilize o manifesto anterior depois de reproduzir. Execute as
suítes pesadas serialmente conforme results.json, sem `--ignored`. Não instala
binários nem reinicia a instância instalada.

As confirmações são roteirizadas pelo CLI real no domínio confiável do operador.
O peer é Python sintético confinado; escrever e sha256sum/sleep são efeitos reais.
Isso não prova humano físico, SSH real ou Copilot autenticado. Imagens de crash
approved/claimed são fixtures SQLite explicitamente descritas na matriz; não
são um emissor humano wire. Credenciais/ambiente privados usam markers sintéticos.
Não houve leitura de credenciais reais, provider, cobrança ou instalação global.

As classes negativas verificam syscalls e efeitos de filesystem de fato, não
mensagens do peer. A separação só cobre código admitido pelo boundary obrigatório
desta instância: conta/host comprometidos ou agentes fora da contenção não são
identificados como humanos pelo endpoint. IPC ordinária continua sem aprovação
positiva. Os perfis Copilot nativo/Autônomo isolado completo/YOLO real ficam BLOCKED.
