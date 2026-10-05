# Arquitetura

## Visão geral

```
                ┌──────────────────────── servidor (Node.js, sem dependências) ────────────────────────┐
 runtime real   │  dsr/            world/                 entities/            navigation/              │
 data/abiyss.db ─► runtime-sqlite ─► simulation ─► world ─► abiyss.js  immo.js ─► grid  astar  mover    │
 (ou emulador)  │  demo-runtime     clock  rng   persistence  agent.js (planos)                          │
                │  bridge  ritmo    environment/ (sol, clima de Contagem, luz)                           │
                │                   net/http.js  (estáticos, API, SSE)                                   │
                └──────────────────────────────────────┬────────────────────────────────────────────────┘
                                                        │ SSE: hello / tick (10/s) / meta (1/s)
                ┌────────────────────── cliente (navegador, three.js) ─────────────────────────────────┐
                │ net/sync-client (interpolação) → render/ (cena, escritório, personagens, telas,       │
                │ efeitos, clima, câmera) + ui/ (HUD, debug, legenda)                                  │
                └───────────────────────────────────────────────────────────────────────────────────────┘
                       shared/  layout.js (geometria ÚNICA)  states.js  lighting.js  protocol.js
```

## Estrutura de pastas

```
office/
  config/office.config.json     configuração padrão (comentada no README)
  shared/                       código usado no servidor E no cliente
    layout.js                   paredes, portas, janelas, móveis, POIs, IMMos — fonte única da geometria
    states.js                   estados lógicos do runtime (literais) e atividades do escritório
    lighting.js                 sol + clima → parâmetros de luz (função pura)
    protocol.js                 formato dos quadros SSE
  server/
    main.js                     ponto de entrada: configuração, fonte, simulação, HTTP, persistência
    config.js  log.js
    dsr/                        DSR PROVISÓRIO (isolado): runtime-sqlite, demo-runtime, bridge, ritmo, snapshot
    world/                      world (estado autoritativo), simulation (laço), clock, rng, persistence
    entities/                   agent (executor de planos), immo (cérebro do IMMo), abiyss (cérebro do Abiyss)
    navigation/                 grid (espaço de configuração), astar (A* + suavização), mover (seguidor)
    environment/                sun (NOAA), weather (Open-Meteo + climatologia), environment
    net/http.js                 servidor HTTP + SSE
  client/
    index.html  style.css  main.js
    net/sync-client.js
    render/                     scene, office-builder, furniture, materials, characters/, indicators,
                                displays (telas no mundo), fx, weather-fx, camera
    ui/                         hud, debug
  scripts/                      screenshots, runtime-live.sh, runtime-integration, soak, check-syntax
  tests/                        node:test — navegação, DSR, simulação, ambiente, servidor
  artifacts/screenshots/        capturas reais + JSON do estado em cada captura
  docs/
```

## Servidor

### Laço

`main.js` roda a simulação em **passos fixos de 100 ms** (`simulation.tickHz`)
acompanhando o relógio real × `timeScale` (acumulador; se atrasar mais de
60 passos, descarta em vez de acumular). A cada `pollIntervalMs` (1 s) de
tempo de simulação a fonte é lida e o instantâneo vai para a ponte e para o
mundo. A transmissão SSE é um temporizador separado.

### Mundo (`world/world.js`)

Guarda as entidades, as **reservas** de lugares (POIs), o último
instantâneo do runtime, o ambiente, o registro de acontecimentos (`journal`)
e os efeitos visuais de curta duração (`fx`). Cada passo:

1. avança o relógio;
2. para cada entidade (ordem fixa): `think()` (cérebro) e `run()` (plano);
3. confere as **invariantes**: nenhuma entidade em célula de parede/móvel/
   fora do prédio; nenhuma entidade em assento reservado por outra; nenhuma
   sobreposição (< 30 cm) entre entidades em pé. Violação conta nas
   estatísticas e, se for parede/móvel, recupera para a célula livre mais
   próxima (os testes exigem zero violações e zero recuperações);
4. descarta efeitos vencidos.

Um erro dentro de um agente é registrado e o agente recomeça parado — o
mundo não cai.

Reservas: cada POI tem conflitos (ele mesmo, os que ele trava e os que o
travam — deitar no sofá trava os três lugares; sentar num lugar trava o
"deitado"). Um plano só começa se todas as reservas forem possíveis
(`startPlan`). O lugar onde o corpo está continua reservado até ele se
afastar 1,2 m.

### Entidades e máquinas de estado

`entities/agent.js` executa **planos**: listas de passos com estado
explícito (`goto`, `point`, `face`, `pose`, `stand`, `stay`, `wait`, `call`).
Os cérebros só escolhem planos:

- `immo.js`: modo derivado do estado lógico (`work`, `report`, `cancel`,
  `free`, `enter`) e escolha ponderada da atividade livre;
- `abiyss.js`: modo físico (`offline`, `sleep`, `console`, `free`) +
  mente (`thinking`, `delegating`, `receiving`, `conversing`).

Detalhes em [ESTADOS.md](ESTADOS.md).

### Navegação

- `grid.js`: grade de 10 cm (252 × 201 células) construída do layout:
  áreas andáveis (interior, calçada, vãos das portas), paredes e móveis
  **inflados pelo raio do agente (30 cm)** — se o centro está numa célula
  livre, o corpo não encosta em nada. Janelas bloqueiam; portas não.
  Assentos (cadeiras, sofás, pufes, banquetas) viram **máscaras de bits**:
  proibidos para quem passa, liberados para quem reservou.
- `astar.js`: A* com 8 vizinhos, sem cortar quinas, heurística octil,
  desempate determinístico; depois "puxa a corda" com linha de visada
  **exata** (travessia de grade de Amanatides–Woo, incluindo as duas
  células nos cruzamentos de quina). Pontos inicial e final exatos também
  são validados.
- `mover.js`: a posição anda **sobre a polilinha validada** (nunca corta
  caminho); o que fica orgânico é a velocidade (aceleração, freio nas
  curvas e na chegada) e a orientação (giro com velocidade angular
  limitada). Cada IMMo tem velocidade própria (1,0–1,3 m/s).
- Desvio entre entidades (em `agent.js`): cone à frente + espaço pessoal
  (60 cm); parado atrás de alguém por 0,7 s → replaneja tratando os outros
  como obstáculos; 2,5 s → quem tem menor prioridade dá passagem (anda para
  o lado/trás e espera) e quem está parado numa atividade é convidado a
  sair; 14 s → o passo falha e o cérebro decide outra coisa. Sem progresso
  por 15 s = "travado" (contado; os testes exigem zero).

### Ambiente

`environment/sun.js` calcula elevação/azimute do sol (algoritmo da NOAA) e
nascer/pôr do sol para Contagem (−19,93; −44,05). `weather.js` busca o
tempo **atual** na Open-Meteo a cada 10 min (cache em
`data/weather-cache.json`); se falhar, usa uma estimativa climatológica da
região (médias mensais aproximadas de Belo Horizonte) **marcada como
"estimado"**. `shared/lighting.js` transforma sol + clima em cor do céu,
intensidade/cor do sol, luz ambiente, neblina e se as lâmpadas acendem
(noite, céu muito nublado, chuva forte, tempestade).

### Rede

`net/http.js`: arquivos estáticos (com proteção contra `../`), `/api/*` e
SSE com descarte de quadros para clientes lentos. Escuta em `127.0.0.1` por
padrão (os rótulos mostram textos de tarefas do runtime).

## Cliente

- `net/sync-client.js`: SSE, reconexão, interpolação.
- `render/office-builder.js` + `furniture.js`: cena 100% procedural a partir
  de `shared/layout.js`. Peças estáticas são **unidas por material**
  (`mergeGeometries`): ~400 draw calls com sombras, ~27 mil triângulos.
  Paredes externas do lado da câmera são **cortadas** (rebaixadas) para
  mostrar o interior, como em jogos de gerenciamento.
- `render/characters/`: modelos low-poly e animação procedural por pivôs
  (andar, sentar, deitar no sofá, encolher no pufe, digitar, café,
  conversar, relatório, dormir, piscar).
- `render/indicators.js`: ícones desenhados em canvas (sem fontes de
  emoji) e rótulos discretos; crescem com a distância da câmera.
- `render/displays.js`: telas no próprio mundo — painel de status do
  runtime na sala do Abiyss, quadro de goals, TV com hora/clima de
  Contagem, relógio de parede, monitores e LEDs das mesas, bandeja de
  entrada (sub-agentes pendentes), LEDs do rack.
- `render/weather-fx.js`: chuva (só fora do prédio, que não tem teto),
  relâmpagos, nuvens, vento.
- `render/camera.js`: órbita, visões prontas com transição, seguir entidade.
- `ui/hud.js`, `ui/debug.js`: HUD mínimo e painel de debug. Todo texto vindo
  do runtime é escapado antes de ir para HTML.

## Determinismo

Toda aleatoriedade passa por `world/rng.js` (mulberry32) com semente
explícita; o A* desempata por índice; os agentes são atualizados em ordem
fixa. Mesma semente + mesmos instantâneos do runtime = mesma simulação
(teste `mesma semente → mesma simulação`).

## Configuração, logs e erros

- Configuração: `config/office.config.json` → `--config` → variáveis
  `OFFICE_*`/`ABIYSS_DB` → argumentos. Validada ao subir; erro = mensagem
  clara e código de saída 2.
- Logs: `AAAA-MM-DDTHH:MM:SS.mmmZ NÍVEL [módulo] mensagem {campos}` no
  stderr (e em arquivo com `--log-file`); os últimos aparecem em
  `/api/health`.
- Porta ocupada: código 1 com mensagem. Exceções não tratadas são
  registradas.

## Desempenho

| Medida | Valor |
|---|---|
| Tick da simulação | ~0,03 ms em média (5 entidades) |
| 6 h de simulação, sem render | ~3,5 s de CPU |
| Banda SSE | quadro `tick` ≈ 2,3 KB × 10/s + `meta` ≈ 3,6 KB × 1/s por cliente (~27 KB/s) |
| Cena | ~400 draw calls (com passada de sombra), ~27 mil triângulos, poucos materiais Lambert |
| Na VM (SwiftShader, sem GPU) | 1–3 FPS a 1600×900 — só para screenshots |
