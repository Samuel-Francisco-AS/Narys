# LR-7D2 — fallback chain + Auto/score + affinity

Estado: **PASS completo em 01/10/2026 e integrada à `main` pela PR #11.**
Auditoria independente e gate humano foram aprovados. Preferred, fallback real,
Auto/score/affinity, restart com affinity somente runtime, responsividade do
CharacterStage e a redução real da latência do preflight foram validados.
Base: `origin/main` confirmada em
`965c17ae9bd989d4bad746c4987926f4264d23b6`, com LR-7D1 integrada pela PR #10.
Branch de entrega: `lr-7d2-smart-routing`.

## Autoridade e arquitetura

Luna Core continua responsável por identidade, sessão, policy, budgets,
capabilities, permissões, cancelamento e validação dos resultados. Providers
executam trabalho cognitivo. Auto não concede permissões, não chama ferramentas,
não executa `PlanV1` e não cria subtarefas. Codex continua `AgentBackend` separado.

`CognitiveRolePolicy` contém `routingMode` e `targets: CognitiveTargetPolicy[]`.
Cada target contém `providerId`, `model` e `thinkingLevel`. Budgets, retry,
histórico, input de resumo e contexto permanecem por papel. Timeouts são
persistidos por provider e carregados para **todos** os targets no preflight;
cada invocação recebe somente a configuração do target escolhido.

As mesmas funções de policy constroem targets para Conversation, Summary e
Orchestrator. A UI usa o catálogo do backend, oferece ↑/↓, adicionar e remover,
impede providers repetidos e preserva model/thinking ao mover um target.
Defaults preenchem targets novos; não substituem configuração salva.
`conversation_routing_status` devolve uma lista ordenada de estados, sem secrets.

## Migration 009 / schema 9

A migration reconstrói `cognitive_role_policies` sem os seis campos antigos de
invocação primary/fallback e cria `cognitive_role_targets`. A chave primária
`(role, position)`, UNIQUE `(role, provider_id)`, posição entre 0 e 7 e FK para
a policy mantêm a consistência relacional. Save altera policy e targets em uma
transação; load usa um único snapshot de leitura e rejeita posições com lacunas.

Upgrade v8 → v9 preserva budgets, retry, limites e `updated_at`. Todo primary
vira posição 0, sem trocar model/thinking. Fixed recebe só o primary efetivo;
fallback dormente não é promovido a autorização ativa. Preferred recebe seu
fallback em posição 1. Summary/Orchestrator atuais continuam Fixed equivalentes.
`PRAGMA user_version=9`; credenciais permanecem no Stronghold, fora do SQLite.

Validação estrutural: 1–8 targets, IDs até 64 bytes, modelos até 128 bytes,
sem controles/whitespace indevido ou providers repetidos. Fixed exige 1 target;
Preferred/Auto exigem pelo menos 2 e `maxProviderCalls >= 2`. Toda policy exige
pelo menos uma chamada. O orçamento pode ser menor que o tamanho da cadeia.
Thinking inválido é recusado pelo contrato tipado/persistência; thinking suportado,
provider registrado/enabled, capability e credencial são validados no catálogo
para cada target ativo. Erros são sanitizados e falham fechado. Cooldown não
invalida a configuração nem impede salvar.

## Fixed, Preferred e Auto

- **Fixed:** usa o único target ativo. Cooldown/indisponibilidade não autorizam
  outro provider. Score e affinity não substituem a escolha.
- **Preferred:** percorre rigorosamente `request.targets`, independentemente da
  ordem/prioridade do Registry. Pula targets cooling sem chamada ou evento falso.
  Falhas elegíveis anteriores ao primeiro chunk podem avançar. Retry do target
  atual pode consumir o orçamento antes de alcançar o próximo.
- **Auto:** só considera targets explicitamente autorizados. Registry,
  enabled/capability e invocação válida são gates; cooldown elimina candidatos
  antes do score. Providers registrados fora da request nunca são candidatos.
  Targets/configurações inválidos continuam fail-closed no preflight.

Nenhum fallback após chunk, Authentication, QuotaExceeded terminal,
InvalidRequest, Fatal, EventSinkClosed ou cancelamento. RateLimited, Timeout e
Unavailable preservam a elegibilidade/retry/cooldown herdados. Não há mistura de
respostas parciais entre providers. Budget/usage continuam pertencendo ao Core.

## Score efetivo

Ordinal começa em zero; `N` é o número de targets autorizados:

```text
policy   = (N - ordinal) * 100
registry = 32 - min(provider_priority, 32)
affinity = provider corresponde à affinity válida e contexto > 0
           ? min(500, 50 + ceil(estimated_context_bytes / 1024) * 25)
           : 0
total    = policy + registry + affinity
```

Coeficientes são constantes nomeadas. Aritmética usa saturação e ceil sem soma
que possa exceder `usize`. Maior total vence; empate usa menor ordinal e depois
`provider_id`. A preferência posicional pesa mais que a prioridade do Registry.
Um byte de contexto com affinity não vence sozinho uma posição anterior;
continuidade suficiente pode superar uma pequena diferença de preferência.
Não há bônus por marca, preços/quota/latência fictícios ou componentes da LR-8.

`ProviderSelected` inclui `provider_id`, `attempt`, `routing_reason` e `score`
(null para Fixed/Preferred). Motivos: `fixed`, `preferred_order`, `auto_score` e
`auto_affinity`. Este último identifica uma affinity que mudou o vencedor em
comparação ao score sem o bônus. Eventos são emitidos na seleção real; cooldown
pulado não fabrica Selected/Fallback. Conversation e diagnóstico Orchestrator
mostram motivo/score factual; os tipos TypeScript acompanham o contrato.

## Affinity e lifecycle

Conversation usa `conversation:<session_id>`. A chave fica somente no Scheduler;
`ProviderRequest` enviado ao adapter não a contém. A estimativa soma bytes UTF-8
do input atual e de todo histórico efetivamente enviado, limitada a 1 MiB, sem
serializar secrets ou inventar economia de tokens/cache do provider.

Cada sucesso do Scheduler registra o provider vencedor. Auto consulta a chave
apenas dentro dos targets elegíveis/autorizados; falha/cooldown não prende a
sessão e sucesso do alternativo atualiza a affinity. Fixed/Preferred podem
registrar sucesso para uma futura mudança a Auto, mas nunca usam affinity para
ordenar. Summary/Orchestrator enviam `None`: não há continuidade inventada.

Armazenamento é somente runtime, com no máximo 256 entradas e chaves limitadas
a 128 bytes. Cada sucesso refresca a entrada; ao atingir o limite, a entrada com
sucesso mais antigo é descartada deterministicamente. Restart limpa affinity;
policy, mode, ordem, model/thinking e timeouts continuam persistidos.

Orchestrator preserva retorno responsivo do TaskId, preflight em spawn_blocking,
cancelamento em preflight/running, cleanup e exatamente um terminal factual.
Summary preserva claim/recovery, worker oportunista, transcript isolado e
persistência de título/resumo sem modificar mensagens. O snapshot exato da
policy é revalidado após claim. JSON inválido após resposta bem-sucedida é erro
do parser do Core/worker e **não** autoriza retry/fallback de routing.

## Testes e gates

Testes locais, sem internet, quota ou credenciais reais, cobrem:

- v8 → v9, Fixed/Preferred efetivos, timestamp/budgets/limites, reopen,
  integrity_check e foreign_key_check; Auto roundtrip e roles independentes;
- cardinalidade, limite 8, duplicatas, modelos/IDs/thinking inválidos,
  corrupção de posições e gates de todos os targets/credenciais;
- cadeia sintética A/B/C saudável, uma/duas falhas, ordem contra Registry,
  cooldown sem eventos falsos, budget/retry, terminais e erro após chunk;
- Auto restrito aos autorizados, determinismo, posição/prioridade, empate,
  custo bounded, affinity por sessão, falha/cooldown/alternativo, remoção de
  target, Fixed/Preferred, eviction e Scheduler novo sem affinity;
- request real de Conversation com affinity por session_id e estimativa UTF-8;
- Summary Preferred/Auto, claim, transcript, parser sem fallback mágico;
- Orchestrator Fixed/Preferred/Auto, fallback de transporte, PlanV1 sem execução,
  parser fail-closed e preflight recusando credencial secundária, além do
  lifecycle/cancelamento/terminal/cleanup herdados.

Gates requeridos no HEAD de entrega:

```bash
npm run typecheck
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --release --manifest-path src-tauri/Cargo.toml
git diff --check
cargo fmt --check --manifest-path src-tauri/Cargo.toml
```

Na entrega inicial da D2, a base possuía drift preexistente de rustfmt em
**44 arquivos**, verificado
comparando cópias de `origin/main` com o mesmo formatter. Os 18 arquivos Rust
novos/modificados daquela entrega foram formatados; o gate global apontava **27
arquivos intocados**, incluindo `build.rs`, módulos agents/security e persistência.
A divergência não é exclusiva de build.rs. Não foram reformados arquivos alheios
apenas para tornar esse gate verde. A formatação dos arquivos tocados amplia o
diff; a revisão funcional compara também a base formatada com o código final.

O harness do Orchestrator aguarda até 30 s por evento/fechamento para acomodar
preflight Stronghold sob carga local (a espera de 5 s falhou nesta rodada).
Selected precede execute; o teste de cancelamento aguarda entrada efetiva no
provider antes de cancelar. São ajustes de teste; lifecycle de produção permanece.
Resultados exatos dos gates ficam na PR de entrega; não equivalem à auditoria
independente nem ao gate humano.

## Limitações e segurança

Só Gemini/Groq são providers reais. Cadeia 3+ é provada sinteticamente, sem
adicionar terceiro provider. Affinity é heurística determinística sobre custo
estimado de continuidade; não anuncia latência, quota, cache ou savings reais.
Não há LR-7D2.5, LR-7D3, task graph, paralelismo, LR-8, grounding web/tools,
novas permissões Codex ou mudanças de identidade/memória/avatar/3D. A D2.5 já
está planejada documentalmente, mas sua implementação só começa após o fechamento
desta D2. Dois testes de integração real
existentes permanecem ignorados por exigirem autorização/ambiente externo.

## Gate humano pendente

1. Salvar Conversation Preferred Groq → Gemini; verificar Groq saudável primeiro.
2. Inverter Gemini → Groq apenas por ↑/↓ e confirmar ordem/fallback elegível.
3. Salvar Auto com Gemini + Groq; observar apenas targets autorizados e
   motivo/score nos eventos. Em Fixed, confirmar ausência de desvio oculto.
4. Manter a mesma sessão com histórico suficiente e observar affinity quando
   aplicável; verificar que cooldown a quebra e o alternativo bem-sucedido assume.
5. Testar Orchestrator Preferred/Auto, cancelamento e PlanV1 sem executar passos.
6. Testar Summary e verificar título/resumo, isolamento e mensagens intactas.
7. Reiniciar: mode/ordem/model/thinking persistem e affinity começa limpa.

LR-7D2 só fecha após auditoria independente e aprovação humana. O próximo
checkpoint após esse fechamento será **LR-7D2.5 — provider redundancy + Gemini
de-risking**; somente após a D2.5 começa a LR-7D3. Consulte
[LR-7D2.5-PROVIDER-REDUNDANCY.md](LR-7D2.5-PROVIDER-REDUNDANCY.md).

## FIX humana — responsividade no início da Conversation

No gate humano real, Preferred/Auto/fallback/affinity funcionaram, mas o avatar
ficou visualmente congelado por aproximadamente **3–4 s**, após enviar a mensagem
e antes de `ProviderSelected`. Isso é uma observação humana; ainda não há medição
que atribua os 3–4 s completos a uma única operação.

A inspeção confirmou trabalho inadequado no caminho síncrono de
`start_conversation_task`: lock das sessões mantido durante abertura SQLite,
validação da sessão, leitura da policy, validação dos targets/Stronghold e leitura
de timeouts. `conversation_routing_status` já executava seu I/O em spawn_blocking;
continua informativo para UX, sem substituir a validação autorizativa do backend.

A FIX deixa no caminho síncrono somente validação estrutural barata e registro
do TaskId/foreground work. O worker async emite TaskStarted e executa em
spawn_blocking a validação da sessão atual/ativa, releitura da policy persistida,
validação de **todos** os targets/credenciais, timeouts, histórico isolado e
ContextBuilder. O registro das sessões agora tem ownership compartilhado por Arc;
o lock é liberado antes do I/O. Não há cache novo de secrets.

Antes: preflight bloqueante → registro/TaskId → TaskStarted → Scheduler.
Depois: registro/TaskId → TaskStarted → preflight bloqueante fora do caminho da
UI → ContextBuilt → seleção real do Scheduler → chunks → resultado/persistência
→ terminal. Erro de preflight produz TaskFailed sanitizado, sem seleção fictícia.

Cancelamento é checado antes do spawn_blocking, dentro do worker e após seu
retorno, inclusive se retornou erro. O worker bloqueante já em andamento pode
precisar terminar seu I/O; ele não autoriza chamada ao provider após cancelamento.
Um cancelamento aceito produz exatamente um TaskCancelled, nenhuma resposta
persistida e cleanup da TaskRegistry/foreground work. Falha de escrita do histórico
da task é diagnosticada e não transforma esse cancelamento em failed.

Score, affinity, cooldown, request targets, retry/fallback, fronteira anterior ao
primeiro chunk, usage, persistência e isolamento da Conversation permanecem iguais.
Summary/Orchestrator, permissões, identidade, memória e avatar/Three.js não foram
alterados. Luna Core continua a autoridade.

Somente em debug, `[Conversation][diag] preflight_ms=...` mede o agregado entre
despacho e conclusão do spawn_blocking, incluindo espera no pool, DB, credenciais,
timeouts e preparação do contexto/histórico. Não registra input, histórico,
identificadores privados ou secrets; não é emitido em release. Barreiras artificiais
nos testes entram no tempo medido e não devem ser confundidas com latência real.

Dez testes adicionais cobrem retorno/registro anterior à conclusão do preflight,
barreiras antes/depois do trabalho e dentro do keystore de teste, cancelamento
inclusive após erro/DB indisponível, exatamente um terminal e cleanup, sessão
inválida/inativa/não registrada, credenciais ausentes em Fixed/Preferred/Auto,
releitura de model/thinking/timeouts após registro, execução e persistência normais,
Preferred/fallback/Auto/affinity e histórico isolado. A suíte completa também
continua cobrindo Summary/Orchestrator. Nenhuma credencial ou provider real é usado.
Resultados dos gates no HEAD da FIX ficam na mesma PR #11. Os quatro arquivos
Rust tocados na FIX foram formatados, incluindo gemini_commands.rs (o diff de
formatação desse arquivo já apresentava drift na base). Naquela entrega, o gate
global permaneceu com divergência preexistente em **26 arquivos intocados**.

Repetir o gate humano no início da Conversation: enviar em Preferred e Auto,
observar a animação entre envio e ProviderSelected, registrar o preflight_ms no
build debug e repetir na mesma sessão com affinity. Cancelar durante preflight:
nenhuma chamada/resposta e um único terminal cancelled. Confirmar a rota e histórico
após sucesso. **Resultado humano posterior: PASS da responsividade; o avatar permaneceu fluido**.
Restart também confirmou affinity somente runtime. A LR-7D2 continua aberta pelo
finding de latência local descrito abaixo.


### FIX humana — latência de credenciais e presença batch

Com o freeze resolvido, a aplicação real registrou `preflight_ms=6294` e `6341`
antes da seleção do Scheduler. A decomposição adicionada **antes de otimizar**
mede somente em debug: `session_policy_ms`, `credentials_ms`, `timeouts_ms`,
`history_context_ms` e `preflight_total_ms`. O cofre mede `operation_wait_ms`,
`key_store_load_ms`, `secret_unlock_ms`, `secret_open_client_ms`, `secret_lookup_ms`
e, quando há migração, `snapshot_validation_ms`. Labels são fixos; não há input,
histórico, memória, secrets, bytes de chave, payload ou caminhos privados nos logs.
O agregado substitui a label anterior `preflight_ms`; timings internos se sobrepõem
(por exemplo, open_client está contido em unlock) e não devem ser somados duas vezes.

Na base, cada target provocava um get_secret independente. Além disso, unlock_key
validava o snapshot com Stronghold::new/load_client e with_client abria/carregava
novamente o mesmo snapshot/client. A medição local confirmou que essa repetição
é cara; não atribui automaticamente os 6,3 s reais a um único backend.

Comparação localizada com o mesmo teste
`preferred_auto_affinity_and_session_history_use_the_same_scheduler_contract`,
`--nocapture --test-threads=1`, quatro preflights de dois targets, keystore fake,
snapshot Stronghold real local, mesmo perfil debug e sem rede/providers reais:

| Etapa | Antes (ms) | Depois (ms) |
| --- | --- | --- |
| sessão/policy | 0–2 | 0–2 |
| credenciais | 5737–5842 | 1394–1414 |
| timeouts | <1 | <1 |
| histórico/contexto | <1 | <1 |
| preflight total | 5739–5845 | 1396–1416 |

Antes, cada validação/abertura do snapshot levou aproximadamente 1,4–1,5 s;
lookup e keystore fake ficaram abaixo de 1 ms. Depois houve uma abertura de
aproximadamente 1,4 s por preflight. São medições sintéticas/localizadas, sem
barreiras nesse teste: não comprovam latência do keyring real nem substituem a
nova medição na aplicação Fedora. Cada rodada dirigida passou o teste.

`SecretStore::secret_presence(&[SecretKey]) -> Result<HashMap<SecretKey, bool>, SecretError>`
retorna somente presença. Deduplica keys, mantém uma aquisição do mutex por batch,
desbloqueia uma vez e verifica todos os valores no mesmo client. Cada valor é
descartado dentro da closure. O snapshot validado é reutilizado **somente na
operação atual**; Stronghold/client/unlock key não são mantidos em campo/cache.
O helper comum também evita reabrir esse client nas APIs existentes, preservando
seus contratos e o consumo da unlock key pelo Stronghold.

Com snapshot provisionado e sem legado, contagens por operação:

| Caminho (dois providers) | Keystore antes → depois | Stronghold/client antes → depois |
| --- | --- | --- |
| preflight da policy | 2 → 1 | 4 → 1 |
| conversation_routing_status | 2 → 1 | 4 → 1 |
| get_ai_settings (availability + infos) | 4 → 1 | 8 → 1 |

Status e task são operações independentes: o preflight posterior **reabre e
revalida** o cofre. Nenhuma presença da UX é autoridade. Fixed continua válido;
todos os targets de Preferred/Auto continuam obrigatórios; cooldown não invalida
save. validate_policy_registered permanece intacto. Erros mantêm códigos públicos
sanitizados, como provider_not_configured; o SecretStore preserva erros tipados,
e debug registra apenas seu código sanitizado. Settings também obtém availability
e configured do mesmo batch, sem devolver conteúdo de secrets.

A leitura não chama write_client/save nem altera bytes do snapshot. Checks de
arquivos, permissões, comprimento da unlock key, decriptação/load_client e
migração permanecem. O first-run legítimo ainda provisiona/verifica a unlock key
no keystore com dois loads e um store, constantes por batch; não grava snapshot
vazio. Migração/cleanup de legado preserva validações independentes e pode exigir
aberturas adicionais como antes. Batch vazio não faz I/O.

`Couldn't get key from code: Quote` foi localizado no **tao 0.35.3**,
`src/platform_impl/linux/keyboard.rs`, branch debug do mapeamento GTK de tecla
lógica não identificada, que retorna None. A versão foi confirmada no Cargo.lock.
Não é um erro de keyring, Secret Service/libsecret ou Stronghold; esse caminho
não consulta o cofre. Não há medição que ligue esse log aos 6,3 s. Não foi feito
parsing de stderr, silenciamento, workaround de teclado ou troca do armazenamento.

Quatro testes novos de SecretStore cobrem booleanos/presente/ausente, duplicatas,
contadores de lock/load/open/client/get, ausência de escrita do snapshot, batch
vazio, first-run constante, keystore/snapshot/chave inválidos e concorrência.
Dois novos testes de Conversation cobrem status correto para todos os targets,
revalidação após remover credencial e erro batch sem seleção/chamada/persistência.
A cobertura de catálogo foi ampliada para Fixed/Preferred/Auto com um load,
credencial secundária ausente, cooldown, gates antes do I/O e Settings sem secrets.
Os testes existentes de policy/model/thinking/timeouts, cancelamento, affinity,
Summary/Orchestrator e migração continuam na suíte. Os contadores de abertura
existem somente nos testes. Resultados finais ficam na mesma PR #11.

Esta FIX não altera Scheduler, score, affinity, cooldown, Preferred/Auto,
retry/backoff/timeout do Gemini, modelos, providers ou UI/3D. `timeout → retry →
backoff 1500 ms → 503` é um finding separado e permanece inalterado. O custo de
Snapshot/Stronghold nesta medição não inclui chamada ao provider.

**Gate humano de latência pendente:** no Fedora, repetir envios Preferred/Auto,
registrar a decomposição acima, observar continuidade/affinity e avatar fluido,
abrir IA e modelos e verificar status corretos. Repetir com credencial secundária
ausente: falha fechada, nenhuma chamada. Confirmar cancelamento durante preflight.
Comparar com 6294/6341 ms somente após essa rodada real; a redução material local
não autoriza declarar resolvida a latência real nem fechar a LR-7D2.

O formatter foi aplicado apenas aos arquivos desta FIX. secrets.rs e settings.rs
já tinham drift e agora são arquivos tocados; o gate global conserva drift
preexistente em **24 arquivos intocados**, sem corrigir arquivos alheios.


## Fechamento — PASS completo em 01/10/2026

A auditoria independente aprovou a implementação original e as duas FIXes humanas
sem finding bloqueante. O gate humano real confirmou o comportamento de Conversation:

- **Preferred:** Groq em primeiro concluiu diretamente; com Gemini em primeiro,
  o provider retornou HTTP 503 `service_unavailable`, entrou em cooldown de 30 s
  e a mesma tarefa avançou para Groq antes do primeiro chunk;
- **Auto:** a primeira escolha respeitou score e targets autorizados; após sucesso
  por Groq, a mesma sessão passou a favorecê-lo por affinity;
- **affinity:** com Groq no segundo target, `auto_affinity` venceu a vantagem de
  posição do Gemini (scores observados de 255 e 280), provando que continuidade
  pode mudar a seleção sem alterar Fixed/Preferred;
- **restart:** a policy persistiu, mas a affinity anterior não sobreviveu ao
  processo, como definido pelo contrato runtime-only;
- **responsividade:** o freeze visual de aproximadamente 3–4 s entre envio e
  `ProviderSelected` desapareceu após mover o preflight bloqueante para
  `spawn_blocking`; o avatar continuou animando durante o preflight;
- **latência local:** antes do batch de credenciais foram observados
  `preflight_ms=6294` e `6341`. Após a FIX, medições reais ficaram em
  `preflight_total_ms=1625` e `1666`, redução aproximada de 74%;
- **Stronghold:** o custo dominante restante é abrir/carregar o client do cofre,
  tipicamente ~1,5–2,5 s neste Fedora. Status UX, preflight autorizativo,
  execução do provider e Summary Worker ainda podem realizar operações
  independentes. Isso fica como oportunidade de otimização futura, não como
  bloqueio da D2.

A mensagem `Couldn't get key from code: Quote` foi rastreada ao `tao 0.35.3`
no mapeamento de teclado Linux/GTK e não ao keyring/Stronghold.

Summary e Orchestrator não receberam uma repetição humana dedicada após as FIXes
de performance. Seus contratos D2 foram cobertos pela auditoria independente e
pela suíte automatizada; o fechamento foi aceito explicitamente pelo usuário com
essa distinção registrada, sem afirmar um gate humano que não ocorreu.

No HEAD final anterior ao fechamento documental, a implementação reportou
**256 testes aprovados, 0 falhas e 2 ignorados**, além de typecheck, build,
cargo check, release check e `git diff --check` em PASS. O
`cargo fmt --check` global permanece vermelho por drift histórico em arquivos
não tocados; os arquivos Rust modificados pela D2 passaram na verificação
individual.

A D2 não implementa task graph, paralelismo, ferramentas ou LR-8. O próximo
checkpoint é **LR-7D2.5 — provider redundancy + Gemini de-risking**, seguido de
LR-7D3.
