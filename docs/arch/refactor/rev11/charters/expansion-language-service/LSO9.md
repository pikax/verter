<!-- unified-charter-v2
id=LSO9
name=Vertical language-service conformance and coexistence matrix
predecessors=LSO1,LSO3,LSO4,LSO5,LSO6,LSO7,LSO8,VIM1,COX0
conditional_predecessors=NCK7:when-opened
phase=expansion
train=expansion.language-service
product=language_service
kind=proof
semantic_role=delivery
class=successor
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=vertical_manifest,capability_catalog,performance_evidence
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-dag-amendment.md:L1,source:legacy-arch-reconciliation.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/expansion-language-service/LSO9.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO9 — Vertical language-service conformance and coexistence matrix

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

## Independently acceptable outcome

Generate and execute the authoritative vertical language-service conformance matrix across operations, profiles, providers, recovery states, coexistence modes, coordinate encodings, and consumer surfaces. LSO9 certifies operation families and identifies residual external ownership; it implements no new feature semantics.

The current owner is **scattered feature tests, provider-specific fixtures, legacy editor designs, manually maintained capability claims, and sampled integration checks**. The final and sole owner is **one versioned operation/profile/provider conformance manifest, deterministic generated tests/receipts, and exact capability maturity table**.

## Concrete surfaces and APIs

- Production surfaces: `docs/arch/refactor/rev11`, `crates/verter_session`, `crates/verter_lsp`, `crates/verter_vue_conformance`, `crates/verter_svelte_conformance`, `crates/verter_type_runtime`, `crates/verter_bench`.
- Pack production inventory:
- `docs/arch/refactor/rev11` VIM/catalog/generated authority
- `crates/verter_session`/`crates/verter_lsp` conformance harnesses
- `crates/verter_vue_conformance` and `crates/verter_svelte_conformance`
- `crates/verter_type_runtime` gated provider canaries
- `crates/verter_bench` and audit receipts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `LanguageServiceConformanceManifest`, `OperationConformanceRow`, and stable row IDs
- `OperationCoverage::{Required, Optional, Unsupported, ExternalOwner}`
- `ConformanceExpectation` over targets/occurrences/fragments/intents/outcomes/work
- `ProviderTopology`, `RecoveryState`, `CoexistenceMode`, and `EncodingProfile`
- `OperationCertificationReceipt` and generated capability/maturity input

## Exact predecessor contracts

- **LSO1:** exact current receipt ID and digest for “Tolerant carrier recovery and two-rail syntax/semantic diagnostics”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LSO3:** exact current receipt ID and digest for “Definition, type-definition, implementation, and symbol navigation”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LSO4:** exact current receipt ID and digest for “References, hierarchy, and bounded occurrence planning”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LSO5:** exact current receipt ID and digest for “Semantic rename planning and conflict analysis”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LSO6:** exact current receipt ID and digest for “Completion candidates and provider-neutral resolve intents”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LSO7:** exact current receipt ID and digest for “Hover, signature-help, and inlay presentation composition”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LSO8:** exact current receipt ID and digest for “Authored edit transaction engine for rename, fixes, and imports”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VIM1:** exact current receipt ID and digest for “Deterministic manifest compiler and conformance generator”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **COX0:** exact current receipt ID and digest for “Per-profile editor participation and coexistence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

### Binding architecture

- The manifest enumerates semantic expectations and operational outcomes, not just request success or message counts.
- Rows are stable, versioned, hermetic by default, and generated into tests/receipts/capability maturity.
- Provider topology is a dimension, not separate hand-authored suites; unavailable topologies are explicit.
- Recovery/coexistence/encoding/profile dimensions cover exact applicable subsets and zero-work requirements.
- Performance expectations use equivalent-work counters and bounded allocations/retention, not wall time alone.
- A green matrix certifies only listed operation/profile rows; unsupported/external ownership remains truthful.
- When NCK7 is unopened, diagnostics rows remain external/native-parser/lint according to existing authority and perform zero NCK work.

### Internal subblocks

#### LSO9-SB1 - Manifest schema and stable row taxonomy

**Independently testable outcome:** Every required operation/profile behavior has one stable row and exact applicability.

**Architecture:**

- Define row dimensions, expected semantic IDs/results/outcomes/work, fixtures, owners, and maturity.
- Separate required/optional/unsupported/external ownership.
- Version row changes and prevent silent deletion.

**Expected changes:**

- Extend VIM0/VIM1 generator for language-service rows.
- Import durable legacy acceptance cases into rows.

**Discriminating proof:**

- Bijection/completeness guard catches missing/duplicate/renumbered rows.
- Reordering inputs does not change row identity/generated artifacts.

#### LSO9-SB2 - Hermetic fixture and oracle corpus

**Independently testable outcome:** Core conformance runs without network/editor installation and uses exact authored expected products.

**Architecture:**

- Create compact Vue/Svelte/native/project/barrel/recovery/global-component/edit fixtures.
- Store semantic target/occurrence/fragment/intent expectations in typed snapshots.
- Use provider oracles only behind exact gated topology.

**Expected changes:**

- Generate fixture runners for operations.
- Delete redundant branch-era fixture prose after transfer.

**Discriminating proof:**

- Fixtures are deterministic across machines/paths.
- A planted wrong target/role/anchor/intent is detected.

#### LSO9-SB3 - Provider, profile, recovery, and coexistence matrix

**Independently testable outcome:** Each applicable topology has exact behavior and capability/zero-work evidence.

**Architecture:**

- Enumerate provider off/tsgo/tsserver/extension/shared where available.
- Enumerate Vue/Svelte and future profile rows, clean/broken states, Full/WorkspaceOnly/Disabled/auto coexistence.
- Cover UTF-8/UTF-16, CRLF, emoji, embedded maps.

**Expected changes:**

- Generate matrix cases and receipts.
- Mark unsupported/shared harness gaps explicitly rather than assuming parity.

**Discriminating proof:**

- Capability claims equal passing applicable rows.
- Disabled/inapplicable combinations prove zero parse/index/provider/semantic work.

#### LSO9-SB4 - Consumer-surface equivalence

**Independently testable outcome:** LSP/custom methods/CLI/library surfaces preserve the same core semantic products where opened.

**Architecture:**

- Compare core IDs, basis, completeness, provenance, intents and outcomes before rendering.
- Allow presentation/encoding differences only at adapter layer.
- Include NCK7 diagnostics conditionally.

**Expected changes:**

- Generate cross-surface adapters/tests.
- Identify any surface-specific semantic DTO as blocking.

**Discriminating proof:**

- Equivalent operations match core results across surfaces.
- Missing inputs yield NeedInputs/unsupported consistently.

#### LSO9-SB5 - Performance, cancellation, churn, and memory evidence

**Independently testable outcome:** Certified operation rows are bounded under cold/warm/incremental/cancel/churn workloads.

**Architecture:**

- Capture parse/index/resolve/provider/map/target/occurrence/intent counters, allocations, latency distributions, RSS.
- Run repeated edits, profile/provider changes, project open/close, abandoned cursors/resolve keys.
- Require incremental equals fresh.

**Expected changes:**

- Generate PER0 scenarios and receipts.
- Route regressions to owning implementation node.

**Discriminating proof:**

- Warm work and retained memory meet ratified thresholds.
- Cancelled/stale/partial work is never published/admitted as complete.

#### LSO9-SB6 - Certification, capability generation, and residual ledger

**Independently testable outcome:** Passing rows produce immutable operation certification and truthful capability maturity; gaps remain named.

**Architecture:**

- Bind implementation/manifest/fixture/provider/toolchain/evidence digests.
- Generate PUB0/COX0 capability input.
- Record residual external/unsupported rows with owner and reopening criteria.

**Expected changes:**

- Emit certification receipts and generated matrix docs.
- Prevent manual capability promotion.

**Discriminating proof:**

- Any source/row/evidence change invalidates receipt.
- Public claims exactly match certified applicable rows.

### Identity, invalidation, and publication

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Conformance row identity is stable and independent of generated test file location.
- Certification is row-scoped and cannot be inferred from aggregate pass percentage.
- External provider observations never become hermetic expectations without explicit pinned oracle basis.

### Migration and cutover

- Seed manifest from current tests/legacy clauses, then close gaps per operation owner.
- Run hermetic matrix continuously and gated provider/real-editor canaries separately.
- Do not delete legacy routes until applicable required rows are certified.

### Consumers and unlocks

- Unlocks LSO10 terminal/deletion.
- Feeds PUB0/COX0 capability truth and future vertical release manifests.
- Provides exact residual ownership ledger.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **LSO9-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **LSO9-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **LSO9-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **LSO9-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **LSO9-AC-MANIFEST:** exact required row completeness/bijection and stable generation.
- **LSO9-AC-MATRIX:** provider/profile/recovery/coexistence/encoding applicability and zero-work are explicit.
- **LSO9-AC-SURFACES:** opened consumers preserve core semantic products/outcomes.
- **LSO9-AC-PERF:** incremental/fresh/cancel/churn/allocation/RSS receipts satisfy PER0.
- **LSO9-AC-CAPABILITY:** generated public capability/maturity equals certified rows exactly.
- **LSO9-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO9-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO9-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO9-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete duplicated feature/provider test matrices superseded by generated rows only after coverage equivalence.
- Delete manual capability/maturity tables and sampled parity claims.
- Delete branch-era legacy design acceptance prose after atom/row transfer.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Aggregate green count used as semantic certification.
- Network-dependent mandatory tests.
- Manual row IDs/capability promotion outside the generator.
- Ignoring unsupported topology while claiming universal parity.
- Fixing semantic defects locally in the proof block.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Conformance overhead is test/offline; production capability lookup uses immutable generated tables and performs no fixture/oracle work.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort certification if a required row lacks authoritative expected semantics or exact applicable topology.
- Abort if a proof failure is patched in the harness instead of owning implementation.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Manifest generator determinism/bijection/source coverage.
1. Full hermetic operation matrix and gated provider topology matrix.
1. Cross-surface, zero-work, incremental/fresh, cancellation, churn, allocation, latency, and RSS receipts.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired round handle; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-dag-amendment.md:L1`
- `source:legacy-arch-reconciliation.md:L1`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.
