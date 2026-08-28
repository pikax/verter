<!-- unified-charter-v2
id=NCK3
name=Shared-proof semantic diagnostic rule kernel
predecessors=NCK2,D8,LRA0
conditional_predecessors=
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,flowslice,diagnostic_action_service
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
charter=charters/expansion-native-checker/NCK3.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK3 — Shared-proof semantic diagnostic rule kernel

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

## Independently acceptable outcome

Implement the shared-proof diagnostic rule kernel that plans fact demands, reads authoritative relation/call/flow/contextual/declaration facts, emits stable diagnostics and fix intents, and proves that no rule re-resolves semantic meaning. Only representative canary rules land here; catalogue parity belongs to generated NCF nodes.

The current owner is **scattered hard-coded checks, provider diagnostic messages, framework-specific validation, and prospective checker walkers**. The final and sole owner is **a static, typed, demand-declared diagnostic rule kernel over shared semantic proofs**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_diagnostics`, `crates/verter_semantic`, `crates/verter_session`, `crates/verter_actions`, `crates/verter_session/tests`.
- Pack production inventory:
- `crates/verter_diagnostics` for rule descriptors, emission, suppression identity, and compact diagnostic construction
- `crates/verter_semantic` for read-only fact/proof views and typed rule demands
- `crates/verter_session` for query dispatch integration, rule planning, and exact read-set capture
- `crates/verter_actions` for typed fix intents, not direct edits
- `crates/verter_session/tests` and semantic fixtures for no-second-resolver guards and canary rules

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `DiagnosticRuleDescriptor`, `DiagnosticRulePlan`, and static `DiagnosticRuleRegistry`
- `FactRequirement`, `RuleApplicability`, `RuleBudget`, and `RuleExecutionContext`
- `DiagnosticFactView` exposing typed relation/call/flow/contextual/declaration results only
- `ProofRef`, `DiagnosticEmitter`, `SuppressionKey`, and `FixIntentRef`
- `RuleExecutionReceipt` with facts read, work counters, and completeness

## Exact predecessor contracts

- **NCK2:** exact current receipt ID and digest for “Incremental diagnostic query and result domain”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **D8:** exact current receipt ID and digest for “U6 convergence and complete-result admission proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LRA0:** exact current receipt ID and digest for “Profile-scoped diagnostics, lint, fixes, and actions”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

### Binding architecture

- Rules declare fact requirements before execution. Applicability and demand planning must permit zero work for irrelevant rules.
- The fact view exposes final typed outcomes and proof references, not mutable semantic stores or resolver callbacks.
- A negative relation or failed call applicability becomes a diagnostic through a rule; the rule does not rerun relation or overload matching.
- Control-flow diagnostics consume existing reachability, completion, return, capture, and narrowing facts.
- Rule registration is static/catalog-driven. Executing arbitrary third-party code inside the semantic engine is out of scope.
- Suppressions are keyed by stable rule/subject/provenance identity and cannot hide diagnostics from unrelated authorities.
- Fixes are semantic intents that LSO7 later materializes against authored current source.

### Internal subblocks

#### NCK3-SB1 - Static rule descriptor and registry

**Independently testable outcome:** Every rule has exact family/slice identity, applicability, fact requirements, severity class, fix capability, and owner.

**Architecture:**

- Define a generated/static registry keyed by DiagnosticRuleId.
- Separate semantic rules from lint and framework-owned rule descriptors while allowing one public shape.
- Declare profile/language/region applicability without framework switches in core.

**Expected changes:**

- Implement descriptor types and registry generation hooks.
- Bind every rule to NCK4 manifest rows and LRA0 action policy.

**Discriminating proof:**

- Registry completeness and duplicate-ID mutation tests.
- An inapplicable rule records zero fact reads and zero allocations.

#### NCK3-SB2 - Demand planning and applicability

**Independently testable outcome:** The kernel requests only facts required by applicable rules and never whole-checks by default.

**Architecture:**

- Compile applicable rule requirements into a deterministic RulePlan.
- Coalesce identical fact demands while preserving rule attribution.
- Propagate budget, cancellation, and NeedInputs before evaluation.

**Expected changes:**

- Implement planner and demand counters.
- Add plan dumps for tests/evidence, not production semantic authority.

**Discriminating proof:**

- Permutation tests produce byte-identical plans.
- A leaf CheckExpression proves unrelated file/project rules perform zero work.

#### NCK3-SB3 - Read-only shared fact and proof view

**Independently testable outcome:** Rules can inspect authoritative facts without access to private resolver algorithms or mutable stores.

**Architecture:**

- Expose typed read methods for relation, resolve-call, overload, flow, contextual, declaration, and project-index facts.
- Record every read into the Check query read set.
- Return typed incomplete/NeedInputs rather than synthesizing fallback facts.

**Expected changes:**

- Implement capability-limited view wrappers.
- Add compile/static guards banning resolver entry points from rule modules.

**Discriminating proof:**

- A planted direct resolver call or store mutation fails static architecture tests.
- Read-set mutation invalidates the right cache entry and no broader family.

#### NCK3-SB4 - Diagnostic emission, proof, and dedup

**Independently testable outcome:** Rule output is stable, authored, evidence-linked, and deterministic.

**Architecture:**

- Construct semantic diagnostic identity before localized message rendering.
- Attach primary and related authored anchors plus optional ProofRef.
- Deduplicate by stable identity/authority, not message text.

**Expected changes:**

- Implement DiagnosticEmitter and canonical sorting.
- Create proof retention/refcount policy compatible with NCK2 reclamation.

**Discriminating proof:**

- Equivalent reordered fact delivery yields byte-identical batches.
- Two distinct semantic subjects with identical messages never collapse.

#### NCK3-SB5 - Suppression and fix-intent boundary

**Independently testable outcome:** Suppressions and fixes preserve owner, profile, source basis, and safety class.

**Architecture:**

- Model suppression directives separately from diagnostics and lint configuration.
- Emit fix intents containing semantic target and transformation class, never generated-coordinate TextEdits.
- Classify safe, suggested, and unsafe intents under LRA0.

**Expected changes:**

- Implement typed refs and validation hooks; LSO7 remains the edit materializer.
- Add duplicate/suppression provenance guards.

**Discriminating proof:**

- Stale or foreign-profile suppression fails closed.
- No fix intent can be converted without an exact authored basis.

#### NCK3-SB6 - Representative canary rules and one-engine guards

**Independently testable outcome:** The kernel proves its architecture on a small cross-family set without absorbing the parity train.

**Architecture:**

- Canaries: assignment relation failure, failed call applicability, missing return/unreachable region, and duplicate declaration project rule.
- Each canary must consume an existing authoritative fact and carry an oracle fixture.
- No additional family breadth is accepted in NCK3.

**Expected changes:**

- Implement canaries and named guards.
- Record remaining catalogue work only in the NCK4 manifest.

**Discriminating proof:**

- Mutation of the underlying shared fact changes the diagnostic; mutation of a duplicate checker algorithm is impossible because none exists.
- Canary differential and incremental/fresh tests pass across native and provider observation.

### Identity, invalidation, and publication

- A rule result is complete only when all declared required facts are complete on the same basis.
- Rule execution order does not affect diagnostic identity, ordering, or read-set signature.
- Rules cannot mutate semantic facts or write index state.
- Framework-owned rules enter through NCK5 descriptors/contributions but run on the same kernel.
- Proof references are opaque stable handles with lifecycle tied to the batch/store generation.

### Migration and cutover

- Move only representative checks whose fact authority and exact replacement can be proven.
- Leave lint rules in LRA0/LNT ownership and provider semantic families external until their generated NCF slice is certified.
- Delete a displaced hard-coded rule only in the same candidate that routes its complete demand and output through the kernel.

### Consumers and unlocks

- Unlocks NCK4 manifest/oracle generation and NCK5 framework rule ingress.
- Supplies the sole diagnostic rule execution contract for every generated NCF slice.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **NCK3-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **NCK3-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **NCK3-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **NCK3-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **NCK3-AC-FACTS:** every canary diagnostic is traceable to declared authoritative facts and exact read-set entries.
- **NCK3-AC-NO-RESOLVER:** static architecture guard rejects resolver calls and duplicate semantic algorithms in rule modules.
- **NCK3-AC-ZERO-WORK:** inapplicable rules execute no fact demand, provider call, or allocation.
- **NCK3-AC-CANARIES:** four cross-family canaries pass oracle, incremental, cancellation, and proof tests.
- **NCK3-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK3-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK3-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK3-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete canary-equivalent ad hoc semantic checks and duplicate rule registries.
- Delete direct TextEdit construction from semantic checker rules.
- Delete any checker-private relation/call/flow helper introduced during implementation.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Rules parsing source text, regexing type text, synthesizing/reparsing TypeScript, or walking types to reproduce resolver decisions.
- Dynamic third-party rule code in the trusted semantic process.
- Message-text dedup or range-only suppression.
- Rules emitting LSP coordinates or provider handles.
- Expanding NCK3 into the full diagnostic catalogue.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Rule planning cost is proportional to applicable registered rules for the selected profile/slice, with catalog indexing preventing global scans.
- Repeated warm canary checks allocate only the returned batch representation or share it according to NCK2 policy.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. `cargo nextest run -p verter_diagnostics -p verter_actions -p verter_semantic -p verter_session`.
1. Static one-engine guards and rule-registry generation tests.
1. Canary differential, zero-work, incremental/fresh, cancellation, and proof-lifecycle tests.

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

### SRC-LEGACY-NCK-SHARED-PROOF-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:38-44`
- Applicability: `NCK3`, `NCF-AT-CYCLE`, `NCF-AT-QUERY`, `NCF-AT-REDUCE`, `NCF-BD-DUP`, `NCF-BD-INIT`, `NCF-BD-SCOPE`, `NCF-CF-CONTEXT`, `NCF-CF-THIS`, `NCF-CF-VAR`, `NCF-CO-CALL`, `NCF-CO-INFER`, `NCF-CO-OVER`, `NCF-FD-CFLOW`, `NCF-FD-DEF`, `NCF-FD-NARROW`, `NCF-JD-DEC`, `NCF-JD-JS`, `NCF-JD-JSDOC`, `NCF-JF-JSX`, `NCF-JF-SVELTE`, `NCF-JF-VUE`, `NCF-MP-AUG`, `NCF-MP-MODULE`, `NCF-MP-PROJECT`, `NCF-OC-HERIT`, `NCF-OC-MEM`, `NCF-OC-MERGE`, `NCF-RO-ASSIGN`, `NCF-RO-EXCESS`, `NCF-RO-OPER`
- Exact text SHA-256: `0997e868a549534ac49f69642b9b8580eb49d6f50457b3b36efa4d28b84ab429`

~~~~markdown
### NCK-SHARED-PROOF-001 — Diagnostics derive from shared proofs

- Assignability diagnostics consume `Relate` outcomes/proofs.
- Call/overload diagnostics consume `ResolveCall`/`ResolveOverloadSet` evidence.
- flow diagnostics consume accepted flow/return/completion/narrowing facts.
- contextual diagnostics consume `ContextualTypeAt` and shared relation evidence.
- Targets: `NCK3`, generated `NCF-*` nodes.
~~~~
