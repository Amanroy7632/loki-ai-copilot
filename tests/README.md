# tests

Workspace-level tests that cross crate boundaries. P1-T09 found no need for a separate crate yet:
the P1 end-to-end tests drive the real `loki` binary from `crates/loki-cli/tests/`. Create this
crate when the first SEC-xx test lands (P3).

- `e2e/`: CLI-driven scenarios run against a temp data dir
- `security/`: one test per SEC-xx ID in [docs/SECURITY_MODEL.md](../docs/SECURITY_MODEL.md). These are **release blocking**.
