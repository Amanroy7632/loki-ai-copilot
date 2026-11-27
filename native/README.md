# native

This folder will hold native bridges that can't live in Rust:
- `apple/`: Swift for App Intents, the share extension and AVAudioSession (P9), plus any macOS helpers.
- `android/`: Kotlin for AudioRecord, intents, SAF and the foreground service (P8).

Each bridge exposes the smallest possible API to `loki-platform`, and is documented in [docs/PLATFORMS.md](../docs/PLATFORMS.md).
