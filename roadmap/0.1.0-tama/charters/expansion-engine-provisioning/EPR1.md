<!-- unified-charter-v2
id=EPR1
name=Engine artifact identity, compatibility, integrity, and cache contract
predecessors=EPR0,VID0
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=contract
semantic_role=delivery
class=successor
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,source_lineage,program_authority
resource_class=docs-light
gate_profile=docs-domain
review_profile=security-3
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
charter=charters/expansion-engine-provisioning/EPR1.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# EPR1 — Engine artifact identity, compatibility, integrity, and cache contract

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Define the exact identity, compatibility, origin, integrity, installation, cache, revocation, and validation contract for any executable engine candidate. This block is contract-only and applies uniformly to project/system/editor/download/bundled sources.

The current owner is **path/version probes, source-specific validation, bundle manifests, consume-only cache checks, package metadata, and ad hoc compatibility rules**. The final and sole owner is **one EngineArtifactDescriptor/ValidationReceipt law and one trusted cache/install layout consumed by every source adapter before selection or execution**.

## Concrete surfaces and APIs

- Future production-owner inventory (read-only in this contract): `crates/verter_identity`, `crates/verter_tsgo_api`, `crates/verter_type_runtime`, `crates/verter_protocol`.
- Planned implementation-consumer inventory (read-only here):
- `crates/verter_identity` for artifact/platform/origin/digest IDs
- `crates/verter_tsgo_api::toolchain` and `crates/verter_type_runtime` validation contracts
- `crates/verter_protocol` public status/provenance projections
- cache/install manifest schemas and release artifact metadata
- security/audit tests and revocation catalogs

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `EngineArtifactId`, `EnginePlatform`, `EngineFlavor`, and `EngineVersion`
- `EngineOrigin`, `EngineOriginReceipt`, and `EngineArtifactDescriptor`
- `EngineCompatibilityRequirement` and `EngineCompatibilityVerdict`
- `EngineIntegrityEvidence`, `EngineSignatureEvidence`, and `EngineValidationReceipt`
- `EngineInstallLayout`, `EngineReadyMarker`, `EngineCacheKey`, and `EngineCacheEntry`
- `EngineRejection`, `EngineRevocation`, and exact rejection reason codes

## Exact predecessor contracts

- **EPR0:** implemented ledger row for “External engine provisioning policy and trust constitution”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **VID0:** implemented ledger row for “Orthogonal identities and exact-release law”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Artifact identity includes exact engine/version/flavor/platform/build/origin/content digest and policy-compatible metadata.
- Path is a locator, never identity; replacing bytes at one path invalidates validation.
- Compatibility is checked before execution and includes protocol/API/feature constraints, not version string alone.
- Integrity/signature/origin evidence is source-specific but normalized to one validation receipt.
- Cache/install entries are private, non-symlink/reparse, ownership/permission checked, immutable after READY, and atomically installed.
- READY is written last only after validation; incomplete/corrupt entries are never candidates.
- Revocation and policy epoch invalidate candidate/validation caches immediately.

### Internal subblocks

Every subblock below produces contract schemas, decision tables, owner mappings, and positive/negative fixture vectors. Its runtime laws describe obligations for the named later production owner, not runtime effects required to complete EPR1. EPR1 does not add production types, evaluators, caches, adapters, installers, or status paths.

#### EPR1-SB1 - Artifact and platform identity

**Independently testable outcome:** The identity schema and fixture vectors specify one collision-resistant structural candidate identity independent of path.

**Architecture:**

- Define engine flavor/version/build/platform/ABI/protocol/content/origin components.
- Use full structural fields or content digest where digest is the artifact itself, not lossy replacement for semantic axes.
- Canonicalize platform triples and executable layout.

**Expected changes:**

- Specify identity fields in serialization/catalog schemas and non-production fixture vectors.
- Assign production identity types and migration of source-specific version/path tuples to EPR4-SB3.

**Discriminating proof:**

- Different bytes/build/origin/platform never alias.
- Same verified artifact reached through two locators canonicalizes appropriately while origin receipts remain distinct.

#### EPR1-SB2 - Compatibility and feature contract

**Independently testable outcome:** The compatibility decision table specifies selection eligibility for exact API/protocol/feature requirements.

**Architecture:**

- Define version ranges, protocol versions, command/API capabilities, project/toolchain compatibility.
- Separate compatible, unsupported, too-old/new, wrong-platform/flavor, and unknown.
- Bind compatibility policy version.

**Expected changes:**

- Ratify one compatibility decision table, including rejection and invalidation cases.
- Assign the production compatibility evaluator and deletion of adapter/version-string-only decisions to EPR4-SB3.

**Discriminating proof:**

- Boundary/mutation matrix detects each incompatibility.
- Compatibility changes invalidate selection without reusing stale receipt.

#### EPR1-SB3 - Origin, integrity, signature, and provenance evidence

**Independently testable outcome:** The evidence schema specifies which bytes, authorized channel, and trust root a runtime validation receipt must prove.

**Architecture:**

- Normalize registry integrity, release checksum/signature/attestation, bundle manifest, manual/project/system evidence.
- Require digest over executed artifact and critical sidecar files.
- Record SBOM/license/provenance references where policy demands.

**Expected changes:**

- Specify receipt schemas and source-evidence obligations; negative schema fixtures reject self-asserted “trusted” booleans.
- Assign normalized production receipt construction and source-adapter validation to EPR4-SB3; EPR2 and EPR3 supply acquisition and shipping evidence to that shared validator.

**Discriminating proof:**

- Byte mutation, origin substitution, signature/trust-root mismatch, and manifest omission fail.
- Receipt is deterministic and redacts secrets/local absolute roots where required.

#### EPR1-SB4 - Safe cache/install layout and concurrent writers

**Independently testable outcome:** The install/cache state model forbids exposure of partial, mutable, symlinked, or attacker-controlled executables and assigns enforcement to the implementation owners.

**Architecture:**

- Use private root, temp sibling, no-follow creation, ownership/permission checks, bounded extraction, atomic rename, READY-last.
- Define cross-process lock/loser cleanup and immutable versioned entries.
- Reject group/world-writable or reparse/symlink components.

**Expected changes:**

- Ratify layout, crash-point, concurrency, and forbidden-path fixture vectors for EPR2 installation and EPR3 installed packages.
- Specify corruption/quarantine cleanup policy; EPR4-SB7 implements read-only candidate validation and EPR2 implements managed-root mutation.

**Discriminating proof:**

- Crash at every install step never yields a selectable partial entry.
- Concurrent installers converge to one verified entry without overwrite races.

#### EPR1-SB5 - Validation cache and exact invalidation

**Independently testable outcome:** The cache decision table permits receipt reuse only while every artifact/origin/policy/revocation fact matches.

**Architecture:**

- Key by artifact locator stat identity/content evidence/origin/policy/revocation epoch.
- Revalidate mutable/manual/system/project locators as policy requires.
- Keep immutable downloaded/bundled entries fast after READY.

**Expected changes:**

- Specify validation receipt cache keys, finite retention limits, invalidation cases, and work counters for EPR4-SB6.
- Negative contract vectors forbid reuse of rejected unknowns across facts that could change; EPR4 supplies the runtime cache and counters.

**Discriminating proof:**

- Replace bytes/metadata/policy/revocation forces validation.
- Warm immutable validation performs zero rehash/stat beyond the ratified trust boundary.

#### EPR1-SB6 - Revocation, corruption, and quarantine

**Independently testable outcome:** Revocation and quarantine tables specify rejection without silent fallback and assign every transition to its implementation owner.

**Architecture:**

- Define revocation catalog/epoch and emergency policy.
- Distinguish integrity failure from revocation/incompatibility/operational failure.
- Quarantine/remove only entries owned by managed channels; never mutate user project/system installs.

**Expected changes:**

- Define rejection/status/audit schemas and retry/update/rollback decision tables.
- Assign revocation-aware validation and reporting to EPR4-SB3/SB5/SB6, activation-race handling to EPR5-SB1, managed quarantine to EPR2-SB5, and bundled withdrawal to EPR3-SB4.

**Discriminating proof:**

- Revocation race cancels activation and invalidates caches.
- Managed corruption is quarantined; manual corruption is reported without destructive mutation.

#### EPR1-SB7 - Public validation status and secret/path hygiene

**Independently testable outcome:** Public status schemas and redaction fixtures specify actionable source/version/status without secrets or unstable machine roots in portable receipts.

**Architecture:**

- Define public summary versus private diagnostic detail.
- Normalize/redact paths, proxy credentials, tokens, and trust material.
- Provide stable reason/action codes.

**Expected changes:**

- Specify PUB0-compatible public status projections, redaction rules, and portable golden fixtures.
- Assign production validation/status/log adapters to EPR4-SB5 and activation status to EPR5-SB6.

**Discriminating proof:**

- Golden tests contain no secrets/machine roots.
- Every rejection has stable typed reason and remediation class.

### Identity, invalidation, and publication

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Validation receipts are immutable and bind exact policy/trust/revocation epochs.
- Managed cache writer owns only managed roots; project/system/editor artifacts are read-only.
- A rejected candidate cannot be reclassified as not-found to continue fallback silently.

### Migration and cutover

- Characterize current source validators and bundle/cache manifests without changing their production routes.
- Ratify the normalized descriptor/receipt schemas and map every displaced path/version tuple, trust shortcut, cache, and status projection to its production owner and acceptance criterion below.
- EPR4 implements the shared descriptor/validator/cache and migrates every resolver source adapter before its selection cutover. EPR2 and EPR3 consume that implementation after EPR4; neither becomes a predecessor of the required resolution/activation path.
- EPR1 acceptance checks that this transfer is complete and internally consistent. It does not claim those later migrations or their runtime proofs have executed.

| Contract obligation | Later production owner and acceptance |
| --- | --- |
| Structural artifact types, compatibility evaluator, normalized receipt construction, source-adapter migration, and removal of path/version/trust shortcuts | EPR4-SB2/SB3, EPR4-AC-CANDIDATES |
| Bounded receipt cache, revocation invalidation, read-only cache/READY/path validation, and public validation status/redaction | EPR4-SB5/SB6/SB7, EPR4-AC-CANDIDATES and EPR4-AC-CONTRACT |
| Managed download evidence, safe extraction, atomic READY-last installation, concurrent writers, and managed quarantine | EPR2-SB4/SB5, EPR2-AC-VERIFY-FIRST and EPR2-AC-SAFE-EXTRACT |
| Shipping provenance, installed package layout, and bundled withdrawal | EPR3-SB2/SB4/SB5, EPR3-AC-INSTALLED and EPR3-AC-SUPPLY |
| Stale-selection/revocation races at activation and activation status | EPR5-SB1/SB6, EPR5-AC-REVALIDATE |


### Ownership inventory deliverables

EPR1 implements `catalogs/engine-validation-ownership.toml`, `schemas/engine-validation-ownership.schema.json` and `tools/validate-engine-validation-ownership.mjs` with focused fixtures. These are future EPR1 contract artifacts; this amendment does not claim that they already exist or that the static charter-header validator inspects their contents.

EPR1 owns its contract schemas, inventories, validators and fixture artifacts. The successor bindings inventory the runtime outcomes and consumer obligations specified by those artifacts; they do not transfer EPR1's contract deliverables to a production owner. That runtime population covers every subblock's specified behavior, named API/data boundary, consumer and source-class obligation in this charter, plus the displaced routes identified by read-only source reconciliation. Include manual, project, system, editor, managed and bundled sources with their policy applicability; optional channels remain inventoried without becoming mandatory production outcomes. Each row identifies its population member and source reference, role, concrete owner node and exact receiving acceptance ID. The schema checks row structure; the validator checks full population coverage, unique ownership, production-capable owners, current DAG successor paths and acceptance IDs in the receiving charters. The prose table above is a summary, not a substitute for that complete checked inventory.

Bind the validator's actual invocation into EPR1's docs-domain acceptance before completion. Its negative fixtures cover the omissions and invalid bindings named by EPR1-AC-SOLE. The validator must accept pending receiving implementation nodes: its purpose here is to verify ownership and dependency contracts, not to claim those future runtime owners are implemented.

### Consumers and unlocks

- Unlocks required EPR4; optional EPR2/EPR3 also require its shared production validator.
- Provides the uniform validation receipt specification to resolution, acquisition, shipping, and activation owners.
- Owns the safe managed cache/install contract; later owners provide the runtime behavior.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **EPR1-AC1 — sole-owner outcome:** deliver the schema-checked ownership inventory required by contracts/successor-charter-quality.md for every in-scope runtime outcome, consumer and displaced route. EPR1 retains ownership of its contract deliverables; each specified runtime outcome/consumer binds one production-capable implementation owner; each displaced route binds one later production-capable deletion/rejection owner. Resolve concrete DAG node IDs, a successor dependency path from EPR1, and the exact receiving acceptance criterion. This zero-production node validates the complete transfer without performing the later runtime migration.
- **EPR1-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **EPR1-AC3 — incremental contract:** the cache and invalidation decision tables bind unchanged reuse and changed-basis rejection to exact fixture vectors and later behavioral owners. EPR4/EPR5 supply runtime incremental/fresh evidence; EPR1 records that it is not yet executed.
- **EPR1-AC4 — bounded-work contract:** declare finite validation/cache limits, work counters, and zero-work modes with their later implementation owners. No runtime hot-path or soak result is required from this zero-production contract.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Contract test homes: roadmap schemas, catalogs, and their focused validation fixtures. Existing production tests may be inspected for characterization; new runtime integration tests belong to the mapped implementation owners.


### Pack-specific proof obligations

These acceptance IDs require schema/decision-table fixtures and an exact production-owner transfer, not an implemented runtime validator. A contract fixture proves the specification rejects a mutation; the later owner's behavioral test proves the running system rejects it.

- **EPR1-AC-IDENTITY:** schema fixtures distinguish artifact bytes/build/origin/platform and preserve structural identity across permitted locator changes; runtime identity proof belongs to EPR4-SB3.
- **EPR1-AC-COMPAT:** decision-table mutations discriminate every compatibility boundary and feature requirement; EPR4-SB3 implements and tests the evaluator.
- **EPR1-AC-INTEGRITY:** byte/origin/signature/manifest mutation vectors specify pre-execution rejection and map to EPR4-SB3 plus the EPR2/EPR3 evidence producers.
- **EPR1-AC-INSTALL:** layout and state-transition fixtures forbid partial/untrusted READY entries and enumerate crash/concurrency/symlink/permission cases for EPR2-SB5, EPR3-SB5, and EPR4-SB7.
- **EPR1-AC-REVOCATION:** decision tables specify invalidation at validation/selection/activation and bind implementation to EPR4-SB3/SB6 and EPR5-SB1.
- **EPR1-AC-SOLE:** the executable ownership-inventory validator rejects an omitted outcome, consumer or displaced route; an unknown owner node; an owner without a successor dependency path from EPR1; a missing or nonexistent receiving acceptance criterion; and conflicting owners for one population member. Each negative fixture starts from a valid complete inventory and proves rejection for its intended reason.
- **EPR1-AC-CONTRACT:** schema fields, identities, typed outcomes, provenance, and public redaction vectors are exact, deterministic, and complete.
- **EPR1-AC-INCREMENTAL:** contract vectors distinguish unchanged reuse from stale, cancelled, partial, and changed-basis rejection; EPR4-SB6 and EPR5-SB1 own runtime incremental/fresh proof.
- **EPR1-AC-WORK:** finite validation/cache budgets and equivalent-work counters are specified for EPR4-SB6; acquisition, shipping, and activation work remains with EPR2/EPR3/EPR5. EPR1 makes no runtime performance claim.

## Deletions and forbidden designs

- Inventory and assign later production deletion ownership for path/version-only candidate identity and duplicated compatibility/integrity decisions.
- Inventory and assign later production deletion ownership for READY/manifest trust that does not bind executed bytes.
- Inventory and assign later production deletion ownership for unsafe mutable/symlink-permissive cache paths.

This contract may remove superseded authority prose after preserving its durable clauses. Every production deletion listed here is an inventory obligation: bind it to an exact later production-capable owner and acceptance criterion before completing this contract. It authorizes no runtime route deletion.

- Path as artifact identity.
- Executing to discover compatibility before validation.
- Checksum/signature verification after installation/execution.
- Following symlinks/reparse points in managed install roots.
- Silent fallback after integrity/trust/revocation failure.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Inventory and assign a later deletion owner for every compatibility path that would preserve a second production owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Immutable READY entries may use bounded receipt validation; mutable locators revalidate according to explicit policy without repeated full scans.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- The contract assigns the 100-identical-request warm validation and retained-byte proof to EPR4-SB6, with activation lifecycle proof under EPR5-SB7. EPR1 ratifies its finite limits and required counters; it does not run a future production soak.

## Abort conditions

- Abort if any source cannot produce evidence sufficient for its authorized trust class.
- Abort if a managed install cannot be created with no-follow/atomic/private semantics on a supported platform.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Validate artifact identity, compatibility, integrity, signature, and origin schemas against positive and negative contract fixture vectors.
1. Run the ownership-inventory schema and validator against the complete required outcome/consumer/displaced-route population and each omission, unknown-node, missing-path, missing-acceptance and conflicting-owner negative fixture. Validate the cache-layout/state-transition decision tables and their runtime-owner bindings for permission, symlink/reparse, crash, concurrency, quarantine, and revocation cases.
1. Validate public status redaction/portability goldens and the declared warm-validation budgets/counter schema; bind runtime execution to the mapped later owners.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `security-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `supply-chain-platform`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
