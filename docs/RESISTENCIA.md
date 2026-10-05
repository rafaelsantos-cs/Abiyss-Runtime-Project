# Teste de resistência (soak)

[`tools/resistencia.sh`](../tools/resistencia.sh) roda o daemon de verdade
por um tempo longo, com heartbeats acelerados, contra o mock do NIM, e mede
se algo cresce sem parar.

## O que ele faz

1. Monta um projeto temporário (`abiyss.toml`, núcleo, `.env` de mentira):
   - heartbeat a cada 2 s (`--heartbeat`), `revisao_minima_segundos = 1`:
     **toda** volta chama o modelo;
   - pools a 600 req/min (o mock não limita);
   - manutenção do banco e checkpoint do WAL a cada 1 min (a retenção em
     dias continua a real: nada envelhece o bastante num teste curto);
   - supervisão MCP a cada 10 s, com o servidor de exemplo (`--sem-mcp` tira).
2. Sobe `abiyss mock-nim --roteiro resistencia`: o "cérebro" devolve
   decisões válidas (a cada 4 ciclos delega um sub-agente `low`, nas outras
   aguarda), os sub-agentes devolvem relatório `concluido`. Respostas de
   alguns KB, com raciocínio, e 200 ms de latência (`--atraso-ms`).
3. Cria um goal (sempre em foco) e um cron por minuto (eventos na fila).
4. Roda `abiyss daemon` e, a cada `--intervalo` s, grava uma linha no CSV:

   | coluna | significado |
   |---|---|
   | `rss_daemon_kb` | memória residente do daemon |
   | `rss_mcp_kb` | memória dos servidores MCP (uv + Python) |
   | `threads`, `fds` | threads e descritores abertos do daemon |
   | `db_bytes`, `wal_bytes` | `data/abiyss.db` e `data/abiyss.db-wal` |
   | `chamadas`, `ciclos` | linhas em `chamadas_modelo` e `ciclos` |

5. No fim: SIGTERM (confere que o daemon sai com 0), resumo, o trecho de
   latência do `abiyss status`, reinícios MCP e erros no log. Sai com 1 se o
   daemon morreu no meio ou não parou limpo.

Os logs ficam na pasta do projeto (`daemon.log`, `mock.log`), impressa no início.

## Resultado: 10 minutos (container de desenvolvimento, x86_64)

Duas execuções de 600 s, heartbeat de 2 s, mock com 200 ms de latência,
servidor MCP de exemplo ligado, binário `--release`. CSVs completos em
[`resistencia/`](resistencia/). A única diferença entre elas é o teto do
cache de páginas do SQLite (`[banco] cache_kib`).

| | A: `cache_kib = 2048` (padrão) | B: `cache_kib = 256` |
|---|---|---|
| CSV | [`2026-10-05-10min.csv`](resistencia/2026-10-05-10min.csv) | [`2026-10-05-10min-cache256.csv`](resistencia/2026-10-05-10min-cache256.csv) |
| chamadas ao modelo | 404 (303 heartbeat + 101 sub-agente), 0 falhas | 404, 0 falhas |
| latência p50 / p95 (1º token e total) | 202 / 202 ms | 202 / 202 ms |
| RSS do daemon: 0 s → 60 s → 600 s | 12,9 → 16,3 → 18,3 MiB | 13,2 → 16,1 → 16,4 MiB |
| inclinação do RSS na 2ª metade | +12,5 MiB/h (acompanha o banco) | +0,8 MiB/h (e caindo: +28 kB nos últimos 3 min) |
| RSS dos servidores MCP (uv + Python) | 94,8 MiB, estável | 92,5 MiB, estável |
| threads / descritores | 5–6 / 16–18, estáveis | 5–6 / 16–18, estáveis |
| banco (`abiyss.db`) | 0,16 → 1,78 MiB (~9,6 MiB/h) | igual |
| WAL | máx. 2,5 MiB, truncado a cada checkpoint (1 min) | máx. 2,5 MiB |
| reinícios MCP / erros no log | 0 / 0 | 0 / 0 |
| saída no SIGTERM | 0 | 0 |

Leitura:

- **Sem vazamento visível de threads, descritores, WAL nem processos MCP.**
- O RSS do daemon na execução A sobe junto com o banco porque o cache de
  páginas do SQLite (por conexão) vai se enchendo até o teto: em 10 min o
  banco (1,8 MiB) ainda não passou dos 2 MiB do cache. Com o teto em 256 KiB
  (B), o RSS para de subir assim que o banco passa do teto, o que confirma a
  causa. Com o padrão, o platô esperado é ~19 MiB: 16 MiB de base mais os
  2 MiB do cache.
- Resta um resíduo pequeno na execução B (+0,8 MiB/h na 2ª metade, ~0,5
  MiB/h nos últimos 3 min: +28 kB). Dez minutos não bastam para dizer
  se é aquecimento do alocador ou vazamento lento: é exatamente isso que a
  execução de 24–48 h na VM precisa responder (ver abaixo).
- Crescimento do banco: ~9,6 MiB/h para ~2 400 chamadas/h, ou ~4 KiB por
  chamada (registro da chamada + ciclo com a resposta + diário + sub-agente).
  Com o heartbeat real (300 s, e só quando há novidade) são no máximo
  ~12 chamadas/h do heartbeat, ou seja, ~50 KiB/h e ~35 MiB em 30 dias de
  retenção.

## Como rodar 24–48 h na VM

O teste usa só o mock: não gasta cota, não precisa das chaves reais e não
mexe no `data/` de produção (o projeto é temporário). Pode rodar com o
daemon de produção no ar. A VM tem só 2 OCPU, então use um heartbeat menos
agressivo.

```bash
cd ~/Abiyss-Runtime-Project
git pull && cargo build --release
sudo apt-get install -y sqlite3        # opcional (sem ele, usa o python3)

# Fora da sessão SSH: o teste sobrevive ao logout (linger = o systemd do
# usuário continua de pé sem sessão aberta; PATH = o uv para o servidor MCP).
mkdir -p ~/resistencia
loginctl enable-linger "$USER"
systemd-run --user --unit=abiyss-resistencia --setenv=PATH="$PATH" \
  ~/Abiyss-Runtime-Project/tools/resistencia.sh \
    --duracao 172800 --intervalo 60 --heartbeat 5 \
    --saida ~/resistencia/48h.csv --pasta ~/resistencia/projeto
# (se o systemd --user não estiver disponível: tmux, ou
#  nohup tools/resistencia.sh ... > ~/resistencia/48h.txt 2>&1 &)

# Acompanhar
journalctl --user -u abiyss-resistencia -f   # resumo final aparece aqui
tail -f ~/resistencia/48h.csv
tail -f ~/resistencia/projeto/daemon.log
```

`--duracao 86400` = 24 h; `172800` = 48 h. Para parar antes: `systemctl
--user stop abiyss-resistencia` (o script para o daemon e o mock).

### O que conferir no CSV

- **`rss_daemon_kb`**: sobe nos primeiros minutos (caches do SQLite, pool de
  conexões, buffers do tokio) e depois fica num **platô**. Uma subida lenta e
  constante por horas é vazamento. Inclinação da segunda metade do teste, em
  kB/h:

  ```bash
  awk -F, 'NR>1 {t[NR]=$2; r[NR]=$3; n=NR}
           END {m=int(n/2); printf "%.1f kB/h\n", (r[n]-r[m])/((t[n]-t[m])/3600)}' ~/resistencia/48h.csv
  ```

- **`threads` e `fds`**: devem ficar constantes (variação de ±2 é normal
  durante uma chamada). Crescimento = tarefa ou conexão vazando.
- **`rss_mcp_kb`**: constante; um degrau seguido de queda é a supervisão
  reiniciando o servidor (confira `reiniciando` no `daemon.log`).
- **`wal_bytes`**: cai para perto de zero a cada checkpoint (1 min aqui); não
  deve crescer de um checkpoint para o outro.
- **`db_bytes`**: cresce **linearmente** com as chamadas, porque nada
  envelhece além da retenção (30 dias) em 48 h. Para estimar o tamanho em
  regime: (bytes por hora ÷ chamadas por hora) × chamadas por dia reais ×
  30 dias. Com o heartbeat real (300 s) há ~150× menos chamadas que com 2 s.
- No fim: `saída do daemon no SIGTERM: 0`, `Erros no log: 0` e latência p95
  perto do `--atraso-ms`.

A retenção em si não age num teste de 48 h (nada passa de 30 dias). Ela é
coberta por `cargo test --test i3_retencao`, com 60 dias de dados sintéticos.
