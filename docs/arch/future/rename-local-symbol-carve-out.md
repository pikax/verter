# Future work — allow local-symbol rename in multi-claimant packages

Status: **deferred / not yet scheduled.** A UX improvement to the interim rename behavior shipped
with the project-selection fix. This document captures the full scope + recommendation so it can be
planned properly later; it is not an approved block.

## Background — the interim behavior we ship today

The project-selection fix (multi-claimant carrier ownership) made a carrier claimed by two+
configured projects resolve to a single `Bound` owner instead of a terminal `Ambiguous`, so
hover / definition / completion / references now work on such carriers across all three provider
routes.

**Rename was deliberately left fully fail-closed for any multi-claimant carrier.** Both
`handle_prepare_rename` and `handle_rename` return a "cross-project rename not yet supported"
error when `carrier_is_multi_claimant(uri)` is true
(`crates/verter_lsp/src/server/nav_features_navigation.rs`). This was the safe interim: a
single-project rename of a symbol that ESCAPES its owning project (exported and used from another
configured project) would silently update only the owner project and leave the same symbol dangling
elsewhere — a silent partial rename, which for a shared component is a correctness hazard worse than
no rename. Failing closed entirely guarantees no partial rename.

## The cost of the interim

In a component-library monorepo, **every** source file in the UI package is typically multi-claimant
(an app tsconfig and a components/build tsconfig both `include` the same `src/**`). So
fail-closed-entirely means **rename is unavailable across the whole package** — including renaming a
purely LOCAL symbol (a `const`, a helper function, a `<script setup>` binding used only within that
one file), which is completely safe and has no cross-project dimension at all.

This is **more conservative than Volar / tsserver**, which perform a single-project rename there — so
for local symbols Verter is currently a regression against the tools we aim to beat, even though the
fail-closed is safer for the genuinely-dangerous exported case.

## Proposed carve-out

Split the rename gate by whether the target symbol escapes its owning project:

- **LOCAL (non-exported) symbol** → rename normally (single-project rename is complete and correct;
  matches Volar). Safe because a non-exported symbol cannot be referenced from another project.
- **EXPORTED symbol that may escape** → keep failing closed with the `verter(project)` diagnostic
  until true cross-project rename lands (see "Relationship to XR").

This restores everyday in-file rename in multi-claimant packages while preserving the safety floor
exactly where the hazard actually exists.

## Feasibility (assessed during the PS review)

| Level | Estimate | Missing data |
|-------|----------|--------------|
| Allow rename when the symbol is **not exported** (local-only carve-out) | **SMALL** | The host export / analysis tables are already on the rename path — check export-ness of the symbol under the cursor before the multi-claimant gate. |
| Fail closed only when **exported AND actually imported outside the owner** | **MEDIUM→LARGE** | No cheap reverse cross-project import index exists; provider rename/refs are single-owner. This is the full cross-project fan-out (the XR block). |

So the **SMALL** version (local-non-export renames normally, ALL exported symbols fail closed) is the
cheap, high-value interim. The finer distinction (allow rename of an exported-but-not-actually-
imported-elsewhere symbol) is not worth building separately — it collapses into the XR fan-out work.

## Recommendation

**Do the SMALL local-non-export carve-out.** It removes a real daily-workflow regression (renaming
local bindings in components), it is cheap, it matches Volar for the local case and stays safer than
Volar for the exported case, and it changes nothing about the safety guarantee (no silent partial
rename ever ships). Gate the carve-out on a conservative export-ness check: if there is ANY doubt
that a symbol is local, fail closed — err toward the safe side.

Acceptance when scheduled:
- A non-exported symbol in a multi-claimant carrier renames normally (single-project, complete).
- An exported symbol in a multi-claimant carrier still fails closed with the `verter(project)`
  diagnostic (no partial edit) — unchanged from today.
- Discriminating tests: (1) local `const` in a dual-claimant fixture renames and updates all its
  in-file occurrences; (2) an exported symbol still returns the fail-closed error, never a partial
  `WorkspaceEdit`.
- Conservative-doubt rule proven: a symbol whose export-ness cannot be determined fails closed.

## Relationship to XR (cross-project fan-out)

The separate XR block (`references` / `rename` / `implementations` fan-out across all configured
projects containing a carrier) is the true end state: an exported symbol renamed correctly across
every project that uses it. This carve-out is the cheap interim between today's fail-closed-entirely
and XR's full fan-out — it does not replace XR, and once XR lands the exported-symbol gate this
carve-out leaves in place is what XR upgrades from "fail closed" to "rename everywhere."

## Priority / effort

- **Effort:** SMALL.
- **Priority:** MEDIUM — it removes a visible daily-workflow regression in exactly the monorepo shape
  that motivated the project-selection fix, but it is not correctness-gating (the safe fail-closed is
  already shipped).

## Confidentiality

The originating project is a private third-party codebase used only for local stress testing. Do not
name it or its paths in any committed content or fixture; use hermetic synthetic fixtures.
