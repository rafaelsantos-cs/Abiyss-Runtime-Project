#!/usr/bin/env sh
# Abiyss Office — lançador para Linux e macOS.
# Uso: ./iniciar.sh [opções do servidor]   (ex.: ./iniciar.sh --fresh)
cd "$(dirname "$0")" || exit 1

if ! command -v node >/dev/null 2>&1; then
  echo "O Node.js não foi encontrado. Instale a versão LTS (22.13 ou mais nova): https://nodejs.org/"
  exit 1
fi
if ! node -e "const [a,b]=process.versions.node.split('.').map(Number);process.exit(a>22||(a===22&&b>=13)?0:1)"; then
  echo "Seu Node.js ($(node -v)) é antigo demais: o Abiyss Office precisa da 22.13 ou mais nova."
  exit 1
fi
if [ ! -f node_modules/three/build/three.module.js ]; then
  echo "Instalando a dependência (three.js)..."
  npm ci --omit=dev || exit 1
fi

echo "Abiyss Office iniciando em http://127.0.0.1:8090/ (Ctrl+C para parar)"
exec node --disable-warning=ExperimentalWarning server/main.js --open "$@"
