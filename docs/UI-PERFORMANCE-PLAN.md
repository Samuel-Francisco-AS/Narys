# Plano de UI e Performance — UIP

> Planejamento aprovado em 26/09/2026 após o fechamento da LR-6.
>
> Esta trilha é um interlúdio deliberado antes da LR-7. Ela transforma o protótipo técnico em uma presença desktop utilizável sem acoplar UI, conversa, Luna Core e Avatar Runtime. O objetivo não é "embelezar um chat", e sim fazer da própria Luna o centro da aplicação, com a interface aparecendo ao redor dela somente quando necessária.

## 1. Estado de partida

LR-6 está fechada com PASS completo no Fedora: Gemini real, streaming, usage, cancelamento, persistência local e credencial protegida foram validados.

O frontend atual ainda é a casca visual do protótipo M0-B:

- `App.tsx` mantém topbar, painéis e controles diagnósticos;
- a janela Tauri atual é decorada, opaca e grande (`1120x760`);
- `SceneRuntime` usa `renderer.setAnimationLoop()` diretamente;
- o renderer limita `devicePixelRatio` a 1.5;
- um `ResizeObserver` redimensiona canvas/câmera conforme o container;
- a UI diagnóstica LR-2/LR-4/LR-5/LR-6 ainda divide espaço com a personagem.

Nenhuma métrica de performance deve ser presumida antes da UIP-0.

## 2. Visão de produto

A Luna deve funcionar como companhia inteligente persistente no desktop.

Princípio visual:

> **A personagem é o aplicativo. A UI é acessória.**

O estado mais importante é o de presença: Luna visível sobre o desktop, em uma janela pequena, transparente e sem bordas, sem ocupar uma aba inteira nem impedir o uso normal do computador.

Estados conceituais da interface:

~~~text
PRESENCE
Luna + acionador mínimo

        ↓ abrir compositor

COMPOSER
Luna + caixa de texto + controles essenciais

        ↓ conversar / abrir painel

CONVERSATION
painel lateral de sessão + Luna + compositor
~~~

Recolher UI não encerra a sessão atual.

## 3. Fronteiras arquiteturais

Manter quatro sistemas independentes que se comunicam por contratos:

~~~text
                    LUNA CORE
            cognition / tasks / memory
                     ↕ events
                       │
        ┌──────────────┼──────────────┐
        │              │              │
        ▼              ▼              ▼
 Conversation      Window/UI       Avatar
   Runtime         Controller      Runtime
 sessions          modes           Three.js
 history           shortcuts       animation
 summaries         windows         behavior
        │                              ▲
        │                              │
        └────── future Behavior Bus ───┘
                        ▲
                        │
               Desktop Context
              apps / ambiente / etc.
                    [futuro]
~~~

Regras:

- React/UI não vira dona da memória, tarefas ou credenciais;
- Conversation Runtime não conhece detalhes de Three.js;
- Avatar Runtime não conhece providers ou sessões;
- Luna Core não conhece layout, posição visual ou detalhes do renderer;
- mudanças artísticas não devem exigir alteração no Core;
- futuras reações ao ambiente entram por eventos/intenções, não por regras enterradas no renderer.

## 4. Decisões de UX já fechadas

### Janela principal

- pequena em relação ao monitor, aproximadamente na escala de um quadrante do desenho conceitual;
- fundo transparente;
- sem borda/decoração visual no modo normal;
- personagem como elemento central;
- tamanho e posição ajustáveis futuramente e persistíveis;
- `always-on-top` configurável;
- modo normal em que outras janelas podem cobrir a Luna;
- modo click-through configurável;
- recuperação segura do click-through por atalho global/tray antes de torná-lo utilizável;
- reposicionamento por região dedicada e/ou comando como `Alt + arrastar`;
- a região da personagem deve continuar disponível para interações futuras.

### Compositor

- pode desaparecer completamente;
- quando recolhido, resta apenas um pequeno acionador abaixo da área 3D;
- pode ser aberto por clique ou atalho;
- não encerra nem troca a sessão ao ser recolhido.

### Painel de conversa

- não é permanente;
- abre à esquerda da personagem;
- fecha por botão dedicado e/ou atalho;
- abrir/fechar não altera a dimensão de render do CharacterStage;
- o painel deve crescer para a esquerda sem deslocar visualmente a Luna na tela sempre que a plataforma permitir esse comportamento de forma estável.

### Configurações

Duas superfícies independentes da janela principal:

1. configurações gerais;
2. IA/modelos/providers/credenciais/logins/SDKs.

Devem usar janelas Tauri separadas e capabilities próprias.

## 5. Sessões de conversa

A conversa não será um histórico infinito enviado à LLM.

### Ciclo de vida

- cada execução do aplicativo inicia uma sessão de conversa nova;
- fechar/recolher compositor ou painel mantém a sessão;
- `Nova conversa` encerra a sessão atual e cria outra;
- sessões anteriores permanecem no SQLite;
- sessões antigas não entram automaticamente no contexto da sessão atual;
- histórico permite visualizar sessões;
- retomar uma sessão antiga deve ser ação explícita.

### Contexto cognitivo

O Context Builder deve enviar apenas o contexto necessário para a tarefa/conversa atual. Histórico bruto completo não deve ser reenviado por padrão.

## 6. Resumo assíncrono de sessão

Ao encerrar uma sessão, ela pode receber estado de resumo pendente.

Fluxo desejado:

~~~text
encerrar sessão
→ liberar imediatamente nova conversa
→ registrar summary_pending
→ tarefa de resumo em segundo plano
→ persistir título/resumo/metadados
~~~

Regras:

- nunca bloquear `Nova conversa` esperando uma LLM;
- se o app fechar antes do resumo, o trabalho pode permanecer pendente para retomada posterior;
- o modelo de resumo não é obrigatoriamente o modelo principal;
- roteamento deve ser por papel cognitivo configurável (`conversation`, `summary`, futuramente `voice`, `worker` etc.);
- provider/modelo não devem ser hardcoded ao papel;
- ausência de modelo de resumo não quebra a sessão nem o histórico;
- **resumo de sessão não é memória persistente da Luna**.

A extração inteligente de memória semântica/episódica/relacional entre sessões permanece como subsistema futuro.

## 7. Performance como requisito de arquitetura

A Luna deve poder permanecer visível enquanto o usuário programa, estuda, navega ou executa outras tarefas.

Baseline pretendido:

- **30 FPS como teto padrão** para presença, idle e interação normal;
- perfis reduzidos para ausência de foco/visibilidade;
- suspensão ou quase suspensão quando minimizada/oculta, conforme o comportamento real da plataforma;
- valores reduzidos não são fixados antes da medição UIP-0/UIP-2.

Não perseguir 60 FPS por princípio.

### Regras duras

- o CharacterStage possui dimensão de render controlada;
- abrir conversa não aumenta a resolução do canvas;
- resize da janela não deve gerar resize contínuo desnecessário do renderer;
- evitar `backdrop-filter`, blur contínuo, sombras caras e efeitos visuais sem benefício mensurável;
- medir antes de otimizar;
- não criar gate obrigatório de horas nesta trilha;
- testes longos virão do uso cotidiano quando a Luna estiver viável como companhia.

### Métricas mínimas

- FPS efetivo;
- frame time;
- CPU do processo;
- RAM;
- dimensão física/lógica do canvas;
- `devicePixelRatio`;
- estado de foco/visibilidade;
- regressão de contexto WebGL;
- crescimento de memória em ciclos repetidos de abrir/fechar UI.

## 8. UIP-0 — contratos + baseline

**Estado:** **PASS completo em 26/09/2026.** Contratos, instrumentação DEV e baseline foram implementados; typecheck/build/diff-check passaram; Tauri/WebGL permaneceram funcionais; a validação humana confirmou minimizar/restaurar, resize manual e Idle/aceno sem regressão perceptível. Consulte [UIP-0-BASELINE.md](UIP-0-BASELINE.md).

**Objetivo:** formalizar estados, fronteiras e capturar a linha de base antes da transformação visual.

### Trabalho

- documentar estados `presence`, `composer`, `conversation`;
- definir contratos entre UI, Conversation Runtime, Luna Core e Avatar Runtime;
- definir estados de janela: normal, always-on-top, click-through;
- definir política inicial de render em 30 FPS;
- registrar dimensões atuais da janela/canvas;
- instrumentar ou coletar FPS, frame time, CPU e RAM;
- registrar comportamento atual de resize/foco;
- validar que a instrumentação não altera significativamente o runtime.

### Gate

- baseline reproduzível e registrado;
- contratos não exigem dependência circular;
- nenhuma mudança artística grande;
- typecheck/build permanecem verdes.

## 9. UIP-1 — Presence Shell

**Estado:** **PASS completo em 26/09/2026.** A Presence Shell transparente e sem bordas foi validada no Fedora/Wayland; após FIX-1 e FIX-2, a escala da Luna, barra inferior, shell DEV responsivo e compactação da janela foram aprovados pelo usuário. A janela final ficou em 320×420 e o CharacterStage/drawing buffer em 300×360, preservando a escala visual da personagem. Consulte [UIP-1-PRESENCE.md](UIP-1-PRESENCE.md).

**Objetivo:** substituir a casca M0-B pela presença desktop mínima.

### Trabalho

- janela principal transparente;
- sem decoração/borda;
- retirar topbar e superfícies diagnósticas do fluxo visual normal;
- CharacterStage com tamanho próprio;
- manter personagem interativa;
- acionador mínimo inferior;
- preservar acesso de desenvolvimento aos diagnósticos sem deixá-los dominar a UI de produto.

### Gate humano

Ao abrir a aplicação, a Luna deve parecer estar diretamente sobre o desktop, e não dentro de uma janela de aplicativo tradicional.

### Gate técnico

- Tauri abre no Fedora;
- transparência funcional;
- WebGL/Idle funcionam;
- interação existente não regride;
- sem resize patológico do canvas.

## 10. UIP-2 — Render Budget + primeiro gate de performance

**Objetivo:** limitar o custo do avatar antes de expandir a UI.

### Trabalho

- substituir render irrestrito pelo teto de 30 FPS;
- avaliar `devicePixelRatio=1.0` versus valores maiores no hardware real;
- implementar perfis de render de acordo com foco/visibilidade onde confiável;
- impedir trabalho de render desnecessário quando minimizada/oculta;
- medir impacto de idle e animações;
- registrar comparação com UIP-0.

### Gate

Testes curtos e reproduzíveis, sem requisito de horas:

- presença/idle;
- interação/animação;
- sem foco;
- minimizar/restaurar.

Sem perda de contexto WebGL e sem regressão visual relevante.

## 11. UIP-3 — ergonomia da janela

**Objetivo:** permitir convivência real com outras aplicações.

### Trabalho

- alternar `always-on-top`;
- modo normal;
- reposicionamento por região dedicada e/ou `Alt + arrastar`;
- atalho global/tray de recuperação;
- click-through completo somente após existir recuperação segura;
- persistir posição/modo quando isso puder ser feito sem complicar a primeira entrega.

### Nota

Não assumir que a API da plataforma permite click-through somente em pixels transparentes. A primeira implementação pode ser um modo de passagem de clique para a janela inteira.

### Gate

- usuário consegue alternar sobreposição;
- consegue reposicionar a Luna;
- consegue entrar e sair de click-through sem ficar preso;
- outras aplicações continuam utilizáveis normalmente.

## 12. UIP-4 — Composer + painel de conversa + sessão atual

**Objetivo:** transformar o chat LR-6 em interface de produto.

### Trabalho

- compositor recolhível;
- atalho de abrir/fechar;
- envio real reutilizando streaming/cancelamento/usage da LR-6;
- painel lateral esquerdo da sessão atual;
- `Nova conversa`;
- fechamento de painel sem destruir sessão;
- nova execução do app inicia sessão limpa;
- mensagens persistem localmente para histórico.

### Regra de layout

Abrir o painel não aumenta a dimensão de render do CharacterStage. A janela pode crescer para a esquerda; o canvas continua com orçamento fixo.

### Gate

- conversa Gemini real funciona pela nova UI;
- streaming permanece incremental;
- cancelar continua funcional;
- abrir/fechar painel não altera FPS/canvas de forma indevida;
- `Nova conversa` cria sessão distinta.

## 13. UIP-5 — histórico + resumo assíncrono

**Objetivo:** organizar continuidade sem criar prompt infinito.

### Trabalho

- listar sessões por data/título;
- abrir sessão em modo de visualização;
- retomada explícita;
- `summary_pending` ou equivalente;
- worker/tarefa assíncrona de resumo;
- roteamento de resumo por papel cognitivo;
- persistência de resumo e metadados;
- falha de resumo não corrompe nem bloqueia sessão.

### Gate

- sessões não se misturam;
- sessão antiga não entra no prompt atual sem ação explícita;
- nova conversa abre imediatamente mesmo com resumo pendente;
- fechamento/reabertura preserva histórico e estado de resumo.

## 14. UIP-6 — janelas independentes de configuração

**Objetivo:** retirar configuração operacional da superfície de presença.

### Janela geral

- always-on-top;
- click-through;
- atalhos;
- comportamento da janela;
- preferências de performance;
- futuras opções de presença.

### Janela IA/modelos

- providers;
- credenciais;
- modelos;
- papéis cognitivos;
- login/SDKs quando existirem;
- sem inventar funcionalidades ainda não sustentadas pelo Luna Core.

### Segurança

- capabilities Tauri por janela/feature;
- segredos continuam no Rust/SecretStore;
- React nunca recebe valor de API key persistida.

### Gate

- janelas abrem/fecham independentemente;
- janela principal não carrega UI de configuração desnecessária;
- permissões são revisadas por superfície.

## 15. UIP-7 — consolidação + segundo gate de performance

**Objetivo:** fechar a trilha com a interface funcional e custo conhecido.

### Cenários curtos

- Luna sozinha;
- compositor aberto;
- conversa + streaming;
- painel abrindo/fechando repetidamente;
- always-on-top;
- modo normal;
- sem foco;
- minimizar/restaurar;
- ciclos repetidos para observar crescimento de RAM e resize indevido.

### Gate

- 30 FPS continua sendo teto normal;
- canvas não cresce ao abrir conversa;
- nenhuma regressão evidente de WebGL;
- ausência de crescimento de memória grosseiro em ciclos curtos;
- UI permanece responsiva durante streaming;
- baseline e resultado final ficam documentados.

Após esse gate, o uso cotidiano passa a ser o teste de endurance real.

## 16. Trabalho explicitamente posterior

Não faz parte do fechamento UIP:

- segundo provider real / LR-7;
- Rate Limit Manager completo / LR-8;
- memória semântica avançada;
- embeddings/vector DB;
- desktop context awareness;
- comportamento contextual com props/cenários;
- mesa/cadeira/computador e animações de `coding_mode`;
- detecção ampla de aplicativo ativo;
- Android;
- mensageiros.

## 17. Reações futuras ao ambiente

A arquitetura deve preservar desde já um caminho desacoplado:

~~~text
DesktopContextEvent
    ↓
Behavior Engine
    ↓
BehaviorIntent
    ↓
Avatar Runtime
    ↓
animação / expressão / prop / cenário
~~~

Exemplo futuro:

~~~text
VS Code ativo
→ coding_mode
→ reação de entrada
→ cadeira/mesa/PC
→ coding_idle
~~~

Isso não deve exigir chamada de LLM para cada evento. Regras determinísticas são preferíveis quando suficientes; LLM entra apenas quando interpretação ou escolha contextual justificar custo/latência.

## 18. Sequência e relação com LR

Ordem acordada:

~~~text
LR-6 PASS
   ↓
UIP-0
UIP-1
UIP-2
UIP-3
UIP-4
UIP-5
UIP-6
UIP-7
   ↓
LR-7
~~~

A trilha UIP não renumera o roadmap cognitivo LR. Ela é uma intervenção de produto e performance antes de aumentar complexidade de providers.

## 19. Próxima ação

Iniciar **UIP-2 — Render Budget**.

A próxima rodada deve partir do baseline final da UIP-1 (janela 320×420, stage/buffer 300×360, ~55 FPS e ~200,8% de CPU agregada na amostra curta) e medir separadamente o efeito do teto de 30 FPS e dos perfis de foco/visibilidade, sem antecipar ergonomia/UIP-3.
