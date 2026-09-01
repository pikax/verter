<!-- unified-charter-v2
id=CPER0M
name=NAPI memory-audit snapshot coherence
phase=compiler
train=compiler.compiler-perf
product=compiler_perf
kind=repair
semantic_role=delivery
class=compiler
predecessors=CCA0
owner=compiler.compiler-perf:native memory-audit counter snapshot coherence
conflict_domains=performance_evidence
resource_class=rust-mixed
review_profile=concurrency-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-perf/CPER0M.md
max_production_loc=150
max_production_files=1
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CPER0M — NAPI memory-audit snapshot coherence

## Independently acceptable outcome and rollback boundary

The durable problem: the native memory audit documents its reported peak as a high-water mark of live bytes, but the counting allocator publishes the two values in separate relaxed steps — an allocating call adds to live bytes and only afterwards advances the peak. A snapshot taken between those two steps returns a live value the peak has not absorbed, so a caller can observe a peak below the live value it just read and the documented invariant is observably false. Re-arming the high-water mark has the same shape: it stores the peak from a separately loaded live value, so a concurrent allocation can be lost from the new window. Every access is atomic, so this is a snapshot-coherence defect rather than a data race, and any measurement or acceptance that consumes the peak inherits an unsound number.

Outcome: every snapshot the native surface returns satisfies the documented relationship between the peak and live bytes, under concurrent allocation and across a high-water re-arm, so the reported invariant is true as written. Reverting restores the current publication order and the false invariant.

## Concrete surfaces and APIs

- `crates/verter_napi/src/memory_audit.rs`: the counting global allocator's record path, the snapshot reader, and the high-water re-arm.
- A focused acceptance test in that crate's existing test surface, exercising concurrent allocation and snapshot observation together with a high-water reset.
- Invariants the repair must preserve: the disabled default path stays exactly one cached relaxed atomic load plus a branch, gaining no counter update, lock, allocation, or thread-local access; the enabled path takes no lock on the allocator hot path; the signed live-bytes epoch semantics, including negative values from pre-epoch blocks freed after enable, are unchanged; the reported field names, units, and the disabled-state return values are unchanged.
- Allocation-site sampling, the audit's public surface shape, benchmark harnesses, and every non-native crate are excluded.

## Exact predecessor contract

- **CCA0:** implemented ledger row for “Compiler authority, policy, demand, and admission constitution”; ledger presence alone satisfies it, and its commit message, approximate timezone-bearing date, and optional pull request are locator hints only.

## Acceptance and evidence

- An acceptance test drives concurrent allocation and snapshot observation and asserts that every returned snapshot satisfies peak-at-least-live; it fails against the current publication order and passes after the repair.
- The same relationship holds across a high-water re-arm: a concurrent allocation cannot be lost from the new window, and snapshots taken during the re-arm still satisfy it.
- Enabling still starts a fresh counter epoch, and the first snapshot after enabling satisfies the relationship.
- The disabled path performs the same per-allocation work as before, with no lock, allocation, or extra bookkeeping added.
- Live bytes remain exact across allocation, deallocation, and reallocation, including the negative pre-epoch case, and failed allocations are still unrecorded.
- The module's own documented invariant matches the enforced behavior.

## Deletions, budgets, and aborts

- Delete nothing; the change is a publication-order and read-coherence repair inside the existing counters.
- Planning guidance: roughly 150 production LOC in 1 production file in 1 crate. These figures are guidance, not a cap; rescope only under the program's mandatory thresholds or when the audit's public surface or sampling path enters.
- Abort on a lock or allocation on the allocator hot path, on a repair that reorders only the snapshot reads while leaving the re-arm incoherent, on a reported peak that is not a value live bytes actually reached in the window, or on a test that also passes against the current publication order.

## Verification and review

Write the concurrent acceptance test first and prove it fails against the current publication order, then run the native crate suites and `targeted-domain`. Apply `concurrency-3`; add only CPER0M's ledger row. It must land before any acceptance consumes the reported peak.
