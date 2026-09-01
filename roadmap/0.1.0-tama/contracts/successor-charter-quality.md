# Charter quality gate

The charters in this pack are implementation-grade drafts, not merely node descriptions. The canonical amendment generator should reject a successor charter that lacks any required section or violates the atomicity tests below.

## Required charter content

Every static implementation charter must contain:

1. **Independently acceptable outcome** — one result that can be accepted and used without the next node.
2. **Current and final owner** — no vague shared ownership.
3. **Architectural role and end state** — how the block fits the final product, not only current code edits.
4. **Expected production surfaces** — likely crates/modules plus the rule that dispatch binds exact paths/symbols.
5. **Named APIs/data boundaries** — concrete types, operations, identities, receipts, and outcomes.
6. **Exact predecessor contracts** — what each predecessor provides and why the edge exists.
7. **Binding architecture** — permanent invariants and authority laws.
8. **Internal subblocks** — each with an independently testable outcome, architecture, expected changes, and discriminating proof.
9. **Identity/invalidation/publication laws** — cache keys, epochs, read sets, provenance, and complete-only admission.
10. **Migration and cutover** — characterization, order, shadow/dual-read restrictions, and atomic route switch.
11. **Exact deletions** — only displaced authority owned by this block.
12. **Forbidden designs** — specific ways an implementer could superficially pass while violating architecture.
13. **Acceptance IDs** — positive and planted-negative proof, incremental/fresh, cancellation/admission, bounded work.
14. **Performance evidence** — equivalent work, allocation, latency, retained memory, and zero-work states.
15. **Scope-contradiction rescope/abort conditions** — no heroic implementation after authority or atomicity assumptions are disproved; LOC/file estimates remain planning references.
16. **Verification and consumers** — targeted commands/fixtures and what the block unlocks.
17. **Roadmap consistency** — the charter, DAG node, owning contracts, and implementation ledger agree.

Contract/constitution nodes may have zero production LOC, but their schemas and focused negative guards are still real acceptance artifacts.

## Atomicity tests

A node must be split before dispatch when any of these are true:

- two subblocks have independently useful results, separate deletion populations, or separate public ownership;
- a semantic algorithm is combined with a public/wire migration and a concurrency/lifetime change;
- a provider/supply-chain side effect is combined with candidate selection or process activation;
- semantic rename policy is combined with final edit application;
- authority arbitration is combined with every consumer adapter and terminal deletion;
- proof/conformance work starts patching missing semantics;
- investigation of material LOC/file/package drift shows hidden independently acceptable work, or one review context cannot understand the complete diff; the estimate alone is not a split trigger;
- source investigation disproves the named sole owner or predecessor contract.

The revised topology applies these tests explicitly:

- NCK6 authority arbitration is separate from NCK7 consumer integration and NCK8 terminal.
- LSO4 occurrences is separate from LSO5 rename semantics and LSO8 edit transactions.
- EPR2 acquisition and EPR3 shipping are separate from EPR4 selection and EPR5 activation.
- generated `NCF-*` feature slices are separate from NCK4 infrastructure and NCKF0 convergence.

## Discriminating proof standard

A test is not discriminating merely because it passes on the completed implementation. Each acceptance family must include at least one planted wrong implementation or mutation that fails for the intended reason, such as:

- duplicate or missing authority;
- stale/mixed epoch publication;
- source/mapper snapshot mismatch;
- wrong semantic target/occurrence role;
- provider key dispatched after swap;
- mapping fallback to 0:0/current file/nearest token;
- partial result warm-admitted as complete;
- hidden provider/network/workspace work in a zero-work mode;
- artifact/path substitution, signature/integrity failure, unsafe archive entry, or half activation;
- edit overlap silently dropped/reordered/partially applied.

Snapshot and count-only tests are supporting evidence, not architecture proof.

## Performance standard

Wall time alone is insufficient. Every hot or potentially broad block must declare and measure the relevant equivalent-work counters. Typical counters include:

- parse/lower/index/query/rule/provider/mapping/target/occurrence/conflict operations;
- candidates, graph nodes/edges, diagnostics/fragments/intents/edits/files;
- allocations, copied/staged bytes, retained regions/snapshots/cursors/keys;
- source adapter calls, stats/hashes/network bytes, validation, comparisons;
- process spawn/handshake/restart/swap/orphan handles.

Required modes are cold, first warm, repeated warm, incremental edit, edit-revert, cancellation, profile/provider/policy transition, project open/close, long churn, disabled/inapplicable/unopened zero-work.

A successor adding a real capability may have non-zero new work. The requirement is bounded declared work and ratified replacement SLOs, not an impossible blanket “0% regression” rule.

## Terminal-node restriction

`NCK8`, `LSO10`, and `EPR6` are proof/deletion/promotion nodes. They may:

- validate current implementation state and required manifests;
- run class-wide negative controls and terminal gates;
- delete displaced authority and migration shims;
- relocate product/editor documentation;
- emit capability snapshots.

They may not add semantic rules, target logic, occurrence roles, rename transforms, edit algorithms, source adapters, validation mechanisms, selection dimensions, or lifecycle behavior. Discoveries reopen the owning predecessor.
