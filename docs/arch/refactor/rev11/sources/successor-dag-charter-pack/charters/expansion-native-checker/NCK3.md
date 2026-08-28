<!-- unified-charter-v2
id=NCK3
name=Shared-proof semantic diagnostic rule kernel
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
predecessors=NCK2,D8,LRA0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,flowslice,diagnostic_action_service
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
charter=charters/expansion-native-checker/NCK3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK3 - Shared-proof semantic diagnostic rule kernel

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the shared-proof diagnostic rule kernel that plans fact demands, reads authoritative relation/call/flow/contextual/declaration facts, emits stable diagnostics and fix intents, and proves that no rule re-resolves semantic meaning. Only representative canary rules land here; catalogue parity belongs to generated NCF nodes.

The current owner is **scattered hard-coded checks, provider diagnostic messages, framework-specific validation, and prospective checker walkers**. The final and sole owner is **a static, typed, demand-declared diagnostic rule kernel over shared semantic proofs**.

## Architectural role and end state

NCK3 is the semantic checker engine in the narrow sense: not a resolver, but a rule planner/evaluator over existing facts. It establishes one reusable execution contract so every generated family slice implements semantic rules without forking infrastructure.

## Expected production surfaces

- `crates/verter_diagnostics` for rule descriptors, emission, suppression identity, and compact diagnostic construction
- `crates/verter_semantic` for read-only fact/proof views and typed rule demands
- `crates/verter_session` for query dispatch integration, rule planning, and exact read-set capture
- `crates/verter_actions` for typed fix intents, not direct edits
- `crates/verter_session/tests` and semantic fixtures for no-second-resolver guards and canary rules

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `DiagnosticRuleDescriptor`, `DiagnosticRulePlan`, and static `DiagnosticRuleRegistry`
- `FactRequirement`, `RuleApplicability`, `RuleBudget`, and `RuleExecutionContext`
- `DiagnosticFactView` exposing typed relation/call/flow/contextual/declaration results only
- `ProofRef`, `DiagnosticEmitter`, `SuppressionKey`, and `FixIntentRef`
- `RuleExecutionReceipt` with facts read, work counters, and completeness

## Exact predecessor contracts

- **NCK2:** consume scoped diagnostic queries, typed batches, same-key admission, and bounded stores.
- **D8:** consume complete authoritative flow/call/contextual results and completion algebra.
- **LRA0:** consume profile-scoped rule/action registration, provenance, suppression, and authored fix safety contracts.

External custody: none beyond the package activation boundary.

## Binding architecture

- Rules declare fact requirements before execution. Applicability and demand planning must permit zero work for irrelevant rules.
- The fact view exposes final typed outcomes and proof references, not mutable semantic stores or resolver callbacks.
- A negative relation or failed call applicability becomes a diagnostic through a rule; the rule does not rerun relation or overload matching.
- Control-flow diagnostics consume existing reachability, completion, return, capture, and narrowing facts.
- Rule registration is static/catalog-driven. Executing arbitrary third-party code inside the semantic engine is out of scope.
- Suppressions are keyed by stable rule/subject/provenance identity and cannot hide diagnostics from unrelated authorities.
- Fixes are semantic intents that LSO7 later materializes against authored current source.

## Internal subblocks

### NCK3-SB1 - Static rule descriptor and registry

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

### NCK3-SB2 - Demand planning and applicability

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

### NCK3-SB3 - Read-only shared fact and proof view

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

### NCK3-SB4 - Diagnostic emission, proof, and dedup

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

### NCK3-SB5 - Suppression and fix-intent boundary

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

### NCK3-SB6 - Representative canary rules and one-engine guards

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

## Data, identity, invalidation, and publication laws

- A rule result is complete only when all declared required facts are complete on the same basis.
- Rule execution order does not affect diagnostic identity, ordering, or read-set signature.
- Rules cannot mutate semantic facts or write index state.
- Framework-owned rules enter through NCK5 descriptors/contributions but run on the same kernel.
- Proof references are opaque stable handles with lifecycle tied to the batch/store generation.

## Migration and cutover

- Move only representative checks whose fact authority and exact replacement can be proven.
- Leave lint rules in LRA0/LNT ownership and provider semantic families external until their generated NCF slice is certified.
- Delete a displaced hard-coded rule only in the same candidate that routes its complete demand and output through the kernel.

## Deletions

- Delete canary-equivalent ad hoc semantic checks and duplicate rule registries.
- Delete direct TextEdit construction from semantic checker rules.
- Delete any checker-private relation/call/flow helper introduced during implementation.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Rules parsing source text, regexing type text, synthesizing/reparsing TypeScript, or walking types to reproduce resolver decisions.
- Dynamic third-party rule code in the trusted semantic process.
- Message-text dedup or range-only suppression.
- Rules emitting LSP coordinates or provider handles.
- Expanding NCK3 into the full diagnostic catalogue.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK3-AC-FACTS:** every canary diagnostic is traceable to declared authoritative facts and exact read-set entries.
- **NCK3-AC-NO-RESOLVER:** static architecture guard rejects resolver calls and duplicate semantic algorithms in rule modules.
- **NCK3-AC-ZERO-WORK:** inapplicable rules execute no fact demand, provider call, or allocation.
- **NCK3-AC-CANARIES:** four cross-family canaries pass oracle, incremental, cancellation, and proof tests.
- **NCK3-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK3-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK3-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK3-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Rule planning cost is proportional to applicable registered rules for the selected profile/slice, with catalog indexing preventing global scans.
- Repeated warm canary checks allocate only the returned batch representation or share it according to NCK2 policy.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `cargo nextest run -p verter_diagnostics -p verter_actions -p verter_semantic -p verter_session`.
1. Static one-engine guards and rule-registry generation tests.
1. Canary differential, zero-work, incremental/fresh, cancellation, and proof-lifecycle tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK4 manifest/oracle generation and NCK5 framework rule ingress.
- Supplies the sole diagnostic rule execution contract for every generated NCF slice.

## Source reconciliation

- `docs/arch/native-checker.md` diagnostics-from-facts and named guard sections.
- D8 flow/call authority and LRA0 rule/action contract.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
