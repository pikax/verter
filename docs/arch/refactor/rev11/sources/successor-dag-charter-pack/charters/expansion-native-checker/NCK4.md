<!-- unified-charter-v2
id=NCK4
name=Diagnostic-family manifest, hermetic oracle, certification, and node generator
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
predecessors=NCK3,TCM4,VIM1,PER0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,vertical_manifest,performance_evidence
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
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md
external_requirements=
activation_gate=ORC0
charter=charters/expansion-native-checker/NCK4.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK4 - Diagnostic-family manifest, hermetic oracle, certification, and node generator

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the machine-readable diagnostic-family manifest, hermetic TypeScript oracle corpus, deterministic diagnostic canonicalizer, review-gated correction overlays, generated NCF DAG/charter production, and evidence receipts. This block creates the parity production system; it does not implement all family slices itself.

The current owner is **free-form parity prose, scattered ignored tests, manually curated provider expectations, and no checker-family DAG generator**. The final and sole owner is **one source-digest-bound manifest and generator that creates bounded, independently acceptable native checker family slices**.

## Architectural role and end state

NCK4 converts the multi-person-year checker catalogue into explicit program work. It prevents parity claims from being hidden in a monolithic block and makes certification reproducible, reviewable, and tied to exact TypeScript engine identity.

## Expected production surfaces

- `docs/arch/refactor/rev11/catalogs` for diagnostic family and correction-overlay schemas
- `docs/arch/refactor/rev11/generated` and authority DAG/charters for generated NCF nodes
- `crates/verter_session/tests`, `crates/verter_diagnostics/tests`, and hermetic conformance corpora
- `crates/verter_type_runtime` or dedicated test harness code for oracle observation only
- `tools` or a dedicated Rust generator binary; tests never write generated authority artifacts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `DiagnosticFamilyManifest`, `DiagnosticFamilyRow`, `DiagnosticFeatureSliceRow`
- `DiagnosticOracleCase`, `OracleEngineIdentity`, `OracleSnapshot`, `DiagnosticCanonicalizer`
- `CorrectionOverlay`, `CorrectionOverlayEntry`, and review/expiry metadata
- `GeneratedCheckerNodeSpec`, `DiagnosticFamilyReceipt`, and `FamilyPromotionEvidence`
- `gen-native-checker-dag` as the sole writer of generated NCF DAG/charter/index artifacts

## Exact predecessor contracts

- **NCK3:** consume the exact rule kernel and canary execution/evidence format.
- **TCM4:** consume certified TypeScript engine binding, input basis, mapping, and observation identity.
- **VIM1:** consume deterministic manifest compilation and conformance generation patterns.
- **PER0:** consume equivalent-work, allocation, latency, and retained-memory evidence methodology for certification and generated slices.

External custody: none beyond the package activation boundary.

## Binding architecture

- Manifest rows, not prose section headings, define required checker scope and terminal completeness.
- One generated NCF node owns one bounded semantic feature slice, exact rule population, exact deletion population, oracle corpus, and certification receipt.
- Oracle execution is hermetic and test-only. Production native queries have no access to provider observation.
- Diagnostic comparison canonicalizes codes, semantic family, subject, authored locations, related locations, severity, and stable message parameters; raw localized strings are not primary equality.
- Correction overlays are sparse, review-gated exceptions for clear TypeScript bugs and cannot become a second runtime behavior.
- The generator is the sole writer; tests render in memory and diff committed outputs.
- Generated node identity remains stable under manifest reordering and changes only when its semantic slice identity changes.

## Internal subblocks

### NCK4-SB1 - Manifest schema and family partition

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

### NCK4-SB2 - Hermetic oracle corpus and engine identity

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

### NCK4-SB3 - Diagnostic canonicalization and comparison

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

### NCK4-SB4 - Correction overlay and divergence registry

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

### NCK4-SB5 - Generated NCF DAG and charter writer

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

### NCK4-SB6 - Certification receipts and promotion evidence

**Independently testable outcome:** A family slice can be promoted only from immutable implementation, oracle, performance, and review evidence.

**Architecture:**

- Bind candidate tree, implementation receipt, manifest row digest, oracle engine/input, diff result, correction overlays, incremental/fresh proof, and work counters.
- Separate observation success from authority promotion.
- Make NCK6 consume receipts rather than rerun hidden certification logic.

**Expected changes:**

- Implement receipt schema and validator.
- Generate human-readable evidence summaries from structured data.

**Discriminating proof:**

- Changing any input invalidates the receipt.
- A clean observation without exact candidate or manifest digest cannot promote authority.

## Data, identity, invalidation, and publication laws

- The family manifest is the exact scope authority; generated reports are derivative and never hand-edited.
- Oracle snapshots and correction overlays are test/evidence artifacts, not production semantic dependencies.
- Every generated NCF node owns an exact rule set and legacy deletion set; overlapping ownership is invalid.
- Certification receipts are immutable and content-addressed.
- A non-comparable provider observation is not a pass and cannot be hidden as an ignored test.

## Migration and cutover

- Import durable parity rows from legacy TypeInfo/checker docs and existing ignored tests into the manifest with explicit status.
- Do not mechanically convert every old test into required checker scope without classifying its semantic family and authority.
- Generate NCF nodes through an amendment and keep them locked until predecessors and implementation receipts exist.

## Deletions

- Delete free-form checker parity ledgers and generator-by-test patterns displaced by the manifest/generator.
- Delete wildcard ignored-test acceptance and manually stamped parity percentages.
- Delete runtime compatibility-mode scaffolding if any exists.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- One NCK4 implementation claiming the full TypeScript diagnostic catalogue.
- Tests mutating checked-in manifests, DAGs, charters, or snapshots.
- Localized message text as the sole parity comparator.
- Oracle execution in production or network-dependent required certification tests.
- Correction overlays without row-exact scope and independent review.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK4-AC-BIJECTION:** required manifest rows, generated NCF nodes, charters, and terminal coverage are exact bijections.
- **NCK4-AC-ORACLE:** hermetic recomputation is deterministic and engine/input identity is exact.
- **NCK4-AC-GENERATOR:** dedicated generator is sole writer; tests only assert in-memory equality.
- **NCK4-AC-OVERLAY:** sparse correction overlays satisfy evidence, scope, review, and expiry laws.
- **NCK4-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK4-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK4-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK4-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Certification harness performance is measured separately from runtime; generated slice charters still require runtime equivalent-work counters.
- Manifest parsing/generation is deterministic and bounded by row count with no repository-wide semantic scan.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `cargo nextest run` for manifest, canonicalizer, oracle harness, overlay, receipt, and generator crates/tests.
1. Run explicit oracle recompute in hermetic mode and compare committed snapshots.
1. Run generator in check mode plus planted missing/duplicate/oversized/cycle mutations.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Generates the NCF implementation backlog and evidence contract.
- Supplies certification receipts consumed by NCK6 authority promotion and NCK7 terminal completeness.
- Provides checker rows consumed by LSO8 and CLI conformance when native diagnostics are enabled.

## Source reconciliation

- `docs/arch/native-typeinfo-parity.md` parity/oracle discipline, corrected so coverage is not semantic parity.
- `docs/arch/native-checker.md` separate checker manifest requirement.
- `docs/arch/ts-compat-two-mode-model.md` correction-overlay and one-spec rules.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
