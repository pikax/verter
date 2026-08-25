---
discovery_id: "SESSION-TEST-SEAM-UNREACHABLE"
classification: ["DISC-ARCH"]
date: "2026-08-24"
date_source: "stated"
status: "RECORDED — unowned; no charter holds this surface"
owner: "none"
resolution_gate: "whenever a block takes ownership of verter_session's test-support surface"
---

# Discovery — the compile-pipeline test seam is unreachable from integration tests

Integration-level compile-pipeline concurrency cannot be deterministically
staged in `verter_session`. The seam that would stage it exists and is
correctly positioned; it is simply not visible from the build that would need
it.

## The seam exists and is placed correctly

`compile_input_seam_hook` (`crates/verter_session/src/lib.rs:625-634`) fires in
the cold compile path of `get_virtual_file`
(`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1815-1822`) —
after the warm-hit consult has resolved to a miss and the coherent owner plus
block-content capture has completed, and before the cold compute consumes the
assembled input. A test installing a content mutation there lands it inside the
capture→compute window deterministically, which is precisely the overlap a
compile-pipeline concurrency test needs to stage.

**Noted, not fixed:** the field's own doc comment (`lib.rs:625-631`) says the
seam fires "BEFORE the compile input is assembled", but it fires *after*
`CacheMiss { compile_input, .. }` is constructed — the fire-site comment is the
accurate one. Flagged for whoever owns the surface; correcting it is a
production edit, which is precisely what this document says a test-focused
effort must not make incidentally.

Placement is not the problem. Reachability is.

## Two independent barriers, either sufficient alone

The field carries `#[cfg(test)]` (`:632`) and `pub(crate)` (`:633`).
`test_force` carries the same pair (`:813-814`).

`crates/verter_session/tests/main.rs` exists, so tests under
`crates/verter_session/tests/` compile as a separate **integration binary** that
links `verter_session` as an ordinary dependency rather than building the crate
in test mode. Against that build:

- `#[cfg(test)]` items are not compiled at all — the field does not exist.
- `pub(crate)` items are not nameable from outside the crate — the field would
  not be reachable even if it did.

Removing one barrier leaves the other. Both must move together.

The only consumers today are in-crate unit tests
(`crates/verter_session/src/compile_content_publish_fence_tests.rs:260,269,518,526`),
declared as a `#[cfg(test)] mod` at `lib.rs:86`. That is the sole build in which
the seam is reachable.

This is not specific to one hook. All eight `*_seam_hook` fields on `VerterHost`
(`lib.rs:604,613,623,633,643,654,665,676`) carry the identical
`#[cfg(test)] pub(crate)` pair. None carries a `test-support` gate.

## `test-support` exists for exactly this, and does not cover it

The `test-support` feature (`crates/verter_session/Cargo.toml`) was introduced
to bridge this gap, and its own documentation states the mechanism precisely:
the integration binary links the crate as a non-test dependency so a
`#[cfg(test)]` item is invisible to it, `#[cfg(debug_assertions)]` would leave
the item present in every ordinary debug build, and the feature threads between
the two via the `[dev-dependencies]` self-edge. An item gated
`#[cfg(any(test, feature = "test-support"))]` is reachable from both test builds
and compile-absent in every production profile.

It carries no compile-pipeline seam. What it exposes on the compile pipeline is
three state knobs — `CompileForceOverflowGuard`
(`host_resolve/virtual_file_pipeline.rs:190`),
`reset_compile_tier_prefetch_invocations` (`:217`), and
`compile_tier_prefetch_invocations` (`:226`). Those arm a forced count and read
a counter. None is an execution seam: none parks execution inside a window, so
none can stage an overlap.

## Consequence

Integration tests in this crate can remove timing windows. They cannot stage the
overlap they are named for. A test that must interleave a mutation into the
capture→compute window has to be written as an in-crate unit test, or not
written.

## Ownership — none

No charter owns this surface.

An exhaustive search of `docs/arch/refactor/rev11/charters/` (48 charters) finds
`test-support` in exactly two, in neither case as owned scope:

| charter | appearance |
|---|---|
| `K3` | `K3.md:78` — an exclusion rule for K3's own sweep: how to recognise test gating so a hit inside it is dropped |
| `C1` | `C1.md:286` — an incidental observation that the bare `impl ResolverContext for VerterHost` bodies live only under `#[cfg(any(test, feature = "test-support"))]` |

No charter carries "test seam", "test surface" or an equivalent as owned scope;
the search returns zero hits.

`K3` is **ADJACENT, and explicitly not the owner**: its objective is deleting
transitional mechanisms and proving residue gone, not exposing test seams — and
it is `LOCKED` and not separately authorized.

Owner is recorded as none. No block id, no date, and no owner is invented here.
The resolution gate is: **whenever a block takes ownership of
`verter_session`'s test-support surface.**

## Resolution is a production change

Widening either item's `cfg` or its visibility, or adding a compile-pipeline
seam to `test-support`, changes what the library compiles and exports. That is a
production change to `verter_session`, not test scaffolding, and therefore not
something a test-focused block may do incidentally on its way to a test it
wants. It belongs to whichever block acquires the surface.
