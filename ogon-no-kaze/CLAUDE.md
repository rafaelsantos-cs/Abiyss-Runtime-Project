# Ōgon no Kaze (風 · "Vento Dourado")

Jogo de ação em terceira pessoa, **Sekiro-like**, ambientado num Japão feudal na hora dourada.
Este arquivo é o contexto permanente do projeto. Leia inteiro no início de cada sessão e
mantenha a seção "Decisões" atualizada quando algo for definido.

---

## Localização (importante)

O jogo vive **só** na pasta `ogon-no-kaze/`, dentro do repositório do ABIYSS. O resto do
repositório pertence ao ABIYSS e **não deve ser alterado em nada** (nem `.gitignore`,
`.gitattributes`, CI ou README da raiz). Todo arquivo do jogo, inclusive configuração de
git (`.gitignore`, `.gitattributes`), fica dentro de `ogon-no-kaze/`. Todos os comandos
abaixo rodam a partir de `ogon-no-kaze/`.

---

## Como trabalhar comigo (Rafael)

- **Responda em português.** Código, nomes de arquivos e identificadores em inglês;
  comentários no código em português.
- Programo em C# há anos, mas sou novo em Godot, shaders e game dev 3D. Aprendo fazendo:
  ao implementar algo não trivial (matemática de shader, arquitetura, física), explique
  **o porquê** em poucas linhas. Sem enrolação e sem repetir o que já foi dito.
- Ambiente: Linux, VS Code, notebook Samsung Galaxy Book2.
- **Eu sou o juiz da sensação de jogo.** Nunca afirme que algo "está responsivo" ou
  "ficou bonito" sem evidência: visual se valida por captura de tela (ver Ferramentas),
  feel de controle se valida comigo jogando.
- Antes de refatorações grandes ou mudanças de arquitetura, proponha o plano e espere aprovação.

---

## Visão

**Referência visual:** `docs/ref/referencia.jpg`. Descrição, caso a imagem não esteja lá:
sol baixo entre montanhas com raios de luz atravessando a névoa do vale; colinas com
florestas escuras de coníferas; em primeiro plano um campo vasto de trigo/grama alta
dourada balançando ao vento e brilhando contra o sol; à direita, numa colina, bordos de
folhas vermelhas, um portal torii vermelho e uma lanterna de pedra; ronins com chapéu de
palha (kasa) e roupas escuras, katana na cintura.

**Pilares:**
1. **Sensação de controle Sekiro-like**: resposta imediata, golpes com peso, deflexão no
   tempo certo, mobilidade vertical.
2. **Luz como protagonista**: hora dourada, raios volumétricos, trigo translúcido.
3. **O campo é vivo e é gameplay**: o trigo reage ao vento e aos passos e serve de
   esconderijo (furtividade agachado).

**Propriedade intelectual:** Sekiro é referência de *mecânica* apenas. Não copiar nomes,
interface, ícones, sons, modelos ou designs de nenhum jogo. Tudo com identidade própria.

---

## Escopo atual (Etapa 1)

Inclui:
- **Tela inicial viva**: a cena do campo ao fundo com câmera cinematográfica lenta, título
  em estética de tinta sumi-e/caligrafia, transições suaves.
- **Configurações** completas e persistentes (detalhes abaixo).
- **"Jogar"** → tela de carregamento → mundo aberto jogável.
- **Movimento shinobi**: andar, correr, sprint, pulo com controle aéreo, agachar
  (furtividade no trigo), gancho (kaginawa) em pontos de agarre, pendurar e escalar bordas.
- Personagem ronin com chapéu de palha, katana na cintura como objeto (sem uso ainda).
- Efeitos, partículas, som ambiente e animações completas de locomoção.

Não inclui (ainda): progressão, inventário, missões, diálogo, salvamento de jogo.
Combate: a **arquitetura** já nasce pronta para ele (ver Arquitetura); a jogabilidade de
combate depende de decisão pendente.

---

## Stack

| Área | Ferramenta |
|---|---|
| Engine | Godot **4.7.2** (.NET/C#, .NET 8), renderizador **Forward+**, física **Jolt** |
| Linguagem | C# para toda a lógica; Godot Shading Language para shaders |
| Modelagem | Blender, via scripts Python (`bpy`) executados em modo background |
| Personagem | Base humana do **MPFB2** (addon do Blender) + roupas geradas por script |
| Rig e animações | **Mixamo** (eu baixo, exige minha conta). Deflexão, quebra de postura e execuções serão animadas à mão no Blender |
| Texturas | CC0: Poly Haven, ambientCG |
| Áudio | Freesound, apenas CC0 (ou CC-BY com crédito registrado) |
| Fontes | OFL (Google Fonts), recortadas por `tools/fonts/subset_fonts.sh` |
| Terreno | Avaliar o plugin Terrain3D; alternativa: malha de heightmap gerada no Blender |
| Versionamento | git + **git LFS** para `.glb`, `.png`, `.jpg`, `.wav`, `.ogg`, `.blend` (regras em `ogon-no-kaze/.gitattributes`) |

Todo asset de terceiros: registrar fonte e licença em `docs/CREDITS.md`.

---

## Comandos

> O binário oficial do Godot se chama `Godot_v4.7.2-stable_mono_linux.x86_64`. Abaixo ele
> aparece como `godot`: crie um link (`ln -s <caminho>/Godot_v4.7.2-stable_mono_linux.x86_64 ~/.local/bin/godot`).
> Confirmar na máquina do Rafael e registrar aqui se for diferente.

```bash
dotnet build                                   # compila o C#
godot --headless --path . --import             # importa assets e sai (checa erros de import)
godot --path .                                 # roda o jogo
godot --path . res://scenes/world/World.tscn   # roda uma cena direto
blender --background --python tools/blender/<script>.py -- --out assets/models/<arquivo>.glb

# Captura de tela (precisa de GPU/janela; não usar --headless)
godot --path . res://tools/capture/CaptureRunner.tscn -- \
      --scene=res://scenes/menu/MainMenu.tscn --out=docs/captures/menu.png \
      [--camera=<marcador>] [--graphics=medium] [--size=1920x1080] [--frames=90] \
      [--set=<caminho/do/nó>:<Propriedade>=<valor>]

# Medidor de desempenho (acrescenta uma linha em docs/perf/results.csv)
godot --path . res://tools/perf/PerfRunner.tscn -- \
      --scene=res://scenes/world/World.tscn --graphics=medium --seconds=20 [--camera=<marcador>]
```

Sem GPU (ex.: container de CI), dá para capturar com renderização por software:
`VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json xvfb-run -a -s "-screen 0 1920x1080x24" godot ...`
(pacote `mesa-vulkan-drivers`). É lento e **não serve para medir desempenho**.

---

## Ferramentas

- **Captura de tela** (`tools/capture/`): carrega uma cena-alvo, posiciona a câmera num
  marcador (`Marker3D` no grupo `capture_point`), espera N frames, salva a viewport em PNG
  e fecha. Use isso para conferir todo trabalho visual antes de dar como pronto, e salve
  as capturas em `docs/captures/`.
- **Medidor de desempenho** (`tools/perf/`): roda a cena por X segundos e registra FPS
  médio, 1% low, draw calls e memória de vídeo em `docs/perf/results.csv`. Usado para
  calibrar os presets. Força VSync desligado e sem limite de FPS durante a medição.
- Ferramentas nunca gravam `user://settings.cfg` (aplicam o preset pedido só em memória).
- **Prévias do Blender** (a criar na Fase 3): cada script de geração renderiza uma imagem
  de prévia (Eevee) junto com o `.glb`.

---

## Estrutura de pastas

```
project.godot  OgonNoKaze.csproj  OgonNoKaze.sln  CLAUDE.md  default_bus_layout.tres
docs/        ref/ captures/ perf/ CREDITS.md
assets/      models/ textures/ audio/ animations/ fonts/
data/        settings/ (presets .tres)  player/ (tuning e ações .tres)
scenes/      menu/ world/ player/ ui/
scripts/     Core/ Settings/ Player/ Combat/ World/ UI/ Audio/
shaders/     grass/ sky/ post/ fx/ ui/
tools/       blender/ capture/ perf/ fonts/
```

---

## Arquitetura

### Personagem (núcleo Sekiro-like)

- `CharacterBody3D` com cápsula; lógica em `_PhysicsProcess` a 60 ticks.
- **Máquina de estados hierárquica** em C#:
  - Locomoção: Idle, Walk, Run, Sprint
  - Ar: Jump, Fall
  - Crouch (furtividade)
  - Grapple, LedgeHang, Climb
  - Combate (só estrutura por enquanto): Attack, Deflect, Guard, Stagger,
    PostureBroken, Deathblow, Death
- **Ações orientadas a dados**: cada ação é um `Resource` (`ActionData`) com frames de
  início, ativo e recuperação, janelas de cancelamento e transições permitidas.
  Todos os valores de ajuste ficam em `.tres` para eu mexer pelo editor.
- **Buffer de input** (valor inicial ~150 ms) e **coyote time** no pulo (~100 ms).
  Valores iniciais, a ajustar comigo jogando.
- **Root motion** em ataques, aterrissagem do gancho e escalada; locomoção comum é
  dirigida pelo controlador.
- `AnimationTree`: BlendSpace para locomoção e máquina de estados para ações. Eventos de
  animação controlam janelas (hitbox, cancelamento, passos).
- Deixar interfaces prontas para: hit-stop, tremor de câmera, trava de alvo (lock-on),
  postura/vida.
- Física secundária nas cordas do chapéu e nas mangas (`SpringBoneSimulator3D`).

### Câmera

- Terceira pessoa com spring arm, colisão e suavização.
- Zoom e leve deslocamento ao lançar o gancho. Modo lock-on fica preparado para depois.

### Gancho (kaginawa)

- Nós `GrapplePoint` (Area3D) nos galhos dos bordos, no topo do torii e em rochas altas.
- Seleção do ponto por alcance máximo, ângulo em relação à câmera e linha de visão.
- Indicador na tela quando há ponto válido; puxão com curva controlada e aterrissagem
  em root motion.

### Trigo e grama (sistema central)

- **Geometria vinda do Blender**: várias variações de lâmina e de espiga, com LOD0/1/2
  (LOD distante pode ser cards cruzados). Preferir geometria a alpha, porque alpha
  scissor com sombras custa caro.
- **Chunks** de `MultiMeshInstance3D` (ponto de partida: 16×16 m), posicionados por mapa
  de densidade e altura do terreno; `visibility_range` por chunk para LOD; densidade e
  distância controladas pelas configurações.
- **Vento (vertex shader)** em camadas: balanço base (seno com fase por instância) e
  rajadas (textura de ruído em espaço de mundo rolando na direção do vento).
  Direção, força e textura do vento como **uniforms globais**
  (`RenderingServer.GlobalShaderParameterSet`), para partículas e áudio sincronizarem
  com o mesmo vento.
- **Afundar ao pisar**: `SubViewport` com câmera ortográfica de cima seguindo o jogador,
  gravando os rastros numa textura que decai com o tempo (ping-pong entre duas texturas).
  O shader da grama lê essa textura em espaço de mundo e curva as hastes para baixo e
  para fora; recuperação lenta (~2–4 s).
- **Fragment**: gradiente da raiz à ponta, oclusão na base, **translucidez contra o sol**
  (termo de luz traseira usando direção da vista × direção da luz) e especular sutil.
- Sombras: grama projeta sombra só em Alto/Ultra; nos demais, apenas recebe.
- **Furtividade**: expor `GrassQuery.GetHeightAt(Vector3)`. Agachado em grama acima de
  certa altura, o jogador fica com fator de visibilidade reduzido (usado pela IA no futuro).

### Mundo, luz e ambiente

- Mundo **vertical**: campo de trigo no centro, cercado de colinas, penhascos e ruínas;
  torii e bordos no alto de uma colina; rotas por cima usando o gancho.
- `DirectionalLight3D` como sol rasante da hora dourada.
- `WorldEnvironment`: shader de céu próprio (disco solar, gradiente, nuvens), **névoa
  volumétrica** (raios de sol), névoa por altura, SSAO, glow, tonemapping AgX (ou ACES),
  correção de cor por LUT. SDFGI apenas no Ultra.
- Partículas: poeira e pólen nos feixes de luz, folhas de bordo caindo, sementes
  levadas pelo vento, todas lendo o vento global.
- `scenes/world/Landscape.tscn` é a paisagem compartilhada: o mundo jogável
  (`World.tscn`) e o fundo do menu (`MainMenu.tscn`) instanciam a mesma cena.

### Menu, configurações e carregamento

- Autoload `SettingsManager`: carrega `user://settings.cfg` (`ConfigFile`) e aplica tudo
  no boot.
- **Presets**: Baixo, Médio, Alto, Ultra, com opção Personalizado ao alterar qualquer item.
  Valores de cada preset em `data/settings/preset_*.tres`.
- **Vídeo**: resolução, modo de janela, VSync, limite de FPS, escala de renderização com
  FSR, qualidade de sombras, densidade e distância da grama, névoa volumétrica,
  iluminação global, SSAO, FOV.
- **Controles**: sensibilidade, inverter Y, remapeamento de teclas (InputMap), suporte a
  controle.
- **Áudio**: barramentos Master, Música, Efeitos e Ambiente.
- Carregamento com `ResourceLoader.LoadThreadedRequest` e tela com progresso
  (autoload `SceneLoader`).

### Áudio

- Volume e filtro do vento acompanham a intensidade das rajadas.
- Passos na grama variam com velocidade e com o agachar.

---

## Desempenho

- **Máquina de desenvolvimento**: Galaxy Book2, provavelmente com GPU integrada Intel.
  Na primeira sessão na máquina do Rafael, conferir com `lspci | grep -iE "vga|3d"` e
  registrar em "Decisões".
- **Meta**: preset **Médio** a 60 FPS estáveis em 1080p (com FSR) nesta máquina.
  Ultra é pensado para GPUs dedicadas.
- Calibrar presets com o medidor de desempenho, nunca no chute.

---

## Fases

Cada fase termina com: build sem erros, capturas em `docs/captures/` e minha aprovação.

1. **Base**: projeto, estrutura, autoloads, menu, configurações, carregamento, esqueleto
   da máquina de estados, ferramentas de captura e desempenho.
2. **Mundo**: terreno vertical, céu, luz, névoa, pós-processamento.
3. **Trigo**: geração no Blender, chunks, vento, interação com passos, LOD, furtividade.
4. **Personagem**: modelo (MPFB2 + roupas por script), rig e animações do Mixamo,
   controlador, câmera, gancho, bordas.
5. **Polimento**: partículas, áudio, calibragem dos presets.

---

## Regras

- Compile após cada mudança de código. Em mudança visual, gere captura e olhe antes de
  dizer que está pronto.
- Commits pequenos, um assunto por commit.
- Nada de assets pagos ou que exijam contas minhas, exceto o Mixamo. Quando eu precisar
  baixar algo, diga exatamente o quê, de onde e em qual pasta colocar.
- Valores ajustáveis sempre expostos (`[Export]` ou `Resource`), nunca enterrados no código.
- Nada fora de `ogon-no-kaze/` é tocado (ver Localização).

---

## Decisões

**Tomadas:**
- Engine Godot 4 + C#: aproveita meu C#, roda nativo no Linux, Forward+ cobre os efeitos.
  Unreal descartado (pesado para meu hardware, C++/Blueprints).
- Câmera em terceira pessoa (2.5D descartado).
- Personagem com base MPFB2 + Mixamo, não gerado só por script.
- Escopo da Etapa 1 sem progressão.
- O jogo fica isolado em `ogon-no-kaze/` dentro do repo do ABIYSS, sem tocar no resto.
- Versão do Godot: **4.7.2-stable .NET** (última estável em 2026-10), com .NET 8 SDK.

**Pendentes:**
- [ ] Incluir na Etapa 1 um **duelo de teste** (um inimigo com deflexão e barra de postura)
      ou travar primeiro movimento e gancho?
- [ ] Modelo exato da GPU (rodar `lspci` na máquina do Rafael).
- [ ] Terrain3D ou malha de terreno própria.
- [ ] Animações de combate: à mão no Blender ou pacote pago.
