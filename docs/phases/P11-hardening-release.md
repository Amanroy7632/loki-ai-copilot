# P11: Hardening and v1.0 release

| | |
| --- | --- |
| **Effort** | 3–5 weeks |
| **Depends on** | P10 gate |

## Tickets

### P11-T01: Threat-model gate
- Every SEC-01 to SEC-11 test passes on every platform. Run an adversarial review session and record its findings.

### P11-T02: Performance profiling
- Cold and warm model load, first token, speech latency, RAM, idle CPU and battery (with and without the wake word), and tool dispatch time (NFR-03). Record the results in `docs/benchmarks/`.

### P11-T03: Accessibility audit
- Keyboard-only use, a screen reader on each desktop OS, contrast, and reduced motion.

### P11-T04: Signed installers and updates
- Sign the builds: macOS Developer ID with notarization, Windows code signing, and Android/iOS store signing. Ship signed update manifests (Tauri updater) with staged rollout and **tested rollback**.

### P11-T05: Supply chain
- `cargo deny`, `cargo audit` and `pnpm audit` are clean. Pin CI actions by SHA. Generate an SBOM. Verify model checksums.

### P11-T06: Release documentation
- Install, uninstall, data export, model cleanup, privacy summary and known limitations per platform. Complete `docs/checklists/release.md`.

## Phase gate

- [ ] The security and platform release checklist is complete, with evidence.
- [ ] No known critical security issue remains.
