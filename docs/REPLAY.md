# Replay do Travessia — formato 1

O servidor do jogo (`recursos/mcp/jogo/`) grava um arquivo JSONL por
execução: uma linha JSON por evento, na ordem em que aconteceu. O
visualizador (Unity) reproduz a partida a partir dele, **sem rodar regra
nenhuma**: tudo o que muda vem escrito em cada linha.

O harness (`abiyss travessia`) grava, ao lado, a **linha do tempo** com as
delegações e as chamadas ao modelo, no mesmo relógio (milissegundos desde a
época Unix), para o visualizador mostrar os sub-agentes trabalhando ao lado
do mapa.

Validar e resumir:

```bash
cd recursos/mcp/jogo
uv run validar_replay.py CAMINHO/replay.jsonl [--linha-do-tempo CAMINHO/linha-do-tempo.jsonl]
```

O validador confere o formato, a ordem e que nada do gabarito vazou, e refaz
cada fase no motor a partir da linha `fase` (os passos e as mudanças precisam
bater). Sai com 0 (válido), 1 (inválido) ou 2 (não conseguiu ler).

## Convenções

- **Instante** (`t`): milissegundos desde a época Unix, o mesmo relógio da
  linha do tempo do harness.
- **Coordenadas**: as do jogo, `[x, y]` a partir do canto superior
  esquerdo; x cresce para a direita, y para baixo. No Unity, uma célula vira
  1 m, x continua x e **y vira −z**.
- **Versão** (`versao`): o contador de estado do jogo (o mesmo que o modelo
  vê); nunca diminui.
- **Nunca aparece no replay**: o ótimo, a solução, o limite de jogadas, a
  seed e o molde. O replay fica fora do alcance do Abiyss e pode trazer o
  mapa inteiro (o visualizador desenha a névoa por cima com as linhas
  `revelacao`).

## Tipos de linha

### `cabecalho` (primeira linha)

| Campo | Tipo | O quê |
|---|---|---|
| `formato` | int | versão deste formato (1) |
| `execucao` | texto | id da execução |
| `braco` | texto | braço do teste (A, B, C, T...) |
| `modo` | texto | `avaliacao` ou `campanha` |
| `relogio_segundos` | número ou null | orçamento por fase no braço com relógio |
| `data` | texto | data e hora locais (ISO 8601) |
| `t` | int | instante |

### `fase` (ao carregar uma fase)

| Campo | Tipo | O quê |
|---|---|---|
| `numero` | int | número da fase (1 a 12) |
| `nome` | texto | nome da fase (o modelo nunca vê) |
| `largura`, `altura` | int | tamanho da grade |
| `celulas` | lista de textos | o mapa inteiro, uma linha por y, células-base: `. # ~ = : _ D + G` |
| `objetos` | objeto | `avatares` {letra: [x,y]}, `blocos` [[x,y]], `itens` [{item, pos}] (item = `dourada` ou a cor), `placas` {id: [x,y]}, `portas` [{id, pos, placas}], `trancadas` [{cor, pos}] |
| `avatares` | lista | `{letra, nome, pos, habilidades, raio}` na ordem V, R, C |
| `nevoa` | bool | a fase tem névoa |
| `versao` | int | versão do estado ao carregar |

Logo depois, numa fase com névoa, vem uma `revelacao` com o que os avatares
já enxergam no começo.

### `passo` (cada passo executado, válido ou não)

| Campo | Tipo | O quê |
|---|---|---|
| `versao` | int | versão depois do passo (inválido: não muda) |
| `avatar` | texto ou null | `V`, `R` ou `C` (null: o texto nem era um passo) |
| `acao` | texto | o passo na notação do jogo (`"R L"`, `"V soltar"`, `"V entregar R"`) |
| `resultado` | texto | `ok` ou `invalida` |
| `motivo` | texto | só nos inválidos: por quê |
| `mudancas` | lista | o que mudou (vazia nos inválidos), na ordem |

Tipos de mudança:

| `tipo` | Campos | O quê |
|---|---|---|
| `avatar` | `avatar`, `de`, `para` | o avatar andou |
| `bloco` | `de`, `para`, `afundou` | o bloco foi empurrado; `afundou` = caiu na água e a célula `para` virou ponte |
| `trancada` | `pos`, `cor` | a porta trancada abriu de vez (virou chão) e a chave foi gasta |
| `portao` | `pos` | o portão abriu: a fase terminou |
| `pegar` | `avatar`, `item`, `pos` | o avatar pegou o item |
| `soltar` | `avatar`, `item`, `pos` | o avatar soltou o item |
| `entregar` | `de`, `para`, `item` | o item passou de um avatar para outro |
| `porta` | `id`, `pos`, `aberta` | uma porta de placa abriu ou fechou |

### `revelacao` (a névoa revelou células)

| Campo | Tipo | O quê |
|---|---|---|
| `celulas` | lista de [x,y] | células reveladas agora (ficam reveladas para sempre) |

### `reinicio` (reiniciar_fase)

| Campo | Tipo | O quê |
|---|---|---|
| `versao` | int | versão depois do reinício |
| `reinicios` | int | quantos reinícios na fase até aqui |
| `estado` | objeto | o estado inicial de novo: `avatares` {letra: {pos, carrega}}, `blocos`, `itens`, `pontes`, `trancadas_abertas`, `portas_abertas` |

O mapa já revelado continua revelado.

### `fim_fase` (concluir, falhar ou desistir)

| Campo | Tipo | O quê |
|---|---|---|
| `numero` | int | a fase |
| `resultado` | texto | `concluida`, `falha` ou `desistencia` |
| `motivo` | texto | na falha: `jogadas`, `reinicios` ou `tempo`; na desistência: `desistencia` |
| `jogadas`, `invalidas`, `reinicios`, `simulacoes` | int | contados pelo jogo |
| `tempo_s` | número | relógio de parede da fase |

### `fim_execucao` (não há mais fases)

| Campo | Tipo | O quê |
|---|---|---|
| `motivo` | texto | `campanha_completa` ou `falha_na_campanha` (modo campanha) |

## Linha do tempo do harness

Arquivo JSONL separado (`linha-do-tempo.jsonl`, ao lado do replay), escrito
pelo harness, que é quem sabe das delegações (o jogo não sabe quem delegou o
quê). Cada linha tem `tipo` e `t`:

| `tipo` | Campos principais |
|---|---|
| `fase` | `numero`, `evento` (`inicio` ou `fim`) |
| `chamada_modelo` | `pool` (`cerebro` ou `subagentes`), `origem` (nível do sub-agente ou `conversa`), `modelo`, `nivel_esforco`, `status` (`ok`, `limite` = 429, `erro`), `duracao_ms`, `espera_ms` (tempo na fila do orquestrador), `tokens_entrada`, `tokens_saida` |
| `delegacao` | `id` do sub-agente, `nivel`, `esforco`, `tarefa` |
| `subagente_fim` | `id`, `estado`, `status` do relatório, `tokens`, `rodadas` |
| `ferramenta` | `quem` (`cerebro` ou `subagente:<id>`), `nome`, `erro` |

A linha do tempo também nunca traz o ótimo, a solução nem o limite.
