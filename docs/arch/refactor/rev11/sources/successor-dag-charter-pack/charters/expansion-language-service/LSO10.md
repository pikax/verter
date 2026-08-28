<!-- unified-charter-v2
id=LSO10
name=Language-service convergence and legacy route deletion
phase=expansion
train=expansion.language-service
product=language_service
kind=terminal
semantic_role=delivery
class=successor
predecessors=LSO9,PER0,UAI0,UAP0,BR0
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=semantic_authority,mapping_geometry,lsp_publication,program_authority
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
release_gating=product
source_refs=live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md
external_requirements=
activation_gate=ORC0
charter=charters/expansion-language-service/LSO10.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO10 - Language-service convergence and legacy route deletion

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Converge and promote the language-service product after exact required conformance, performance, consumer, and deletion proofs. Remove every displaced feature route, mapper fallback, raw-edit path, duplicate target/occurrence/presentation authority, and legacy architecture document. LSO10 adds no new feature semantics.

The current owner is **accepted LSO nodes plus residual feature-specific handlers, provider/native merge paths, mapping fallbacks, raw edit builders, duplicated tests/docs, and manual capability claims**. The final and sole owner is **one promoted language-service product receipt, one operation capability snapshot, and structurally enforced use of the canonical target/occurrence/presentation/transaction authorities**.

## Architectural role and end state

LSO10 is a terminal proof and deletion block. Discovering missing semantic behavior, unsupported required rows, or incorrect performance sends work back to LSO1-LSO9 or a vertical owner; terminal cleanup may not implement it opportunistically.

## Expected production surfaces

- Language-service/session/LSP/type-runtime/action modules named in the terminal route inventory
- Rev11 authority, generated conformance/capability/deletion receipts
- Legacy `docs/arch` feature and editor architecture paths classified for deletion/relocation
- Performance/audit evidence and public capability tables

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `LanguageServiceProductReceipt`
- `LanguageServiceCapabilitySnapshot` and exact certified row/implementation digests
- `LegacyRouteDeletionManifest` and no-bypass architecture guard
- `LanguageServiceResidualLedger` for unsupported/external operation families

## Exact predecessor contracts

- **LSO9:** consume exact required operation certifications, residual ledger, and generated capability table.
- **PER0:** consume terminal equivalent-work/latency/allocation/RSS methodology.
- **UAI0:** consume final identity/carrier/parser/coordinate contract lock.
- **UAP0:** consume capability/coexistence/rule-action/public contract lock.
- **BR0:** consume successor product promotion authority.

External custody: none beyond the package activation boundary.

## Binding architecture

- Terminal work proves, deletes, and promotes; it does not add targets, occurrence roles, rename policy, candidate logic, presentation semantics, or edit algorithms.
- Every legacy/bypass route has one exact deletion owner and a structural negative guard.
- Residual external/unsupported ownership is explicit and capability-visible.
- Deleting TypeScript provider feature routes is per certified operation family; providers remain for residual owners.
- Product receipt binds exact DAG/charter/manifest/implementation/evidence/capability/deletion digests.
- Public/editor documentation is relocated outside `docs/arch`; Git history is the archive.

## Internal subblocks

### LSO10-SB1 - Terminal certification and residual closure

**Independently testable outcome:** All required rows have accepted receipts and every non-required row has exact residual owner/maturity.

**Architecture:**

- Validate LSO9 manifest/receipts against current implementation tree.
- Reject stale/partial/sampled certification.
- Generate residual ledger and public capability snapshot.

**Expected changes:**

- Run terminal validator and freeze candidate.
- Reopen owning node for any gap.

**Discriminating proof:**

- Every required row maps to exact implementation/evidence receipts.
- Capability claims contain no unproven operation/profile topology.

### LSO10-SB2 - Feature route and authority deletion

**Independently testable outcome:** No consumer/handler bypasses canonical LSO authorities for migrated operations.

**Architecture:**

- Generate call/path/symbol inventory for old target, occurrence, merge, mapper, presentation, edit routes.
- Delete routes/stores/flags/helpers and register negative guards.
- Retain only typed provider adapters behind canonical services.

**Expected changes:**

- Perform bounded deletions by exact manifest.
- Remove dead compatibility shims in same candidate.

**Discriminating proof:**

- Planting each deleted route fails architecture tests.
- No direct provider/raw edit/current-file mapper bypass remains.

### LSO10-SB3 - Legacy architecture cleanup and product-doc relocation

**Independently testable outcome:** All durable clauses are in Rev11 and product/editor docs live beside products rather than as competing architecture.

**Architecture:**

- Validate blob-SHA disposition for every legacy path.
- Relocate as-built editor usage/packaging docs to editor/product directories.
- Delete historical plans/backlogs/ledgers after source atom transfer.

**Expected changes:**

- Apply legacy disposition and permanent tree guard.
- Do not create archive/old/legacy docs directories.

**Discriminating proof:**

- No unclassified file remains outside Rev11 under docs/arch.
- No live authority references deleted paths.

### LSO10-SB4 - Cross-surface and coexistence terminal

**Independently testable outcome:** Opened editor/public surfaces and coexistence modes consume canonical operations and capability withdrawal correctly.

**Architecture:**

- Run LSO9 exact matrix on landing-frozen candidate.
- Test dynamic register/unregister, provider/profile transitions, stale clearing, and zero-work disabled modes.
- Verify consumer render differences do not alter core semantics.

**Expected changes:**

- Capture terminal surface receipt.
- Delete manual client branches displaced by generated descriptors.

**Discriminating proof:**

- Only overlapping capabilities withdraw under auto coexistence.
- No stale results survive authority/capability transitions.

### LSO10-SB5 - Performance/cancellation/memory terminal

**Independently testable outcome:** The product is bounded under representative cold/warm/incremental/churn/parallel/cancel workloads.

**Architecture:**

- Run equivalent-work counters and latency/allocation/RSS gates.
- Test project open/close, provider swap, broken edits, cursor/resolve-key abandonment, large candidate sets.
- Require memory release and no hidden eager work.

**Expected changes:**

- Capture PER0 terminal receipt.
- Reopen owning node for regressions; no blind terminal micro-optimization.

**Discriminating proof:**

- Warm operations meet ratified work thresholds.
- Long churn plateaus and teardown releases retained state.

### LSO10-SB6 - Product receipt and promotion

**Independently testable outcome:** Promotion is exact, immutable, honest, and invalidated by any authority/evidence change.

**Architecture:**

- Bind DAG/charter/source/manifest/implementation/review/gate/performance/deletion/capability digests.
- State residual provider/unsupported operations explicitly.
- Publish maturity/capability through PUB0/COX0.

**Expected changes:**

- Emit product receipt and successor promotion state.
- Remove temporary migration flags.

**Discriminating proof:**

- Receipt validation fails on any changed input.
- Public claims exactly match certified scope.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- LSO10 may not introduce a new operation family or semantic algorithm.
- A retained provider adapter must have an exact residual operation owner and capability row.
- Deletion receipts bind both absence and structural rejection.

## Migration and cutover

- Run only after LSO9 required certifications are accepted.
- Freeze candidate, run route/source inventory, perform deletions/relocations, rerun complete gates/reviews.
- Stop and reopen predecessors for any semantic/performance gap.

## Deletions

- Delete all displaced language-service routes/stores/flags/helpers/tests/docs named by terminal manifests.
- Delete remaining mapping/0:0/nearest/current-file/raw-edit fallbacks.
- Delete manual capability claims and migration shims.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Implementing missing semantics in terminal cleanup.
- Retaining duplicate routes “for safety”.
- Claiming universal/full parity beyond certified rows.
- Deleting residual provider capabilities without separate certification.
- Archiving legacy docs under another docs/arch folder.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO10-AC-CERTIFIED:** every required operation/profile topology has current exact certification.
- **LSO10-AC-DELETED:** route/source manifests prove absence and structural rejection of displaced authority.
- **LSO10-AC-SURFACES:** opened consumers/coexistence modes pass exact terminal matrix.
- **LSO10-AC-PERF:** equivalent-work/latency/allocation/RSS/cancel/churn terminal passes.
- **LSO10-AC-HONEST:** residual provider/unsupported ownership and public capability claims are exact.
- **LSO10-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO10-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO10-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO10-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Terminal thresholds are ratified equivalent-work replacement thresholds, not unsupported blanket zero-delta claims.
- Target ceiling: 300 production LOC, 3 production files, and 1 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if route/source inventory is incomplete or deletion cannot be structurally guarded.
- Abort if any required certification/evidence receipt is stale.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Full authority/source/route/deletion/capability validation.
1. Complete LSO9 matrix plus terminal performance/cancellation/churn/memory suite.
1. Configured architecture review and immutable product receipt validation.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Promotes the authored-coordinate language-service product.
- Provides stable substrate for future framework/editor operations.
- Does not by itself retire external TypeScript semantics outside certified operation families.

## Source reconciliation

- All LSO charters, LSO9 manifests/receipts, legacy disposition, PER0/UAI0/UAP0/BR0 contracts.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
