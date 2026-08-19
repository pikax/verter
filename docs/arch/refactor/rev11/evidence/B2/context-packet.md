# B2 — landing context packet

## Predecessor state at dispatch

`program/architecture-lock` at `41e039c2f`. BV0, BF3, BA0, BS0, BRT0 ACCEPTED. B2
READY; B3 serialized behind it (not a DAG edge — both write
`framework_common/vue_bridge.rs`, `svelte/carrier.rs`, `framework_common/carrier_compiler.rs`,
where `parse`/`compile_bundle` are members of one trait declaration no line-range
split can separate).

## Binding inputs

- Charter: [`charters/B2.md`](../../charters/B2.md), as amended by
  [`amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md`](../../amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md).
- `CLAUDE.md`'s "Carrier Geometry From Registered Facts (MANDATORY)" and "a wrong
  output is a bug, not an error path".

## Recovery note

This block's implementation and first review round were driven by a prior train
manager instance that was terminated between rounds. Its work (30 commits on
`block/b2`, tip `5f0b0c285`) and its completed round-1 3-mandate review (conformance/
architecture/adversarial, all `BLOCKING`) plus a completed 6-commit fix and delta
re-review (also `BLOCKING`, 7 of 9 items still open) were recovered from the prior
session's scratch logs rather than re-derived. See "Review arc" in the landing
record for what those rounds found and how they closed.

## Design input for the central finding (round 2, item 2)

The round-1/round-2 reviews' central finding was that the carrier-capability
sealing (`CarrierAccessToken` + `RegisteredProjectorSeal`) was not a real
capability — any external caller could mint a valid token via a public function
and bypass the "only the elected store leader may run the registered projector"
boundary. A grok-4.6 (xhigh effort) architecture consult was run before the round-2
fix dispatch to get a concrete structural redesign rather than have the
implementer guess under round-cap pressure. Its recommendation (delete the
token-vending API; replace the public `&dyn CarrierCompiler`-accepting projector
entry with a closed-enum `CarrierCompilerRegistry::project_registered`; recover
typed carriers through monomorphic per-adapter openers installed at registry-build
time) was implemented, then independently re-verified against the landed code by
the round-3 architecture and adversarial seats (the latter with a live
external-crate-shaped attacker `CarrierCompiler` proof-of-concept).
