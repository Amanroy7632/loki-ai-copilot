# P10: Secure desktop pairing (v0.9)

| | |
| --- | --- |
| **Effort** | 3–5 weeks |
| **Depends on** | P8 or P9 gate |
| **Must read** | SECURITY_MODEL.md (local IPC, SEC-09) |

```text
Mobile UI ──► authenticated, encrypted channel ──► Desktop agent
          ◄── task status / streamed response / audit summary
```

## Tickets

### P10-T01: Device identity and pairing
- Each device has a keypair, stored in the OS secret store. Pairing uses a short-lived QR code or numeric code with public-key verification, and both devices must confirm. Pairing writes a `devices` row with scopes.

### P10-T02: Transport
- TLS with pinned device keys (or Noise; decide in an ADR), and authenticated sessions with replay protection (nonces and timestamps). Start on the trusted LAN only; remote access is through a VPN the user manages. **Never expose an unauthenticated public port.**

### P10-T03: Per-device scopes and policy
- Remote requests go through the same policy as local ones, plus the device's scopes. Sensitive actions still need confirmation on the desktop or device authentication.

### P10-T04: Revocation
- Revoke a device from either side, with immediate effect; open sessions are dropped.

### P10-T05: Offline behaviour
- If the desktop is asleep or offline, the phone shows that clearly. **Destructive actions are never queued** for automatic execution on reconnect.

## Phase gate

- [ ] SEC-09: unauthorized, revoked and replayed requests fail (automated tests).
- [ ] Offline behaviour verified.
