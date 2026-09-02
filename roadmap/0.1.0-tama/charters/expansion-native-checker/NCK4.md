<!-- unified-charter-v2
id=NCK4
name=Diagnostic-family manifest, hermetic oracle, certification, and node generator
predecessors=NCK3,TCM4,VIM1,PER0
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,vertical_manifest,performance_evidence,successor_generator_tooling
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
external_requirements=
charter=charters/expansion-native-checker/NCK4.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# NCK4 — Diagnostic-family manifest, hermetic oracle, certification, and node generator

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Implement the machine-readable diagnostic-family manifest, hermetic TypeScript oracle corpus, deterministic diagnostic canonicalizer, review-gated correction overlays, generated NCF DAG/charter production, and evidence receipts. This block creates the parity production system; it does not implement all family slices itself.

The current owner is **free-form parity prose, scattered ignored tests, manually curated provider expectations, and no checker-family DAG generator**. The final and sole owner is **one source-digest-bound manifest and generator that creates bounded, independently acceptable native checker family slices**.

## Concrete surfaces and APIs

- Production surfaces: `roadmap/0.1.0-tama/catalogs`, `roadmap/0.1.0-tama/authority`, `roadmap/0.1.0-tama/charters`, `crates/verter_session/tests`, `crates/verter_diagnostics/tests`, `crates/verter_type_runtime`, `tools`.
- Pack production inventory:
- `roadmap/0.1.0-tama/catalogs` for diagnostic family and correction-overlay schemas
- Authority DAG/charters and the native-checker family manifest for generated NCF nodes
- `crates/verter_session/tests`, `crates/verter_diagnostics/tests`, and hermetic conformance corpora
- `crates/verter_type_runtime` or dedicated test harness code for oracle observation only
- `tools` or a dedicated Rust generator binary; tests never write generated authority artifacts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `DiagnosticFamilyManifest`, `DiagnosticFamilyRow`, `DiagnosticFeatureSliceRow`
- `DiagnosticOracleCase`, `OracleEngineIdentity`, `OracleSnapshot`, `DiagnosticCanonicalizer`
- `CorrectionOverlay`, `CorrectionOverlayEntry`, and review/expiry metadata
- `GeneratedCheckerNodeSpec`, `DiagnosticFamilyReceipt`, and `FamilyPromotionEvidence`
- `gen-native-checker-dag` as the sole writer of generated NCF DAG/charter/index artifacts

## Exact predecessor contracts

- **NCK3:** implemented ledger row for “Shared-proof semantic diagnostic rule kernel”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **TCM4:** implemented ledger row for “Atomic activation and deletion”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **VIM1:** implemented ledger row for “Deterministic manifest compiler and conformance generator”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **PER0:** implemented ledger row for “Cache/backend identity, cancellation, budgets, and zero work”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Manifest rows, not prose section headings, define required checker scope and terminal completeness.
- One generated NCF node owns one bounded semantic feature slice, exact rule population, exact deletion population, oracle corpus, and certification receipt.
- Oracle execution is hermetic and test-only. Production native queries have no access to provider observation.
- Diagnostic comparison canonicalizes codes, semantic family, subject, authored locations, related locations, severity, and stable message parameters; raw localized strings are not primary equality.
- Correction overlays are sparse, review-gated exceptions for clear TypeScript bugs and cannot become a second runtime behavior.
- The generator is the sole writer; tests render in memory and diff committed outputs.
- Generated node identity remains stable under manifest reordering and changes only when its semantic slice identity changes.

### Internal subblocks

#### NCK4-SB1 - Manifest schema and family partition

**Independently testable outcome:** The full required diagnostic catalogue is partitioned into stable, bounded slices with no unowned rows.

**Architecture:**

- Define family, slice, rule population, applicability, prerequisites, oracle cases, deletion owner, and performance counters.
- Require explicit required/optional status and terminal coverage.
- Allow later versioned additions without renumbering existing slice identity.

**Expected changes:**

- Implement schema parser/validator and canonical renderer.
- Populate initial required families and representative rows.

**Discriminating proof:**

- Coverage bijection and duplicate/missing mutation tests.
- Reordering input produces identical canonical manifest and generated IDs.

#### NCK4-SB2 - Hermetic oracle corpus and engine identity

**Independently testable outcome:** Every certified row is reproducible against an exact TypeScript/tsgo engine and exact project inputs.

**Architecture:**

- Pin engine artifact/version/platform, libs, compiler options, module graph, source encoding, and expected observation surface.
- Keep third-party corpora optional and external; required certification fixtures are vendored/hermetic.
- Separate syntax/provider failures from semantic diagnostic observations.

**Expected changes:**

- Implement oracle runner and fixture format.
- Capture deterministic snapshots only through an explicit recompute command.

**Discriminating proof:**

- Fresh recompute on the same engine/input is byte-identical.
- Engine/options/lib mutation changes the oracle identity and invalidates affected receipts.

#### NCK4-SB3 - Diagnostic canonicalization and comparison

**Independently testable outcome:** Native/provider outputs compare semantically rather than by unstable localized text or generated coordinates.

**Architecture:**

- Normalize provider codes, categories, message arguments, authored locations, related info, and family mapping.
- Map generated/provider coordinates through exact TCM basis and drop unverifiable observations from certification rather than guessing.
- Represent missing, extra, mismatched, and non-comparable outcomes explicitly.

**Expected changes:**

- Implement canonicalizer and structured diff output.
- Add cross-platform/locale stability fixtures.

**Discriminating proof:**

- Locale and ordering mutations preserve semantic canonical result.
- Synthetic/unmappable provider locations cannot be silently accepted as parity.

#### NCK4-SB4 - Correction overlay and divergence registry

**Independently testable outcome:** Approved TypeScript bugs are represented as sparse data with explicit evidence, never runtime modes.

**Architecture:**

- Require issue reference or equivalent evidence, affected rows, TS oracle value, Verter correct value, rationale, reviewer receipts, and review date.
- Default every non-overlay row to exact TypeScript parity.
- Provide expiry/revalidation when TypeScript fixes the bug.

**Expected changes:**

- Implement overlay schema, validator, and co-presence metadata rules.
- Compile only static issue metadata into production when explicitly authorized; oracle values remain test data.

**Discriminating proof:**

- Unreviewed, orphaned, broad wildcard, or stale overlay entries fail validation.
- Removing an overlay after an upstream fix restores ordinary parity comparison.

#### NCK4-SB5 - Generated NCF DAG and charter writer

**Independently testable outcome:** Each semantic feature slice becomes a real bounded DAG node with a detailed charter and exact predecessors.

**Architecture:**

- Derive node ID, name, owner, conflict domains, budgets, source atoms, rule population, oracle fixtures, deletions, and acceptance IDs.
- Generate a detailed family charter from row-specific architecture templates; do not emit generic one-line charters.
- Require amendment review before generated authority enters the live DAG.

**Expected changes:**

- Implement `gen-native-checker-dag` and generated output directories.
- Add in-memory render/diff tests and cycle/reachability validation.

**Discriminating proof:**

- Tests never write generated files.
- A row exceeding limits or containing multiple independently acceptable outcomes fails generation and requests manual rescope.

#### NCK4-SB6 - Certification receipts and promotion evidence

**Independently testable outcome:** A family slice can be promoted only from immutable implementation, oracle, performance, and review evidence.

**Architecture:**

- Record the implementation-ledger row, oracle engine/input, diff result, correction overlays, incremental/fresh proof, and work counters without binding them to a Git identity.
- Separate observation success from authority promotion.
- Make NCK6 consume receipts rather than rerun hidden certification logic.

**Expected changes:**

- Implement receipt schema and validator.
- Generate human-readable evidence summaries from structured data.

**Discriminating proof:**

- Changing any input invalidates the receipt.
- A clean observation without exact candidate or manifest digest cannot promote authority.

### Identity, invalidation, and publication

- The family manifest is the exact scope authority; generated reports are derivative and never hand-edited.
- Oracle snapshots and correction overlays are test/evidence artifacts, not production semantic dependencies.
- Every generated NCF node owns an exact rule set and legacy deletion set; overlapping ownership is invalid.
- Certification receipts are immutable and content-addressed.
- A non-comparable provider observation is not a pass and cannot be hidden as an ignored test.

### Migration and cutover

- Import durable parity rows from legacy TypeInfo/checker docs and existing ignored tests into the manifest with explicit status.
- Do not mechanically convert every old test into required checker scope without classifying its semantic family and authority.
- Generate NCF nodes through an amendment and keep them locked until predecessors and implementation receipts exist.

### Consumers and unlocks

- Generates the NCF implementation backlog and evidence contract.
- Supplies certification receipts consumed by NCK6 authority promotion and NCK7 terminal completeness.
- Provides checker rows consumed by LSO8 and CLI conformance when native diagnostics are enabled.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **NCK4-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **NCK4-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **NCK4-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **NCK4-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **NCK4-AC-BIJECTION:** required manifest rows, generated NCF nodes, charters, and terminal coverage are exact bijections.
- **NCK4-AC-ORACLE:** hermetic recomputation is deterministic and engine/input identity is exact.
- **NCK4-AC-GENERATOR:** dedicated generator is sole writer; tests only assert in-memory equality.
- **NCK4-AC-OVERLAY:** sparse correction overlays satisfy evidence, scope, review, and expiry laws.
- **NCK4-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK4-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK4-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK4-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete free-form checker parity ledgers and generator-by-test patterns displaced by the manifest/generator.
- Delete wildcard ignored-test acceptance and manually stamped parity percentages.
- Delete runtime compatibility-mode scaffolding if any exists.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- One NCK4 implementation claiming the full TypeScript diagnostic catalogue.
- Tests mutating checked-in manifests, DAGs, charters, or snapshots.
- Localized message text as the sole parity comparator.
- Oracle execution in production or network-dependent required certification tests.
- Correction overlays without row-exact scope and independent review.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Certification harness performance is measured separately from runtime; generated slice charters still require runtime equivalent-work counters.
- Manifest parsing/generation is deterministic and bounded by row count with no repository-wide semantic scan.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. `cargo nextest run` for manifest, canonicalizer, oracle harness, overlay, receipt, and generator crates/tests.
1. Run explicit oracle recompute in hermetic mode and compare committed snapshots.
1. Run generator in check mode plus planted missing/duplicate/oversized/cycle mutations.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
