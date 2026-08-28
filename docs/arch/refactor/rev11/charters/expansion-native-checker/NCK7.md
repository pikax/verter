<!-- unified-charter-v2
id=NCK7
name=Shared diagnostic service and consumer-surface integration
predecessors=NCK6,PUB0
conditional_predecessors=CLI2:when-opened,CLI4:when-opened
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=diagnostic_action_service,public_protocol,lsp_publication,cli_application,capability_catalog
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
charter=charters/expansion-native-checker/NCK7.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK7 — Shared diagnostic service and consumer-surface integration

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

## Independently acceptable outcome

Expose one shared DiagnosticService across LSP, CLI, MCP, NAPI, WASM, and library consumers. Consumers receive authored-coordinate, provenance-complete diagnostic batches from NCK6 and apply only presentation policy; they cannot call semantic/provider engines directly or re-arbitrate authority.

The current owner is **consumer-local diagnostic DTOs, LSP-specific provider merge code, command-local typecheck composition, and inconsistent mapping/drop behavior**. The final and sole owner is **one shared DiagnosticService request/result contract with thin surface adapters and one authored-coordinate projection law**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_diagnostics`, `crates/verter_session`, `crates/verter_protocol`, `crates/verter_lsp`, `crates/verter_mcp_server`, `crates/verter_napi`, `crates/verter_wasm`, `packages/binary-launcher`, `packages/verter-lsp`.
- Pack production inventory:
- `crates/verter_diagnostics` and `crates/verter_session` for the shared service and project snapshot access
- `crates/verter_protocol` for versioned public requests/results and stable IDs
- `crates/verter_lsp` for diagnostics publication and code-action references
- `crates/verter_mcp_server`, `crates/verter_napi`, `crates/verter_wasm`, and FFI/public packages for thin adapters
- `packages/binary-launcher`, `packages/verter-lsp`, and CLI application services when their conditional predecessors are opened

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `DiagnosticRequest { scope, profile, demand, basis, cancellation, budget }`
- `DiagnosticService::check_region`, `check_file`, and `check_project_rules`
- `DiagnosticBatch { basis, completeness, diagnostics, authority_snapshot }`
- `AuthoredDiagnostic`, `AuthoredRelatedLocation`, `DiagnosticProofRef`, and `DiagnosticFixIntentRef`
- `DiagnosticSurfaceAdapter` as serialization/presentation only, not semantic extension
- `DiagnosticStreamCursor` for bounded project/watch enumeration where supported

## Exact predecessor contracts

- **NCK6:** exact current receipt ID and digest for “Family-scoped diagnostic authority arbitration and atomic publication”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

### Binding architecture

- Core diagnostic batches are authored-coordinate results; generated/provider coordinates do not cross the service boundary.
- Every surface observes the same semantic diagnostics, authority state, basis, completeness, related locations, proof refs, and fix-intent refs.
- Presentation fields such as LSP severity tags, terminal colors, JSON layout, progress UI, and streaming framing are adapter policy.
- A surface cannot convert NeedInputs, unsupported, cancelled, stale, or partial into empty complete success.
- Fixes remain typed intents/references until LSO8/LRA0 validates an authored edit transaction.
- Project checks are bounded coordinators with streaming/pagination; a consumer cannot request hidden unbounded workspace work.
- Provider calls and semantic queries occur inside the shared service/authority layer only.

### Internal subblocks

#### NCK7-SB1 - Shared service request and scope contract

**Independently testable outcome:** All consumers request the same region/file/project-rule diagnostic operations with exact basis, demand, cancellation, and budgets.

**Architecture:**

- Define scope selectors without LSP URI or CLI presentation fields.
- Require exact project/profile/source basis and capability availability.
- Model project-rule enumeration as bounded pages/streams with explicit completeness.

**Expected changes:**

- Add the shared service facade over NCK6 publication plans and NCK2 queries.
- Replace consumer-local project loading and diagnostic plan selection.

**Discriminating proof:**

- Equivalent requests from two surfaces produce the same core request identity.
- Unbounded or ambiguous project selection is rejected rather than silently choosing the first project.

#### NCK7-SB2 - Authored-coordinate diagnostic projection

**Independently testable outcome:** Every returned primary and related location is mapped to exact authored source or refused with typed provenance loss.

**Architecture:**

- Use UAI0/TCM authored mapping and source lineage for native, framework, and external diagnostics.
- Preserve source unit, profile, revision, mapping chain, and anchor confidence.
- Drop or return a typed incomplete result for unmappable provider artifacts; never synthesize 0:0 or nearest ranges.

**Expected changes:**

- Centralize diagnostic range projection before consumer adapters.
- Delete LSP-only range fallbacks and duplicated carrier mapping branches.

**Discriminating proof:**

- UTF-8/UTF-16/CRLF/emoji/embedded carrier cases round-trip exact authored spans.
- Stale mapper/source revisions are rejected and cannot publish.

#### NCK7-SB3 - LSP diagnostics and code-action reference adapter

**Independently testable outcome:** LSP publication consumes shared authored batches and exposes exact code-action references without rechecking or remapping semantics.

**Architecture:**

- Translate authored spans through negotiated position encoding only at the LSP edge.
- Publish latest-basis batches under H3 and clear only capabilities withdrawn by COX0.
- Resolve fix-intent references through LRA0/LSO8 rather than embedding unchecked workspace edits.

**Expected changes:**

- Route foreground/background diagnostic publication through one adapter.
- Delete provider/native merge and authority selection from LSP code.

**Discriminating proof:**

- Foreground and background paths publish identical core diagnostic identities.
- Dynamic capability withdrawal cancels work and clears only owned diagnostics.

#### NCK7-SB4 - CLI, MCP, NAPI, WASM, and library adapters

**Independently testable outcome:** Non-LSP surfaces preserve core semantics and report unavailable inputs/capabilities truthfully.

**Architecture:**

- Define stable JSON/protobuf/FFI projections from PUB0 without surface-specific semantic DTOs.
- CLI typecheck writes nothing and uses explicit project/reference/watch selection.
- WASM/MCP report NeedInputs when filesystem/provider/project services are unavailable.

**Expected changes:**

- Replace command-local or binding-local diagnostic composition.
- Generate bindings and compatibility tests from the public schema.

**Discriminating proof:**

- Cross-surface differential fixtures match diagnostic identity, basis, completeness, provenance, related/fix refs.
- A missing input never becomes empty success.

#### NCK7-SB5 - Watch, cancellation, streaming, and supersession

**Independently testable outcome:** Long-running and watch consumers receive deterministic latest-basis batches without stale cache admission or retained-work growth.

**Architecture:**

- Use cancellation/deadline/budget tokens through region/file/project coordinators.
- Supersede in-flight work on source/profile/authority/provider epoch changes.
- Bound stream cursors and release snapshots after completion/cancellation.

**Expected changes:**

- Unify watch and one-shot paths over the same service.
- Remove polling/sleep readiness and consumer-owned debounce semantics from diagnostic correctness.

**Discriminating proof:**

- Rapid edit/revert/provider restart tests publish only the latest basis.
- Cancelled project streams release retained regions/results and admit nothing partial.

#### NCK7-SB6 - Consumer route inventory and migration proof

**Independently testable outcome:** Every public diagnostic consumer is known, migrated, and structurally prevented from bypassing the shared service.

**Architecture:**

- Generate a call-site inventory for direct provider diagnostics, native checker calls, and legacy DTO construction.
- Migrate one surface at a time behind behavior characterization, then delete bypasses.
- Keep optional conditional consumers zero-work and unclaimed when unopened.

**Expected changes:**

- Add static architecture guards and generated consumer matrix.
- Record exact deletions and residual unsupported surfaces.

**Discriminating proof:**

- Planting a direct provider/checker call in a consumer crate fails the guard.
- The inventory reaches zero unexplained bypasses before NCK8.

### Identity, invalidation, and publication

- Core result identity is independent of surface encoding and presentation.
- Authored span projection validates the exact source/mapping basis used to obtain the range.
- Consumers may filter only explicitly policy-filterable classes under a named capability/configuration rule; they cannot suppress semantic families silently.
- Project stream cursors are scoped to an immutable basis and become stale on any authority/source/profile change.
- No consumer adapter owns semantic caching; it may cache serialization only by full core result identity.

### Migration and cutover

- Characterize each consumer surface against existing behavior and identify intentional corrections.
- Introduce the shared service with LSP as first consumer, then CLI/MCP/NAPI/WASM/library surfaces.
- Delete direct provider/native merge paths immediately after the last consumer moves.
- Keep unopened conditional CLI predecessors outside acceptance and prove zero hidden integration work.

### Consumers and unlocks

- Unlocks NCK8 terminal closure.
- Provides the checker diagnostic service consumed conditionally by LSO9 and future verticals.
- Supports CLI typecheck without claiming full TypeScript engine retirement.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **NCK7-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **NCK7-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **NCK7-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **NCK7-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **NCK7-AC-SURFACES:** all opened consumers match core diagnostic identity, basis, completeness, provenance, related locations, and fix refs.
- **NCK7-AC-AUTHORED:** no public diagnostic leaves the service with generated coordinates or unvalidated mapping basis.
- **NCK7-AC-NO-BYPASS:** static inventory proves consumer crates cannot call diagnostic providers/resolvers directly.
- **NCK7-AC-WATCH:** watch/stream cancellation and supersession publish only latest complete batches and release retained state.
- **NCK7-AC-NEEDINPUTS:** unavailable surfaces return typed NeedInputs/unsupported, never empty complete success.
- **NCK7-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK7-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK7-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK7-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete consumer-local diagnostic authority arbitration, semantic deduplication, and provider/native merge logic.
- Delete Range::default/0:0/nearest-position diagnostic fallbacks and surface-specific semantic DTOs.
- Delete command-local project/checker construction displaced by shared application/service integration.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- A surface adapter calling tsgo/tsserver or native Check queries directly.
- LSP URI/Position, terminal formatting, or provider handles in core diagnostic results.
- Embedding raw text edits in diagnostics instead of typed fix-intent references.
- Converting unavailable/partial/stale results to empty success.
- Hidden full-workspace checks on file-open, hover, completion, or unrelated leaf operations.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Thin adapters add no parse/resolve/provider/checker work and perform bounded serialization/allocation proportional to returned diagnostics.
- Repeated serialization may cache only by full result/basis/schema identity and must plateau in retained bytes.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if any consumer requires a surface-specific semantic result not representable under PUB0; amend PUB0 rather than forking.
- Abort if exact authored projection is unavailable and a fallback location is proposed.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Cross-surface differential matrix, authored mapping/encoding tests, watch/cancel/supersession tests, and no-bypass architecture guard.
1. LSP foreground/background equivalence and dynamic capability withdrawal tests.
1. CLI/MCP/NAPI/WASM NeedInputs and schema-compatibility tests for every opened consumer.

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

### SRC-LEGACY-NCK-SURFACE-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:62-66`
- Applicability: `NCK7`, `NCK8`
- Exact text SHA-256: `80bb125bd976d6c582f8b9ccd44ac69efe608f2eef261b9523bf2c9810e439a0`

~~~~markdown
### NCK-SURFACE-001 — One shared diagnostic service

- LSP, CLI, MCP, NAPI, WASM, and library consumers observe the same authored diagnostic identity, basis, completeness, provenance, related locations, proof refs, and fix-intent refs.
- Consumers cannot call providers/checker queries directly or re-arbitrate authority.
- Targets: `NCK7`, `NCK8`.
~~~~
