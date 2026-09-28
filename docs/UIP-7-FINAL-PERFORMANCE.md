# UIP-7 — Gate final de UI/performance

**Estado: CANDIDATA ao gate humano final em 28/09/2026.** Branch `main`; HEAD inicial `e794042647772de622de7587a669ca56efca9634`; pull fast-forward sem mudanças; worktree inicialmente limpa. Nenhum commit ou push nesta rodada. LR-7 e LR-8 não foram iniciadas.

## Ambiente e método

Fedora 44, GNOME/Wayland, Tauri 2, WebKitGTK e Mesa software (`LIBGL_ALWAYS_SOFTWARE=1`). O gate físico usou **uma instância** de `tauri dev`, inspector remoto do WebKit e janelas curtas de aproximadamente 5–16 s. `SceneDiagnostics` DEV mede frames efetivamente processados e tempo síncrono de update + render; não mede apresentação na tela nem tempo de GPU. CPU e RSS vieram da árvore do processo Tauri em `/proc`; **100% de CPU equivale a um núcleo lógico**. RSS é a soma de `VmRSS` do Tauri, WebKitNetworkProcess e WebKitWebProcess relacionados, sem descontar páginas compartilhadas. Vite e inspector ficam fora da soma. As amostras de estados diferentes não são um benchmark controlado de custo incremental.

O asset medido é a Luna atual com Idle manual: Idle e Wave têm aproximadamente **462 canais por clipe**. As medições anteriores à troca do GLB, na UIP-2, não são comparáveis diretamente. Referências pós-asset: ~165% CPU e ~659 MiB focada; ~143% sem foco.

## Configuração visual e política

| Modo | Tamanho solicitado | Tamanho efetivo nesta execução |
| --- | ---: | ---: |
| Presence | 310×410 | 310×410 |
| Composer | 310×490 | 310×490 |
| Conversation | 625×490 | 625×490 |

`CharacterStage` e canvas CSS permaneceram em **300×360**, drawing buffer em **300×360**, DPR e pixel ratio em **1**. O RenderBudget persistido iniciou em **30/24/0 FPS** (foco/sem foco/oculto). A alteração temporária para 15/10 foi recebida pela main sem reload; 30/24 foi restaurado e conferido no SQLite. O target é teto: em alguns estados sem foco, o WebKit entregou somente ~2 callbacks/s, limitação já observada na UIP-2.

## Métricas do gate dev

Valores aproximados de CPU/RSS em amostras curtas. `WebViews` conta WebKitWebProcess; há também um WebKitNetworkProcess compartilhado. `glError=0` onde consultado. A coluna FPS distingue medição efetiva de target.

| Estado | Janela efetiva | Canvas / buffer | FPS processado (target) | CPU | RSS agregado | WebViews | glError |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Presence focada | 310×410 | 300×360 / 300×360 | ~30 (30) | ~161% | ~663 MiB | 1 | 0 |
| Presence sem foco | 310×410 | 300×360 / 300×360 | ~24 (24) | ~142% | ~647 MiB | 1 | 0 |
| Composer | 310×490 | 300×360 / 300×360 | target 30/24 conforme foco | ~135% | ~655 MiB | 1 | 0 |
| Conversation | 625×490 | 300×360 / 300×360 | ~2, WebKit sem foco (24) | ~13% | ~679 MiB | 1 | 0 |
| Geral | 625×490 main | main 300×360; Geral 0 canvas | main sem foco | ~18% | ~1.029 MiB | 2 | 0 main |
| Geral + IA | 625×490 main | main 300×360; Settings 0 canvas | main sem foco | ~22% | ~1.369 MiB | 3 | 0 main |
| Após fechar Settings | 625×490 main | 300×360 / 300×360 | main sem foco | ~17% | ~717 MiB | 1 | 0 |
| Suspended por sinal controlado | 625×490 | 300×360 / 300×360 | **0** (0) | ~0,5% | ~717 MiB | 1 | 0 após restaurar |
| Após dez ciclos UI | 625×490 | 300×360 / 300×360 | ~2, WebKit sem foco (24) | ~17% | ~706 MiB | 1 | 0 |
| Conversa Gemini em andamento | 625×490 | 300×360 / 300×360 | ~24 (24) | ~205% | ~725 MiB | 1 | 0 |

Na Presence focada, as janelas diagnósticas estáveis marcaram ~29,95–30,10 FPS e tempo médio síncrono de update + render ~18–19 ms. Sem foco, antes de expandir a janela, marcaram ~23,94–24,13 FPS e ~21 ms. Após o resize para Conversation, o GNOME deixou a main sem foco e o WebKit reduziu callbacks para ~2/s; `renderMode=background`, target 24, sem erro de detecção de foco. Durante a tentativa de conversa, callbacks voltaram a permitir ~24 FPS; não houve travamento do renderer ou aumento do buffer.

## Gates funcionais e lifecycle

- **Auditoria estática:** `AvatarViewport` cria um `SceneRuntime` por montagem e o libera no cleanup; `SceneRuntime.stop()` desativa o loop, `ResizeObserver.disconnect()`, `RenderBudget.dispose()` remove listeners, `SceneDiagnostics.dispose()` limpa interval e o renderer/canvas são liberados. Atualizar FPS chama `updateRenderConfig`, sem reconstruir `SceneRuntime`. Listener Tauri de Settings e listener de teclado têm cleanup; `Composer` limpa seu interval de cooldown. Settings são importadas dinamicamente por `main.tsx`, sem App/Three, e `open_window` reutiliza a mesma label ou cria uma janela real que fecha. Não foi encontrado vazamento ou regressão concreta que justificasse alterar código.
- **Composer/Conversation:** painéis abriram, fecharam e exibiram conteúdo sem recriar Luna; tamanhos efetivos coincidiram com os solicitados nesta execução. Dez ciclos de quatro transições (40 observações) passaram por 310×410, 310×490 e 625×490, sempre com um canvas e buffer 300×360. RSS de ~663 MiB no início e ~706 MiB após os ciclos não demonstra crescimento grosseiro/monotônico; cache e variação de foco continuam possíveis.
- **Histórico:** lista com nove itens, detalhe de sessão com mensagens, volta e retomada **explícita** funcionaram. Abrir detalhe não retomou a sessão automaticamente. Não se fez teste extenso do Summary.
- **Settings:** Geral e IA tiveram zero canvas e nenhuma instância Three. Reabrir Geral não duplicou a WebKitWebProcess. Ao fechar IA e Geral, seus processos desapareceram e o RSS caiu de ~1.369 para ~717 MiB, próximo dos ~706 MiB anteriores à abertura.
- **RenderBudget/WebGL/avatar:** configuração 15/10 persistiu e chegou ao diagnóstico da main sem reload; 30/24 foi restaurado. Um sinal `visibilityState=hidden` temporário, inserido apenas no documento pelo inspector, produziu zero frames processados e callbacks descartados como suspended; ao restaurar o sinal, Idle retomou, buffer ficou 300×360 e `glError=0`. Isso **não prova** que o compositor emite `hidden` ao minimizar fisicamente. Wave foi acionado por hit test e voltou ao Idle; abrir/fechar UI/Settings não apresentou reset do mixer. Escala/câmera do avatar não foram alteradas.
- **Conversa:** houve **uma** tentativa curta. A UI entrou em streaming, o avatar continuou renderizando e o canvas permaneceu fixo. A API terminou em 429; o Composer mostrou cooldown com contador, bloqueou Enviar, restaurou o rascunho e não persistiu uma resposta incompleta. Nenhuma chamada adicional foi feita. Assim, persistência de uma resposta bem-sucedida não foi revalidada nesta UIP-7; já havia PASS na LR-6.
- **Logs:** sem panic, erro SQLite/migration, perda de contexto WebGL, loop de resize ou erro de listener observado. Avisos Rust de dead code e aviso Vite de chunk >500 kB já conhecidos permaneceram; não foram silenciados.

## Gates de código, persistência e segurança

`npm run typecheck`, `npm run build`, `cargo check`, `cargo test` (**80 testes**), `cargo check --release`, teste puro existente `node --experimental-strip-types tests/render-budget.mjs` e `git diff --check`: **PASS**. `cargo fmt --check` falha pelo drift global histórico; não houve alteração de arquivo Rust e não foi executado `cargo fmt` global. O banco local retornou `PRAGMA user_version=5`, `integrity_check=ok` e `foreign_key_check` vazio. Nenhuma migration foi criada.

As capabilities de Geral e IA não incluem shell/filesystem. Geral só grava preferências; IA recebe comandos de policy/credencial, mas `get_ai_settings` retorna estado, não valor da chave. A main não ganhou capability de gravar a API key. A capability estática da main ainda lista comandos diagnósticos de segredo LR-3, já conhecidos, cujos handlers não são registrados em release. A chave Gemini continua no SecretStore fora de SQLite/localStorage; o payload mantém `store:false`. Isolamento de sessão/histórico foi rechecado pelo fluxo de detalhe e retomada explícita, além dos testes Rust. Esta foi uma checagem curta de regressão, não nova auditoria LR-3.

## Release local

`npm run tauri build` compilou o binário otimizado e gerou pacotes `.deb` e `.rpm`, mas retornou código 1 na etapa **AppImage**, com `failed to run linuxdeploy`. A repetição direcionada `npm run tauri build -- --bundles deb,rpm` passou e confirmou os dois pacotes locais. A falha de packaging AppImage não levou a mudança de configuração ou arquitetura. O binário local foi executado com Mesa software e inspector WebKit apenas para esta medição. Em Presence release, janela 310×410, um canvas e buffer 300×360, DPR 1 e `glError=0`. Um contador temporário de chamadas `gl.clear` no inspector observou **479 clears em 15,95 s (~30,03/s)**; o método original foi restaurado após a amostra. Isso indica cadência de render próxima a 30 FPS, sem adicionar telemetria ao produto.

| Release local | CPU aproximada | RSS agregado | WebViews | Comparação dev |
| --- | ---: | ---: | ---: | --- |
| Presence focada | ~142–146% | ~564–566 MiB | 1 | ~100 MiB abaixo da Presence dev desta rodada |
| + Geral | ~120% | ~870 MiB | 2 | ~+306 MiB frente à main release |
| + Geral + IA | ~85% | ~1.181 MiB | 3 | ~+311 MiB adicionais para IA |
| Após fechar Settings | ~11% | ~590 MiB | 1 | processos das Settings desapareceram; ~+26 MiB sobre a main inicial |

As amostras de CPU das Settings foram feitas com a main sem foco e não servem para calcular custo isolado de cada janela. O custo de memória de cada WebView permanece próximo de ~300 MiB mesmo em release neste ambiente; a base da main caiu. Não houve tentativa de usar o pacote AppImage incompleto.

## Limitações de plataforma e dívidas adiadas

- GNOME/Wayland pode ignorar always-on-top e resize, além de produzir spring/deslocamento espacial nos painéis. Nesta execução os tamanhos se materializaram, mas isso não encerra a dívida. Click-through continua bloqueado sem recuperação externa segura. Minimização física não foi controlada neste gate; o teste suspended acima usou sinal sintético.
- Cada Settings WebView adiciona custo grande no WebKitGTK dev. O fechamento liberou processos; não se reestruturou Settings. Software Mesa continua necessário nesta máquina.
- Na UIP-6 foi observado que `SummaryWorker` pode chamar Gemini antes da primeira mensagem manual e causar cooldown compartilhado. Foreground já tem prioridade quando existe; `Retry-After` é respeitado, cooldown impede novas chamadas e o Composer mostra tempo restante. Faltam reserva de capacidade para foreground, prioridade de quota, provider separado para Summary, multi-provider e queue/rate manager. Essa política fica para LR-7/LR-8 ou rodada dedicada de estabilidade; nenhum código SummaryWorker foi alterado aqui.
- Idle e Wave têm ~462 canais por clipe; otimização do asset/exportação fica para trabalho artístico/performance próprio. O drift global de rustfmt permanece.
- Depois da aprovação humana da UIP-7, LR-7 deverá provar um segundo provider real. Groq e Mistral são candidatos documentados; a decisão dependerá de pesquisa/API real na LR-7. Nenhum provider foi escolhido ou implementado nesta rodada.

## Veredito e gate humano

**UIP-7 = CANDIDATA.** Os gates técnicos mínimos passaram, exceto o `cargo fmt --check` global já conhecido; não houve blocker novo de canvas, memória, WebGL, RenderBudget, Settings ou banco. UIP-0 → UIP-6 permanecem PASS; LR-7 ainda não começou. Fechar a trilha como PASS depende de Sam.

1. Presence → Composer → Conversation → Presence; conferir transições e tamanho efetivo.
2. Confirmar que Luna mantém escala e qualidade visual.
3. Abrir/fechar Geral e IA.
4. Mudar FPS e restaurar 30/24.
5. Acionar Wave e confirmar retorno ao Idle.
6. Minimizar/restaurar e confirmar que Luna continua funcionando.
