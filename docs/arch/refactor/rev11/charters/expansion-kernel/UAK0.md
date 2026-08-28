<!-- unified-charter-v2
id=UAK0
name=Current-head authority and displacement reconciliation
phase=expansion
train=expansion.kernel
product=kernel
kind=audit
semantic_role=delivery
class=successor
predecessors=BR0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=carrierprofileid
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
source_refs=source:successor-expansion.md:L761
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/UAK0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# UAK0 — Current-head authority and displacement reconciliation

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Current-head authority and displacement reconciliation. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **BR0:** exact current receipt ID and digest for “Post-L4 successor product promotion”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** determine exactly what the successor reuses, amends, replaces, or deletes.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **central framework switch**, **untagged coordinate/public identity**, **duplicate component information authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **UAK0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **UAK0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **UAK0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **UAK0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **central framework switch**.
- Delete or structurally reject: **untagged coordinate/public identity**.
- Delete or structurally reject: **duplicate component information authority**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L761`

## Reconciled source-plan contract

**Intent:** determine exactly what the successor reuses, amends, replaces, or deletes.
**Predecessors:** `BR0`.
**Subblocks:** (1) inventory `FileLanguage`, framework/carrier registries, `CarrierGrammarConfig`, `CarrierCompiler`, TypeInfo wire/graph, component-meta, maps/encodings, configuration, LSP routing, public bindings, CLI binaries, and repository skills; (2) walk producer→consumer paths, not names alone; (3) map the superseded proposal’s `KX/CDX/EMB/CMX/SGX/PJX/ACT/OBS/SEL/RFX/AIX/FCX` ideas to retained owners; (4) assign every deletion unit/row/adapter/schema/generated artifact exactly one cutover owner and enumerate all consumers; (5) produce the machine-readable deletion/retag ledger with no unowned artifact; (6) pin zero-work/performance baselines.
**Acceptance:** one mechanically complete owner/consumer ledger and an independently reviewed “no parallel authority” proof.
**Forbidden:** cosmetic catalog renames, assuming an old charter is implemented because prose exists, or preserving a stale DTO for convenience.
**Deletion/abort:** old global `EXT0/TVG0/PJG0` coupling is superseded; rescope if any current owner cannot be placed without inventing a second authority.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L761-10BF89C5899D

- Kind: `context`
- Source: `successor-expansion.md:761-761`
- Applicability: `UAK0`
- Exact text SHA-256: `10bf89c5899d18dc3538e8383ea0eb89168df58cce10b39b0eb64b3d5476172d`

~~~~markdown
### `UAK0.md` — Current-head authority and displacement reconciliation
~~~~

### SRC-EXP-L763-F65BA5A4610E

- Kind: `forbidden`
- Source: `successor-expansion.md:763-768`
- Applicability: `UAK0`
- Exact text SHA-256: `f65ba5a4610ee2928dd518da03f498a5e0014eb4e6b1874e08994114db2c61ef`

~~~~markdown
**Intent:** determine exactly what the successor reuses, amends, replaces, or deletes.
**Predecessors:** `BR0`.
**Subblocks:** (1) inventory `FileLanguage`, framework/carrier registries, `CarrierGrammarConfig`, `CarrierCompiler`, TypeInfo wire/graph, component-meta, maps/encodings, configuration, LSP routing, public bindings, CLI binaries, and repository skills; (2) walk producer→consumer paths, not names alone; (3) map the superseded proposal’s `KX/CDX/EMB/CMX/SGX/PJX/ACT/OBS/SEL/RFX/AIX/FCX` ideas to retained owners; (4) assign every deletion unit/row/adapter/schema/generated artifact exactly one cutover owner and enumerate all consumers; (5) produce the machine-readable deletion/retag ledger with no unowned artifact; (6) pin zero-work/performance baselines.
**Acceptance:** one mechanically complete owner/consumer ledger and an independently reviewed “no parallel authority” proof.
**Forbidden:** cosmetic catalog renames, assuming an old charter is implemented because prose exists, or preserving a stale DTO for convenience.
**Deletion/abort:** old global `EXT0/TVG0/PJG0` coupling is superseded; rescope if any current owner cannot be placed without inventing a second authority.
~~~~

### SRC-LEGACY-TRANSFER-DC2AA371457B

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:19-24`
- Applicability: `UAK0`
- Exact text SHA-256: `c3bca2b24adfff5d7e573f3bc4ebabe654685347da74de262fd96d7136fc06bc`

~~~~markdown
### LEGACY-TRANSFER-DC2AA371457B

- Original path: `docs/arch/followups/replacement-deviations.json`; Git blob: `dc2aa371457bfb90e7158c48e2e5b59730de554e`; exact source SHA-256: `0f90a6bd6c9d891a74afe835250e6cccdd9bce08e62feaffb14af9622f5d88c6`.
- Exact retained source: `sources/legacy-architecture-transfers/followups/replacement-deviations.json`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-667CD51797D3

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:26-31`
- Applicability: `UAK0`
- Exact text SHA-256: `a97b1728ca7e529b4438b87347c9168ff363b0f917ec3cb64f810642c0b2e443`

~~~~markdown
### LEGACY-TRANSFER-667CD51797D3

- Original path: `docs/arch/followups/replacement-deviations.schema.json`; Git blob: `667cd51797d30c43abfa6e73935693334451779f`; exact source SHA-256: `474f67963b911a5fde5850ed44de69f560ba80fd47f47af4d77715efbb59fe19`.
- Exact retained source: `sources/legacy-architecture-transfers/followups/replacement-deviations.schema.json`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-0B2781AF50CB

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:229-234`
- Applicability: `UAK0`
- Exact text SHA-256: `88ba5ddcb036e4ffcbca527de7640cea44f238870024ef2a3e1f4b3326479bd0`

~~~~markdown
### LEGACY-TRANSFER-0B2781AF50CB

- Original path: `docs/arch/gate-integrity-ledger.md`; Git blob: `0b2781af50cbb5060c211bb2302abd943f31a35e`; exact source SHA-256: `7d9b34f721c919ed75beefdbf3f3715419b744c5abc53f3656cd00b507e93b29`.
- Exact retained source: `sources/legacy-architecture-transfers/gate-integrity-ledger.md`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-C166295A826E

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:299-304`
- Applicability: `UAK0`
- Exact text SHA-256: `59a6cafec5e481a7a87936b6ab48fc007b8e111773a8207c633b9885b9b3e72d`

~~~~markdown
### LEGACY-TRANSFER-C166295A826E

- Original path: `docs/arch/last/verter-core-retirement.md`; Git blob: `c166295a826ee069794401dc9a13f7e3b99ed768`; exact source SHA-256: `e1a965cf373534f6092081e3f05e07bc5297a8e3eda645c010bfd7a7247ef90e`.
- Exact retained source: `sources/legacy-architecture-transfers/last/verter-core-retirement.md`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-4D55354FFACC

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:306-311`
- Applicability: `UAK0`, `BR0`
- Exact text SHA-256: `e9e6a68bdb6ed42098ee6710019da276f3233dda3d235b981c3b7abcaf7437c7`

~~~~markdown
### LEGACY-TRANSFER-4D55354FFACC

- Original path: `docs/arch/memos/release-candidate-merge-review.md`; Git blob: `4d55354ffacc3dd3e9e67a61abfccfd69f6a58d2`; exact source SHA-256: `f98eb2e9e60857cdb38be00aa6d49a66adb62f7944d779be2bedbd55ac1ef27b`.
- Exact retained source: `sources/legacy-architecture-transfers/memos/release-candidate-merge-review.md`.
- Applicable authority: `UAK0`, `BR0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-E91D1436B975

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:369-374`
- Applicability: `UAK0`
- Exact text SHA-256: `e8e37cca49b6e83be266a0150fd84c93f4f7345b0087d5e82c45728870dcab47`

~~~~markdown
### LEGACY-TRANSFER-E91D1436B975

- Original path: `docs/arch/next/01-gate-integrity-block.md`; Git blob: `e91d1436b975151ec8b981b41d5d771002604d24`; exact source SHA-256: `23abc1274b8734022d427b5d869a8eb1b44a958ec94df17f9ceddc1ca9927e86`.
- Exact retained source: `sources/legacy-architecture-transfers/next/01-gate-integrity-block.md`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-B09C09C84754

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:376-381`
- Applicability: `UAK0`
- Exact text SHA-256: `27f6605b989ef3669845daad2933eca767c3c8ad670b0df673affc29180b07a6`

~~~~markdown
### LEGACY-TRANSFER-B09C09C84754

- Original path: `docs/arch/next/04-open-decisions.md`; Git blob: `b09c09c847542e696af903433ceacc7de262f36f`; exact source SHA-256: `37444ea4fdf2c064cde47e5970bf2e29dc9c187effb373a52e5120efa9fa0513`.
- Exact retained source: `sources/legacy-architecture-transfers/next/04-open-decisions.md`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-DCC5FD34E392

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:390-395`
- Applicability: `UAK0`
- Exact text SHA-256: `34de6fc7d59f9d6641f034f4d287ca76d21bee8914606050dd4af779c552eb6b`

~~~~markdown
### LEGACY-TRANSFER-DCC5FD34E392

- Original path: `docs/arch/next/deferred-cleanup-debt.md`; Git blob: `dcc5fd34e39271b4f8c6ee3e0ba755006c363eb1`; exact source SHA-256: `4af86477ad00fc0cad966321036308c2e54fa00a6ef95849d6810744e466cc0d`.
- Exact retained source: `sources/legacy-architecture-transfers/next/deferred-cleanup-debt.md`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-F00E10DDE896

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:404-409`
- Applicability: `UAK0`
- Exact text SHA-256: `514916b9e5d88adef6da219979841083549e25759050790b1e4be555a0bae548`

~~~~markdown
### LEGACY-TRANSFER-F00E10DDE896

- Original path: `docs/arch/next/README.md`; Git blob: `f00e10dde89679a17f549a2f93deb649bb6c7b85`; exact source SHA-256: `fe51b53c9e78654d095c7e0a21fffd5ae1e44bd82eb5b496e0855b5f8567e061`.
- Exact retained source: `sources/legacy-architecture-transfers/next/README.md`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-51A9487AA9E2

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:439-444`
- Applicability: `UAK0`
- Exact text SHA-256: `2603ab9b490085c069b33f0d32a89f07e17f54fa55ac0d505eccfea9eb3581dc`

~~~~markdown
### LEGACY-TRANSFER-51A9487AA9E2

- Original path: `docs/arch/portability-fixed-marker-scanner-rulings.md`; Git blob: `51a9487aa9e2980dcdf8e9076549b60175bedbe2`; exact source SHA-256: `f464839163c82770d1b83682402ac3f80ec679efd464ae55bbb2d4b2acacb327`.
- Exact retained source: `sources/legacy-architecture-transfers/portability-fixed-marker-scanner-rulings.md`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-9A75E4D30C5A

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:446-451`
- Applicability: `UAK0`
- Exact text SHA-256: `77c250249d857e53b3744ab4fa5a7ed6848d7172f2ac28a92e2d13e817f4aba9`

~~~~markdown
### LEGACY-TRANSFER-9A75E4D30C5A

- Original path: `docs/arch/portability-machine-marker-evidence-exceptions.tsv`; Git blob: `9a75e4d30c5a351c74cd9342a3e374ed00eed474`; exact source SHA-256: `d9f16e414d317b4426192994352e64abcd3ddf0fe27367c0a3fa54a0fa31cd78`.
- Exact retained source: `sources/legacy-architecture-transfers/portability-machine-marker-evidence-exceptions.tsv`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-1D69D6F486F4

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:551-556`
- Applicability: `UAK0`
- Exact text SHA-256: `7a982a0c81034d780074a1876bcbf9f51461ffa6172cff36e8dc466b4eb2da98`

~~~~markdown
### LEGACY-TRANSFER-1D69D6F486F4

- Original path: `docs/arch/stage5-cutover-plan.md`; Git blob: `1d69d6f486f4eccb95787cf053029422ecebc77e`; exact source SHA-256: `ef2c3b9a346e81398ba2533fd24d9dc84593142b379f18331bc617393c34731d`.
- Exact retained source: `sources/legacy-architecture-transfers/stage5-cutover-plan.md`.
- Applicable authority: `UAK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
