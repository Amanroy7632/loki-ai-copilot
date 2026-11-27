# License review

> **Partial.** P0-T07 (full review including Tauri, Ollama, llama.cpp, whisper.cpp, models, VAD,
> wake word and TTS voices) is still open. This page records the Rust crates added in P1, which
> `cargo deny check` (see `deny.toml`) enforces on every CI run.

## Rust dependency graph (P1), as resolved in `Cargo.lock` on 2026-11-27

| License expression | Crates | Redistribution | Notes |
| --- | --- | --- | --- |
| MIT OR Apache-2.0 (and variants) | ~80, e.g. serde, tokio, futures, clap, uuid, regex, tracing | yes | Permissive |
| MIT | ~24, e.g. libsqlite3-sys, ts-rs, mio, bytes | yes | Permissive |
| Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | rustix, linux-raw-sys, wasi | yes | Permissive |
| Unlicense OR MIT | aho-corasick, memchr | yes | MIT chosen |
| Zlib | foldhash | yes | Permissive |
| (MIT OR Apache-2.0) AND Unicode-3.0 | unicode-ident | yes | Unicode-3.0 is permissive |
| MIT OR Apache-2.0 OR LGPL-2.1-or-later | r-efi | yes | MIT chosen |
| SQLite (public domain) | bundled in libsqlite3-sys | yes | |
| **MPL-2.0** | **option-ext 0.2.0** (via `directories` -> `dirs-sys`) | yes, binary distribution allowed; modified MPL files must be published | **Needs owner approval: D-07.** Allowed by a crate-scoped exception in `deny.toml` until decided. Alternative: drop `directories` and resolve paths by hand or through Tauri path APIs. |

TypeScript (devDependency only, not shipped): typescript 6.0.3, Apache-2.0.
