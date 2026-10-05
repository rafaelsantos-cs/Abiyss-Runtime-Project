# Sincronização

```
 runtime do Abiyss (kernel Rust)            ┐
   data/abiyss.db (SQLite, WAL)             │  fonte da verdade lógica
            │  leitura SOMENTE LEITURA, 1×/s │
            ▼                               ┘
 server/dsr/runtime-sqlite.js   (ou demo-runtime.js, mesmo formato)
            │  instantâneo normalizado  "abiyss-office/dsr-provisorio@1"
            ▼
 server/dsr/bridge.js           diferenças → eventos de domínio,
            │                   atribuição sub-agente → IMMo
            ▼
 server/world/world.js          SIMULAÇÃO AUTORITATIVA (10 ticks/s):
            │                   posições, caminhos, atividades, reservas
            ▼
 server/net/http.js             SSE /api/events  (tick 10/s, meta 1/s)
            ▼
 client/net/sync-client.js      interpolação (150 ms atrás) → three.js
```

Princípios:

- **O runtime é a fonte da verdade lógica**; o escritório nunca escreve nele.
- **O servidor do escritório é a fonte da verdade física** (onde cada um
  está, para onde vai, o que o corpo faz). O cliente não simula.
- O cliente pode cair, abrir em várias abas ou chegar atrasado: todos
  veem o mesmo mundo.

## 1. Leitura do runtime real (`runtime-sqlite.js`)

- Abre `data/abiyss.db` com `readOnly: true` e `PRAGMA query_only = 1`.
  Um teste prova que o arquivo não muda (hash SHA‑256 antes/depois) e que
  uma escrita é recusada.
- Cada leitura é uma transação de leitura (instantâneo consistente no WAL,
  sem bloquear o daemon).
- Precisa da migração **4+** (daemon e sub-agentes); conhece até a **10**.
  Banco mais novo: lê só as colunas conhecidas e avisa no log.
- Consultas (todas pequenas):
  - `estado_daemon` (pid, iniciado, sinal de vida, parado);
  - `subagentes` vivos + terminados nos últimos 10 min (máx. 64);
  - `goals` ativos + finalizados nas últimas 24 h, e contagem por estado;
  - últimos 8 `ciclos`;
  - `fila_eventos`: pendentes (contagem) e os 12 mais recentes **sem o
    conteúdo** (pode ser grande e é dado externo);
  - `chamadas_modelo` dos últimos 10 min agrupadas por `origem`
    (conversa e sono);
  - `sonos`, só se a tabela existir (E5 ainda não publicada).
- Falhas (banco ausente, travado, antigo): a fonte vira "com erro", o
  mundo mantém o último estado bom, o HUD mostra "sem leitura" e o Abiyss
  fica `offline`. Tenta de novo a cada leitura.
- `horas_ativas` e `cron_verificacao_segundos` vêm do `abiyss.toml` do
  runtime (`--runtime-config`), para a fase e a regra de "daemon vivo"
  baterem com as do kernel.

## 2. Instantâneo normalizado (DSR provisório)

```js
{
  schema: 'abiyss-office/dsr-provisorio@1',
  source: 'runtime-sqlite' | 'demo',
  ok, error, capturedAtMs, runtimeSchemaVersion,
  daemon:   { estado: 'rodando'|'parado'|'desconhecido', pid, iniciadoMs, sinalDeVidaMs, paradoMs },
  ritmo:    { fase: 'vigília'|'descanso'|'sono', horasAtivas, origemFase: 'relogio'|'sono' },
  subagentes: [{ id, nivel, estado, tarefa, goalId, origem, criadoMs, iniciadoMs, terminadoMs,
                 prazoSegundos, relatorio: { status, resumo, confianca } | null, tokens, rodadas }],
  goals:    { lista: [{ id, titulo, estado, prioridade, atualizadoMs }], foco, contagem },
  ciclos:   [{ id, inicioMs, fimMs, chamouModelo, motivo, goalFoco, resultado, erro, tokens }],
  eventos:  { pendentes, recentes: [{ id, momentoMs, tipo, origem, consumido }] },
  conversa: { ultimaChamadaMs },
  sono:     { ativo, evidencia },
}
```

Os valores de estado são os do runtime, sem tradução. Quando o DSR oficial
existir, só `server/dsr/` muda.

## 3. Ponte (`bridge.js`)

Compara instantâneos e emite eventos de domínio: `subagente.criado`,
`subagente.iniciado`, `subagente.atribuido` (com o IMMo), `subagente.terminado`,
`subagente.sumiu`, `relatorio.substituido`, `ciclo`, `evento`, `daemon`,
`fase`, `foco`, `conversa`, `fonte.conectada`, `fonte.erro`,
`fonte.sincronizada`.

- **Primeiro instantâneo**: não reencena o passado (concluídos antigos não
  viram relatório; cursores de ciclos/eventos começam no último id).
- **Atribuição**: ver [ESTADOS.md](ESTADOS.md#3-immos-o-que-são-no-runtime).
- **Leitura falha**: mantém atribuições e estado.

## 4. Protocolo com o cliente

`GET /api/events` (Server-Sent Events):

| Evento | Quando | Conteúdo |
|---|---|---|
| `hello` | ao conectar | `{ protocol: 1, layoutVersion, server: { version, source, timeScale, clockReal } }` |
| `tick` | 10/s (`server.broadcastHz`) | `{ tick, simMs, wallMs, clock, entities[], fx[] }` |
| `meta` | 1/s | `{ tick, runtime (resumo + status da fonte + transbordo), journal, env (sol, clima, luz), stats }` |

Cada entidade em `tick.entities`: `id, kind, name, present, x, z, h`
(orientação), `spd, act` (atividade), `pose, poseFrom, poseT` (transição),
`poi, intent, dest, path` (caminho restante), `mode`, `logical` (estado
lógico do runtime) e, no Abiyss, `mind`.

Ao conectar, o servidor manda `meta` e `tick` imediatamente. Clientes lentos
não travam o servidor: se o buffer do socket enche, quadros são descartados
para aquele cliente até ele esvaziar (contados em `/api/health`).

Outros endpoints:

| Rota | Uso |
|---|---|
| `GET /api/state` | visão completa do mundo (JSON) |
| `GET /api/health` | versão, fonte, clima, estatísticas, clientes, log recente |
| `POST /api/demo/delegate?nivel=ultra` | só com a fonte demo: força uma delegação (responde 409 com o runtime real) |

### Interpolação no cliente

O cliente toca a linha do tempo ~150 ms atrás do quadro mais novo, na
escala de tempo anunciada pelo servidor, interpolando posição e orientação
entre os dois quadros vizinhos; estados discretos (atividade, pose) vêm do
quadro mais próximo. Reinício do servidor (tempo voltando) zera a linha do
tempo. Reconexão é automática (`retry: 2000`).

## 5. Persistência e reinício

`data/office-state.json` é gravado a cada 15 s e ao parar (`SIGINT`/`SIGTERM`),
de forma atômica (arquivo temporário + `rename`). Guarda posições, energia,
última atividade livre e o estado da ponte (atribuições sub-agente → IMMo),
marcado com a **chave da fonte** (o caminho do banco; na demo a chave é única por execução, porque o emulador recomeça do zero — então só as posições voltam).

Ao reiniciar:

- posições inválidas (dentro de parede, móvel ou assento) são levadas à
  célula livre mais próxima;
- as atribuições só são restauradas se a fonte for a mesma; sub-agentes
  que terminaram há mais de 1 min com o escritório desligado não viram
  relatório;
- os planos não são salvos: cada entidade decide de novo a partir de onde
  está (um IMMo que estava trabalhando volta a sentar na mesa se o
  sub-agente ainda estiver `executando`);
- arquivo corrompido é renomeado para `.corrompido` e o escritório começa
  do zero (com a entrada pela porta).

`--fresh` ignora o estado salvo; `--no-persist` não grava.
