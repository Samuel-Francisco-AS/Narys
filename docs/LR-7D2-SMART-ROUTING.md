# LR-7D2 — fallback chain + Auto/score + affinity

Estado: **PASS técnico da auditoria / gate humano em andamento**. A FIX de
responsividade abaixo precisa ser revalidada no gate real. Sem PASS completo.
Base: `origin/main` confirmada após fetch em
`965c17ae9bd989d4bad746c4987926f4264d23b6`, com LR-7D1 integrada pela PR #10.
Branch dedicada: `lr-7d2-smart-routing`. Não há merge nesta entrega.

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
Não há LR-7D3, task graph, paralelismo, LR-8, grounding web/tools, novas permissões
Codex ou mudanças de identidade/memória/avatar/3D. Dois testes de integração real
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

LR-7D2 só fecha após auditoria independente e aprovação humana. LR-7D3 é o
próximo checkpoint **somente após esse fechamento**.

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
formatação desse arquivo já apresentava drift na base). O gate global permanece
com divergência preexistente em **26 arquivos intocados**.

Repetir o gate humano no início da Conversation: enviar em Preferred e Auto,
observar a animação entre envio e ProviderSelected, registrar o preflight_ms no
build debug e repetir na mesma sessão com affinity. Cancelar durante preflight:
nenhuma chamada/resposta e um único terminal cancelled. Confirmar a rota e histórico
após sucesso. **A ausência do freeze só será confirmada pelo novo gate humano**;
testes automatizados não encerram esse finding nem a LR-7D2.
