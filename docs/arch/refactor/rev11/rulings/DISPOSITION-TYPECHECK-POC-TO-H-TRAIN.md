---
ruling_id: "TYPECHECK-POC-TO-H-TRAIN"
type: "disposition"
date: "2026-08-18"
date_source: "stated"
binds: ["H2", "H3 (future Track H blocks)"]
source_file: "DISPOSITION-TYPECHECK-POC-TO-H-TRAIN.md"
summary: "Routes a typecheck-performance POC (branch origin/poc/api-tax-combined) to the H train as INPUT/REFERENCE, not an approved design — the H train owns the actual design decision. Verifies the mechanism (off-overlay host FS callbacks reach the Rust actor over the transport) but flags the claimed numbers as UNKNOWN/unmeasured. Names three things H must NOT copy from the POC: the illegitimate CLI-vs-API transport-split implementation (bypasses the mandatory ExternalTsProjectResolver->CarrierRegistry->EngineBackend->BoundProject path); the overlay-FS-skip micro-optimization (dead in the combined branch, process-global counters don't belong in a permanent API surface); the sibling-declaration mechanism (wrong companion identity, silently skips a real user file, leaks/races/overwrites unconditionally)."
supersedes: []
superseded_by: []
contradicts: []
notes: "Placement: H2 owns the single-owner backend cutover and benchmark evidence; H3 owns atomic companion publication. Explicitly not B2, not B3, no new block."
---

# Disposition — typecheck performance POC routed to the H train (2026-08-18)

Maintainer: "add that to H2/H3, might be useful there, I'm particularly interested in the performance
improvements, but I also think the solution might not be great, we can leave the decision to H train."

## Status: INPUT to H2/H3, not an approved design

The POC is preserved as REFERENCE and EVIDENCE, not as work to land. **The design decision belongs to
the H train**, which owns this surface. No block before H touches it. Nothing here is ratified as a
mechanism.

Branch: `origin/poc/api-tax-combined`, tip `768325c67`, base `ff3728ec0` (well behind current tip —
applicability must be re-checked). Commits: `eb017108a`, `78ef50122`, `768325c67`.
Full architecture ruling: `/tmp/poc-tsc-out.txt` (copy it into H's evidence when H opens).

## The performance finding — the part the maintainer cares about

`verter-tsc --noEmit` currently runs through `tsgo --api`. Off-overlay host FS callbacks reach the Rust
actor and `get_accessible_entries` unconditionally consults `RealDirSource`
(`crates/verter_tsgo_api/src/snapshot.rs:173`), implemented in `verter-tsc` as `NativeFs::read_dir`
(`crates/verter_tsc/src/api_check.rs:643`). Every engine `Call` frame is serviced over the transport
(`crates/verter_tsgo_api/src/actor/mod.rs:432`), and off-overlay callbacks return fallthrough only
AFTER reaching Rust (`:474`).

**Mechanism: VERIFIED in code.** Many off-overlay callbacks incur pipe traffic, and directory callbacks
also incur host directory reads. That cost is real.

**Numbers: UNKNOWN.** The claimed ~10k callbacks, CLI ~5.1s vs `--api` ~9s, and the eight-file overlay
are NOT established — the POC adds counters but commits no workload output. "tsgo always consults the
host first" is also unverified here. **H must re-measure before designing to these figures.**

## What the ruling says H should NOT copy

1. **CLI-vs-API transport split is legitimate; this implementation is not.** It duplicates project
   selection, carrier membership, option normalization and diagnostic semantics. It bypasses the
   mandatory `ExternalTsProjectResolver → CarrierRegistry → EngineBackend → BoundProject` path; validates
   `Capability::Lsp` because it is "closest" rather than the capability used
   (`checker.rs:1039` on the POC branch); nulls `baseUrl`/`paths` while the resolver still uses the
   selected tsconfig (`:1703`), so the "same dumped program" claim is FALSE; and replaces structured API
   diagnostics with textual compiler-output parsing, deleting the mapping tests instead of proving one
   shared normalization owner.
   **Correct shape:** CLI and API as two `EngineBackend` implementations that both require the same
   `BoundProject` witness and consume the same published snapshot, with cross-backend equivalence tests
   proving semantic identity. The existing type-state already enforces this model
   (`crates/verter_session/src/external_ts/engine.rs:404,410`).
2. **The overlay FS skip does not remove the RPC** — it runs after the callback reaches Rust — and change
   (1) deletes the only production consumer of that path, so it is dead in the combined branch. Its
   process-global counters do not belong in a permanent API surface. Extract the parent-index
   optimization ONLY with a live API consumer and a measured end-to-end benefit; drop the counters.
3. **The sibling-declaration mechanism is categorically unacceptable.** It uses the wrong companion
   identity — repository authority is `Child.d.vue.ts`, explicitly NOT `Child.vue.d.ts`
   (`crates/verter_session/src/framework/descriptor.rs:135,288`) — and silently SKIPS an existing user
   file at that path instead of reporting a conflict, violating the guarded never-shadow rule
   (`CLAUDE.md:319`, `external_ts/resolver.rs:9`). It also leaks files on crash, races concurrent runs,
   fails on read-only trees, and overwrites a fixed root `verter-tsc-check.tsconfig.json` with no
   existence check.
   **Correct mechanism:** descriptor-owned companions through `CarrierRegistry`, real-path conflict
   detection before binding, published virtually through a `BoundProject`. A CLI backend that cannot
   serve virtual files needs a private HERMETIC SHADOW PROJECT — it must never write into the authored
   tree.

## Placement

- **H2** — owns project-scoped provider routes and explicitly preserves `verter_tsc` as a narrow
  batch-checker boundary (`program.md:367`). The single-owner backend cutover and the benchmark evidence
  land here.
- **H3** — owns atomic companion publication and stale-safe readiness (`program.md:373`). Companion
  delivery lands here.
- **Not B2, not B3, no new block.** B2 excludes codegen/planning/publication (`charters/B2.md:5`); B3
  excludes publication and route replacement (`charters/B3.md:17`).

## Remaining ceiling, per the ruling

The durable answer is NOT "CLI everywhere": CLI for properly encapsulated batch checking, engine-side
path allowlisting for long-lived API/LSP sessions (only a tsgo-side "could this path intersect the
overlay?" gate before host dispatch can remove the round trips — a Rust-side allowlist cannot), and ONE
shared project/carrier/diagnostic authority above both.
