<!-- unified-charter-v2
id=PRF0
name=Sequential representative-slice lock
phase=expansion
train=expansion.architecture-proof
product=architecture_proof
kind=lock
semantic_role=delivery
class=successor
predecessors=HWC5,CEJ0,UAK2,FMT1,PUB0
conditional_predecessors=
owner=expansion.architecture-proof:sequential counterexample evidence over concrete framework geometries
conflict_domains=carrierprofile
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L1226
external_requirements=
activation_gate=ORC0
charter=charters/expansion-architecture-proof/PRF0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# PRF0 — Sequential representative-slice lock

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Sequential representative-slice lock. The current owner is **provisional universal-kernel claims**. The final and sole owner is **sequential counterexample evidence over concrete framework geometries**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `docs/arch/refactor/rev11/unified`.
- Named API/data boundaries: `CarrierProfile`, `EmbeddedCodec`, `SemanticOverlay`, `AttachmentModel`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **HWC5:** exact current receipt ID and digest for “Neutral HTML/WC conformance, performance, and Experimental terminal”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CEJ0:** exact current receipt ID and digest for “Vue/Svelte Custom Element interoperability soak join”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **UAK2:** exact current receipt ID and digest for “Read-only provisional universal-kernel convergence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **FMT1:** exact current receipt ID and digest for “Document algebra, renderer, edits, cursor, and maps”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** freeze one minimal, discriminating experiment for each unproven source geometry.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **framework-specific exception hidden in kernel**, **parallel proof slices that contaminate evidence** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **PRF0-AC1 — sole-owner proof:** add `prf0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **PRF0-AC2 — positive contract:** add `prf0_publishes_exact_carrierprofile`; assert exact identities, provenance, completeness, and deterministic ordering.
- **PRF0-AC3 — incremental equivalence:** add `prf0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **PRF0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **framework-specific exception hidden in kernel**.
- Delete or structurally reject: **parallel proof slices that contaminate evidence**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1226`

## Reconciled source-plan contract

**Intent:** freeze one minimal, discriminating experiment for each unproven source geometry.
**Predecessors:** `HWC5`, `CEJ0`, `UAK2`, `FMT1`, `PUB0`.
**Subblocks:** (1) pin exact releases/oracles/corpora; (2) define falsified invariant per slice; (3) lock one private-harness path per required semantic seam; (4) lock numeric budgets and zero-work controls; (5) require sequential dispatch and learning import between slices; (6) ratify proof-code deletion/promotion and amendment rules.
**Acceptance:** each slice can fail the kernel rather than merely demonstrate a happy path; later criteria cannot be relaxed based on earlier implementation.
**Forbidden:** parallel full vertical work, production capability advertisement, shared mutable test infrastructure that hides ownership, or a “universal” assertion from fixtures alone.
**Deletion/abort:** no code; a failed slice opens a bounded kernel amendment and invalidates downstream proof locks.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1226-3BB535FF910F

- Kind: `context`
- Source: `successor-expansion.md:1226-1226`
- Applicability: `PRF0`
- Exact text SHA-256: `3bb535ff910f95dedec0c4b9d611583e5c082740bb78fe2a35dd4cdc38547fc0`

~~~~markdown
### `PRF0.md` — Sequential representative-slice lock
~~~~

### SRC-EXP-L1228-F0AECBFE9086

- Kind: `forbidden`
- Source: `successor-expansion.md:1228-1233`
- Applicability: `PRF0`
- Exact text SHA-256: `f0aecbfe908689f8174b2a311fd14050674869cf93cfcc450e57a9248c28377b`

~~~~markdown
**Intent:** freeze one minimal, discriminating experiment for each unproven source geometry.
**Predecessors:** `HWC5`, `CEJ0`, `UAK2`, `FMT1`, `PUB0`.
**Subblocks:** (1) pin exact releases/oracles/corpora; (2) define falsified invariant per slice; (3) lock one private-harness path per required semantic seam; (4) lock numeric budgets and zero-work controls; (5) require sequential dispatch and learning import between slices; (6) ratify proof-code deletion/promotion and amendment rules.
**Acceptance:** each slice can fail the kernel rather than merely demonstrate a happy path; later criteria cannot be relaxed based on earlier implementation.
**Forbidden:** parallel full vertical work, production capability advertisement, shared mutable test infrastructure that hides ownership, or a “universal” assertion from fixtures alone.
**Deletion/abort:** no code; a failed slice opens a bounded kernel amendment and invalidates downstream proof locks.
~~~~
