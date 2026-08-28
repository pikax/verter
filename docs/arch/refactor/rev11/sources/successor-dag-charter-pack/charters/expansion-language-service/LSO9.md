<!-- unified-charter-v2
id=LSO9
name=Vertical language-service conformance and coexistence matrix
phase=expansion
train=expansion.language-service
product=language_service
kind=proof
semantic_role=delivery
class=successor
predecessors=LSO1,LSO3,LSO4,LSO5,LSO6,LSO7,LSO8,VIM1,COX0
conditional_predecessors=NCK7:when-opened
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=vertical_manifest,capability_catalog,performance_evidence
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
charter=charters/expansion-language-service/LSO9.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO9 - Vertical language-service conformance and coexistence matrix

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Generate and execute the authoritative vertical language-service conformance matrix across operations, profiles, providers, recovery states, coexistence modes, coordinate encodings, and consumer surfaces. LSO9 certifies operation families and identifies residual external ownership; it implements no new feature semantics.

The current owner is **scattered feature tests, provider-specific fixtures, legacy editor designs, manually maintained capability claims, and sampled integration checks**. The final and sole owner is **one versioned operation/profile/provider conformance manifest, deterministic generated tests/receipts, and exact capability maturity table**.

## Architectural role and end state

LSO9 is the proof boundary before deletion. It prevents “works in one editor/provider” from being mistaken for a universal language-service architecture. Missing semantic behavior reopens LSO1-LSO8 or vertical owners.

## Expected production surfaces

- `docs/arch/refactor/rev11` VIM/catalog/generated authority
- `crates/verter_session`/`crates/verter_lsp` conformance harnesses
- `crates/verter_vue_conformance` and `crates/verter_svelte_conformance`
- `crates/verter_type_runtime` gated provider canaries
- `crates/verter_bench` and audit receipts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `LanguageServiceConformanceManifest`, `OperationConformanceRow`, and stable row IDs
- `OperationCoverage::{Required, Optional, Unsupported, ExternalOwner}`
- `ConformanceExpectation` over targets/occurrences/fragments/intents/outcomes/work
- `ProviderTopology`, `RecoveryState`, `CoexistenceMode`, and `EncodingProfile`
- `OperationCertificationReceipt` and generated capability/maturity input

## Exact predecessor contracts

- **LSO1:** consume tolerant recovery and two-rail behavior.
- **LSO3:** consume navigation engine.
- **LSO4:** consume references/hierarchy occurrence planner.
- **LSO5:** consume semantic rename planning.
- **LSO6:** consume completion/resolve intents.
- **LSO7:** consume presentation composition.
- **LSO8:** consume authored transaction engine.
- **VIM1:** consume deterministic manifest compiler/conformance generator.
- **COX0:** consume exact coexistence/participation modes.
- **NCK7:when-opened:** when opened, include the shared native diagnostic service in operation/surface conformance; when unopened, prove no checker dependency or hidden work.

External custody: none beyond the package activation boundary.

## Binding architecture

- The manifest enumerates semantic expectations and operational outcomes, not just request success or message counts.
- Rows are stable, versioned, hermetic by default, and generated into tests/receipts/capability maturity.
- Provider topology is a dimension, not separate hand-authored suites; unavailable topologies are explicit.
- Recovery/coexistence/encoding/profile dimensions cover exact applicable subsets and zero-work requirements.
- Performance expectations use equivalent-work counters and bounded allocations/retention, not wall time alone.
- A green matrix certifies only listed operation/profile rows; unsupported/external ownership remains truthful.
- When NCK7 is unopened, diagnostics rows remain external/native-parser/lint according to existing authority and perform zero NCK work.

## Internal subblocks

### LSO9-SB1 - Manifest schema and stable row taxonomy

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

### LSO9-SB2 - Hermetic fixture and oracle corpus

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

### LSO9-SB3 - Provider, profile, recovery, and coexistence matrix

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

### LSO9-SB4 - Consumer-surface equivalence

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

### LSO9-SB5 - Performance, cancellation, churn, and memory evidence

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

### LSO9-SB6 - Certification, capability generation, and residual ledger

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

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Conformance row identity is stable and independent of generated test file location.
- Certification is row-scoped and cannot be inferred from aggregate pass percentage.
- External provider observations never become hermetic expectations without explicit pinned oracle basis.

## Migration and cutover

- Seed manifest from current tests/legacy clauses, then close gaps per operation owner.
- Run hermetic matrix continuously and gated provider/real-editor canaries separately.
- Do not delete legacy routes until applicable required rows are certified.

## Deletions

- Delete duplicated feature/provider test matrices superseded by generated rows only after coverage equivalence.
- Delete manual capability/maturity tables and sampled parity claims.
- Delete branch-era legacy design acceptance prose after atom/row transfer.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Aggregate green count used as semantic certification.
- Network-dependent mandatory tests.
- Manual row IDs/capability promotion outside the generator.
- Ignoring unsupported topology while claiming universal parity.
- Fixing semantic defects locally in the proof block.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO9-AC-MANIFEST:** exact required row completeness/bijection and stable generation.
- **LSO9-AC-MATRIX:** provider/profile/recovery/coexistence/encoding applicability and zero-work are explicit.
- **LSO9-AC-SURFACES:** opened consumers preserve core semantic products/outcomes.
- **LSO9-AC-PERF:** incremental/fresh/cancel/churn/allocation/RSS receipts satisfy PER0.
- **LSO9-AC-CAPABILITY:** generated public capability/maturity equals certified rows exactly.
- **LSO9-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO9-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO9-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO9-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Conformance overhead is test/offline; production capability lookup uses immutable generated tables and performs no fixture/oracle work.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort certification if a required row lacks authoritative expected semantics or exact applicable topology.
- Abort if a proof failure is patched in the harness instead of owning implementation.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Manifest generator determinism/bijection/source coverage.
1. Full hermetic operation matrix and gated provider topology matrix.
1. Cross-surface, zero-work, incremental/fresh, cancellation, churn, allocation, latency, and RSS receipts.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks LSO10 terminal/deletion.
- Feeds PUB0/COX0 capability truth and future vertical release manifests.
- Provides exact residual ownership ledger.

## Source reconciliation

- All legacy navigation/completion/recovery/editor acceptance clauses classified by reconciliation.
- VIM/PER0/COX0 authority.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
