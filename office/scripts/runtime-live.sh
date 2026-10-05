#!/usr/bin/env bash
# Abiyss Office ligado ao RUNTIME REAL (kernel em Rust), sem gastar cota:
#
#   abiyss mock-nim --roteiro resistencia   (NIM de mentira do próprio runtime)
#   abiyss daemon                           (heartbeat, crons, sub-agentes de verdade)
#   node server/main.js --source runtime    (escritório lendo data/abiyss.db)
#
# O projeto do runtime é montado numa pasta temporária, como em
# tools/resistencia.sh do runtime. Para haver vários sub-agentes ao mesmo
# tempo, o script registra pedidos de delegação do mesmo jeito que a
# ferramenta `delegar` do kernel (INSERT em `subagentes` com
# estado='pendente'); quem os EXECUTA é o daemon real.
#
# Uso:
#   scripts/runtime-live.sh --binario /caminho/para/target/release/abiyss [opções]
#
#   --binario ARQ        binário do runtime (ou ABIYSS_BIN)
#   --pasta DIR          pasta do projeto temporário (padrão: mktemp)
#   --heartbeat SEG      intervalo do heartbeat (padrão 8; o real é 300)
#   --atraso-ms MS       latência de cada resposta do mock (padrão 9000)
#   --delegar-cada SEG   intervalo entre delegações extras (padrão 12; 0 = não delegar)
#   --porta N            porta do escritório (padrão 8090)
#   --duracao SEG        para tudo depois de SEG segundos (padrão 0 = até Ctrl+C)

set -euo pipefail

BINARIO="${ABIYSS_BIN:-}"
PASTA=""
HEARTBEAT=8
ATRASO_MS=9000
DELEGAR_CADA=12
PORTA_OFFICE=8090
DURACAO=0
RAIZ="$(cd "$(dirname "$0")/.." && pwd)"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --binario) BINARIO="$2"; shift 2 ;;
        --pasta) PASTA="$2"; shift 2 ;;
        --heartbeat) HEARTBEAT="$2"; shift 2 ;;
        --atraso-ms) ATRASO_MS="$2"; shift 2 ;;
        --delegar-cada) DELEGAR_CADA="$2"; shift 2 ;;
        --porta) PORTA_OFFICE="$2"; shift 2 ;;
        --duracao) DURACAO="$2"; shift 2 ;;
        -h|--help) sed -n '2,26p' "$0"; exit 0 ;;
        *) echo "opção desconhecida: $1" >&2; exit 2 ;;
    esac
done

[[ -n "$BINARIO" && -x "$BINARIO" ]] || { echo "binário do runtime não encontrado (use --binario ou ABIYSS_BIN)" >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 é necessário (para registrar delegações no SQLite)" >&2; exit 1; }
[[ -n "$PASTA" ]] || PASTA="$(mktemp -d -t abiyss-office-runtime-XXXXXX)"
mkdir -p "$PASTA/identity" "$PASTA/data"
PORTA_MOCK="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"

echo "Sou o Abiyss rodando para o Abiyss Office." > "$PASTA/identity/nucleo.md"
cat > "$PASTA/.env" <<EOF
NIM_OFFICE_CEREBRO=nvapi-office-cerebro
NIM_OFFICE_SUB=nvapi-office-sub
EOF
cat > "$PASTA/abiyss.toml" <<EOF
# Gerado por office/scripts/runtime-live.sh — heartbeat acelerado contra o mock.
[nim]
base_url = "http://127.0.0.1:$PORTA_MOCK/v1"

[modelos.cerebro]
id = "office/cerebro"
max_tokens = 4096
[modelos.sub_ultra]
id = "office/ultra"
[modelos.sub_medium]
id = "office/medium"
[modelos.sub_low]
id = "office/low"

[pools.cerebro]
api_key_env = "NIM_OFFICE_CEREBRO"
requisicoes_por_minuto = 600
rajada = 5
reserva_conversa_por_minuto = 10
[pools.subagentes]
api_key_env = "NIM_OFFICE_SUB"
requisicoes_por_minuto = 600
rajada = 8

[daemon]
heartbeat_segundos = $HEARTBEAT
cron_verificacao_segundos = 2
revisao_minima_segundos = 30

[ritmo]
horas_ativas = "07:00-23:00"

[subagentes]
verificacao_segundos = 1
max_simultaneos = 4
EOF

ABIYSS=("$BINARIO" --config "$PASTA/abiyss.toml")
PIDS=()
parar_tudo() {
    for ((i=${#PIDS[@]}-1; i>=0; i--)); do kill -TERM "${PIDS[$i]}" 2>/dev/null || true; done
    wait 2>/dev/null || true
}
trap parar_tudo EXIT INT TERM

"$BINARIO" mock-nim --porta "$PORTA_MOCK" --roteiro resistencia --atraso-ms "$ATRASO_MS" > "$PASTA/mock.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 1 50); do curl -sf "http://127.0.0.1:$PORTA_MOCK/v1/models" > /dev/null && break; sleep 0.2; done

"${ABIYSS[@]}" goal add "Migrar a memória do Hermes" --nucleo "Importar e validar a memória antiga." --prioridade 2 > /dev/null
"${ABIYSS[@]}" goal mover 1 comprometido --motivo "escritório: preparação" > /dev/null
"${ABIYSS[@]}" goal mover 1 executando --motivo "escritório: preparação" > /dev/null
"${ABIYSS[@]}" goal add "Calibrar o orçamento diário" --nucleo "Medir chamadas por dia." --prioridade 1 > /dev/null
"${ABIYSS[@]}" cron add pulso "* * * * *" "pulso do escritório" > /dev/null

RUST_LOG=abiyss=info "${ABIYSS[@]}" daemon > "$PASTA/daemon.log" 2>&1 &
PID_DAEMON=$!
PIDS+=($PID_DAEMON)
for _ in $(seq 1 50); do [[ -f "$PASTA/data/abiyss.db" ]] && break; sleep 0.2; done

if [[ "$DELEGAR_CADA" != "0" ]]; then
    python3 - "$PASTA/data/abiyss.db" "$DELEGAR_CADA" <<'PY' &
import random, sqlite3, sys, time
db, every = sys.argv[1], float(sys.argv[2])
tarefas = ["Resumir o changelog do qmd", "Verificar links vencidos em 02_external",
           "Comparar a latência p95 dos modelos", "Classificar propostas de memória",
           "Checar o uso de disco do cofre", "Ler a documentação do Piper"]
niveis = ["ultra", "medium", "low", "low"]
rnd = random.Random(7)
while True:
    con = sqlite3.connect(db, timeout=5)
    vivos = con.execute("SELECT COUNT(*) FROM subagentes WHERE estado IN ('pendente','executando')").fetchone()[0]
    if vivos < 4:
        # Mesmas colunas e valores que ControleSubagentes::delegar grava.
        con.execute("""INSERT INTO subagentes (nivel, tarefa, contexto, prazo_segundos, goal_id, origem, estado, criado_ms)
                       VALUES (?, ?, '', 600, 1, 'office-live', 'pendente', ?)""",
                    (rnd.choice(niveis), rnd.choice(tarefas), int(time.time() * 1000)))
        con.commit()
    con.close()
    time.sleep(every * (0.7 + rnd.random() * 0.6))
PY
    PIDS+=($!)
fi

echo "Runtime:   $PASTA (daemon pid $PID_DAEMON, heartbeat ${HEARTBEAT}s, mock ${ATRASO_MS} ms)"
echo "Escritório: http://127.0.0.1:$PORTA_OFFICE/"
node --disable-warning=ExperimentalWarning "$RAIZ/server/main.js" --source runtime \
    --runtime-db "$PASTA/data/abiyss.db" --runtime-config "$PASTA/abiyss.toml" \
    --port "$PORTA_OFFICE" --fresh --no-persist &
PIDS+=($!)

if [[ "$DURACAO" != "0" ]]; then
    sleep "$DURACAO"
else
    wait
fi
