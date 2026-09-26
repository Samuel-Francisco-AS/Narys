# Avatar, animações e comportamento corporal

> Direção arquitetural pós-M0. Última revisão: 25/09/2026.
>
> Objetivo: permitir que avatar, animações, expressões e comportamento visual evoluam em paralelo ao Luna Core, sem bloquear a construção funcional da agente.

## 1. Princípio

A Luna não é o arquivo do avatar.

~~~text
Luna = identidade + memória + estado + agente + ferramentas
Avatar = representação visual substituível
~~~

Trocar o corpo visual não deve alterar memória, personalidade, providers ou ferramentas.

## 2. Estado atual

O protótipo M0-B carrega:

~~~text
public/models/Luna.glb
~~~

O arquivo contém Idle e Wave embutidos. src/components/CharacterScene.tsx:

- cria cena/câmera/luzes;
- carrega GLB;
- cria AnimationMixer;
- procura clipes por nome;
- executa Idle;
- executa Wave por clique/botão;
- faz transição de volta;
- trata raycast e descarte.

Esse desenho é adequado para validar M0, mas acopla:

- renderer;
- avatar;
- catálogo de animações;
- regras de comportamento;
- input do usuário.

A refatoração deve separar essas responsabilidades.

## 3. Formato-alvo

Adotar como direção:

- **VRM 1.0** para avatar humanoide;
- **VRMA / VRM Animation** para animações reutilizáveis;
- **Three.js + @pixiv/three-vrm** no runtime;
- **Blender + VRM Add-on for Blender** como pipeline artístico.

A direção deve ser comprovada primeiro por um experimento pequeno antes de remover o fallback GLB atual.

## 4. Por que VRM/VRMA

O pipeline atual transforma base.vrm em GLB e remove metadados VRM. Isso simplificou M0, mas perde semântica útil para avatares intercambiáveis.

VRM oferece contrato humanoide semântico. Em vez de depender de índices específicos de ossos, o runtime pode trabalhar com conceitos como:

- head;
- neck;
- chest;
- hips;
- left/right upper arm;
- left/right lower arm;
- hands;
- humanoid expressions/look-at quando disponíveis.

VRMA foi pensado para transportar movimento humanoide retargetável entre avatares VRM 1.0.

Referências:
- https://github.com/pixiv/three-vrm
- https://github.com/pixiv/three-vrm/tree/dev/packages/three-vrm-animation
- https://vrm-addon-for-blender.info/en-us/ui/export_scene.vrma/

## 5. Blender continua sendo a oficina

Não substituir o aprendizado atual.

O usuário pode continuar criando:

- poses;
- keyframes;
- curvas Bézier;
- Graph Editor;
- sobreposição de movimentos;
- animações completas

no Blender.

Com VRM Add-on for Blender, o fluxo-alvo passa a ser:

~~~text
Blender
├─ avatar VRM 1.0
├─ Pose Mode
├─ Dope Sheet
├─ Graph Editor
└─ File → Export → VRM Animation (.vrma)
~~~

O add-on atual suporta import/export de VRM e VRMA e suporta Blender 3.3.x.

Referência:
https://vrm-addon-for-blender.info/en-us/

## 6. Asset pipeline proposto

~~~text
assets/
└─ avatars/
   └─ luna-v1/
      ├─ source/
      │  └─ luna.blend
      ├─ avatar.vrm
      ├─ manifest.json
      └─ animations/
         ├─ idle.vrma
         ├─ wave.vrma
         ├─ thinking.vrma
         ├─ focus-input.vrma
         └─ ...
~~~

O layout final pode mudar, mas avatar e animações devem continuar desacoplados.

## 7. Avatar Pack

Criar um contrato de AvatarPack.

Exemplo conceitual:

~~~text
AvatarPack
- id
- displayName
- model
- format
- capabilities
- animationManifest
- expressionCapabilities
- lookAtCapabilities
- licenseMetadata
~~~

Manifest conceitual:

~~~json
{
  "id": "luna-v1",
  "model": "avatar.vrm",
  "animations": [
    {
      "id": "idle-main",
      "intent": "idle",
      "file": "animations/idle.vrma",
      "loop": true,
      "priority": 0
    },
    {
      "id": "wave-main",
      "intent": "greeting",
      "file": "animations/wave.vrma",
      "loop": false,
      "priority": 5
    }
  ]
}
~~~

Não tratar esse JSON como schema final antes do protótipo VRM/VRMA.

## 8. Intenção semântica, não nome de arquivo

O Luna Core nunca deve mandar:

~~~text
play("wave.vrma")
~~~

Ele emite intenção:

~~~text
AnimationIntent {
  state: greeting,
  intensity: 0.5,
  interruptibility: normal
}
~~~

O Animation Director decide qual asset representa greeting no avatar atual.

Isso permite:

~~~text
Luna v1: greeting → wave.vrma
Luna v2: greeting → bow.vrma
Outro avatar: greeting → hand_raise.vrma
~~~

sem alterar o cérebro da agente.

## 9. Animation Director

Responsabilidades:

- receber eventos do Luna Core/UI;
- escolher animação apropriada;
- controlar prioridades;
- cooldown;
- interruption policy;
- blend in/out;
- loop;
- retorno ao estado-base;
- overlays;
- fallback se o avatar não tiver determinado gesto.

Estados/intents iniciais:

- idle;
- attentive;
- listening;
- thinking;
- working;
- speaking;
- greeting;
- acknowledge;
- success;
- concern/error;
- amused;
- focus-input.

Nem todos precisam existir no primeiro pack.

## 10. Seleção automática pela agente

O botão Acenar atual continua útil como **debug**.

Na interface normal, a escolha deve vir de eventos reais:

~~~text
UserReturned      → greeting
TaskStarted       → attentive/working
ProviderWaiting   → thinking
ToolRunning       → working
Speaking          → speaking
TaskCompleted     → success
TaskFailed        → concern
~~~

Uma LLM pode sugerir nuance emocional opcional em respostas complexas, mas não deve acessar arquivos de animação diretamente.

## 11. Debug panel

Mover controles manuais para uma área de desenvolvimento:

~~~text
Avatar Debug
- selecionar Avatar Pack
- listar animações
- play/stop
- blend time
- loop
- intensidade
- estado atual
- eventos recebidos
~~~

Isso preserva o botão/manual testing sem misturá-lo ao comportamento de produção.

## 12. Camadas e sobreposição

A direção futura deve permitir combinar movimentos menores.

Exemplo:

~~~text
Base layer:
  idle

Overlay:
  breathing

Head/Look:
  olha para campo de texto

Gesture:
  pequena mão/ombro

Expression:
  facial state
~~~

Three.js AnimationMixer suporta pesos/fades e o ecossistema Three possui suporte a additive animation blending. A arquitetura não deve assumir que uma única animação longa controla o corpo inteiro.

Primeiro objetivo, porém, é simples: comprovar um Idle e um gesto separados.

## 13. Refatoração de CharacterScene

Decomposição proposta:

~~~text
src/avatar/
├─ AvatarViewport.tsx
├─ runtime/
│  ├─ SceneRuntime.ts
│  ├─ AvatarManager.ts
│  ├─ AnimationDirector.ts
│  ├─ AnimationRegistry.ts
│  └─ types.ts
└─ adapters/
   ├─ VrmAvatarAdapter.ts
   └─ LegacyGlbAdapter.ts
~~~

Nomes podem mudar durante implementação.

Responsabilidades:

**SceneRuntime**
- renderer;
- camera;
- lights;
- resize;
- render loop;
- WebGL lifecycle.

**AvatarManager**
- carregar/descarregar pack;
- expor capabilities;
- trocar avatar;
- gerenciar root/mixer.

**AnimationRegistry**
- catálogo semântico.

**AnimationDirector**
- máquina de comportamento visual.

**LegacyGlbAdapter**
- manter a Luna atual funcionando durante migração.

**VrmAvatarAdapter**
- novo caminho VRM/VRMA.

## 14. Migração segura

Não converter tudo de uma vez.

### Gate A — preservar M0

Refatorar CharacterScene sem mudar comportamento visível. Idle/Wave embutidos continuam funcionando.

### Gate B — VRM proof

- instalar/configurar VRM Add-on no Blender;
- importar base VRM;
- validar rig;
- exportar/usar um avatar.vrm;
- carregar com three-vrm no app.

### Gate C — VRMA proof

- exportar **apenas Idle** como idle.vrma;
- carregar VRMA separadamente;
- tocar no avatar atual;
- validar Blender → runtime → Tauri.

### Gate D — Animation Director

Trocar chamada direta Wave por intenção semântica. Manter botão de debug.

### Gate E — expansão

Adicionar novas animações como assets plugáveis, sem alteração no Luna Core.

## 15. Paralelismo de desenvolvimento

O trabalho artístico não bloqueia o funcional.

~~~text
TRACK AVATAR/ANIMAÇÃO
Blender → Idle → VRMA → novas animações
          │
          │ contrato de intents
          ▼
     Animation Director

TRACK LUNA CORE
Rust → eventos → memória → providers → tools
          │
          ▼
      Task/Event stream
~~~

Os dois tracks se encontram apenas em contratos estáveis:

- TaskEvent;
- AnimationIntent;
- AvatarCapability.

Enquanto o usuário continua aprendendo Blender e produzindo movimentos, o Luna Core pode avançar independentemente.

## 16. Restrições conhecidas

- O WebKitGTK no hardware atual precisou de Mesa software; toda evolução visual deve manter gate de desempenho.
- Não remover RobotExpressive/fallback histórico até a migração estar validada.
- Preservar licenças/metadados dos assets.
- Não assumir que todo VRM terá todas as expressões/look-at.
- VRMA deve ser validado no Blender 3.3.21 real do usuário antes de congelar schema/pipeline.

## 17. Primeira definição de pronto

A arquitetura visual estará provada quando:

1. o app carrega um Avatar Pack;
2. Idle vem de asset separado;
3. Animation Director recebe intent idle/greeting;
4. botão debug pode disparar greeting sem App.tsx conhecer nome de arquivo;
5. adicionar um novo VRMA exige só asset + manifest;
6. trocar Avatar Pack não modifica Luna Core.
