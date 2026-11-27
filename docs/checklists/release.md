# Release checklist (P11)

## Security
- [ ] SEC-01 to SEC-11 pass on every platform (link the CI run)
- [ ] `cargo deny check`, `cargo audit` and `pnpm audit` are clean, or each exception is documented
- [ ] CI actions pinned by SHA; SBOM generated
- [ ] Model checksums verified; the update manifest is signed
- [ ] Tauri ACL and CSP reviewed; devtools off in release

## Platforms
- [ ] The desktop lifecycle checklist passes on each desktop OS
- [ ] Mobile physical-device tests pass (P8/P9)
- [ ] Unsupported capabilities are shown as unavailable, with reasons, on every platform

## Distribution
- [ ] macOS signed and notarized; Windows signed; Linux packages tested on the target distros
- [ ] Update rollback tested
- [ ] Installer does not bundle model weights (or the size is justified)

## Docs
- [ ] Install, uninstall, data export, model cleanup, privacy summary, known limitations
- [ ] Licenses and third-party notices bundled
- [ ] Trademark/name check done (ADR-0002)
