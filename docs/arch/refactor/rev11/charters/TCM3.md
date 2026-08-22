# TCM3 — TypeScript semantic capability closure

**Status:** DRAFT, pending DAG amendment + authorization record.
**Predecessors:** TCM0, TCM1. May run parallel to TCM2 if TCM0 permits.
**Downstream:** TCM4. **Dormant until TCM4.**

## Scope

Implement TCM0's ratified ownership decisions, in this preference order:

1. **TypeScript LSP directly** owns a feature when content mappings express it
   correctly — Verter emits no duplicate result and does not re-remap the
   response; masks enable only ratified projections; document-wide operations
   have one named owner.
2. **Verter natively**, when framework analysis is authoritative — with
   conflicting TypeScript projections disabled where possible, and its own
   correctness and performance tests.
3. **A narrow official semantic API**, only for Verter-owned features that truly
   need checker/project facts.

**Do not route the old `TypeProvider` methods through a new IPC layer to preserve
its shape.** The oracle is snapshot-bound, structured, batch-oriented,
cancellation-aware, generation-validated, bounded, independent of mapper state,
and incapable of retaining remote compiler objects past snapshot release. Prefer
bulk operations — symbols and types at several positions, reference sets,
component/member surface extraction, batched facts for one template operation.

No markdown parsing, no generic LSP-response reconstruction, no private carrier
protocol, no second generic relay, no second project graph in editor-attached
operation.

**Snapshot correctness:** exact project/session identity and generation;
immutable scope; explicit release; no handle reuse; stale-response rejection;
cancellation under rapid edits; bounded concurrency; honest failure. A
semantic-plane failure may leave direct-LSP features working, but must never
fabricate empty successful results for features that needed the failed capability.

**Closure:** every old capability proven to have exactly one of — TypeScript
answers correctly, Verter native, Verter via certified oracle, or governance-
approved removal. If the official API lacks a capability the legal outcomes are:
reassign to direct LSP, implement natively, require and certify an upstream
addition, or keep TCM4 blocked. Never retain a private legacy query protocol.
