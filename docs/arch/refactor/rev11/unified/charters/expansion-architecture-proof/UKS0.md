<!-- unified-charter-v2
id=UKS0
name=Stable kernel falsification/convergence gate
phase=expansion
train=expansion.architecture-proof
product=architecture_proof
kind=convergence
semantic_role=convergence
class=successor
predecessors=MDXP,LITP,RCTP,MDXR0,SLDP,ALPP,ANGP,ASTP,HWC5,CEJ0
conditional_predecessors=
owner=expansion.architecture-proof:sequential counterexample evidence over concrete framework geometries
conflict_domains=carrierprofile
resource_class=rust-mixed
review_profile=architecture-3
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
release_gating=non_release
source_refs=source:successor-expansion.md:L1307
external_requirements=
activation_gate=ORC0
charter=charters/expansion-architecture-proof/UKS0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# UKS0 — Stable kernel falsification/convergence gate

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Stable kernel falsification/convergence gate. The current owner is **provisional universal-kernel claims**. The final and sole owner is **sequential counterexample evidence over concrete framework geometries**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `docs/arch/refactor/rev11/unified`.
- Named API/data boundaries: `CarrierProfile`, `EmbeddedCodec`, `SemanticOverlay`, `AttachmentModel`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **MDXP:** exact current receipt ID and digest for “MDX carrier/projection/link-intelligence proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LITP:** exact current receipt ID and digest for “Lit embedded-template-with-holes proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **RCTP:** exact current receipt ID and digest for “React TSX semantic-overlay proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **MDXR0:** exact current receipt ID and digest for “React-specific MDX component-provider proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SLDP:** exact current receipt ID and digest for “Solid counterexample over identical TSX geometry”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **ALPP:** exact current receipt ID and digest for “Alpine HTML attribute scope proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **ANGP:** exact current receipt ID and digest for “Angular external/inline attachment proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **ASTP:** exact current receipt ID and digest for “Astro heterogeneous-carrier tooling proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **HWC5:** exact current receipt ID and digest for “Neutral HTML/WC conformance, performance, and Experimental terminal”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CEJ0:** exact current receipt ID and digest for “Vue/Svelte Custom Element interoperability soak join”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** decide whether representative geometries justify a stable extension contract.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **framework-specific exception hidden in kernel**, **parallel proof slices that contaminate evidence** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **UKS0-AC1 — sole-owner proof:** add `uks0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **UKS0-AC2 — positive contract:** add `uks0_publishes_exact_carrierprofile`; assert exact identities, provenance, completeness, and deterministic ordering.
- **UKS0-AC3 — incremental equivalence:** add `uks0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **UKS0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **framework-specific exception hidden in kernel**.
- Delete or structurally reject: **parallel proof slices that contaminate evidence**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1307`

## Reconciled source-plan contract

**Intent:** decide whether representative geometries justify a stable extension contract.
**Predecessors:** `MDXP`, `LITP`, `RCTP`, `MDXR0`, `SLDP`, `ALPP`, `ANGP`, `ASTP`, `HWC5`, `CEJ0`.
**Subblocks:** (1) compare every proof finding against kernel invariants; (2) verify all amendments were ratified and re-reviewed; (3) run mixed-workspace, Unicode, incremental/fresh, cancellation, zero-work, RSS, and public-capability suites; (4) inspect dependency and authority graphs; (5) independent exact-candidate reviews; (6) publish stable versus still-versioned contracts.
**Acceptance:** no proof requires an omni parser, universal framework IR, second TS authority, implicit encoding, or release coupling; every finding is closed by its owner on the reviewed candidate.
**Forbidden:** fixing code in the join, calling proof slices production verticals, or freezing project semantics beyond demonstrated seams.
**Deletion/abort:** a blocker reopens the smallest owning contract and invalidates this gate; it is not waived for schedule or popularity.

## 12. Full native formatter product train

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1307-2EA5F42167D4

- Kind: `context`
- Source: `successor-expansion.md:1307-1307`
- Applicability: `UKS0`
- Exact text SHA-256: `2ea5f42167d4d90f18c89f81c14935d57d3c67282e145848d91902a124c03d10`

~~~~markdown
### `UKS0.md` — Stable kernel falsification/convergence gate
~~~~

### SRC-EXP-L1309-08475D01440F

- Kind: `forbidden`
- Source: `successor-expansion.md:1309-1314`
- Applicability: `UKS0`
- Exact text SHA-256: `08475d01440f7048b49e76c5cce6cb829dd06ad4db5600d35a0107a99369940b`

~~~~markdown
**Intent:** decide whether representative geometries justify a stable extension contract.
**Predecessors:** `MDXP`, `LITP`, `RCTP`, `MDXR0`, `SLDP`, `ALPP`, `ANGP`, `ASTP`, `HWC5`, `CEJ0`.
**Subblocks:** (1) compare every proof finding against kernel invariants; (2) verify all amendments were ratified and re-reviewed; (3) run mixed-workspace, Unicode, incremental/fresh, cancellation, zero-work, RSS, and public-capability suites; (4) inspect dependency and authority graphs; (5) independent exact-candidate reviews; (6) publish stable versus still-versioned contracts.
**Acceptance:** no proof requires an omni parser, universal framework IR, second TS authority, implicit encoding, or release coupling; every finding is closed by its owner on the reviewed candidate.
**Forbidden:** fixing code in the join, calling proof slices production verticals, or freezing project semantics beyond demonstrated seams.
**Deletion/abort:** a blocker reopens the smallest owning contract and invalidates this gate; it is not waived for schedule or popularity.
~~~~
