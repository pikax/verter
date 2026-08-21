---
ruling_id: "ONE-BUILD-ONE-RUN"
type: "maintainer-directive"
date: "2026-08-21"
date_source: "stated"
binds: ["gate architecture", "verification infrastructure"]
source_file: "MAINTAINER-DIRECTIVE-ONE-BUILD-ONE-RUN.md"
summary: "The gate's target is ONE full workspace build and ONE full test run, plus a very small alternate-configuration guard — not two large test universes. Surface 2's blanket replay is removed and replaced by an explicit `shared_process_contract` suite that performs many operations inside one test process deliberately, rather than hoping incidental ordering exposes contamination. Surface 3's 15,578-test replay is removed and replaced by `cargo check --workspace --all-targets --profile no-debug-assertions` plus a tiny `verter_shipped_cfg_contract` target of at most dozens of tests. The configuration difference is REAL and must not simply be deleted: debug_assert! does not evaluate its expression when debug assertions are off, and cfg(debug_assertions) code is absent, so a release cargo check cannot observe a state transition disappearing. Long term, making that divergence structurally impossible — a Verter assertion macro taking a precomputed bool, no semantic code behind cfg(debug_assertions), explicit checked/wrapping/saturating arithmetic — allows no-debug-assertions to become the single canonical profile and Surface 3 to disappear entirely."
supersedes:
  - ruling: "GATE-PERFORMANCE-BLOCK"
    claim: "Refines that directive's steps 2-3 with the concrete target architecture and the specific replacements. The ordered plan, the seeded-defect requirement and the orchestrator ownership all stand unchanged; this supplies the shapes those steps were to produce."
superseded_by: []
contradicts: []
notes: "Surface 2 costs test time, not compile time — it executes the SAME archive Surface 1 built, so removing it does not reduce compilation. Surface 3 is the second whole-workspace compile. Records the standard for deletion: each surface may only be removed once a seeded defect proves its replacement catches what it existed for, per the mutation table. Also notes the gate-integrity ledger's finding that current surfaces lack independent inventory parity and that Surface 2 can accept a zero-test suite — which is itself an argument for explicit enumerated contracts over broad reruns."
---

# Maintainer Directive — one full build, one full run

**Status:** RATIFIED by the maintainer, 2026-08-21.

> One full workspace build and one full test run, plus a very small
> alternate-configuration check — not two large test universes and not an
> accidental rerun of thousands of tests.

## Surface 2 — delete the blanket rerun

It costs test time, not compile time: it executes the same archive Surface 1
built. Its rationale is real — nextest isolates each test in its own process, so
process-global contamination is invisible to it — but the implementation is weak.
Detection depends on incidental ordering, a leak can be masked by another test
resetting the state, unrelated tests turn flaky merely by sharing a process, and
source scanners and goldens gain nothing from shared-process execution.

Replace with `verter_session/tests/shared_process_contract.rs`, which performs
many operations inside ONE test process deliberately: create/use/drop/recreate,
multiple projects in one process, cache and registry invalidation, scheduler
shutdown and restart, environment restoration, `OnceLock` and singleton
lifecycle, failure then successful recovery, repeated initialization under
different configurations.

## Surface 3 — the replay goes, the configuration difference stays

`debug_assert!` does not evaluate its expression when debug assertions are off,
and `cfg(debug_assertions)` code is absent. These are compile-time differences: a
binary built with debug assertions cannot be told at runtime to behave otherwise,
and a release `cargo check` compiles the program but cannot observe a state
transition disappearing. The documented failure is
`debug_assert!(session.commit_completed())` — passing in debug because the call
runs, silently removed in shipped builds.

So the coverage cannot be folded into the same compiled binaries, and 15,578
tests is an excessive answer to a narrow class. Replace with:
- `cargo check --workspace --all-targets --profile no-debug-assertions`, which
  catches items wrongly hidden behind `cfg(debug_assertions)`, cross-crate APIs
  that vanish in shipped configuration, and targets that only compile in debug;
  where release LSP, N-API or WASM builds already compile that configuration,
  their success satisfies the requirement rather than compiling the graph again;
- `verter_shipped_cfg_contract`, a small target of at most dozens of tests
  covering only behaviour that can differ by debug assertions, overflow checks,
  conditional compilation on debug state, or a previously observed shipped-cfg
  regression.

## The end state, and how to earn it

Once semantic dependence on the profile is structurally impossible, the full run
moves to `no-debug-assertions` and Surface 3 disappears. That requires:

- **No semantic work inside `debug_assert!`.** Forbidden:
  `debug_assert!(session.commit_completed())`. Required: compute unconditionally,
  then assert on the result. Enforce by disallowing the standard macro in
  production crates via clippy's `disallowed-macros` — not a name scanner — and
  providing a Verter macro that accepts only a precomputed bool.
- **No semantic production code behind `cfg(debug_assertions)`.** Diagnostics
  only, and never required for control flow.
- **No reliance on implicit overflow behaviour** — explicit `checked_*`,
  `wrapping_*`, `saturating_*` where overflow is meaningful.

Before switching the full suite, establish that the invariants debug assertions
currently exercise are covered by explicit tests; otherwise a broken debug
assertion lands unnoticed and makes debug builds unusable.

## Required proof before deleting either surface

| Seeded defect | Required detector |
|---|---|
| process-global cache leaks between two sessions | `shared_process_contract` |
| runtime cannot reinitialize after shutdown | `shared_process_contract` |
| state transition placed inside `debug_assert!` | clippy/macro policy |
| public or cross-crate item hidden by `cfg(debug_assertions)` | shipped-cfg compile check |
| arithmetic differing when overflow checks are off | focused shipped-cfg contract |
| shipped configuration silently selects zero tests | independent expected inventory |

Do not remove a surface because its replacement looks plausible.
