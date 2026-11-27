#!/usr/bin/env sh
# Runs the same checks as CI. Use it before every commit.
set -eu
cd "$(dirname "$0")/.."

echo "==> cargo fmt"
cargo fmt --all -- --check

echo "==> cargo clippy"
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p loki-core --features ts --all-targets -- -D warnings

echo "==> cargo test"
cargo test --workspace

echo "==> contracts (generated TS is current)"
./scripts/gen-contracts.sh --check

if [ -d node_modules ]; then
  echo "==> contracts (tsc --strict)"
  pnpm --filter @loki/contracts typecheck
else
  echo "==> skipping tsc: run 'pnpm install' first"
fi

if command -v cargo-deny >/dev/null 2>&1; then
  echo "==> cargo deny"
  cargo deny check
else
  echo "==> skipping cargo deny: install it with 'cargo install cargo-deny --locked'"
fi

if [ -d apps/desktop ] && [ -f apps/desktop/package.json ]; then
  echo "==> pnpm lint + test"
  pnpm -r --if-present lint
  pnpm -r --if-present test
fi

echo "All checks passed."
