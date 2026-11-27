# Security policy

## Reporting a vulnerability

Report security problems privately to the project owner. Don't open a public issue. Include the
steps to reproduce, the affected version or commit, and the platform. Until a public release
exists, there is no formal response-time commitment.

## Trust model in one page

Loki can launch apps, read your files and reach the internet, so the design starts from a
default of **no authority**:

- **Typed tools only.** The model can only *propose* calls to allowlisted, schema-validated tools.
  Nothing the model writes is run as shell, SQL or code.
- **Policy first, fail closed.** Every tool call is evaluated before it runs. If the policy can't
  positively allow a call, the call doesn't run.
- **Exact approvals.** A sensitive action shows a preview of the exact arguments. The approval is
  bound to those arguments, the request ID and an expiry, and is used once.
- **Content is data.** Web pages, documents and tool output can't grant permissions (prompt-injection defence).
- **Scoped files.** File tools only see roots the user approved. Paths are canonicalized, and
  symlink escapes are rejected.
- **Network is opt-in.** Local-only mode makes no network calls except user-started model
  downloads and user-authorized internet tools. There is never a silent cloud fallback.
- **Secrets stay in the OS secret store.** They aren't in the database, the logs or the model context.
- **Voice isn't identity.** A wake word never approves a sensitive action. A global mute and an
  emergency stop are always available.
- **Paired devices are authenticated.** Pairing uses a short-lived code with public-key
  verification and per-device scopes, and keys can be revoked.

The full threat model, required controls and the release-blocking test list are in
[docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md).
