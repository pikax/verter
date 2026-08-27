# F26 — `FactVersionRef`'s true scope corrects F25's payload-closure ruling

**Trigger:** while starting to execute F25's ruling ("`FactVersionRef` MOVEs
to `verter_semantic`"), reading the type's full definition
(`crates/verter_workspace/src/fact_cache.rs:904-935`) revealed it is NOT a
resolution-scoped type — it is `verter_workspace`'s crate-wide,
general-purpose fact-versioning identity vocabulary. Only 1 of its 10
variants (`ResolveImports`) is resolution-related; the other 9 cover
whole-file hashing, derived-fact hashing, OXC parse facts, route-surface
facts, program-analysis/flow-body facts, file source-environment facts,
project-generation counters, domain-aggregate compaction, and
strict-self-root-world cache-validation witnesses. It is referenced in 11
other `verter_workspace` files and **40+ `verter_session` files**
(`fact_cache.rs` itself is 1914 lines) — none of which have anything to do
with module resolution. F25's prompt showed only the `ResolveImports` arm,
so its ruling ("move the whole enum, every payload type required by its
variants") was made without visibility into this true scope. Per the
sixth-deviation protocol, escalated rather than guessing whether to move
the full crate-wide vocabulary into a resolver-charter crate or narrow the
type semantic's `AttemptOutput` actually holds.

**Command:** same `codex exec` invocation as prior consults. Full
prompt/output at `/tmp/c1-f26-prompt.md` / `/tmp/c1-f26-output.md` (not
committed; condensed here).

## Ruling: refined Option A — F25's conclusion survives, its inventory was incomplete

`AttemptOutput.observed_facts` continues to carry the full canonical
`FactVersionRef` by value (`Vec<FactVersionRef>`), re-pathed to
`verter_semantic::facts::FactVersionRef`. Rationale: `AttemptOutput` is not
merely `ModuleResolverCore`'s resolution witness — it is the common
outbound-effects envelope for the whole relocated non-flow kernel;
`observed_facts` is the relocated replacement for
`ResolverContext::observe_borrowed_signature`, and already-versioned
non-resolution facts (`FileWholeHash`, `DerivedFactHash{kind: Route}`) are
genuinely constructed and merged into this same envelope at other call
sites (`verter_session/src/host_manage/prepared_decl.rs:2703`,
`verter_session/src/host_resolve/frontier_engine.rs:647`) — a
`Vec<ResolveImportsFactRef>` would lose real currency those sites already
carry. The `FileWholeHash` unit-test fixture is representative of intended
design, not an accidental over-generalization. `consumed_resolution_
observations` (`ConsumedResolutionObservationKey`) stays the separate,
narrower field for unversioned resolution selectors the workspace driver
enriches — `observed_facts` and that field have deliberately different
jobs and neither replaces the other.

**Exact ownership boundary — MOVE into `verter_semantic::facts`:**
- `FactVersionRef` and every type embedded by its variants, transitively
  (enumerated below).
- `FactAttribution` (returned by `FactVersionRef`'s inherent value ops).
- `CompactionDomain`, `DomainGenerationFact`, and their immutable
  stamp/population closure (`FactVersionRef` payload identity).
- The resolution-reference/key/version DTOs already named by F25
  (`ResolveImportsFactRef`, `ResolutionFactRef`, `ResolutionFactVersion`,
  `ResolutionFactKey`, `ResolutionQueryKey`).

**STAYS in `verter_workspace`/`verter_session`** (cache authority, not
vocabulary — unchanged from F25's framing, now stated precisely):
- `FactVersionValidator` (the trait).
- `FactReadSet` and tracer/finalization machinery.
- Cache admission, validation, mutation propagation, counters, compaction
  algorithms, replay ledgers, publication, invalidation.
- Candidate-retention policy (`CANDIDATE_CAP` and siblings).

This does NOT move `fact_cache.rs` wholesale — it moves the immutable
discriminated identity IR; outer layers continue to produce, validate,
compact, store, and publish it. The `ProgramAnalysis` arm moving does not
pull flow semantics into C1 — only the neutral identity carrier needed to
keep one exhaustive fact-reference type.

## Ranking of the 4 options considered

1. **Refined A (adopted)** — move the full immutable value graph, not the
   cache implementation.
2. **C (adopted as a complement, not a replacement)** — keep
   `ConsumedResolutionObservationKey` as the resolution-replay rail; do NOT
   remove `observed_facts`.
3. **B (rejected)** — `Vec<ResolveImportsFactRef>` loses non-`ResolveImports`
   facts already present in real route-signature call sites and can't serve
   generic TypeInfoCore signature replay.
4. **D (rejected)** — consumer count does not determine inward ownership;
   making `verter_session` canonical would force `verter_workspace` to
   depend upward on `verter_session` or invent another neutral crate,
   contradicting the ratified dependency direction and the
   extract-into-existing-`verter_semantic` ruling.

Workspace may retain compatibility re-exports pointing inward to
`verter_semantic`; semantic must never alias outward.

## Deviation-protocol calibration (explicitly asked, explicitly answered)

Escalation was correct, not over-cautious. F25's binding text said "the
whole dependency-neutral fact-reference value graph" — when the real type
made that wording unexpectedly broad (crate-wide vocabulary, not a
resolution-scoped closure), stopping for disposition was the right
judgment, not something inferable from context. Disposition:
**ADOPT-NOW refinement/confirmation** — not an abort/rescope, not a
reversal of F25.

## Disposition

Not a rule conflict, not a STOP condition. The sequencing record's F25 row
is updated to enumerate the full general value closure and state which
cache machinery does not move (this consult). Execution proceeds per F25's
already-ratified commit shape (one atomic Stage-2 commit, WIP-then-squash
checkpoints sanctioned) — this consult only corrects the payload inventory
for one of the 5 items, not the overall plan.
