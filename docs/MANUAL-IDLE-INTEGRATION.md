# Integração da Idle manual — checkpoint entre UIP-2 e UIP-3

Data: **26/09/2026** (America/Fortaleza).

## Cronologia

Esta mudança foi integrada deliberadamente **depois do fechamento da UIP-2/FIX-1 e antes do início da UIP-3**.

Referências de ordem no histórico de `main`:

- `c03be1f` — `UIP-2-FIX`, elevando o modo background de 15 para 24 FPS;
- `f8d1ca4` / `2f5c279` / `1086104` / `fd16111` — fechamento humano e atualização documental da UIP-2;
- `862b445` — `Manual Idle Integration`, substituição do GLB usado pelo app pela exportação Blender com a nova Idle;
- **UIP-3 ainda não havia sido iniciada** no momento desta integração.

Essa ordem é importante: qualquer medição executada na UIP-3 ou depois ocorre com um asset de avatar diferente daquele usado nas medições numéricas da UIP-0/UIP-1/UIP-2.

## Resultado artístico

A Idle procedural de M0-B1 foi substituída por uma animação manual criada por Sam no Blender 3.3.21 durante a formação prática de animação.

A animação manual foi construída com poses, timing, holds, interpolação Bézier e movimentos em tempos diferentes entre tronco, cabeça, braço, antebraço e mão. O objetivo artístico foi sair do repouso quase estático anterior e obter uma presença corporal visivelmente viva.

Validação humana no Tauri após a substituição:

- aparência geral da Luna preservada;
- nova Idle tocando continuamente;
- movimento percebido como uma melhora clara sobre a Idle anterior;
- `Wave` legado preservado para compatibilidade e retorno à nova Idle;
- runtime TypeScript não precisou mudar para a integração.

M0-B continua aberto porque o `Wave` legado ainda não recebeu a mesma revisão manual e a Idle atual ainda é uma primeira candidata, não uma animação final de produção.

## Contrato legado atual

O runtime ainda usa `LegacyGlbAdapter` + `GLTFLoader` e exige no mesmo arquivo:

- `public/models/Luna.glb`;
- um clipe chamado exatamente `Idle`;
- um clipe chamado exatamente `Wave`.

A direção futura continua sendo VRM 1.0 + VRMA separado. Esta integração é um checkpoint útil do caminho GLB existente, não reversão da arquitetura planejada.

## Mudança material do asset e risco de performance

O GLB exportado do Blender é significativamente diferente do M0-B1 gerado por Python.

| Métrica | M0-B1 histórico | Idle manual integrada |
| --- | ---: | ---: |
| Tamanho do GLB | 2.490.812 bytes | **4.206.852 bytes** |
| Variação de tamanho | — | **+68,9%** |
| Idle | 6 s, 181 amostras | **~10,04 s** |
| Wave | 3,6 s, 109 amostras | **~3,58 s** |
| Canais por clipe | 17 | **462** |

A exportação Blender atual assou transforms de grande parte do armature, resultando em 462 canais por clipe. Esse aumento pode alterar:

- custo de `AnimationMixer.update()`;
- tempo de carregamento/parse do GLB;
- memória associada às tracks;
- custo síncrono observado em `update+render`;
- comportamento de CPU nas políticas 30/24/0 FPS.

Portanto:

> **as métricas numéricas da UIP-0/UIP-1/UIP-2 são baseline do asset anterior.**

O PASS funcional da UIP-2 continua válido para a política de Render Budget, mas comparações de CPU, RSS e `update+render` com UIP-3+ precisam identificar explicitamente o novo asset. Antes de atribuir regressão ou melhora à ergonomia da janela, registrar uma nova amostra com a Idle manual e a política 30/24/0 já existente.

O canal-count elevado é um item de otimização futura. Não otimizar cegamente durante a UIP-3 se isso misturar duas variáveis; preferir medir primeiro e, se necessário, fazer uma rodada separada de export/asset optimization.

## Pipeline após esta integração

`scripts/prepare_luna.py` deixa de ser publicador de `public/models/Luna.glb`.

Seu novo papel é **bootstrap legado**:

```text
assets/luna/base.vrm
        ↓
scripts/prepare_luna.py
        ↓
assets/luna/generated/Luna-bootstrap.glb
        ↓
Blender (fonte artística)
        ↓
export GLB
        ↓
public/models/Luna.glb
```

Os clipes procedurais antigos permanecem dentro do bootstrap apenas como referência/preview; não são mais a fonte de verdade das animações distribuídas.

Antes de aceitar uma nova exportação:

```bash
python scripts/validate_luna_glb.py
```

O validador exige `Idle` e `Wave` únicos, reporta duração/canais/tamanho e avisa quando o asset se afasta materialmente do baseline M0-B1.

O backup `public/models/Luna-before-manual-idle.glb` foi removido da árvore atual: o Git já preserva a versão anterior no histórico, evitando manter um segundo binário grande no runtime tree.

## Reprodutibilidade e pendência

A animação manual foi criada em arquivo `.blend` de trabalho local. Enquanto esse source artístico não estiver versionado ou migrado para o pipeline VRM/VRMA, **não alegar reconstrução 100% automatizada do GLB final a partir do repositório**.

Próximos passos artísticos:

1. preservar a Idle manual como baseline visual;
2. criar/refinar o `Wave` manual no Blender;
3. experimentar o Gate VRM/VRMA sem bloquear a trilha UIP;
4. depois avaliar redução de canais/otimização de export em rodada própria.


## Dívida conhecida — brincos após exportação Blender

A micro-investigação de 27/09/2026 demonstrou que o deslocamento visual dos brincos já existe no **GLB exportado**, antes do runtime Three.js.

No GLB histórico, as posições locais dos dois brincos eram aproximadamente:

- `Luna_Ear_-1`: X = **-0,083**;
- `Luna_Ear_1`: X = **+0,083**.

No GLB atual exportado do Blender:

- `Luna_Ear_-1`: X = **-0,138665**;
- `Luna_Ear_1`: X = **+0,025288**.

Ambos continuam filhos de `J_Bip_C_Neck`, usam a mesma pequena mesh rígida, não possuem skin e não têm canais próprios em `Idle` ou `Wave`. Portanto não há evidência de dupla animação, tratamento especial do Three.js ou defeito no `LegacyGlbAdapter`. O par recebeu transformação local diferente durante a exportação; parent inverse/conversão de parenting no Blender permanece como causa plausível a verificar na fonte artística.

Decisão: **não bloquear UIP nem editar o GLB binário de forma ad hoc**. O defeito entra como dívida do pipeline Blender/exportação e pode ser corrigido em futura reexportação, preferencialmente junto de novas animações ou revisão do asset.

Também permanece dívida de otimização a exportação de **462 canais por clipe**. Isso não implica reduzir a riqueza das animações manuais; a meta futura é preservar os movimentos desejados e eliminar tracks/bakes desnecessários quando possível.
