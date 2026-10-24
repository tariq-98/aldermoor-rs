#!/bin/zsh
# Serves the repo root and prints the workshop's address. Usage: tools/workshop/serve.sh [port]
set -euo pipefail
ROOT=${0:A:h:h:h}
cd "${0:A:h}"
[ -d node_modules/three ] || npm install --silent
port=${1:-8765}
while lsof -nP -iTCP:$port -sTCP:LISTEN >/dev/null 2>&1; do port=$((port + 1)); done
url="http://localhost:$port/tools/workshop/"
echo "Aldermoor workshop: $url"
[ -t 1 ] && command -v open >/dev/null && (sleep 1; open "$url") &
exec python3 -m http.server "$port" --bind 127.0.0.1 --directory "$ROOT"
