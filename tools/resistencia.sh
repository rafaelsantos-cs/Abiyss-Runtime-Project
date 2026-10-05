#!/usr/bin/env bash
# Teste de resistência (soak) do daemon do Abiyss.
#
# Sobe o mock do NIM (roteiro "resistencia": responde como heartbeat e
# sub-agentes de verdade), monta um projeto temporário com heartbeats
# acelerados, roda `abiyss daemon` pelo tempo pedido e grava amostras de
# memória e do banco num CSV. No fim, para tudo e imprime um resumo.
#
# Uso:
#   tools/resistencia.sh [opções]
#
#   --duracao SEG       quanto tempo rodar (padrão 600 = 10 min)
#   --intervalo SEG     de quanto em quanto tempo amostrar (padrão 10)
#   --heartbeat SEG     intervalo do heartbeat (padrão 2; o real é 300)
#   --atraso-ms MS      latência de mentira do mock (padrão 200)
#   --saida ARQ         CSV de saída (padrão resistencia-AAAAmmdd-HHMMSS.csv)
#   --pasta DIR         pasta do projeto temporário (padrão: mktemp)
#   --binario ARQ       binário do abiyss (padrão target/release/abiyss)
#   --sem-mcp           não sobe o servidor MCP de exemplo (dispensa o uv)
#   --cache-kib KIB     teto do cache de páginas do SQLite (padrão 2048)
#
# Colunas do CSV:
#   momento         data/hora da amostra (ISO 8601, UTC)
#   segundos        segundos desde o início do daemon
#   rss_daemon_kb   memória residente do daemon (VmRSS)
#   rss_mcp_kb      memória residente dos servidores MCP (uv + Python)
#   threads         threads do daemon
#   fds             descritores de arquivo abertos pelo daemon
#   db_bytes        tamanho de data/abiyss.db
#   wal_bytes       tamanho de data/abiyss.db-wal
#   chamadas        linhas em chamadas_modelo (vazio sem o sqlite3)
#   ciclos          linhas em ciclos (vazio sem o sqlite3)
#
# Requisitos: Linux (/proc), bash, curl. Opcional: sqlite3 (contagens),
# uv (servidor MCP de exemplo).

set -euo pipefail

DURACAO=600
INTERVALO=10
HEARTBEAT=2
ATRASO_MS=200
SAIDA=""
PASTA=""
RAIZ_REPO="$(cd "$(dirname "$0")/.." && pwd)"
BINARIO="$RAIZ_REPO/target/release/abiyss"
COM_MCP=1
CACHE_KIB=2048

while [[ $# -gt 0 ]]; do
    case "$1" in
        --duracao) DURACAO="$2"; shift 2 ;;
        --intervalo) INTERVALO="$2"; shift 2 ;;
        --heartbeat) HEARTBEAT="$2"; shift 2 ;;
        --atraso-ms) ATRASO_MS="$2"; shift 2 ;;
        --saida) SAIDA="$2"; shift 2 ;;
        --pasta) PASTA="$2"; shift 2 ;;
        --binario) BINARIO="$2"; shift 2 ;;
        --sem-mcp) COM_MCP=0; shift ;;
        --cache-kib) CACHE_KIB="$2"; shift 2 ;;
        -h|--help) sed -n '2,36p' "$0"; exit 0 ;;
        *) echo "opção desconhecida: $1" >&2; exit 2 ;;
    esac
done

[[ -x "$BINARIO" ]] || { echo "binário não encontrado: $BINARIO (rode cargo build --release)" >&2; exit 1; }
[[ -n "$SAIDA" ]] || SAIDA="resistencia-$(date +%Y%m%d-%H%M%S).csv"
[[ -n "$PASTA" ]] || PASTA="$(mktemp -d -t abiyss-resistencia-XXXXXX)"
SAIDA="$(realpath -m "$SAIDA")"
mkdir -p "$PASTA/identity" "$PASTA/data"

# Uma porta livre para o mock.
PORTA="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])' 2>/dev/null || echo 18089)"

# ---------------------------------------------------------------------------
# Projeto temporário
# ---------------------------------------------------------------------------

echo "Sou o Abiyss do teste de resistência." > "$PASTA/identity/nucleo.md"
cat > "$PASTA/.env" <<EOF
NIM_RESISTENCIA_CEREBRO=nvapi-resistencia-cerebro
NIM_RESISTENCIA_SUB=nvapi-resistencia-sub
EOF

MCP_TOML=""
if [[ "$COM_MCP" == 1 ]]; then
    MCP_TOML="
[[mcp.servidores]]
nome = \"exemplo\"
comando = \"uv\"
args = [\"run\", \"--frozen\", \"--quiet\", \"servidor.py\"]
diretorio = \"$RAIZ_REPO/recursos/mcp/exemplo\"
timeout_segundos = 30
"
fi

cat > "$PASTA/abiyss.toml" <<EOF
# Gerado por tools/resistencia.sh — heartbeats acelerados contra o mock.
[nim]
base_url = "http://127.0.0.1:$PORTA/v1"

[modelos.cerebro]
id = "resistencia/cerebro"
max_tokens = 4096
[modelos.sub_ultra]
id = "resistencia/ultra"
[modelos.sub_medium]
id = "resistencia/medium"
[modelos.sub_low]
id = "resistencia/low"

[pools.cerebro]
api_key_env = "NIM_RESISTENCIA_CEREBRO"
requisicoes_por_minuto = 600
rajada = 5
reserva_conversa_por_minuto = 10
[pools.subagentes]
api_key_env = "NIM_RESISTENCIA_SUB"
requisicoes_por_minuto = 600
rajada = 5

[daemon]
heartbeat_segundos = $HEARTBEAT
cron_verificacao_segundos = 1
# Chama o modelo em todo ciclo (sem esperar novidade).
revisao_minima_segundos = 1

[subagentes]
verificacao_segundos = 1
max_simultaneos = 4

[banco]
cache_kib = $CACHE_KIB

[retencao]
# Manutenção e checkpoint acelerados (a retenção em dias continua a real).
manutencao_minutos = 1
checkpoint_minutos = 1

[mcp]
timeout_inicio_segundos = 120
supervisao_segundos = 10
$MCP_TOML
EOF

ABIYSS=("$BINARIO" --config "$PASTA/abiyss.toml")

# ---------------------------------------------------------------------------
# Mock, goal, cron e daemon
# ---------------------------------------------------------------------------

PID_MOCK=""
PID_DAEMON=""
parar_tudo() {
    [[ -n "$PID_DAEMON" ]] && kill -TERM "$PID_DAEMON" 2>/dev/null && wait "$PID_DAEMON" 2>/dev/null || true
    [[ -n "$PID_MOCK" ]] && kill -TERM "$PID_MOCK" 2>/dev/null && wait "$PID_MOCK" 2>/dev/null || true
}
trap parar_tudo EXIT

"$BINARIO" mock-nim --porta "$PORTA" --roteiro resistencia --atraso-ms "$ATRASO_MS" \
    > "$PASTA/mock.log" 2>&1 &
PID_MOCK=$!
for _ in $(seq 1 50); do
    curl -sf "http://127.0.0.1:$PORTA/v1/models" > /dev/null && break
    sleep 0.2
done

"${ABIYSS[@]}" goal add "Resistência" --nucleo "Manter o Abiyss estável por horas." > /dev/null
# Um evento por minuto na fila.
"${ABIYSS[@]}" cron add pulso "* * * * *" "pulso do teste de resistência" > /dev/null

RUST_LOG=abiyss=info "${ABIYSS[@]}" daemon > "$PASTA/daemon.log" 2>&1 &
PID_DAEMON=$!
INICIO=$(date +%s)

echo "Projeto:  $PASTA"
echo "Mock:     http://127.0.0.1:$PORTA/v1 (pid $PID_MOCK)"
echo "Daemon:   pid $PID_DAEMON, heartbeat a cada ${HEARTBEAT}s, duração ${DURACAO}s"
echo "CSV:      $SAIDA"

# ---------------------------------------------------------------------------
# Amostras
# ---------------------------------------------------------------------------

# VmRSS (kB) de um PID, ou 0.
rss_kb() { awk '/^VmRSS:/ {print $2; f=1} END {if (!f) print 0}' "/proc/$1/status" 2>/dev/null || echo 0; }

# Soma do VmRSS de todos os descendentes de um PID (os servidores MCP).
rss_descendentes_kb() {
    local total=0 filho
    for filho in $(pgrep -P "$1" 2>/dev/null); do
        total=$(( total + $(rss_kb "$filho") + $(rss_descendentes_kb "$filho") ))
    done
    echo "$total"
}

tamanho() { stat -c %s "$1" 2>/dev/null || echo 0; }

# Linhas de uma tabela (sqlite3 da linha de comando ou, sem ele, o do Python).
contar() {
    local banco="$PASTA/data/abiyss.db"
    if command -v sqlite3 > /dev/null; then
        sqlite3 -readonly "$banco" "PRAGMA busy_timeout=2000; SELECT COUNT(*) FROM $1;" 2>/dev/null | tail -1
    elif command -v python3 > /dev/null; then
        python3 - "$banco" "$1" <<'PY' 2>/dev/null || echo ""
import sqlite3, sys
conexao = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True, timeout=2)
print(conexao.execute(f"SELECT COUNT(*) FROM {sys.argv[2]}").fetchone()[0])
PY
    else
        echo ""
    fi
}

echo "momento,segundos,rss_daemon_kb,rss_mcp_kb,threads,fds,db_bytes,wal_bytes,chamadas,ciclos" > "$SAIDA"
CAIU=0
while true; do
    AGORA=$(date +%s)
    DECORRIDO=$(( AGORA - INICIO ))
    if ! kill -0 "$PID_DAEMON" 2>/dev/null; then
        echo "ERRO: o daemon morreu aos ${DECORRIDO}s (ver $PASTA/daemon.log)" >&2
        CAIU=1
        break
    fi
    printf '%s,%d,%d,%d,%d,%d,%d,%d,%s,%s\n' \
        "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$DECORRIDO" \
        "$(rss_kb "$PID_DAEMON")" "$(rss_descendentes_kb "$PID_DAEMON")" \
        "$(awk '/^Threads:/ {print $2}' /proc/"$PID_DAEMON"/status)" \
        "$(ls /proc/"$PID_DAEMON"/fd | wc -l)" \
        "$(tamanho "$PASTA/data/abiyss.db")" "$(tamanho "$PASTA/data/abiyss.db-wal")" \
        "$(contar chamadas_modelo)" "$(contar ciclos)" >> "$SAIDA"
    (( DECORRIDO >= DURACAO )) && break
    sleep "$INTERVALO"
done

# ---------------------------------------------------------------------------
# Parada e resumo
# ---------------------------------------------------------------------------

kill -TERM "$PID_DAEMON" 2>/dev/null || true
STATUS_DAEMON=0
wait "$PID_DAEMON" 2>/dev/null || STATUS_DAEMON=$?
PID_DAEMON=""

echo
echo "== Resumo =="
awk -F, 'NR == 2 { r0 = $3; d0 = $7; t0 = $2; c0 = $9 }
         NR > 1  { if ($3 > rmax) rmax = $3; if ($4 > mmax) mmax = $4; if ($6 > fmax) fmax = $6;
                   if ($8 > wmax) wmax = $8; if ($5 > tmax) tmax = $5;
                   r1 = $3; d1 = $7; t1 = $2; c1 = $9; n++ }
         END {
           horas = (t1 - t0) / 3600; if (horas <= 0) horas = 1;
           printf "amostras: %d em %d s\n", n, t1 - t0;
           printf "RSS do daemon: início %d kB, fim %d kB, máximo %d kB\n", r0, r1, rmax;
           printf "RSS dos servidores MCP (máx.): %d kB\n", mmax;
           printf "threads (máx.): %d; descritores (máx.): %d\n", tmax, fmax;
           printf "banco: início %d B, fim %d B (%.0f B/h); WAL máx. %d B\n", d0, d1, (d1 - d0) / horas, wmax;
           if (c1 != "") printf "chamadas ao modelo registradas: %d (%.0f/h)\n", c1, (c1 - c0) / horas;
         }' "$SAIDA"
echo "saída do daemon no SIGTERM: $STATUS_DAEMON"
echo
echo "== abiyss status =="
"${ABIYSS[@]}" status | sed -n '/^Daemon/,/^Identidade/p;/^Latência/,/^Interocepção/p' | grep -v '^Interocepção' || true
echo
echo "Reinícios de servidores MCP no log: $(grep -c 'reiniciando' "$PASTA/daemon.log" || true)"
echo "Erros no log: $(grep -c ' ERROR ' "$PASTA/daemon.log" || true)"

if [[ "$CAIU" == 1 || "$STATUS_DAEMON" != 0 ]]; then
    exit 1
fi
