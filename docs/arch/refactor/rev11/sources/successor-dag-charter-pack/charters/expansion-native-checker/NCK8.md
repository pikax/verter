<!-- unified-charter-v2
id=NCK8
name=Native checker terminal and displaced-authority deletion
phase=expansion
train=expansion.native-checker
product=native_checker
kind=terminal
semantic_role=delivery
class=successor
predecessors=NCK7,NCKF0,PER0,UAO0,UAP0,BR0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,diagnostic_action_service,performance_evidence,program_authority
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
charter=charters/expansion-native-checker/NCK8.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK8 - Native checker terminal and displaced-authority deletion

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Close the native checker product only after the required generated diagnostic slices, framework ingress, authority promotions, shared consumer integrations, performance/cancellation/memory proofs, and legacy authority deletion are complete on one exact terminal basis. This block adds no new diagnostic semantics.

The current owner is **accepted NCK/NCF nodes plus residual displaced diagnostic routes, stores, tests, flags, and legacy architecture documents**. The final and sole owner is **the promoted native checker product receipt, exact certified-family authority snapshot, and structurally enforced absence of displaced diagnostic authority**.

## Architectural role and end state

NCK8 is a proof, deletion, and promotion terminal. Any missing diagnostic algorithm, unsupported required family, semantic mismatch, or public-contract gap reopens its owning NCF/NCK predecessor; terminal cleanup may not patch semantics locally.

## Expected production surfaces

- `docs/arch/refactor/rev11/authority`, catalogs, generated manifests, receipts, and legacy disposition
- `crates/verter_session`, `crates/verter_semantic`, `crates/verter_diagnostics`, `crates/verter_lsp`, and CLI only for bounded final cutover/deletion
- `crates/verter_bench` and performance evidence for checker latency, work, allocation, and RSS
- repository-wide architecture guards for deleted diagnostic authority paths

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `NativeCheckerProductReceipt` and exact required/residual family inventory
- `LegacyDiagnosticAuthorityDeletionManifest`
- `CheckerSurfaceEquivalenceReceipt` across CLI/LSP/public consumers
- `CheckerPerformanceReceipt` and long-churn memory evidence

## Exact predecessor contracts

- **NCK7:** consume the shared consumer service and zero-bypass surface integration.
- **NCKF0:** consume the machine-generated required-family convergence receipt, exact manifest/predecessor bijection, current certification/promotion chains, provider-zero-work, and per-slice performance/admission closure.
- **PER0:** consume equivalent-work, latency, allocation, cancellation, and RSS terminal methodology.
- **UAO0:** consume activation, TypeInfo, index, and performance contract lock.
- **UAP0:** consume capability, coexistence, diagnostic/action, and public contract lock.
- **BR0:** consume successor product promotion authority and exact release law.

External custody: none beyond the package activation boundary.

## Binding architecture

- Terminal completeness is manifest-derived. NCK8 cannot declare success by sampling or percentage.
- External residual families are allowed only when explicitly classified as product exclusions or future requirements with honest capability reporting.
- No semantic algorithm work is hidden in the terminal. Any missing rule/family opens or amends an NCF node.
- Every displaced route/store/guard/doc is deleted or explicitly retained with sole ownership and rationale.
- Cross-surface equivalence compares semantic diagnostic identity/basis, not editor formatting.
- Performance acceptance uses equivalent work, first/warm check latency, cancellation waste, allocations, and long-churn RSS.

## Internal subblocks

### NCK8-SB1 - Manifest completeness and residual classification

**Independently testable outcome:** Every required family slice has an accepted implementation/certification/promotion receipt or an explicit product exclusion.

**Architecture:**

- Compute completeness from the canonical manifest and authority table.
- Reject wildcard deferrals and unowned residual rows.
- Record future external-owned scope separately from completed native product claims.

**Expected changes:**

- Generate terminal completeness report and machine receipt.
- Open amendments for any missing independently acceptable work before proceeding.

**Discriminating proof:**

- Planted missing/duplicate/unpromoted required slice blocks terminal.
- Report is reproducible from authority inputs.

### NCK8-SB2 - Displaced authority and store deletion

**Independently testable outcome:** No migrated family has an old producer, cache, merge path, or fallback capable of publishing.

**Architecture:**

- Sweep semantic, session, LSP, provider, framework, and command paths by registered family owners.
- Delete old stores and compatibility branches after final consumers move.
- Retain external provider machinery only for explicitly external families and other language-service capabilities.

**Expected changes:**

- Apply exact deletion manifest and negative guards.
- Remove stale docs/tests/config flags tied to deleted authority.

**Discriminating proof:**

- Planting any deleted route fails architecture tests.
- No migrated family produces provider diagnostic work in runtime counters.

### NCK8-SB3 - Cross-surface semantic equivalence

**Independently testable outcome:** CLI, LSP, MCP, NAPI/WASM/public surfaces observe equivalent native semantic diagnostics and truthful outcomes.

**Architecture:**

- Compare diagnostic identity, basis, completeness, provenance, and related/fix refs.
- Allow presentation-specific formatting only after core equivalence.
- Verify unavailable inputs yield NeedInputs rather than empty success.

**Expected changes:**

- Generate surface matrix fixtures and receipts.
- Fix only bounded adapter discrepancies; semantic gaps reopen NCF work.

**Discriminating proof:**

- Differential matrix passes for all available surfaces/profiles.
- A surface-specific semantic DTO or dropped provenance blocks terminal.

### NCK8-SB4 - Performance, cancellation, and memory terminal

**Independently testable outcome:** The checker is production-bounded under cold, warm, incremental, churn, cancellation, and parallel load.

**Architecture:**

- Measure equivalent fact/rule/query work, allocations, retained bytes, latency distributions, and provider avoidance.
- Test repeated edits, project open/close, profile transitions, and cancelled workspace checks.
- Require no unbounded result/proof/contribution retention.

**Expected changes:**

- Capture checker performance receipt under PER0 methodology.
- Reopen the owning implementation node for unexplained regressions; do not micro-optimize blindly in NCK8.

**Discriminating proof:**

- Long-churn memory plateaus and project teardown releases storage.
- Warm certified families perform zero provider diagnostic work.

### NCK8-SB5 - Legacy architecture reconciliation and deletion

**Independently testable outcome:** All durable legacy checker/type-parity clauses are in Rev11 authority and obsolete files are removed.

**Architecture:**

- Validate exact blob-SHA disposition for every legacy path.
- Ensure no live authority references deleted files.
- Keep product/user docs outside `docs/arch` where appropriate.

**Expected changes:**

- Delete classified legacy files in the same accepted amendment.
- Enable permanent guard forbidding new docs/arch files outside Rev11.

**Discriminating proof:**

- Repository tree contains no unclassified live legacy architecture.
- Source-atom coverage remains complete after deletion.

### NCK8-SB6 - Native checker product receipt and promotion

**Independently testable outcome:** The product is promoted with exact scope, residuals, evidence, and no hidden claim of full TypeScript replacement beyond certified families.

**Architecture:**

- Bind manifest digest, authority snapshot, surface/performance/deletion receipts, and review verdicts.
- State remaining external families and runtime provider uses honestly.
- Separate checker completion from full language-service/provider retirement.

**Expected changes:**

- Emit immutable product receipt and update capability/maturity matrices.
- Do not delete TypeScript provider capabilities still owned by LSO/EPR or external residual families.

**Discriminating proof:**

- Receipt invalidates on any authority/source/evidence change.
- Public capability claims match the exact certified scope.

## Data, identity, invalidation, and publication laws

- NCK8 may not add a new diagnostic algorithm, rule family, or semantic fact authority.
- Residual external ownership is explicit and capability-visible; it is not a failure if product scope says so.
- A product receipt names exact manifest and authority epochs and is immutable.
- Deleting provider diagnostic paths does not imply deleting provider completion/navigation capabilities.

## Migration and cutover

- Run terminal only after required NCF nodes and NCK6 promotions are accepted.
- Perform bounded cleanup/deletion in one landing-frozen candidate with complete negative guards.
- If final sweeps discover semantic gaps, stop and open owning NCF/NCK amendments.

## Deletions

- Delete all displaced checker diagnostic producers, stores, merge paths, flags, and legacy docs named in the terminal manifest.
- Delete stale parity ledgers and ignored-test mechanisms replaced by NCK4 authority.
- Delete any claim that NCK8 retires the entire TypeScript engine unless separate LSO/EPR/provider retirement authority exists.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Adding missing semantic features in the terminal block.
- Treating sampled parity, green coverage, or message counts as full certification.
- Deleting provider capabilities still owned outside diagnostic families.
- Accepting unexplained performance or memory regressions as cleanup noise.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK8-AC-MANIFEST:** all required slices have exact accepted implementation, certification, and promotion receipts.
- **NCK8-AC-DELETION:** every displaced diagnostic route/store/doc is absent and structurally rejected.
- **NCK8-AC-SURFACES:** semantic diagnostic results and outcomes are equivalent across supported public surfaces.
- **NCK8-AC-TERMINAL-PERF:** cold/warm/incremental/cancel/churn work, allocation, latency, and RSS satisfy PER0 evidence.
- **NCK8-AC-HONESTY:** residual external ownership and non-checker provider uses are explicitly documented.
- **NCK8-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK8-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK8-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK8-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Terminal performance thresholds must be replacement/equivalent-work thresholds ratified by PER0, not arbitrary zero-regression assertions when capability work differs.
- Target ceiling: 300 production LOC, 3 production files, and 1 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Full native checker manifest/authority/source/deletion validation.
1. Canonical cross-surface, provider-avoidance, incremental/fresh, cancellation, and long-churn test matrix.
1. Configured architecture-3 review and product promotion receipt validation on the exact candidate.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Promotes the native checker product for certified families.
- Provides a stable diagnostic service for CLI, language-service conformance, lint/fix composition, and future framework verticals.
- Does not by itself unlock full TypeScript engine retirement.

## Source reconciliation

- All NCK/NCF authority and `legacy-arch-disposition.toml` entries targeting native checker/type-parity docs.
- PER0, PUB0, UAO0, UAP0, and BR0 terminal contracts.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
