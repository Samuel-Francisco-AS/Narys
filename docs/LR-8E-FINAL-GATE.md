# LR-8E — checklist do gate humano final

**FECHAMENTO: PASS técnico + auditoria independente + gate final aprovado em 05/10/2026**

Este documento nasceu como checklist do gate e agora preserva também o resultado final. A–D foram validados com evidência humana real; E–L foram substituídos por bateria automatizada local determinística auditada independentemente pela Luna, sem falsificar execução humana; M recebeu aprovação humana final do usuário. O fechamento da LR-8 foi autorizado em 05/10/2026.

Preencher cada resultado com PASS ou FAIL e observação/evidência. Quando um
estado transitório não puder ser observado em 1 s, registrar a limitação e
correlacionar com os contadores/suites; não inventar resultado. Dados coletados
não devem conter credenciais, Account ID, texto de conversa, corpo/headers HTTP,
respostas ou reasoning. Identificadores locais de tarefa/provider/modelo e
contadores são suficientes.

| Identificação | Preenchimento humano |
|---|---|
| Data / usuário / auditor Luna | |
| Branch / HEAD auditado / FIXes | |
| Evidência do PASS técnico e da auditoria | |
| Ambiente / versão do app | |
| Providers / modelos / roles utilizados | |
| Policy local anterior e desejada ao final (sem secrets) | |

## A. Baseline

Preferência atual: **Groq + Cloudflare**, dois Cognitive Providers independentes
configurados e autorizados. Mistral pode representar um 429 natural se essa
condição continuar existindo; não é requisito único nem motivo para gerar erros.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| App inicia normalmente; Settings IA e painel carregam | | |
| Dois providers independentes configurados | | |
| Poll chama somente `get_provider_operational_snapshot`; nenhum polling de `get_ai_settings`, SecretStore ou SQLite | | |
| Nenhum canvas/WebGL novo em Settings IA; nenhum request de provider causado pelo painel | | |
| Poll pausado hidden/fechado; retoma visible; Atualizar agora funciona sem overlap | | |
| Falha de refresh conserva captura anterior e indica dados desatualizados | | |

## B. Observabilidade idle

Repetir para cada provider, incluindo os desabilitados/não configurados.
Closed não significa saudável. A disponibilidade é para tentativa e não garante
sucesso remoto. Quotas Model permanecem por modelo.

| Provider | Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|---|
| | configured/enabled coerentes com Settings | | |
| | circuit Closed e cooldown operacional coerentes | | |
| | active calls, cap local, queue depth/capacity/classes | | |
| | requests e usage desde o início do runtime; reportingRequests e medição parcial | | |
| | quota limit/remaining/reset/provenance independentes; unknown explícito | | |
| | constraints/source/provenance/accounting separados da quota factual | | |
| | custo Desconhecido / sem contrato configurado | | |

## C. Foreground real

Iniciar uma Conversation ou diagnóstico real controlado com rota conhecida.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Active call aparece quando a duração permitir observação | | |
| Requests factuais aumentam somente após início HTTP | | |
| Usage aparece apenas nas dimensões reportadas; ausência continua unknown | | |
| Último outcome/recência coerentes; nenhum secret em snapshot/painel/logs novos | | |

## D. Concorrência real

Combinar de forma controlada TaskGraph, Conversation e Groq probe. Quando
possível, ocupar o cap default 2 de um mesmo provider e iniciar uma terceira
operação nele. Não modificar concurrency ou fairness para facilitar o gate.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Mesmo provider mostra activeCalls=2 e queueDepth>=1 | | |
| Classe da terceira chamada visível; admission posterior e queue delay coerentes | | |
| Peers executando não sofrem preemption; fila/active voltam ao estado esperado | | |

> E–L possuem evidência automatizada complementar em
> [LR-8E-AUTOMATED-GATE-E-L.md](LR-8E-AUTOMATED-GATE-E-L.md).
> Essa evidência não preenche os campos humanos abaixo nem representa execução
> humana E–L pelo usuário.

## E. Cancelamento durante queue

Colocar uma tarefa em fila e cancelar antes de HTTP. Registrar contador factual
antes/depois e descontar chamadas independentes identificadas no mesmo intervalo.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Queue depth retorna após cancelamento | | |
| A tentativa cancelada não acrescenta request factual | | |
| Nenhum reservation, permit ou probe leak | | |

## F. Cancelamento durante request

Iniciar request real e cancelar após início HTTP.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Active calls retorna a zero ao terminar todas as chamadas controladas | | |
| Request iniciada permanece contabilizada factual/localmente | | |
| Nenhum retry/fallback após cancelamento; guards liberados | | |

## G. Foreground + background

Usar uma sessão com Summary configurado, fechando/retomando pelo fluxo atual.
Não alterar UIP-6C: Summary oportunista pode aguardar foreground; Summary já
iniciado não é interrompido.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Summary/background concorre ou aguarda conforme contrato existente | | |
| Classe Background/fila/usage/outcome coerentes quando observáveis | | |
| Nenhum polling extra de background ou preemption | | |

## H. Capacidade / limite

Escolher **uma** rota suficiente: condição factual natural (429/Retry-After) ou
RatePolicy local explicitamente configurada pelo usuário. Não provocar erro
comercial desnecessário. Se usar policy local, copiar a configuração anterior,
mostrar scope/dimensão/capacity/window/anchor UTC explicitamente e usar um limite
pequeno de requests em uma janela segura. Token budget sem bound comprovado não
é prova de bloqueio pré-HTTP dos adapters atuais.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Rota escolhida e estado anterior registrados | | |
| Chamada real controlada estabelece consumo conhecido | | |
| Limite local bloqueia nova tentativa antes de HTTP; request factual não aumenta | | |
| RateCapacityExceeded/DailyBudgetExceeded não causam fallback automático (correto) | | |
| Se 429 natural: Retry-After factual e cooldown operacional distintos | | |
| Policy anterior restaurada ao final; consumed não é zerado por mudança de capacity na mesma janela | | |

## I. Fallback / cooldown

Separadamente, validar somente com erro remoto elegível **antes de qualquer
output**, usando Preferred/Auto e targets explicitamente autorizados. Exemplos:
RateLimited, Unavailable, Timeout. Fixed não pode usar fallback. Não interpretar
bloqueio local como erro remoto elegível.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Origem/destino/motivo remoto de fallback coerentes | | |
| Cooldown respeitado sem confundir com último Retry-After observado | | |
| Fixed preservado; sem fallback após output ou cancelamento | | |

## J. Circuit breaker

Não tentar derrubar provider comercial nem provocar três falhas artificiais.
Aceitar estado natural ou evidência determinística das suites LR-8D e operational.
O painel está preparado para Open/HalfOpen; expiry não agenda probe e pode deixar
Open com remaining 0 até uma próxima autorização.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Fonte de evidência escolhida: natural ou suite determinística | | |
| Open/HalfOpen/probes/remaining/threshold/transition reason corretamente apresentados | | |
| Nenhuma UI para forçar Closed ou remover cooldown | | |

## K. TaskGraph

Usar dois workers independentes, preferencialmente Groq + Cloudflare, com targets
compatíveis e budget de output explícito adequado ao cenário.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Duas subtarefas independentes executadas | | |
| Provenance/provider real por subtarefa preservados no resultado/persistência | | |
| Calls/queue por provider coerentes com a execução | | |
| Consolidação determinística preservada sem regressão | | |

## L. Restart / persistência

Usar RatePolicy local conhecida e segura, com janela suficientemente longa para
não expirar durante o restart. Registrar policy/consumed/reserved/uncertainty
antes de encerrar. Limitações conservadoras de crash e accounting incompleto
seguem LR-8C; não contornar uncertainty ou fail-closed pelo painel.

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Budget/request accounting conhecido estabelecido | | |
| Encerrar/reabrir: health transient Closed/sem cooldown | | |
| DailyBudget/local windows/uncertainty persistidos permanecem | | |
| Nova chamada não ultrapassa limite conhecido pré-HTTP | | |
| Policy final restaurada ao estado desejado pelo usuário | | |

## M. Regressões e decisão humana

| Verificação | PASS/FAIL | Observação/evidência |
|---|---|---|
| Conversation streaming e cancellation | | |
| Summary oportunista e deferral | | |
| Orchestrator / PlanV1 | | |
| Worker / TaskGraph / consolidation / provenance | | |
| Settings, credenciais e rotação legítima | | |
| Responsividade/largura/foco/teclado/legibilidade do painel no hardware alvo | | |
| CPU/RAM sem pressão perceptível; fechar libera lifecycle do painel | | |
| Auditoria independente da Luna e FIXes: resultado humano | | |
| Gate humano real: resultado humano | | |
| Decisão de fechamento posterior da LR-8 pelo usuário + Luna | | |

Nenhum campo acima está pré-aprovado. PASS técnico da candidata, auditoria e gate
humano são evidências distintas. LR-8 permanece aberta até a decisão final.


## Resultado final consolidado

Data de fechamento: **05/10/2026**.

- **A — Baseline:** PASS humano.
- **B — Observabilidade idle:** PASS humano.
- **C — Foreground real:** PASS humano.
- **D — Concorrência real:** PASS humano, incluindo observação no hardware alvo de `activeCalls=2/2`, `queueDepth=1/64` e drenagem ordenada até `0/2`.
- **E–L:** PASS por bateria automatizada local determinística registrada em [LR-8E-AUTOMATED-GATE-E-L.md](LR-8E-AUTOMATED-GATE-E-L.md) e reauditoria independente da Luna. Os campos humanos desses blocos permanecem historicamente vazios porque não foram executados manualmente.
- **M — Regressões e decisão humana:** PASS humano. O usuário considerou o comportamento geral aceitável e autorizou o fechamento padrão da LR-8.

A auditoria independente da bateria E–L confirmou que os testes cruzam as fronteiras que afirmam provar: queue cancellation pré-HTTP, cancellation após HTTP loopback, fairness foreground/background, rate block local terminal, fallback/cooldown, circuit breaker integrado, TaskGraph real com PlanV1/workers/SQLite e restart sobre o mesmo SQLite. A instrumentação adicional em Scheduler existe somente sob `#[cfg(test)]`.

O gate final não revelou blocker de produção. Permanecem dívidas não bloqueantes já registradas, incluindo a ergonomia da superfície longa de **IA e modelos**, cuja reorganização futura foi documentada separadamente sem congelar solução de UI.

**Decisão final: LR-8 autorizada para integração e fechamento.**
