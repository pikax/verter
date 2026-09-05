# Resource bounds and architecture closure

This contract binds MEM0, E4, MEM1, G4 and L1–L4. SG0 owns restoration of shipped-configuration execution. The implementation ledger remains the only completion authority; the artifacts below are ordinary product/verification evidence, never additional readiness state.

## Accounting and ownership

- **MEM0** ratifies the budget and workload before E4/MEM1 change physical retention. It produces `catalogs/semantic-memory-budget.toml`, its schema and hermetic workload fixtures. Required fields include the normal and pressure-test byte ceilings, per-entry admission limit, maximum simultaneously admitted requests, bounded active-request bytes, pinned-result policy, sampling cadence, exact baseline/metric references and all commands that build and exercise the workload. Every limit is a finite integer derived from measured supported workloads and reviewed before implementation; no symbolic unlimited/default placeholder is accepted. Its baseline must distinguish an invalid historical result from equivalent work.
- **E4** owns physical storage lifetime: retained parse snapshots, declaration memo bodies, graph/interner regions, fact/read-set storage and shared payload allocation accounting. Every allocation has one charge owner; shared `Arc` references do not double-charge, and moving an allocation between active and retained ownership transfers its charge rather than resetting it. Live readers cannot lose valid handles through eviction.
- **MEM1** owns the process-local aggregate retention/admission policy over those charges, shared across projects and host sessions. It composes existing per-family caps with total retained-byte and per-entry caps; it does not replace fact validity, semantic identity, query singleflight or E4 reclamation. Reservations precede admission, concurrent reservations cannot oversubscribe, and failure/cancellation releases reservations exactly once. A complete result too large for the retained budget can return uncached when it fits the active request/result budget. Exhausting that budget returns the existing typed resource-limited outcome, never a fabricated complete result. Pressure cannot authorize stale reuse or make a partial result cacheable.
- **G4** certifies that every shared store and retained parse worker uses these owners, with no unaccounted independent cache or alternate admission policy. G4 repairs no missing memory mechanism: a failed row returns to E4 or MEM1.

The budget distinguishes cache-owned, request-owned and externally pinned result bytes. It may bound cache retention and admission, but must not claim a hard bound over arbitrarily many results a caller chooses to retain. Pinned bytes remain visible until release; dispatch pressure limits new work without revoking caller ownership. The public contract explicitly states the externally retained portion and its backpressure behavior. RSS includes allocator/runtime overhead and is measured separately from exact owned-byte accounting.

Required pressure boundaries include: one entry exceeding the cache cap; many distinct keys below their per-family caps; many project sessions sharing one process; edit/revert versions; concurrent cold winners; cancelled and failed construction; closed projects with and without active readers; and held-then-released public results. A pressure decision may change warm versus cold work, never semantic output or completeness for a completed request.

## L1: executable long-churn evidence

MEM0 freezes an ordered hermetic manifest of at least 10,000 actions. Every action names its source fixture, project, query identity, edit/configuration delta, cancellation point and expected result/admission class. The manifest covers both Vue and Svelte, cold and warm requests, high-cardinality keys, edit/revert, project open/close, provider-independent configuration changes, overlapping requests, failed construction and retained-result release. Same-key repetition alone cannot satisfy this workload.

L1 installs an executable runner and binds its exact command in the gate profile or applicable required CI job before its row can be implemented. No generic nextest success substitutes for executing the workload. It runs normal and pressure budgets; inventories all planned actions; requires nonzero completed semantic requests and every declared action class; compares applicable incremental results with fresh construction; and rejects missing summaries, unexpected skips and stale/partial warm admission.

Sample exact allocation classes and aggregate retained bytes at least every 100 actions, and return to the same manifest-defined control live set after each sampling tranche. The first 2,000 actions establish the steady-state envelope; subsequent control samples must satisfy the fixed MEM0 retained-byte envelope and the absolute budget throughout. Project-close and released-reader checkpoints must return attributable owned bytes to the documented remaining live set. The envelope cannot be increased after observing the candidate. MEM0 supplies the separately ratified RSS/statistical bounds, including allocator slack; unexplained positive growth at equivalent control states fails even if a single end sample happens to be small.

Prove the runner catches a planted retained-key leak and a missing workload class. The mutation must demonstrably apply before its negative run is evidence. L1 may improve the runner and its measurement, but memory/semantic failures return to their owning implementation node. A proof-only diff never makes the soak not applicable.

## L2: controlling performance evidence

Each performance-sensitive acceptance report maps its outcome to exact rows in `performance-gates.toml`, the MEM0 budget, or the owning product's ratified performance catalog. Existing applicable rows retain their thresholds, paired-run methodology, noise allowance and baseline identity. Charter boilerplate does not override them.

Three kinds of limits remain distinct:

1. Exact work/correctness invariants (for example zero stale admission or zero duplicate parse) have exact integer limits.
2. Wall time, allocations and RSS use their owning row's measurement method and limits; a statistical metric has no implied 0.0% comparison bound.
3. A new capability or deliberate pressure policy declares bounded new work and replacement SLOs before measurement. Missing coverage requires a reviewed owning-contract amendment, never a post-hoc rebaseline or not-applicable assertion.

L2 compares correctness-equivalent direct/prepared/batch/host results, latency distributions, allocation and RSS with those controlling rows, including L1's completed normal/pressure experiments. Invalid outputs are not benchmark wins. The report identifies unmeasured cells and fails required ones; it never turns the roadmap's node completion percentage into performance evidence.

## SG0 and L4: complete architecture close

SG0 restores `SHIPPED_CFG_LANE_ENABLED` and proves both the shipped-cfg compile surface and behavioral contract execute through `scripts/gate.mjs`. CI and release must run the same restored lane or an explicitly equivalent owned invocation. `cargo check --release` alone is insufficient. L4 cannot close while that execution lane remains disabled, skipped or missing. This pending node is the restoration owner; this amendment does not claim the lane is already restored.

L4's acceptance inventory is derived from the current Rev11 DAG, its compiler-bridge ancestors, owning contracts and approved amendments. Every intended outcome maps to its final owner, public consumers, displaced route disposition and concrete executed evidence. Include per-train cumulative review conclusions and their resolved material findings, zero open deferrals, truthful unsupported/partial capability cells, performance and memory results, source-map/encoding boundaries, native/WASM/provider boundaries and required-job execution coverage. Historical/superseded and optional outcomes receive explicit dispositions rather than disappearing from the inventory.

The close report must establish all of the following:

- All required ancestors are implemented; no terminal is used to add missing semantics or lifetime mechanisms.
- Every displaced authority has one surviving owner and no undeclared production fallback.
- Every applicable required evidence command/job ran on the accepted candidate, selected nonzero work, produced a complete summary and had zero unexpected prerequisite skips. The evidence map states local versus CI ownership, fixture/build prerequisites and negative controls; the core gate does not stand in for specialized suites.
- No unresolved P0/P1, open deferral, unexplained memory/performance failure or undispositioned verification exception remains. The temporary shipped-cfg exception must be resolved by SG0.
- Owning contributor/user documentation describes the accepted system. Remove the temporary program precedence banner from CLAUDE.md only when its architecture pointers and summaries are accurate. Historical charters remain frozen.

L4 emits a reviewable outcome/evidence table and receives the final train review in addition to its own review profile. Independent product terminals remain independent; this is Rev11 closure, not an all-products join. Later product terminals apply the same evidence discipline to their own declared capabilities.
