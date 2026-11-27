# PN-TNN: <title>

- **Phase:** PN
- **Implements:** FR-xx / NFR-xx / SEC-xx
- **Crates/files:** 
- **Depends on:** tickets, ADRs, hardware

## Do
- 

## Acceptance
- 

## Tests
- Unit:
- Integration:
- Manual/native (if any):

## Out of scope
- 

### Example (from the blueprint)

**P3-T07: implement `app.open` on the reference OS.** Inputs: a registered `app_id`. Depends on
the registry, policy and native adapter. Acceptance: an approved installed app launches; an
unknown app is rejected; a denied request does not launch; an audit entry is recorded. Tests:
mock unit tests plus a manual native smoke test.
