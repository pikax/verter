# Codebase simplification

This contract records the maintainer-authorized test, production-code and comment simplification train, rev11.simplification. The decision is [2026-09-08-codebase-simplification-train](../decisions/2026-09-08-codebase-simplification-train.md). SIMP1–SIMP14 are independently landable delivery blocks; SIMP15 verifies their combined result. All are required before final Rev11 closure through L1/L4. This is ordinary amended work authority, not a new execution-state mechanism.

## Outcomes and value

Reduce independently maintained mechanisms, repeated work/state, broad responsibility and redundant explanation. Test retirement is the first workstream, not a substitute for production simplification. File moves, renames, shortened formatting, lower test counts or a net LOC target alone do not establish success.

For the named test populations, classify the assertion's subject:
- Retire deleted-name/file tombstones, historic source-layout budgets, plan/prose checks, guard registries and their self-tests. No replacement is required solely to remember an old spelling.
- Retire source/AST name-shape enforcement when real types, capabilities or behavioral proof cover the live invariant. Map a genuinely uncovered boundary before deleting its only useful evidence.
- Keep useful behavior, live type/privacy/ownership contracts, public serialized/generated output, source maps, schema/artifact freshness, checkout portability and real gate-execution tests.
- A test of Verter's actual parser/scanner is product behavior. Reading fixture text or asserting emitted text does not make a test a source-policy scanner. A trybuild fixture that merely references a long-deleted symbol can still be a tombstone.
- Delete the retired scanner's exclusive parser, traversal, allowlist, synthetic cases, wiring and docs together; do not merely hide or skip it. Preserve shared support until remaining consumers are accounted for.

This contract authorizes retirement of the named grandfathered scanners. It supersedes CLAUDE.md's blanket requirement to retain pre-existing scanners for this train's scoped populations. SIMP3 removes that obsolete retention policy and the CRITICAL-heading registry requirement from current shared guidance; it preserves substantive critical invariants. Existing sufficient behavioral/compiler proof is valid. No new scanner-to-detect-scanners, universal test DSL, mandatory negative companion or one-for-one replacement test is required by this train. This is a scoped test-economy amendment; it does not waive existing critical product, gate or owning-contract evidence.

## Production boundaries and DRY/KISS

SIMP7 reuses coordinate state for an exact existing document snapshot or batch. SIMP8 shares only equivalent pending-request mechanics. SIMP9 unifies equivalent stale-close operations. SIMP10 preserves minimal property information until SSR emission. SIMP13 reuses the final query runtime for any surviving memo coordination copies. SIMP14 narrows final semantic-dispatch responsibilities. Their charters bind concrete starting paths, removed mechanisms and forbidden expansions.

Preserve one resolver, shallow/lazy demand, exact source/view/project identity, negotiated encoding, fact validation, complete-only publication, bounded retention, cancellation and failure recovery. No generic transport/memo/compiler framework, new global cache, second source authority or replacement catch-all context is justified merely by reducing duplicate lines.

D3C owns flow-product replacement; E2/E3 own internal transit/public DTO migration; G1/G2/G4 own query facts, same-key production and cache convergence; H2/H3 own ProviderHub/readiness publication; K3 owns host/service construction. Cleanup uses these owners and their eventual APIs. If a predecessor has already removed a named duplication, verify the resulting owner and preserve its proof instead of manufacturing a second refactor.

## Comments and tests alongside code

Remove repeated syntax narration, copied invariant essays, authorship markers and historical migration narratives in each scoped population. Keep non-obvious local ordering/encoding/correctness rationale, public API documentation, licensing and unsafe invariants. Store shared architecture knowledge once in its owning skill/document. Do not remove a useful correctness explanation merely because it is long.

Consolidate repeated setup and equivalent test permutations only within an actual domain contract, preserving hermeticity, independent state and discriminating failure output. No universal mutable fixture or assertion-count quota. SIMP13/SIMP14 include related test/comment simplification; SIMP15 records each original goal separately.

## Active ownership and readiness

DAG ancestry is the only automatic readiness rule. The new nodes do not modify or add prerequisites to D3R, D3I, D3P, D3C, E1, TCM1 or CCA1O4D. D3C is the existing atomic stack boundary, so waiting for it includes the nominal, binding and lattice predecessors. Shared-guidance/gate cleanup also follows SG0 rather than racing its restoration.

At authoring, the maintainer named PRs #472, #498, #501, #507, #508, #510 and #511. Their reservation mapping is in the decision. Refresh PR/base/file scopes before mutation. Edges encode known dependencies; conflict domains are planning instructions, not proof that an active surface is available.

PR #511 has no corresponding current TAMA node or ledger row. Do not fabricate a completion record or auto-dispatch a duplicate implementation. This train excludes packages/unplugin and pnpm-lock.yaml throughout; SIMP10's Rust-only change preserves downstream emitted contracts without requiring changes to that PR. A future expansion into those files requires ordinary authority reconciliation after the external work lands.

The separately open draft #98 is not incorporated or assumed approved. Svelte runtime, Svelte-specific guards, its compiler integration-test manifest and dependency-closure edits are excluded here. The existing Svelte/compiler owners retain them; this train's final review discloses those excluded populations rather than claiming a repository-wide scanner purge.

SIMP1, SIMP2 and SIMP7 can start from the already implemented A6 baseline. Other blocks follow only their relevant predecessor chains. No train manager may turn a broad test/comment sweep into permission to edit occupied or excluded files.

## Verification and measurement

Each delivery has named retained proof and the applicable final gate. Real behavior changes use TDD for uncovered boundaries; non-behavioral deletions use appropriate existing execution, type/compiler validation and bounded inspection. Preserve nonzero selection, actual execution, fresh prerequisites and truthful skipped-lane disclosures. Dedicated compile contracts, native/TS artifact prerequisites, provider and conformance lanes remain separate where currently owned.

No full Rust gate or runtime benchmarks are required for this roadmap-only amendment. Later implementation runs the owning gates. SIMP11 consolidates the existing metadata check without replacing the gate runner or weakening SG0.

Report separate production/test/comment/generated-data counts, removed operations/representations/state owners and retained proof. Compare build/archive and relevant suite costs on comparable runner/prerequisite states before claiming CI speedups; the source-policy aggregate already shares scans within its process. The manual scanners-replacement campaign tool is not itself a default CI job.

Production performance claims require equivalent-work, allocation and retention evidence under contracts/resource-and-finalization.md and the applicable ratified metric rows. No speculative percentage or blanket zero-regression threshold is introduced. Test deletion does not itself accelerate shipped Verter.

SIMP15 verifies the whole bounded population and final independent cumulative train review, after the ordinary 3–6 block checkpoints from APPLICATION.md. It may write concise contributor guidance and report inherited completed outcomes; it cannot supply missing production mechanisms, waive a required predecessor or create permanent scanner/coverage/receipt machinery. L1/L2 still own final system performance/memory proof, and L4 includes this train in architecture closure.

