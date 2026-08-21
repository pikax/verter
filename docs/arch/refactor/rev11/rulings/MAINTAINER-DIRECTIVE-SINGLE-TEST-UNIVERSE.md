---
ruling_id: "SINGLE-TEST-UNIVERSE"
type: "maintainer-directive"
date: "2026-08-21"
date_source: "stated"
binds: ["gate architecture", "verification infrastructure"]
source_file: "MAINTAINER-DIRECTIVE-SINGLE-TEST-UNIVERSE.md"
summary: "Surface 2 and Surface 3 are not necessary as separate test universes. The gate keeps ONE full workspace build and ONE full nextest run. Surface 2's blanket rerun is removed and shared-process lifecycle tests are added to that single normal universe rather than to a separate surface. Surface 3 is not retained as a 15,578-test replay nor as a second whole-workspace archive; only a SMALL alternate-profile compile/runtime guard remains, and only until semantic dependence on debug_assertions and implicit overflow behavior has been structurally eliminated. Once eliminated, no-debug-assertions becomes the canonical full-test profile and Surface 3 is removed entirely."
supersedes:
  - ruling: "ONE-BUILD-ONE-RUN"
    claim: "Refines the replacement shapes. ONE-BUILD-ONE-RUN placed the shared-process contract in a dedicated `shared_process_contract` target; this directive places shared-process lifecycle tests in the NORMAL universe instead. It also states plainly that Surface 3 is not to be retained as a second whole-workspace archive. The seeded-defect deletion bar, the structural-elimination prerequisites, and orchestrator ownership all stand unchanged."
superseded_by: []
contradicts: []
notes: "Adopted by the maintainer as a decision, not a proposal. The deletion bar from ONE-BUILD-ONE-RUN remains absolute: a surface is removed only after a seeded defect proves its replacement catches what the surface existed for. Structural elimination prerequisites are unchanged — no semantic work inside debug_assert!, no semantic production code behind cfg(debug_assertions), explicit checked/wrapping/saturating arithmetic."
---

# Maintainer Directive — one test universe

**Status:** ADOPTED by the maintainer, 2026-08-21.

The decision:

1. **Remove Surface 2 as a blanket rerun.**
2. **Do not keep Surface 3** as a 15,578-test replay or as a second
   whole-workspace archive.
3. **Keep one full workspace build and one full nextest run.**
4. **Add explicit shared-process lifecycle tests to that normal universe.**
   The suite still exists as a file (e.g.
   `verter_session/tests/shared_process_contract.rs`); what changes is that it
   is executed as part of the ONE full run, not as its own surface with its own
   archive. nextest runs it as one isolated process, and the operations INSIDE
   that test share the process — deliberate, deterministic shared-process
   coverage without replaying the whole `verter_session` universe.
5. **Retain only a small alternate-profile compile/runtime guard**, and only
   until semantic dependence on `debug_assertions` and implicit overflow
   behaviour has been structurally eliminated.
6. **After those dependencies are eliminated**, make `no-debug-assertions` the
   canonical full-test profile and remove Surface 3 entirely.

## What this changes from ONE-BUILD-ONE-RUN

That directive positioned the shared-process contract as a dedicated surface.
This directive keeps the focused suite but folds its execution into the
**normal universe** — one archive, one run. The content is the same — create/use/drop/recreate, multiple projects in one process, cache and
registry invalidation, scheduler shutdown and restart, environment restoration,
`OnceLock` and singleton lifecycle, failure then recovery, repeated
initialization under different configurations — but it is not a separate
surface with its own archive.

It also settles Surface 3 explicitly: the second whole-workspace archive goes.

## Why Surface 2 was weak (retained rationale)

Surface 2 caused no extra build — it executed the SAME archive Surface 1 built,
so removing it reduces test time, not compile time. Its failure class is real
(nextest's process-per-test model hides process-global contamination), but the
implementation was weak: it reran every `verter_session` test including
unrelated ones; detection depended on incidental ordering and concurrency; a
leak could be masked by another test resetting the state; unrelated tests turned
flaky merely by sharing a process; and source scanners, fixture inventories and
compiler goldens gain nothing from shared-process execution.

The focused suite covers: create -> use -> drop -> recreate; multiple projects in
one process; cache and registry invalidation; scheduler shutdown and restart;
environment restoration; `OnceLock`/singleton/process-global lifecycle; failure
then successful recovery; repeated initialization under different configurations.

## The Surface 3 replacement, concretely

Two narrowly scoped mechanisms replace the 15,578-test replay:

1. **Compile validation** — `cargo check --workspace --all-targets --profile
   no-debug-assertions`. Catches items wrongly hidden behind
   `cfg(debug_assertions)`, cross-crate APIs that vanish in shipped config, and
   targets that only compile in debug. **Where the real release LSP, N-API or
   WASM artifact builds already compile that same configuration, their success
   SATISFIES this requirement** rather than compiling the graph again inside
   `gate.mjs`.
2. **A small `verter_shipped_cfg_contract` target** run under
   `no-debug-assertions` — dozens of tests at most, covering only behaviour that
   can differ by debug assertions, overflow-check configuration, conditional
   compilation on debug state, or a previously observed shipped-cfg regression.

nextest remains the primary runner: process-per-test gives deterministic
isolation, independent timeouts and better failure containment. Running
`cargo test` once would still produce separate executables per test target — it
would not collapse the workspace into one process.

## Caveats that bind step 6

- ~~The clippy `disallowed-macros` path for `std::debug_assert` must be proven
  against this project's toolchain before the configuration is relied upon.~~
  **RESOLVED 2026-08-21, measured on the pinned toolchain 1.97.1** (scratch
  crate, real `cargo clippy`):
  - `disallowed-macros = [{ path = "std::debug_assert", ... }]` FIRES.
  - It catches the bare `debug_assert!`, the `std::debug_assert!` and the
    `core::debug_assert!` spellings — `core::` resolves to the same path, so
    NO separate `core::` entry is required.
  - **It does NOT catch `debug_assert_eq!` or `debug_assert_ne!`.** Those are
    distinct macro paths and need their own entries. Verified in both
    directions: with only `std::debug_assert` listed, a `debug_assert_eq!` call
    produced no disallowed-macro warning; with all three listed, all three
    fired. The required configuration is therefore:

    ```toml
    disallowed-macros = [
      { path = "std::debug_assert",    reason = "use a precomputed bool" },
      { path = "std::debug_assert_eq", reason = "use a precomputed bool" },
      { path = "std::debug_assert_ne", reason = "use a precomputed bool" },
    ]
    ```

  - Live scope on trunk, production crates only (`crates/*/src/`):
    `debug_assert!` 210, `debug_assert_eq!` 63, `debug_assert_ne!` 1 — 274
    total, of which 37 carry a call expression inside the assertion and need
    individual audit for semantic work. A one-entry policy would have left the
    63 `debug_assert_eq!` uses completely unguarded, and they carry the same
    hazard: `debug_assert_eq!(session.commit_state(), Ok(()))` vanishes whole
    in shipped builds.
- Before switching the full suite to `no-debug-assertions`, establish that the
  invariants debug assertions currently exercise are covered by EXPLICIT tests.
  Otherwise a false or broken debug assertion lands unnoticed and makes debug
  builds unusable.
- The gate-integrity ledger records that the current surfaces lack independent
  inventory parity and that Surface 2 can accept a ZERO-test suite — which is
  itself an argument for explicit, independently enumerated contracts over broad
  reruns.

## What does NOT change

**The deletion bar is absolute.** A surface is removed only after a seeded
defect proves its replacement fails without the fix:

| Seeded defect | Required detector |
|---|---|
| process-global cache leaks between two sessions | shared-process lifecycle tests |
| runtime cannot reinitialize after shutdown | shared-process lifecycle tests |
| state transition placed inside `debug_assert!` | clippy/macro policy |
| public or cross-crate item hidden by `cfg(debug_assertions)` | shipped-cfg compile check |
| arithmetic differing when overflow checks are off | focused shipped-cfg contract |
| shipped configuration silently selects zero tests | independent expected inventory |

The structural-elimination prerequisites for step 6 are unchanged: no semantic
work inside `debug_assert!` (enforced via clippy `disallowed-macros` plus a
Verter macro taking a precomputed bool — not a name scanner), no semantic
production code behind `cfg(debug_assertions)`, and explicit
`checked_*`/`wrapping_*`/`saturating_*` where overflow is meaningful.
