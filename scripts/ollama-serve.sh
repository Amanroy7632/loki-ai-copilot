#!/usr/bin/env sh
# Starts Ollama the way Loki expects (SECURITY_MODEL "Network modes"):
#   - bound to loopback only (127.0.0.1), never 0.0.0.0
#   - cloud features off (OLLAMA_NO_CLOUD=1): no remote inference, no Ollama web search
# Runs in the foreground; stop it with Ctrl+C. Logs go to the terminal.
set -eu

if ! command -v ollama >/dev/null 2>&1; then
  echo "ollama is not installed. macOS: brew install ollama   (see docs/p0/environment.md)" >&2
  exit 1
fi

if curl -fsS --max-time 2 http://127.0.0.1:11434/api/version >/dev/null 2>&1; then
  echo "An Ollama server is already listening on 127.0.0.1:11434."
  echo "If it was started some other way (the Ollama app, 'brew services'), cloud features may be ON."
  echo "Stop it and re-run this script to get the Loki settings."
  exit 0
fi

echo "Starting Ollama on 127.0.0.1:11434 with cloud features disabled (Ctrl+C to stop)..."
OLLAMA_HOST=127.0.0.1:11434 OLLAMA_NO_CLOUD=1 exec ollama serve
