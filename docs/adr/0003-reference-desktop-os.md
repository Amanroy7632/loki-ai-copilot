# ADR-0003: Reference desktop OS

- **Status:** **Proposed**. Decided in P0-T02.
- **Date:** 2026-11-27
- **Ticket:** P0-T02

## Context

The blueprint names **macOS** as the reference implementation: P3 native tools, the P5
lifecycle gate, and Apple Silicon first. The machine this repository was set up on runs
**Windows 10**. The reference OS needs to be one the developer can test on **every day**, because
P3–P5 gates need real native smoke tests (permissions, tray, sleep/resume).

## Options

| Option | Pros | Cons |
| --- | --- | --- |
| A. macOS (blueprint default) | Strong permission model to design against; the iOS path later needs a Mac anyway | Needs Mac hardware for daily work |
| B. Windows | Matches the current dev machine; the largest desktop user base | Path-safety rules are harder (junctions, UNC); WebView2 differences |
| C. Linux | Free CI, scriptable | Tray and Wayland fragmentation make it a poor *reference* |

## Decision

_To be filled in at P0-T02._ If B is chosen: wherever the phase files say "macOS" for P3–P5,
read "reference OS", and P6 then covers macOS and Linux.

## Consequences

_To be filled in._
