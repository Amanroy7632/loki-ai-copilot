#!/usr/bin/env sh
# One command for a local end-to-end run: Ollama (loopback, cloud off) + the Loki desktop app
# (Tauri + Vite with hot reload), or the CLI chat. Ctrl+C stops everything this script started.
#
#   ./scripts/dev.sh                      desktop app; Ollama if installed; data in .dev-data/
#   ./scripts/dev.sh --model bonsai-2:4b  also select that model for this data dir
#   ./scripts/dev.sh --model bonsai-2:4b --pull   download it first if missing (shows the license)
#   ./scripts/dev.sh --mock               no Ollama: the built-in mock model (clearly labelled)
#   ./scripts/dev.sh --cli                the terminal chat instead of the desktop app
#   ./scripts/dev.sh --fresh              start from an empty .dev-data/ (deletes only that folder)
#   ./scripts/dev.sh --real-data          use your normal per-user Loki data instead of .dev-data/
#   ./scripts/dev.sh --check              print what would happen, start nothing
#
# On Windows, run it from Git Bash. Nothing here downloads anything unless you pass --pull.
set -eu
cd "$(dirname "$0")/.."
ROOT=$(pwd)

MODE=desktop
MODEL=""
PULL=0
MOCK=0
FRESH=0
REAL_DATA=0
CHECK=0
while [ $# -gt 0 ]; do
  case "$1" in
    --model) MODEL=${2:?--model needs an id, e.g. bonsai-2:4b}; shift ;;
    --pull) PULL=1 ;;
    --mock) MOCK=1 ;;
    --cli) MODE=cli ;;
    --fresh) FRESH=1 ;;
    --real-data) REAL_DATA=1 ;;
    --check) CHECK=1 ;;
    -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
  shift
done

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# ------------------------------------------------------------------ prerequisites
command -v cargo >/dev/null 2>&1 || die "Rust is not installed (https://rustup.rs)"
command -v pnpm >/dev/null 2>&1 || die "pnpm is not installed (npm install -g pnpm)"
OLLAMA_OK=1
command -v ollama >/dev/null 2>&1 || OLLAMA_OK=0
if [ "$MOCK" = 0 ] && [ "$OLLAMA_OK" = 0 ]; then
  warn "ollama is not installed (macOS: brew install ollama); using the mock model instead"
  MOCK=1
fi

# ------------------------------------------------------------------ data dir
if [ "$REAL_DATA" = 1 ]; then
  [ "$FRESH" = 1 ] && die "--fresh only works with the dev data dir, never with --real-data"
  unset LOKI_DATA_DIR || true
  DATA_LABEL="your normal per-user Loki data"
else
  LOKI_DATA_DIR="$ROOT/.dev-data"
  export LOKI_DATA_DIR
  DATA_LABEL="$LOKI_DATA_DIR"
fi

if [ "$CHECK" = 1 ]; then
  say "plan (nothing started):"
  echo "  mode:      $MODE"
  echo "  data:      $DATA_LABEL$( [ "$FRESH" = 1 ] && echo ' (wiped first)')"
  if [ "$MOCK" = 1 ]; then echo "  model:     mock (no Ollama)"; else echo "  model:     ${MODEL:-the model selected in the data dir config}$( [ "$PULL" = 1 ] && echo ', downloaded if missing')"; fi
  echo "  ollama:    $( [ "$MOCK" = 1 ] && echo 'not used' || echo 'reuse if running, else start on 127.0.0.1:11434 with OLLAMA_NO_CLOUD=1')"
  exit 0
fi

if [ "$FRESH" = 1 ] && [ -d "$ROOT/.dev-data" ]; then
  say "wiping $ROOT/.dev-data (dev data only)"
  rm -rf "$ROOT/.dev-data"
fi
[ "$REAL_DATA" = 1 ] || mkdir -p "$LOKI_DATA_DIR"

# ------------------------------------------------------------------ cleanup on exit
STARTED_OLLAMA=""
cleanup() {
  if [ -n "$STARTED_OLLAMA" ]; then
    say "stopping the Ollama server this script started (pid $STARTED_OLLAMA)"
    kill "$STARTED_OLLAMA" 2>/dev/null || true
  fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# ------------------------------------------------------------------ JS dependencies
if [ ! -d node_modules ] || [ pnpm-lock.yaml -nt node_modules/.modules.yaml ]; then
  say "installing JS dependencies (pnpm install --frozen-lockfile)"
  pnpm install --frozen-lockfile
fi

loki() { cargo run -q -p loki-cli -- "$@"; }

# ------------------------------------------------------------------ Ollama
OLLAMA_URL=http://127.0.0.1:11434
if [ "$MOCK" = 0 ]; then
  if curl -fsS --max-time 2 "$OLLAMA_URL/api/version" >/dev/null 2>&1; then
    say "reusing the Ollama server already running on 127.0.0.1:11434"
    warn "if it was started by the Ollama app or 'brew services', cloud features may be ON; scripts/ollama-serve.sh starts one with them off"
  else
    LOG_DIR=${LOKI_DATA_DIR:-$ROOT/.dev-data}
    mkdir -p "$LOG_DIR"
    say "starting Ollama on 127.0.0.1:11434 with cloud features off (log: $LOG_DIR/ollama.log)"
    OLLAMA_HOST=127.0.0.1:11434 OLLAMA_NO_CLOUD=1 ollama serve >"$LOG_DIR/ollama.log" 2>&1 &
    STARTED_OLLAMA=$!
    i=0
    until curl -fsS --max-time 1 "$OLLAMA_URL/api/version" >/dev/null 2>&1; do
      i=$((i + 1))
      [ "$i" -gt 30 ] && die "Ollama did not start within 30 s; see $LOG_DIR/ollama.log"
      kill -0 "$STARTED_OLLAMA" 2>/dev/null || die "Ollama exited; see $LOG_DIR/ollama.log"
      sleep 1
    done
  fi

  if [ -n "$MODEL" ]; then
    if ! ollama list 2>/dev/null | awk 'NR > 1 { print $1 }' | grep -qx "$MODEL"; then
      if [ "$PULL" = 1 ]; then
        say "downloading $MODEL (the license is shown first; type yes to accept)"
        loki models pull "$MODEL"
      else
        die "model $MODEL is not installed. Re-run with --pull to download it, or pick one from: ollama list"
      fi
    fi
    say "selecting $MODEL"
    loki models use "$MODEL" >/dev/null
  fi
else
  [ -n "$MODEL" ] && warn "--model is ignored with --mock"
fi

# ------------------------------------------------------------------ run
if [ "$MODE" = cli ]; then
  say "starting the CLI chat (data: $DATA_LABEL). /quit to exit."
  if [ "$MOCK" = 1 ]; then loki --provider mock chat; else loki chat; fi
  exit 0
fi

if lsof -nP -iTCP:1420 -sTCP:LISTEN >/dev/null 2>&1; then
  die "port 1420 (the Vite dev server) is busy; stop the other dev server first"
fi
say "starting the desktop app (Vite + Tauri, hot reload; data: $DATA_LABEL). Ctrl+C to stop."
if [ "$MOCK" = 1 ]; then
  say "mock model: replies are canned and labelled 'Mock provider'"
  LOKI_PROVIDER=mock pnpm --filter desktop tauri dev
else
  pnpm --filter desktop tauri dev
fi
