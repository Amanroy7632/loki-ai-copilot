# Handoff: <ticket ID(s)> <short title>

Copy this into the PR description or the final message after each ticket or milestone. Fill
every section. Write "none" rather than deleting a section.

## Phase and tickets
- Phase: PN
- Tickets: PN-TNN (done), PN-TNN (partial: what's left)

## Changed files
- `path/to/file`: why

## Commands executed and results
```text
$ cargo test --workspace
<paste real output summary: N passed, M failed>
```
Anything **not run**, and why:

## Acceptance criteria
- [x] criterion: evidence (test name, output, screenshot)
- [ ] criterion: why not met

## Known limitations

## Security implications
New capabilities, new network access, new data stored, new IPC surface, and which SEC tests cover them.

## Decisions made
ADR numbers, or "none".

## Docs updated
STATUS.md, plus any of: CONTRACTS, DATA_MODEL, ...

## Next smallest task
The single next ticket, and anything the next agent must know first.
