---
ruling_id: "NO-BUILD-INVOKING-TESTS"
type: "maintainer-directive"
date: "2026-08-20"
date_source: "stated"
binds: ["Rust test suite composition"]
source_file: "MAINTAINER-RULING-NO-BUILD-INVOKING-TESTS.md"
summary: "RATIFIED: a test executes code, it does not spawn a compiler, build a CLI, or build a Rust project — Rust tests must be pure Rust exercising Verter code, JS tests pure JS exercising JS code. Removes trybuild-backed compile-fail fixtures (81 fixtures across 6 crates) that proved structural invariants (E0308 newtype non-interchangeability, E0451 private-field unconstructability, sealing/unreachability) via out-of-process cargo builds. States plainly that this removes the sole regression detector for those 81 invariants and delegates the replacement design (in-crate compile_error!/const-assertions, negative trait bounds, moving compile-fail out of the test surface, or accepting the loss with review-only enforcement) to the architecture seat as an open decision."
supersedes: []
superseded_by: []
contradicts: []
notes: "Triggered by the canonical gate failing at exactly the 360s budget on two trybuild fixtures that pass 3/3 in isolation (98s cold / 0.8s warm) because one trybuild invocation spawns cargo against verter_session's ~233-crate dependency closure. States the asymmetry that matters: loosening a structural restriction never breaks a normal build, so the compile-fail fixture was the sole regression detector in that direction."
---

# Maintainer ruling — tests must not build CLIs or Rust projects

**Date: 2026-08-20. RATIFIED.**

> those trybuild tests do sound unnecessary and should be removed, we should not have tests that build
> cli or any rust project...
>
> tests should be pure rust that run verter code or js tests that run js code

## The rule

A test executes code. It does NOT spawn a compiler, build a CLI, or build a Rust project.
- Rust tests: pure Rust exercising Verter code.
- JS tests: JS exercising JS code.

## What this removes — measured, not estimated

`trybuild` is declared in SIX crates and backs **81 compile-fail fixtures**:

| crate | fixtures |
|---|---|
| verter_session | 58 |
| verter_language | 7 |
| verter_compiler | 7 |
| verter_identity | 4 |
| verter_type_runtime | 4 |
| verter_audit | 1 |

They assert compiler-enforced structural confinement — the errors are real type-system outcomes:
`E0308` (newtype identities are not interchangeable: `SessionHandle` ≠ `StableEntityId`,
`QueryIdentity` ≠ `SemanticFlightKey`, `InputBasisId` ≠ `QueryIdentity`), `E0451` (private fields keep
`AcceptedSource` / `CanonicalGrammar` unconstructable outside their mint), plus sealing and
unreachability proofs.

## The cost, stated plainly

This program's own rule is that landed enforcement must be STRUCTURAL — compiler/type-system based,
never a name-keyed source scanner. Compile-fail fixtures are how a structural invariant is proven to
still hold.

**The asymmetry that matters: LOOSENING a restriction never breaks a normal build.** Make a sealed trait
public, widen a private field, add a `From` impl between two identity newtypes — every ordinary test and
the whole workspace still compiles. The compile-fail fixture is the only thing that fails. So removing
them does not merely delete slow tests; it removes the sole regression detector for 81 invariants, in a
direction the compiler cannot otherwise report.

That is a real consequence of a legitimate ruling, recorded so the replacement decision is made with it
in view rather than discovered later.

## What triggered it

The canonical gate FAILED on two of these at exactly the 360s budget:
`component_api_projection_contract_not_optional_compile_fail` and
`hot_materialize_structural_rails_smoke`. Isolation triage: both pass 3/3, **98s cold / 0.8s warm**.
`.config/nextest.toml:49-70` documents why — one trybuild invocation SPAWNS cargo against a generated
crate and compiles `verter_session`'s ~233-crate dependency closure before checking a single fixture.
The budget had already been raised once (60s×3 → 120s×3) and today's gate work (45 newly-visible tests,
`verter_compiler`'s 6,734 added to Surface 3) increased contention past it again.

The maintainer's ruling reaches past the timeout to the category: this was never a unit test.

## Open decision, delegated to the architecture seat

What replaces the 81 structural proofs — and honestly, whether anything fully can. Options to be ruled
on: in-crate `compile_error!`/const-assertion constructs that need no external build; negative trait
bounds or marker-based encodings; moving compile-fail out of the test surface entirely; or accepting the
loss with review-only enforcement. An honest "no full replacement exists" is an acceptable answer and
better than a mechanism that only appears equivalent.
