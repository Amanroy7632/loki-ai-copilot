# P3: warm tool-dispatch overhead (NFR-03)

- **Date:** 2026-11-27. **Commit:** none (the repo's `.git` is broken; see STATUS.md).
- **Hardware/OS:** MacBook Air, Apple M2, 8 GB, macOS 27.0.1. Rust 1.97.1.
- **What is measured:** `ToolHost::dispatch` for `time.now {"timezone": "Asia/Tokyo"}`: schema
  validation, grant lookup (SQLite), policy, task insert + 3 state transitions + `tool_runs` row
  (SQLite on disk, WAL), executor (semaphore, deadline, result cap). The model is not involved;
  `time.now`'s own work is microseconds, so this is the overhead. 5 warm-up runs, then 50 measured.
- **Command:** `cargo test [--release] -p loki-runtime --test dispatch_overhead -- --nocapture`

| Build | Median | p95 | Max |
| --- | --- | --- | --- |
| release | 0.49 ms | 0.76 ms | 0.86 ms |
| debug (what CI runs) | 0.79 ms | 1.01 ms | 1.29 ms |

**NFR-03 target: < 250 ms. Met, with a margin of more than 200x.** The test asserts p95 < 250 ms.

For comparison, the real tools in the manual smoke test (same day) took: `time.now` 7 ms,
`app.list` 86 ms (bundle discovery), `app.open` 206 ms (launch + process verification).
