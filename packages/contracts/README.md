# packages/contracts

TypeScript types **generated** from `crates/loki-core` with [ts-rs](https://github.com/Aleph-Alpha/ts-rs)
(see [ADR-0005](../../docs/adr/0005-ts-rs-contract-generation.md)).

- `src/generated/`: one file per Rust type, plus `index.ts`. **Never edit these by hand.**
- `src/index.ts`: the package entry point; it re-exports everything from `generated/`.

## Regenerate

```sh
./scripts/gen-contracts.sh          # macOS / Linux
(on Windows, run it from Git Bash)
```

CI runs the same script and fails if `src/generated/` changes (stale contracts), then runs
`pnpm --filter @loki/contracts typecheck` (`tsc --strict`).
