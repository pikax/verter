## 1. Amendment `AMD-004`

Create [AMD-004-defer-completion-to-d6.md](<REPO>/docs/arch/refactor/rev11/amendments/AMD-004-defer-completion-to-d6.md) with exactly:

```markdown
# AMD-004 — Defer structural completion to D6 and reduce A3

**Status:** Registered amendment (maintainer-ratified exception to the normal
verbatim-authority policy — see [`../PROVENANCE.md`](../PROVENANCE.md)).
**Registered in:** [`../README.md`](../README.md) and
[`../evidence/maintainer-rulings.md`](../evidence/maintainer-rulings.md) (R-11).
**Amends:** [`AMD-002-a2c-completion-predecessor.md`](AMD-002-a2c-completion-predecessor.md),
[`AMD-003-a2c-completion-graph-authority.md`](AMD-003-a2c-completion-graph-authority.md),
[`../program.md`](../program.md), [`../program-dag.toml`](../program-dag.toml),
[`../charters/A2C.md`](../charters/A2C.md), [`../charters/A3.md`](../charters/A3.md),
[`../../../u6-flow-return-gaps-and-target.md`](../../../u6-flow-return-gaps-and-target.md),
and the external live `program-state.toml`.

The published consolidated master, release artifacts, `_EXTRACTION_INDEX.md`, and
historical readiness-review prose remain immutable historical originals. The rejected
completion candidates, V3 specification, specification amendment 1, stop findings,
benchmark records, and failed tests remain historical evidence. None is accepted
implementation.

## The defect

The completion predecessor has failed four times:

1. The first candidate discriminated correctly but retained 10,616 bytes, performed
   157 allocations, and failed the latency gate at 746%.
2. The second candidate retained zero bytes and performed zero allocations but still
   failed target-heavy latency cells at 72–78%.
3. V3 implementation stopped because its purportedly exhaustive transient-carrier
   inventory omitted `DrainedFlowReturnMember`. Specification amendment 1 corrected
   that specification defect.
4. The resumed V3 implementation stopped because sections 8 and 9 contradict each
   other. Section 8 derives `observed` solely from `result.can_fall_through`, but X68
   and X80 contribute `undefined` through `implicit_undefined_seen` while
   `can_fall_through == false`. The required X68/X80 clean result therefore cannot
   satisfy both sections. Nine session tests were written; two pass and seven fail.
   No candidate was committed or accepted.

Repeatedly expanding an ahead-of-code list of completion carriers and semantic cases
is not a finishable prerequisite for A3–A6. Forcing full structural completion through
that critical path would either continue the stop/rewrite loop or weaken false-refusal
discipline. Both outcomes are rejected.

## The amendment

1. `A2C` is retired as an executable predecessor. Its DAG and ledger row is retained
   as a reachable historical row so the validated block universe remains exactly 51
   blocks. The live row becomes terminal `SUPERSEDED`. It may not re-enter `READY`,
   `IN_PROGRESS`, `REVIEW`, `ACCEPTANCE_RECOMMENDED`, or `ACCEPTED`.
2. `A3`'s sole predecessor becomes `A2`. `A2C` is not a predecessor of A3 or of any
   later block. `A4` remains dependent on A3; no other predecessor list changes.
3. A3's exit is reduced to the non-G10 A2-catalogued wrong-complete retractions. Each
   retracted result must use the existing typed degradation/non-admission rails and
   remain cold. Every checker-correct clean/warm preservation row, including X05,
   must remain complete, undegraded, admitted once, and warm on replay.
4. A3 has no G10 obligation. It must not add a syntax-only G10 detector, inspect
   completion syntax for G10, interpret skeleton topology or graph edges, or create a
   second completion classifier. It must not introduce false refusals to compensate
   for the deferred completion authority.
5. Exact structural completion and G10 discrimination become recorded debt owned by
   D6 / `U6.LOOP_CLOSURE`. The debt must close before D6 enters review. A4, A5, and A6
   do not depend on its early completion.
6. Heavy completion work may resume only after the D6 lock contains a closed,
   code-first carrier inventory covering every producer, transient carrier,
   construction, transfer, discharge, result-assembly, publication, and admission
   exit. The inventory must be executable and mutation-discriminating; an open-ended
   prose list amended one missed carrier at a time is not an admissible implementation
   specification.
7. The architectural constraints learned from V3 remain binding:
   - the function skeleton carries content-free canonical topology only;
   - the demanded `FunctionFlowGraph` is the sole completion reducer;
   - completion meaning is not reconstructed from statement syntax;
   - A3 responds only to typed `FlowGap` information supplied by an owning producer;
   - no second graph, completion classifier, target-indexed completion set, or
     syntax-only G10 fallback may be introduced.
8. Failed latency candidates remain unlanded. The partial V3 work may be parked only
   as historical code-first evidence after this rescope is recorded. It carries no
   approval and supplies no predecessor satisfaction.

## Supersession of AMD-002

This amendment supersedes AMD-002 point 1 only where that point makes A2C the sole
predecessor of A3. The A2C row remains present, but A3's predecessor is A2.

AMD-002 points 2 through 4 were already superseded by AMD-003 and remain non-operative.
AMD-002 point 5 remains in force: the DAG, exact-state template, and external live
ledger must retain exactly the same A2C row identifier, and the live ledger must bind
the current DAG digest. AMD-002's scope prohibitions remain binding on the deferred D6
work.

AMD-002's execution-precedence lineage `A2 → A2C → A3` is superseded. The executable
critical-path lineage is `A0 → A1 → A2 → A3 → A4 → A5 → A6`.

## Supersession of AMD-003

This amendment supersedes AMD-003 amendment points 1 through 4 as requirements on the
A2–A6 critical path. In particular:

- A2C no longer delivers an early structural slice of D6;
- A3 no longer waits for or consumes a G10 abrupt-completion verdict;
- the AMD-003 completion implementation and performance instrument are not A3–A6
  predecessor gates; and
- the AMD-003 failure-and-stop loop is closed rather than resumed.

AMD-003 remains in force as historical failure evidence and for the architectural
constraints explicitly retained above: content-free skeleton topology, demanded graph
authority, no second classifier, no fixed target ceiling, and no A3-only retained
payload. Its rejected-candidate source disposition and measurements do not transfer to
D6 acceptance. D6 must freeze its own finishable implementation lock only after the
closed code-first carrier inventory exists.

AMD-001 is unaffected and remains fully in force.

## Execution precedence

For execution, AMD-004 and the amended live split files supersede the A2C/A3 lineage
and completion-staging text in AMD-002, AMD-003, `program.md`, the pinned consolidated
master, release artifacts, `_EXTRACTION_INDEX.md`, and historical readiness reviews.

The executable lineage is:

`A0 → A1 → A2 → A3 → A4 → A5 → A6`

A2C remains a reachable terminal historical row with predecessor A2 and status
`SUPERSEDED`. The DAG, tracked template, and external live ledger each contain exactly
51 block identifiers.
```

Registration is not optional. Add R-11 to `maintainer-rulings.md`, using the maintainer’s verbatim decision from the question, and add an AMD-004 registry bullet stating that it supersedes the A2C predecessor and reduces A3 while leaving completion debt with D6. Update `README.md` and `PROVENANCE.md` so they say AMD-004, not AMD-003, is the current execution precedence.

## 2. `program-dag.toml`

In [program-dag.toml](<REPO>/docs/arch/refactor/rev11/program-dag.toml), replace:

```toml
[[block]]
id = "A2C"
name = "Abrupt-completion facts for G10 safety discrimination"
class = "foundational-safety"
predecessors = ["A2"]

[[block]]
id = "A3"
name = "Immediate wrong-complete safety retraction"
class = "foundational-safety"
predecessors = ["A2C"]
```

with:

```toml
[[block]]
id = "A2C"
name = "Retired completion predecessor; deferred to D6"
class = "foundational-safety"
predecessors = ["A2"]

[[block]]
id = "A3"
name = "Immediate wrong-complete safety retraction"
class = "foundational-safety"
predecessors = ["A2"]
```

Resulting relevant predecessor sets:

```text
A2  = [A0, A1]   unchanged
A2C = [A2]       unchanged; terminal historical leaf
A3  = [A2]       changed from [A2C]
A4  = [A3]       unchanged
D6  = [D3]       unchanged
```

No other predecessor changes.

The block count remains exactly **51**. It does not move from 51. A2C remains reachable from the sole root through `A0 → A2 → A2C`, so the retained leaf satisfies the validator’s reachability rule.

## 3. Charter text

Replace [A2C.md](<REPO>/docs/arch/refactor/rev11/charters/A2C.md) completely with:

```markdown
# A2C — Retired completion predecessor

**Status:** SUPERSEDED by AMD-004; retained only as a historical DAG/ledger row.  
**Class:** Foundational safety, historical.  
**Predecessors:** A2.  
**Successors:** None.

## Disposition

A2C is not executable. It has no accepted candidate and may not re-enter `READY`,
`IN_PROGRESS`, `REVIEW`, `ACCEPTANCE_RECOMMENDED`, or `ACCEPTED`.

The rejected eager-skeleton candidates and the incomplete V3 implementation remain
unlanded historical evidence. Their correctness, performance, mutation, and test
results do not transfer to another block.

Exact structural completion and G10 discrimination are deferred to D6 /
`U6.LOOP_CLOSURE` under debt row `FR-D8` in
`docs/arch/u6-flow-return-gaps-and-target.md`.

## Preserved architecture constraints

- `FunctionBodySkeleton` carries content-free canonical topology only.
- The demanded `FunctionFlowGraph` is the sole completion reducer.
- Completion meaning is not reconstructed from statement syntax.
- A3 consumes only typed `FlowGap` information from an owning producer.
- No syntax-only G10 detector, second completion classifier, second graph,
  target-indexed completion set, fixed target ceiling, or A3-only retained payload
  may be introduced.
- Checker-correct clean/warm cases must not be refused to make completion evidence
  appear safe.

## Resume condition

Heavy structural-completion work may resume only under D6 after its implementation
lock contains a closed, code-first inventory of every completion producer, transient
carrier, constructor, transfer, discharge route, result-assembly input, publication
exit, and admission exit. The inventory must be pinned to one checkout and proven by
real-path tests and transfer-site mutations.

## Exit criterion

There is no implementation exit criterion. The live A2C ledger row closes terminally
as `SUPERSEDED` under AMD-004.
```

Replace [A3.md](<REPO>/docs/arch/refactor/rev11/charters/A3.md) completely with:

```markdown
# A3 — Retract known wrong-complete results

**Status:** PREPARED for the reduced non-G10 exit under AMD-004.  
**Class:** Foundational safety.  
**Predecessors:** A2.  
**Gate 0 lineage SHA:** `UNSET`; record the exact restacked candidate for this evidence block.

## Objective

Retract every A2-catalogued wrong-complete result except G10 through typed
non-admissible outcomes, without changing the owning semantics and without refusing
any checker-correct clean/warm result.

## In scope

- typed `FlowGap`, degraded usable-result, or existing `NoValue` outcomes for the
  non-G10 A2-catalogued wrong-complete cases;
- propagation through the existing result, SCC, consumer, and cache-admission rails;
- separation of legitimate semantic `any` from an unmodelled inference fallback;
- evidence and source changes strictly necessary for those non-G10 retractions;
- the 154-row accepted-A2 checker-correct clean/warm preservation cohort;
- deletion or renaming of block-named evidence scaffolding before landing.

## Explicit non-obligations

- G10 discrimination, structural completion facts, completion topology, completion
  edges, root coverage, endpoint disposition, or final completion semantics;
- a syntax-only G10 detector or any inspection of statement syntax for the purpose of
  classifying G10;
- reading skeleton regions, completion events, graph edges, or an endpoint accessor;
- repairing narrowing, nominal relation, closure transfer, value inference, or
  completion semantics owned by later blocks;
- a second classifier, graph, cache policy, admission gate, or final-result typestate;
- changing or refusing a checker-correct clean/warm result to make a detector appear
  conservative.

`FlowGap::AbruptCompletion` may remain a reserved typed carrier, but A3 must have no
producer or syntax-only detector for it.

## Required evidence

- Cold and replay calls for every non-G10 A2-catalogued wrong-complete case show a
  typed degraded usable result or existing typed `NoValue`, zero admitted candidates,
  no warm hit, and non-zero recomputation.
- Authored and otherwise legitimate semantic `any` remains complete and warm.
- The 154-row preservation cohort is fingerprint-locked by script, probe, and checker
  expectation. Every row remains `degradation == None`, admits exactly one candidate,
  is cold on the first public call, warm with zero cold work on the second call, and
  returns byte-identical cold/warm output.
- X05 is included in that preservation cohort and remains checker-correct, clean, and
  warm. X68, X80, and X88 remain adjacent completion false-refusal controls.
- Mutation evidence proves that removing each non-G10 degradation transfer restores
  the corresponding wrong-and-warm publication or bypasses the existing admission
  refusal.
- No test, module, source identifier, source path, diagnostic label, or production
  comment introduced by the candidate contains program revision or block vocabulary.

## Exit criterion

Every A2-catalogued known wrong-and-warm result except G10 returns a typed degraded
usable result or typed `NoValue` and is refused warm admission. Authored `any` and
every member of the 154-row checker-correct clean/warm preservation cohort remain
complete and warm. No syntax-only G10 detector exists, G10 is not falsely refused, and
the candidate introduces no false refusal in the preservation cohort or named adjacent
controls.

G10 remains open debt `FR-D8`, owned by D6. Its exclusion is an explicit rescope, not
evidence that its current result is correct.

## Abort/rescope

Stop when the exact checkout, command target, product capability, current owner,
compatibility obligation, or proof boundary differs materially from the charter
assumptions. Amend the lock/charter rather than widening silently.

## Review

Exact-SHA conformance, architecture, and adversarial/performance mandates apply
according to `governance.md`. A3 is accepted only when its evidence is attached to one
unchanged restacked candidate/evidence SHA.
```

## 4. Durable debt row

The row belongs in [u6-flow-return-gaps-and-target.md](<REPO>/docs/arch/u6-flow-return-gaps-and-target.md), immediately after the §3 ownership mapping table:

```markdown
### Recorded completion debt

| debt ID | disposition | debt | durable owner | resolution gate | acceptance ID |
|---|---|---|---|---|---|
| `FR-D8` | `DEFER` under `AMD-004` | Exact structural completion and G10 discrimination; the current producer can still publish the G10 wrong-and-warm result. | D6 / `U6.LOOP_CLOSURE` | Must close before D6 enters `REVIEW`. Heavy implementation may begin only after the D6 lock contains a closed, code-first carrier inventory. The demanded `FunctionFlowGraph` must be the sole completion reducer; G10 must match the pinned checker, X05/X68/X80/X88 must remain checker-correct clean/warm, and no syntax-only classifier or second completion authority may exist. | `d6_structural_completion_closes_g10_without_false_refusals` |
```

This supplies all four required pieces: durable owner, resolution gate, acceptance ID, and ruling reference.

## 5. External ledger disposition

In [program-state.toml](<EVIDENCE>/program-state.toml), change:

```toml
current_block = "A2C"
```

to:

```toml
current_block = "A3"
```

Replace the A2C row with:

```toml
[[block]]
id = "A2C"
status = "SUPERSEDED"
charter_digest = "<lowercase SHA-256 of the replacement charters/A2C.md>"
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "INVALIDATED"
architecture_review = "INVALIDATED"
adversarial_review = "INVALIDATED"
maintainer_decision = "SUPERSEDED"
notes = "Superseded by AMD-004 before candidate review or acceptance. The prior execution-charter digest was 101d9d718952da40a5b4da88ea232cedd0619b35d291de6052149d86a5dcbff1. V3 and amendment 1 remain historical failed-specification evidence; completion and G10 debt is FR-D8, owned by D6."
```

Replace the A3 row with:

```toml
[[block]]
id = "A3"
status = "READY"
charter_digest = "<lowercase SHA-256 of the replacement charters/A3.md>"
context_packet_digest = ""
base_sha = ""
candidate_sha = ""
candidate_tree = ""
accepted_sha = ""
accepted_tree = ""
landing_equivalence_digest = ""
evidence_digest = ""
stack_id = ""
stack_snapshot_digest = ""
stack_layer = 0
conformance_review = "PENDING"
architecture_review = "PENDING"
adversarial_review = "PENDING"
maintainer_decision = "PENDING"
notes = "Reduced by AMD-004 to non-G10 retraction only. G10 is excluded from this exit and tracked as FR-D8 for D6."
```

`READY` is the honest immediate state: the preserved implementation has not yet been restacked and reverified against the rescope landing.

After the repository amendment lands, also set:

```toml
[repository]
head_sha = "<full SHA of the rescope landing>"
head_tree = "<full tree OID of the rescope landing>"
dirty = false
untracked_count = 0
```

Recompute these three digests:

```powershell
(Get-FileHash -Algorithm SHA256 -LiteralPath docs\arch\refactor\rev11\program-dag.toml).Hash.ToLowerInvariant()
(Get-FileHash -Algorithm SHA256 -LiteralPath docs\arch\refactor\rev11\charters\A2C.md).Hash.ToLowerInvariant()
(Get-FileHash -Algorithm SHA256 -LiteralPath docs\arch\refactor\rev11\charters\A3.md).Hash.ToLowerInvariant()
```

Put the first value in top-level `program_dag_digest` and the latter two in their respective ledger rows.

The tracked template keeps all 51 rows; no template row is added or removed. Validate both surfaces:

```powershell
node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml --state docs/arch/refactor/rev11/templates/program-state.template.toml --mode template
node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml --state <EVIDENCE>/program-state.toml --mode live
```

Both must report `validated 51 blocks` and exit 0.

## 6. U6 ownership and staging edits

In §3, replace the G10 ownership row:

```markdown
| G10 | D6's sole completion graph, delivered early through `A2C`; `A3` owns only retraction/non-admission | canonical control topology/events, completion edges and root coverage on `FunctionFlowGraph`; typed `FlowGap::AbruptCompletion` for A3 |
```

with:

```markdown
| G10 | D6 / `U6.LOOP_CLOSURE`; A3 has no G10 obligation under AMD-004 | debt `FR-D8`: demanded completion reduction, completion edges, root coverage, and final clean semantics on the sole `FunctionFlowGraph`; no syntax-only fallback or second classifier |
```

Insert the debt table from section 4 immediately after that ownership table.

In §4.3, replace the current “Revision 11 staging” paragraph with:

```markdown
Revision 11 staging under AMD-004: exact structural completion and G10 discrimination
are deferred from the A2–A6 critical path and recorded as debt `FR-D8`, owned by D6 /
`U6.LOOP_CLOSURE`. A3 retracts only non-G10 wrong-complete results through typed
degradation and existing non-admission rails. It has no syntax-only G10 detector and
must preserve checker-correct clean/warm cases, including X05, X68, X80, and X88.
When completion work resumes, the skeleton remains content-free topology, the demanded
`FunctionFlowGraph` is the sole completion reducer, and no second graph or completion
classifier is permitted.
```

## 7. Branch and worktree dispositions

- `<REPO>-wt-a2c`: **park as a branch, then remove the worktree**—but only after AMD-004 and the ledger rescope are recorded. Rename or preserve it as `preserved/a2c-v3-partial-carrier-inventory`.

  Preserve the complete dirty snapshot at base `74d6e0f4086a8337b0a63f126ac46ede6664a07b`: all modified semantic/session carrier files, the new `completion.rs`, both new completion test files, and the exact nine-test state where two pass and seven fail. Also preserve the association with V3 and amendment 1. It is worth preserving because it exposes the real transient carrier graph, including the previously omitted `DrainedFlowReturnMember`, and the `can_fall_through`/`implicit_undefined_seen` contradiction. It is code-first inventory evidence, not a candidate.

  Neutral scratch commit message:

  ```text
  wip(semantic): preserve partial completion carrier inventory
  ```

- `block/a3-retraction` at `377bf8fa2`: **keep as the source branch, but do not land it as-is**.

  Substantively, it matches the reduced semantic exit: non-G10 typed retractions, no G10 detector, a reserved `AbruptCompletion` carrier, the 154-row preservation cohort including X05, authored-`any` controls, 58/58 focused tests, and 12/12 guards.

  It still fails the current landing contract in two concrete ways:

  1. It is based on the pre-rewrite candidate parent and must be restacked onto the AMD-004 landing.
  2. It introduces forbidden program vocabulary in source/test identifiers.

  Required renames before review include:

  ```text
  u6_flow_a3_retraction_tests.rs
    → u6_flow_gap_retraction_tests.rs

  a3_* test names
    → flow_gap_* names

  A2_CLEAN_CHECKER_MATCH_COHORT
    → CLEAN_CHECKER_MATCH_PRESERVATION_COHORT

  a3_preserves_every_a2_clean_checker_match
    → flow_gap_retraction_preserves_clean_checker_matches

  /a3 and /a3-preservation fixture paths
    → /flow-gap-retraction and /flow-gap-preservation

  A3_CASE
    → FLOW_GAP_CASE
  ```

  Rewrite “accepted-A2” assertion prose as “locked baseline.” Then rerun the focused tests, both canonical Rust gates, and guards against the unchanged restacked candidate.

- `preserved/a2c-eager-skeleton-candidate` at `04048a947`: **keep unchanged as immutable failed historical evidence**. It stays unlanded. Its zero-allocation result is useful evidence that allocation elimination did not solve target-heavy latency, but no code or acceptance result transfers.

## 8. Next legal block

Immediately after the rescope transition, the next legal block is **A3**:

```text
A2 = ACCEPTED
A3.predecessors = [A2]
A3 = READY
A2C = SUPERSEDED and is not an A3 predecessor
```

Once reduced A3 is accepted, A4 becomes legal because its unchanged predecessor set is `[A3]`. A5 and A6 then follow their existing chain.

Compliant landing messages:

```text
docs(arch): defer structural completion to demand-time flow analysis
fix(session): retract unsupported flow-return results from warm admission
```

If squashed into one landing:

```text
fix(session): retract unsupported flow-return results from warm admission
```

## 9. Honest V3 assessment

The V3 “no holes” review was wrong in a meaningful way, not merely unlucky. It missed both:

- a physical carrier in the real transfer graph; and
- a semantic contributor already present in result construction.

That means this area cannot safely be specified ahead of code as an allegedly exhaustive list of structs, transfers, and completion rules. Prose can freeze ownership, prohibitions, and acceptance behavior. It cannot credibly claim implementation-site closure until the actual carrier graph has been exercised.

Before heavy completion resumes, the closed code-first inventory must contain:

- Every producer of completion-relevant information.
- Every carrier and every constructor for `FlowEvaluationOutcome`, `FlowReturnPendingState`, `DrainedFlowReturnMember`, `FlowDischargeEntry`, `CompletedFlowReturnMember`, root-close outcomes, SCC publication, and admitted/public results.
- Every drain domain: flow-return root, relation root, and resolve-call root.
- Every forward and return transfer through fixed-point discharge.
- Every point at which the sidecar can be copied, merged, cleared, dropped, or converted.
- Every actual input to return assembly—not only `can_fall_through`, but `implicit_undefined_seen`, bare returns, authored return contributors, holds, suffix behavior, and finally-specific inference contributions.
- Separate representations for runtime completion topology and TypeScript return-inference contribution where their observable behavior differs; X68/X80 prove they are not interchangeable.
- A terminal map from each carrier path to public result, degradation, cache admission, and warm replay.
- A mechanically checked closure rule: zero unclassified constructors/transfers at the pinned SHA, real-path tests through every domain, and an independent mutation that breaks each transfer.
- The discriminating semantic matrix: genuine G10; X05; X68; X80; X88; switch and catch siblings; malformed targets; nested-finally state; and clean/warm negative controls.
- A code spike that compiles and drives those paths before the final implementation specification freezes exact carrier sites.

Until that exists, another “exhaustive” section list would be an assertion without evidence. The later lock must be derived from compiled code and executable paths, then reviewed—not written first and patched whenever implementation discovers another carrier.

__DONE__
