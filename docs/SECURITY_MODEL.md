# Security, privacy and trust model

## Risks and required controls

| Risk | Required control | Where it lives |
| --- | --- | --- |
| Prompt injection | Treat web pages, documents and tool output as **data**. Content never grants authority. Tool results reach the model wrapped and labelled as untrusted. | loki-agent, loki-policy |
| Unsafe execution | Typed, allowlisted tools only. No arbitrary shell. Least privilege, timeouts, bounded output. | loki-tools |
| File escape | Canonicalize paths. Verify the allowed root after resolving symlinks. Reject `..` and absolute paths in arguments. Use OS-scoped handles (security-scoped bookmarks, SAF) where they exist. | loki-tools, loki-platform |
| Sensitive actions | Preview the exact change. Explicit **one-time** approval for sends, deletes, installs, purchases and remote actions. | loki-policy, UI |
| Credential exposure | OS secret store only. Redact logs and audit payloads. Never inject secrets into model context. | loki-runtime, loki-memory |
| Network abuse | Network tools are opt-in. Generic fetch blocks private, loopback and link-local destinations (checked after DNS resolution and on **every redirect**). Cap download size and time. | loki-runtime (HTTP client), loki-tools |
| Local IPC | Bind to loopback or a user-only socket. Authenticate requests and validate origin. Never expose unauthenticated model ports. | loki-runtime, apps/desktop |
| Supply chain | Pin dependencies and lockfiles. Run `cargo audit` / `cargo deny` and `pnpm audit` in CI. Verify model checksums. Sign updates. | CI, P11 |

## Voice authorization

- A wake word is **not authentication**.
- A sensitive action needs a visible on-screen confirmation, or device authentication on mobile. Ambient audio is never accepted as approval.
- The desktop approval dialog (P5-T04) shows every argument with invisible and bidi-control characters made visible, starts with focus on Deny, and ignores auto-repeated keys. For a Sensitive action the keyboard can only approve after the user moved focus to Approve. The dialog sends back the arguments it showed, and the runtime refuses a "yes" for different arguments or after expiry.
- A global mute and an emergency stop are always available: tray, UI, and a keyboard shortcut.
- No hidden listening or capture. A listening indicator is always shown while the microphone is open.

## Network modes

| Mode | Allowed outbound traffic |
| --- | --- |
| `local_only` (default) | Loopback (the local model server) only. A model download is allowed only from an explicit user action in the onboarding flow. |
| `tools_allowed` | Plus the internet tools the user has granted (`web.search`, page fetch). There is still no cloud inference. |

Every outbound request goes through **one** HTTP client built by `loki-runtime`, which enforces the
mode. A test asserts that no other crate constructs its own client.

## Logging and audit

- Logs go to the per-user data directory, with rotation. Logs are local only, and telemetry is opt-in only.
- Audit rows (`tool_runs`) store the tool, status, timing and a **redacted** result summary. They
  never store file contents, secrets or full web pages.
- A redaction helper masks paths outside approved roots, tokens, emails (configurable) and long secrets.

## Threat-model gate

**A release is blocked if any of these can bypass policy.** Each line has an automated test in
`tests/security/tests/security.rs` (crate `loki-security-tests`), named after its ID, e.g.
`sec_05_approval_replay_is_refused`. SEC-01..06 and SEC-10 exist since P3-T10; SEC-07 is covered by
`loki-runtime::net` tests (P2-T06); SEC-08, SEC-09 and SEC-11 arrive with P7, P10 and P4.

| ID | Test |
| --- | --- |
| SEC-01 | A malicious web page or document that instructs the model to call a tool doesn't get that call auto-approved. It still goes through policy and approval. |
| SEC-02 | Path traversal (`../`, absolute paths, UNC paths, `~`) in `files.*` arguments is refused. |
| SEC-03 | A symlink or junction inside an allowed root that points outside the root is refused. |
| SEC-04 | Malformed tool JSON, unknown fields, wrong types and oversize strings are refused before policy runs. |
| SEC-05 | Approval replay: reusing an approval, using it after expiry, or using it with changed arguments is refused. |
| SEC-06 | A request for an unknown tool, or a tool whose capability isn't `Supported`, is denied. |
| SEC-07 | In `local_only` mode, any non-loopback request fails, including redirects to the internet. |
| SEC-08 | A generic fetch to private, loopback or link-local addresses (including through DNS rebinding and redirects) is refused. |
| SEC-09 | An unauthorized or revoked paired device, or a replayed pairing message, is refused (P10). |
| SEC-10 | Secret leakage: secrets placed in config or env never appear in logs, the audit log, the DB or model context. |
| SEC-11 | Voice: a transcribed "yes, approve" doesn't approve a pending sensitive action. |

## Privacy defaults

Desktop data controls (P5-T06) expose only four typed editable settings and redacted audit rows.
Export destinations come from a Rust-side native save dialog; neither paths nor JSON bytes are
accepted from the webview, and it has no dialog/filesystem plugin grants. Exports create new
files only. History clear and changed finite retention require visible confirmation with default
Cancel. They require exclusive runtime access; during a turn they fail immediately, leaving
Stop and approval commands usable. New contract types reject unknown fields; runtime tests cover
invalid/cloud model settings, export overwrite refusal and post-write retention errors.

Bulk deletion clears the active history database and WAL (with errors for incomplete cleanup),
and tells the UI to discard cached text even when cleanup fails after the logical deletion.
Audit/permissions and external exports/backups remain. Personal memory/FTS cleanup is not claimed
until P7 implements those tables (ADR-0015).

| Setting | Default |
| --- | --- |
| Raw audio retention | **Off** |
| History retention | Configurable (default: keep, user can clear) |
| Memorizing facts | Only user-approved items. Secrets and sensitive documents are never memorized automatically. |
| Telemetry | Off (opt-in only) |
| Wake word | Off |
| Network tools | Off |
| Start at login | Off |
