# ADR-0003: Reference desktop OS

- **Status:** Accepted
- **Date:** 2026-11-27
- **Ticket:** P0-T02
- **Deciders:** owner

## Context

The blueprint names **macOS** as the reference implementation: P3 native tools, the P5
lifecycle gate, and Apple Silicon first. The reference OS needs to be one the developer can test
on **every day**, because P3–P5 gates need real native smoke tests (permissions, tray, sleep/resume).
This ADR's draft said the setup machine was Windows 10; the actual dev machine is a MacBook Air
M2 (8 GB) on macOS 27 (`docs/p0/environment.md`).

## Options

| Option | Pros | Cons |
| --- | --- | --- |
| A. macOS (blueprint default) | Strong permission model to design against; the iOS path later needs a Mac anyway; it is the dev machine | Windows/Linux need separate test machines (P6) |
| B. Windows | Largest desktop user base | Not available for daily testing; path safety (junctions, UNC) is harder |
| C. Linux | Free CI, scriptable | Tray and Wayland fragmentation make it a poor *reference* |

## Decision

**A: macOS** (Apple Silicon first). The phase files' "macOS" wording for P3–P5 applies as written;
P6 covers Windows and Linux.

## Consequences

Native adapters in `loki-platform` start with `macos`. CI still builds and tests the portable
code on all three OSs; native smoke tests are run manually on the Mac and recorded in
`docs/checklists/`. Windows-specific path rules (junctions, UNC, device paths) still get unit
tests in P3-T08, because they are pure string checks.
