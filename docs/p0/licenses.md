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

| **MIT-0** | **borrow-or-share 0.2.4** (via `jsonschema` -> `referencing` -> `fluent-uri`) | yes | MIT without the attribution clause (more permissive than MIT). **Needs owner confirmation: D-07.** Crate-scoped exception in `deny.toml`. |

P3 additions (2026-11-27): jsonschema 0.63 (MIT), sha2 0.11 (MIT/Apache-2.0), jiff 1.0 (MIT/Unlicense), plist 1.10 (MIT), wiremock 0.6 (dev only, MIT/Apache-2.0).

P4 additions (2026-11-27/28): cpal 0.18.2 (Apache-2.0), rubato 5.1.0 and crossbeam-queue 0.3.14 (MIT OR Apache-2.0), earshot 1.2.2 (MIT OR Apache-2.0 per its Cargo.toml; the VAD weights are embedded in the crate source, but there is no separate license file and the README does not state the weights' training-data provenance: **confirm with upstream before a public release**, as part of P0-T07).

P5 additions (2026-11-28): **Rust** tauri 2.12.5 / tauri-build 2.7.4 (Apache-2.0 OR MIT), plus Servo's **MPL-2.0** cssparser, cssparser-macros, selectors and dtoa-short (via tauri-utils; crate-scoped exceptions in `deny.toml`, pending D-07). **JS shipped in the app:** react / react-dom 19.3.1 (MIT), -apps/api 2.12.4 (Apache-2.0 OR MIT). **JS build/dev tools only (not shipped):** ESLint, Prettier, Vite, Vitest and friends (MIT, ISC, BSD, Apache-2.0, BlueOak-1.0.0); lightningcss (MPL-2.0, Vite's CSS minifier); caniuse-lite (CC-BY-4.0, browser data). Checked with `pnpm licenses list`.

TypeScript (devDependency only, not shipped): typescript 6.0.3, Apache-2.0.

## P4 audio capture dependencies (2026-11-27)

Verified with `cargo info` before adding; pinned exactly and locked. See ADR-0012.

| Crate | Version | License | Use |
| --- | --- | --- | --- |
| cpal | 0.18.2 | Apache-2.0 | Default input, native sample formats and typed backend errors |
| rubato | 5.1.0 | MIT OR Apache-2.0 | Streaming FFT conversion to 16 kHz mono |
| crossbeam-queue | 0.3.14 | MIT OR Apache-2.0 | Preallocated bounded buffer, replacing oldest frames |

The existing `cargo deny` policy applies to their transitive dependencies without new exceptions.
