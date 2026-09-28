# Verter Semantic Signature Kernel — Revision 4.1

**Status:** consolidated implementation contract; supersedes V4 with targeted hardening, not a new architecture.  
**Date:** 17 September 2026.  
**Baseline reported by the proposal:** Verter `1ba2d15c8`; verify the implementing checkout.  
**Scope:** shared signature discovery, ordered intersection reduction, relations, substitutions, calls/constructors, Awaited, existing flow integration, incremental correctness, and read-hot storage.  
**Ordering decision:** load-, query-, allocation-, cache-history-, and schedule-independent semantic ordering and generated type output are mandatory. Exact TypeScript union ordering is not required.

## 1. Decision, scope, and evidence boundary

**Implement one graph-native semantic signature kernel, shared by calls, constructors, utility inference, runtime Awaited, lib conditional inference, and existing flow.** This is the next implementation contract, not a proposal to build a parallel typechecker or to postpone those consumer migrations.

This revision supersedes V4 in full while retaining its authority structure and V0–V8/V9 delivery boundaries. It adopts the intent of the latest review's four hardening suggestions and the owner's clarified requirement: determinism is mandatory regardless of physical loading and future parallel execution; matching TypeScript's incidental ordering is not. The main refinements concern policy ownership, carrier preservation before normalization, complete result-demand identity, causal compatibility evidence, and end-to-end schedule independence. These are Verter design decisions, not claims that the attached review or upstream specifies every detail below.

### 1.1 Decisions

| Concern | Decision |
|---|---|
| Intersection authority | Rename `SemanticMeet` to `ReduceIntersection`. It is not a lattice meet. |
| Intersection inputs | Represent ordered construction and meaningful evaluation boundaries explicitly; keep simple operands inline and intern only larger/structured recipes. Do not preserve every syntax parenthesis as a permanent semantic barrier. |
| Union order | Mandatory deterministic `VerterStableV1`, with lazily cached semantic views and the same representation-only ordering before any order-sensitive union reduction. No production clone of the TypeScript comparator. |
| Authored precedence | Preserve meaningful declaration/overload/intersection order, derive it from logical program facts rather than arrival, and preserve the prescribed reduction grouping even under parallel execution. |
| Policy ownership | One immutable policy set selected by SemanticContextId; private family-key projections derive concrete policies. Callers cannot supply contradictory versions. |
| Output determinism | Existing type generators/serializers participate now; future declaration emit inherits the same gate. Sorting final text is not a substitute for deterministic semantics. |
| Substitution | Descriptors own declaration instantiation; result reads receive an immutable call substitution in the descriptor's binder space. Compose through the existing substitution authority in an explicitly defined direction. |
| Complete outcomes | Inline value plus compact evidence handle; diagnostic/recovery details are shared cold records. No per-read owned vectors or Arc cloning. |
| Relations | Extend the existing graph-native relation authority for the structural domain required by these consumers. Identity, Subtype, StrictSubtype, Assignable, and Comparable remain distinct modes. |
| Options and globals | Actual effective options and atomically published complete script/lib/augmentation populations are prerequisites, not permanent refusal branches. |
| Storage | Keep V3's private, replaceable append-only backend adapter and inline Empty/One results; start with a verified, pinned `boxcar` backend. No hand-written unsafe container for speculative speed. |
| Lifetime | Epoch-safe handles and measured real reclamation behavior are required. Full live-graph compaction is a separate, measurement-gated block, not automatically a semantic-cutover prerequisite. |
| Release boundary | All existing call/construct, signature-utility, Awaited, async-return, and relevant flow consumers use the shared kernel. |

The long-term aim is a faster and more capable TypeScript semantic engine. This train establishes foundations for that aim; it does not claim to implement every future checking feature, prove universal soundness, or establish that Verter already outperforms TypeScript.

### 1.2 What was actually verified

**V4.1 review boundary:** the latest advisory review and the exact supplied V4 were read. The official TypeScript 6.0 `stableTypeOrdering` documentation and selected moving upstream source were consulted for the ordering concern. No Verter implementation, current PR-head audit, new TypeScript probe, random-corpus reproduction, or concurrency/performance benchmark was executed in this amendment. V4's prior probe remains historical evidence with its original limits. [A3, A4, W7]

Microsoft's documentation describes encounter-order IDs affecting union/property ordering and declaration emit, and content-based ordering addressing the native port's parallel execution. That corroborates the class of risk; it does not verify Verter's current implementation or require copying an upstream comparator. [W7]

The supplied V2 document really does report 9,300 random unions, 56,548 observations, and the 289-row grid. The later review says its author had a different, shorter document. That is a version/provenance discrepancy, not evidence that the larger report was invented. The exact available files are hashed in Appendix A and the companion manifest. [A0, A1, A2]

The 9,300-case harness and raw observation corpus were not supplied with these Markdown documents and were not executed during this review. V2's undefined-erasing parameter normalization and ten Awaited residuals remain reported limitations, not completed acceptance evidence. V0 must recover and reproduce the original artifacts, or generate and identify a new replacement corpus without pretending it is the original one. [A0, evidence section]

A small local sanity check was executed using the available **TypeScript 5.8.3** installation. It verifies six call/ReturnType/transparent-grouping outcomes and two reduction-state observations. Its script, source, compiler/library hashes, and output are in the evidence bundle. It is **not** validation against the claimed 7.0.2 oracle, not reproduction of the random corpus, and not a Verter benchmark. [P0]

Public TypeScript documentation and the currently retrieved Go source were used to cross-check order-sensitive inference, overload precedence, intersection construction, and the richer upstream comparator. The retrieved version source reports `7.1.0-dev`; these observations do not silently replace the stated 7.0.2 compatibility target. Public PR #575 was consulted for D12 scope, not to certify every implementation detail on its live head. [W1–W6]

### 1.3 Readiness

The latest reviewer recommends four contract tightenings, then freezing the architecture rather than starting another redesign. V4.1 follows that direction. Its policy solution intentionally uses one context-owned policy set plus derived leaf keys, rather than removing policy information from parents that still depend on it. Its order guarantee applies before information loss and through generation, not only to the final member iterator. [A4]


This document is ready to hand to the orchestrator as a dependency-ordered implementation plan. Start with V0 and the independent foundations. Semantic cutovers must wait for their executable contracts; do not turn a prose report or this architecture review into a passing golden. The orchestrator can execute the architectural choices resolved here without reopening those alternatives; newly proposed language-semantic changes still require the owner's approval. Newly discovered behavioral ambiguities are resolved by the evidence/policy protocol in section 5, not by privately adding a fallback implementation.

## 2. Authorities, ownership, and compact outcomes

Keep all semantic work behind the existing query dispatcher, substitution engine, dependency recorder, and typed obligation runtime. TypeInfo is a projection. No consumer may render/raise types and then privately reconstruct signature or intersection semantics.

```text
SignaturesOfType(subject, Call | Construct, SemanticContextId)
    -> QueryOutcome<SignatureSetRef>

ReduceIntersection(IntersectionInputRef, IntersectionPurpose, SemanticContextId)
    -> QueryOutcome<TypeRef>

SemanticUnionMembers(UnionRef, SemanticContextId)
    -> QueryOutcome<OrderedTypeListRef>

Relate(source, target, RelationKind, RelationContextId)
    -> QueryOutcome<RelationDecision>

ReadSignatureResult(descriptor, CallSubstitutionId, ResultDemand,
                    ResultEvaluationContextId, SemanticContextId)
    -> QueryOutcome<SignatureResultRef>
```

`SemanticUnionMembers` can be a compact internal memo; it must still obey evidence and publication rules. Its private key is `(carrier-qualified UnionRef, derived SemanticOrderPolicyId, derived OrderDomainId)`. The public call does not accept independent policy overrides. `IntersectionPurpose` names a closed semantic operation, not a caller-selected algorithm version; section 4 owns concrete policy selection. `ReadSignatureResult` forces existing FlowReturn/return/effect obligations and reuses their work. `ResultDemand` is a closed projection vocabulary (return, predicate/assertion effect, or both), not a license for private body analyzers. A body obligation's contextual typing, captured environment, and flow inputs must be in its existing demand identity; descriptor-plus-substitution is not enough when those inputs differ.

Union construction and union reduction remain in shared algebra. Their closed policy vocabulary distinguishes no subtype elimination, literal reduction, and the required subtype/strict-subtype reductions. Do not let callers invent arbitrary boolean combinations or confuse these operations in memo keys.

Semantic dependencies are recursive: relations inspect signatures, signature matching asks relations, and returns can depend on calls. Use the existing typed SCC/obligation machinery and verify its contract. A semantic function graph is not necessarily a DAG just because implementation blocks form a DAG.

### 2.1 Logical contract

A complete outcome contains a value, complete dependency evidence, any established invalidity/diagnostic recipe, and any defined recovery provenance. Incomplete reasons include cancellation, exhausted operational resources, unsettled inputs, unsupported semantics, and unresolved obligations. An empty candidate list is a complete negative fact, never the generic representation of those failures.

A closed symbolic type or deferred result recipe can be complete **as that representation**. It is not permission to report a complete signature enumeration when the actual candidate set remains unknown. Language-defined error recovery can be cached with evidence; implementation gaps cannot become `any`, `never`, Empty, or an invented definitive diagnostic.

### 2.2 Physical representation

Use a compact handle-based outcome, conceptually:

```rust
struct Ready<T> {
    value: T,
    evidence: OutcomeEvidenceId,
}

// Shared immutable metadata; not embedded/cloned at each read.
struct OutcomeEvidence {
    proof: DependencyProofId,
    diagnostics: DiagnosticRecipeSetId,
    recovery: RecoveryProvenanceId,
}
```

Use reserved IDs for empty diagnostics, no recovery, and genuinely context-free evidence. A source-backed fast path reuses its existing evidence handle or records the required dependency in the caller's collector; it does not allocate an otherwise identical evidence record on every read. Do not claim that all One results have empty proofs.

Measure Ready, the outer QueryOutcome, hot family keys, and persisted memo records, not just SignatureSetRef. A 24-byte set plus a small handle is a layout target; actual Rust enum layout and supported platforms determine measured size.

Diagnostic records attached to type-only reusable queries are location-independent recipes, or use appropriate declaration-relative stable locators. Materialize a call-site message/span at the call-site demand. Reusing a synthesized signature at two sites must not reuse the first site's error location. Keeping diagnostics cold may defer formatting, **not** deciding validity.

Proofs, diagnostics, and recovery metadata live in the same lifecycle contract as their consumers. Fast paths bypass unnecessary lookup and allocation, not evidence, cancellation, taint, or accounting. Request-local builders reuse scratch capacity; no per-edge owned Vec or Arc refcount is required on the read-hot path.

## 3. Identity, descriptors, provenance, and substitution

### 3.1 Four distinct identities

Keep these concepts separate:

1. Exact identity of a stored representation.
2. Checker type-identity relation, including its operation-specific equivalences.
3. Signature comparison under a particular matching mode and binder mapping.
4. Authored or derived origin, which can affect ordering, body obligations, diagnostics, and invalidation.

A relation returning Identity=true does not authorize replacing one representation everywhere with the other. An `Eq<...>` conditional-type probe is not, by itself, a direct measurement of the compiler's internal identity routine.

Hash-cons pure shapes. Preserve checker-visible alias, declaration, anonymous-origin, and instantiation distinctions in explicit carriers where they remain observable. Never attach whichever origin arrived first to a globally shared shape node and treat it as authoritative thereafter.

### 3.2 Representation

Use the following conceptual split; keep the existing SemanticNodeId carrier where appropriate rather than mechanically adding wrappers at every use site:

```text
SignatureInputShape {
    kind,
    binder_declarations,      // constraints AND defaults
    this_parameter,
    parameter_layout,
    declared_minimum,
    signature_semantic_flags
}

SignatureTemplate {
    input_shape: SignatureInputShapeId,
    result_recipe: SignatureResultRecipeId
}

SignatureDescriptor {
    template: SignatureTemplateId,
    declaration_environment: DeclarationInstantiationId,
    residual_binders: BinderSpaceId
}

SignatureCandidate {
    signature: SignatureDescriptorId,
    provenance: SignatureProvenanceId
}

SignatureSetRef = Empty | One(SignatureCandidate) | Many(SignatureSetId)
```

The initial layout goal remains a 16-byte candidate and a 24-byte small result on supported 64-bit targets. Measure and assert actual layouts, including the outer result envelope and family keys. These figures are targets, not substitutes for measurements.

`SignatureResultRecipe` is a closed immutable domain:

```text
Declared(return_type, predicate_or_assertion)
Body(return_obligation_key)
UnionCommon(representative, ordered_mapped_constituents)
UnionSynthesized(master, ordered_mapped_constituents)
IntersectionConstruct(base_constructor, ordered_mixin_results)
```

The declaration environment is stored once on the descriptor; there is no second persistent Instantiated result-recipe mapping. Instantiation is a descriptor-construction operation. Keep a compact hot header for kind and context-independent declared shape facts; cache context-dependent effective arity/shape views under their correct demand keys. In either case, ordinary shape access must not traverse diagnostic provenance. Benchmark duplication of such header fields against extra table indirections.

A body locator is a semantic dependency. Two different bodies cannot share a result recipe merely because their parameter shapes coincide. Their input shape can still be shared. No recipe may retain a transaction pointer, mutable inference context, or a closure into a current stack frame.

Descriptors and recipes provide semantic information. Provenance provides origin information. Do not force consumers to reconstruct semantic binder mappings or return obligations by interpreting a diagnostic provenance tree.

### 3.3 Provenance and ordering metadata

Preserve declaration-group identity, declaration-parent identity, source/overload ordinal, source locators, and origin relationships through instantiation and synthesis. Cache the effective overload-order metadata so reading it does not require walking a provenance chain.

Treat literal-specialization and other signature flags according to their semantic ownership and propagation rules. Call-site optional-chain state remains in the call context or a call-site overlay, not permanently baked into a reusable declaration signature.

Composite constituent sequences retain arm identity and repeated contributors. They are not deduplicated like a canonical set. Use explicit mapped edges rather than an ambiguous `SignatureSetId` for both a result list and a per-arm construction history.

Build composite edges once in a small temporary builder, or use a shared immutable composition DAG. Never allocate every prefix of a left fold. Diagnostics may traverse cold provenance; ordinary parameter access, representative selection, and last-signature inference must not.

### 3.4 Derived candidates and warm admission

Replace authored-occurrence-only call admission with dependency-rooted admission. A synthesized candidate is admissible when its descriptor, operand origins, environment, substitutions, and all consumed body obligations have a complete stable dependency proof. A genuinely unrooted or incomplete result remains transaction-local.

Do not invent an authored occurrence for a synthesized signature. Preserve synthetic identity and evidence explicitly. This change is necessary for the new signatures to participate in warm call/flow reuse rather than only improving Awaited. V3 reported an authored-origin admission restriction at the supplied baseline; verify its current location during V1 before replacing that restriction. [A1; S2]

### 3.5 Instantiation and inference are different operations

A descriptor represents a declaration already placed in a particular immutable declaration-instantiation environment. It owns the mapping of captured/outer parameters and any required alpha-renaming into its residual binder space. Shape projections and result recipes must agree on that environment. Do not expose a remapped parameter shape paired with an unmapped result recipe.

`CallSubstitutionId` represents explicit/inferred/finalized type arguments for the descriptor's remaining binders. It is immutable and binder-space-qualified. An explicit `Identity`/symbolic substitution is available for non-call consumers; it must preserve ownership of every remaining binder. It is not the mutable inference solver, nor a universal map keyed by parameter spelling.

Define composition by application, never just by an ambiguous `M1 ∘ M2` symbol:

```text
apply(compose_after(first, second), T)
    = apply(second, apply(first, T))

result_at_call(base_result, descriptor_map, call_map)
    = apply(call_map, apply(descriptor_map, base_result))
```

For example, if a declaration binder X maps to the descriptor's residual binder U, and call inference maps U to string, the base return X must become string. The reverse direction leaves the wrong binder behind. Constraints, defaults, receiver types, rest elements, returns, predicates, and assertion effects use the same capture-avoiding composition contract.

Use the existing substitution authority exactly once for each effective base-value/environment demand. The composition equation specifies applying maps to a type value; it does not authorize evaluating a body under a different contextual typing or flow environment. Do not first instantiate a return as part of descriptor construction and then apply that same map again in ReadSignatureResult. Result records carry their binder-space domain. Body obligations evaluated under the composed environment may already return in descriptor/call space; that path must not then pass through a blanket second mapping. For composite edges, compose constituent-declaration → constituent-residual → representative-residual → call mappings in that order, using typed domain/codomain IDs and dropping genuine identities.

Normalize nested instantiation requests to one base template and a composed descriptor environment when valid. Retain provenance history separately. Remove identity maps. Keep descriptor indirection bounded independently of edit/instantiation history.

Do **not** force every substitution composition to eagerly expand all mapped types. Use shared persistent environment maps or normalized substitution DAGs with per-type/per-environment memoization, lazy projection onto referenced binders, and chain-depth control in the existing substitution authority. Otherwise flattening each prefix can replace pointer chasing with quadratic copying. Exact interning means exact normalized recipe equality, not solving arbitrary semantic equivalence of type functions.

Required invariants: applying a composed map equals applying its components in the defined sequence; no double substitution; no capture of a same-spelled binder; no escaping temporary inference variable; no repeated traversal of the historical descriptor chain on an identical warm result read. Cold work remains proportional to demanded bindings/types; no constant-time claim is made for arbitrary type substitution.

Speculative inference that legitimately needs intermediate reads stays in an inference-local, versioned context and cannot publish globally reusable answers as finalized. Only frozen maps over valid binder regions participate in cross-request admission.


### 3.6 Complete result-demand identity

The actual memo contract is explicit:

```text
ReadSignatureResultKey {
    descriptor: SignatureDescriptorId,
    call_substitution: CallSubstitutionId,
    projection: ResultDemand,
    evaluation: ResultEvaluationContextId,
    semantic_context: SemanticContextId,
}

ResultEvaluationContext =
    ContextFree
  | Identified(existing_immutable_return_or_body_context_id)
```

`Identified` is a compact reference into the existing body/return demand system, not a new body analyzer or a mutable inference object. It identifies every contextual-typing input, captured semantic environment, relevant flow/narrowing state, receiver context, and wrapper/result mode consumed by that evaluation unless that fact is already represented transitively by the descriptor or substitution. Existing demands with complete identity can be reused directly. Do not duplicate large environments inside this key.

Construct the identified body/result demand before looking up this memo. Constructing an identity must not analyze the body. `ContextFree` is legal only when no such inputs can affect the requested result, not merely because the caller omitted them. A speculative body/result demand remains inference-local until all referenced binder regions and substitutions can be admitted.

For composite recipes, the evaluation context and explicit mapped edges must identify the appropriate constituent demands; two distinct contextual body evaluations cannot be collapsed by sharing a template. Declaration-map/call-map application remains exactly as specified in section 3.5. This field does not introduce another substitution pass.

Content versions and consumed source facts remain dependency evidence; do not add a changing global content generation to every result key. Conversely, dependency invalidation cannot repair two simultaneously valid contextual demands aliased to the same key.

Gate paired cases that share descriptor and frozen generic arguments but differ only in contextual expected signature, captured binding/receiver, or relevant flow state. Their correct distinct results must coexist. Equal complete demands must reuse work; shape-only and context-free annotated results must not force body evaluation. Verify Return, Effects, and Both projections without dropping dependencies or validity.

## 4. Contexts, options, and query keys

Resolve compiler options once per project configuration, including `extends`, defaults, the `strict` umbrella, explicit overrides, and valid/invalid combinations. Canonicalize effective values rather than configuration spelling. Fix production plumbing and the reported strictness encoding before semantic cutover. V3's baseline audit is a finding to revalidate, not a substitute for inspecting the implementing checkout. [S3]

### 4.1 One authoritative semantic policy selection

Use one immutable `SemanticPolicySetId`, selected when forming the semantic context:

```text
SemanticContext {
    effective_semantic_options,
    resolver_library_project_environment,
    policy_set: SemanticPolicySetId,
    existing_material_R_T_L_J_axes,
}

SemanticPolicySet {
    compatibility_version,
    union_order: SemanticOrderPolicyId,       // VerterStableV1 initially
    intersection_policies_by_purpose,
    other_existing_semantic_policies,
}
```

An ID is an interned immutable exact value, not a mutable slot whose interpretation changes under the same ID. Context is the sole owner of the policy selection. There is one production default; the policy machinery is not a per-consumer preference surface.

The public `SemanticUnionMembers(union, context)` derives a minimal private key:

```text
SemanticUnionMembersKey {
    union: carrier-qualified UnionRef,
    policy: project_union_order(context.policy_set),
    domain: project_order_domain(context),
}
```

This key does not also store the whole general context. The derived domain contains only facts actually needed to interpret stable identities; it is not the set/rank of objects currently loaded. Private constructors perform the projections, so `context = V1` plus an independently supplied `order = V2` is not a representable public request.

`ReduceIntersection(input, purpose, context)` uses `(input, purpose, context)` as its initial compact/interned demand identity. A purpose selects a defined language operation under the policy set; it is not a duplicate concrete policy. Nested `EvaluateSubgroup` terms store the purpose, not an independently supplied policy version. If a future family-specific projection is introduced, it must include **all** transitive policy dependencies of that reducer, not just its intersection rule version.

Higher-level signatures, relations, calls, and results retain the policy-set identity through their semantic contexts or an equivalently complete checked projection. Changing an order policy changes the identities of parents that can observe it. Invalidating only the union-view memo while retaining an old `SignaturesOfType`/call result is unsound. Include a parent-cache policy-change regression test.

This adopts the review's single-owner requirement without deleting the information required by transitive memo consumers. Identifying the bundle at a parent and projecting its relevant member into a private leaf key is not two independent policy selections. [A4, suggestion 1]

### 4.2 Options, evidence, and execution identity

Include every semantic option and environment fact read by these operations, with project isolation and the existing relevant R/T/L/J axes. Parse/body context, flow, and substitution demands remain on the families that actually need them. Parameter optionality, relations, result evaluation, and order views use consistent effective options.

Intern contexts by exact equality after hash lookup. Digests accelerate lookup, not semantic equality. Runtime IDs are not persistent cache identities; serialize versioned logical/content identities for cross-process caches.

Do not add the current content revision or a changing project-wide edit counter to every persistent query key. Content freshness is validated by dependency proofs. An unrelated edit must not invalidate the entire semantic cache. Use compact/interned family keys rather than repeatedly hashing the largest enum envelope; preserve registry/audit coverage and measure both representations.

In-progress work additionally requires a compatible read snapshot, epoch, and speculative-inference domain. The absence of a global edit counter in persistent keys does not permit old-snapshot producers to serve new-snapshot waiters.

Display/emission preferences have their own output contexts and do not alter resolution. A semantic policy change, unlike a formatting change, invalidates affected parent and leaf semantic identities. Persisted schemas include the applicable policy/encoding versions. No test-only alternate interpretation of production options is permitted; causal order injection uses an explicitly isolated test namespace.

## 5. Deterministic semantic ordering and type generation

### 5.1 Four responsibilities, one end-to-end guarantee

| Ordering | Owner and contract |
|---|---|
| Physical membership | Private storage; optimize locality, membership, and interning. Allocation order is not observable precedence. |
| Semantic union traversal | Shared representation-only stable keys and `SemanticUnionMembers`, under `VerterStableV1`. The same policy governs union reductions that choose representatives before a finished union exists. |
| Authored precedence | Preserve the language's overload/declaration/intersection sequence and relevant evaluation grouping, obtained from logical program facts rather than task completion. |
| Presentation/generation | Deterministic rendered types, generated type artifacts, naming, and serialization under fixed output options. No feedback into semantic resolution. |

The guarantee is an implementation requirement, not a selectable slow/strict mode. Correctness cannot depend on serial node allocation or on loading the whole project in a fixed physical order.

Order is not solely presentation. The supplied V2 reports changes to calls and utility results when callable intersection operands are reversed; official documentation describes last-signature inference and overload-group precedence. Preserve these meaningful orders instead of sorting every sequence indiscriminately. [A0; W1, W2]

### 5.2 Exact determinism contract

Let `I` contain the logical program and dependency contents, explicit authored/configured precedence, resolved options/libraries, policy versions, and the fully identified query demand. Let `S` be a legal loading, querying, allocation, cache-history, or execution schedule. For completed observations:

```text
Observe(I, S1) = Observe(I, S2)
```

For fixed emission/display options and logical path mappings, generated type artifact bytes must also agree. Observations include applicability, inferred returns/effects, narrowing, selected representatives, settled diagnostics, ordered signature/union projections, and generated type text—not merely set-equivalent answers.

Permissible schedule changes include physical file-read/parse/lower completion order, lazy versus preloaded modules, request entry point, hover-before-call versus call-before-hover, unrelated query prewarming, randomized interning/hash-table iteration, worker count, work stealing, cancellation/retry, persisted-cache loading, and epoch rebuild/compaction. Request IDs, progress-message timing, performance counters, allocation indices, cache layout, and resource exhaustion timing are not promised identical.

Compare completed results. Cancellation or an exhausted operational budget is explicit incompleteness, not an alternate inferred type or accepted language diagnostic. Do not admit a provisional result as complete because a later-arriving declaration or relation was unavailable. Do not erase meaningful field/order differences when normalizing runtime IDs for tests.

**Logical order and physical arrival are different.** Reordering source overloads, ordered intersection operands, or a configured root/reference sequence with specified precedence changes `I`. Completing the same files/tasks in a different order does not. Directory enumeration and file-read completion cannot define a program's semantic declaration order. Preserve configured semantic sequences; normalize only unordered discovery inputs, through the existing program-order authority.

**Pair locality:** the comparison of unchanged member keys A and B cannot change merely because C was interned, an unrelated file was loaded, or another query ran first. Inserting a genuinely unrelated declaration must not renumber intrinsic/literal order or change comparisons whose logical key inputs are unchanged. Do not promise unchanged order across arbitrary declaration renames, changed binders, or source edits that change those key inputs.

Equivalent relocated checkouts must agree when logical resource identities, resolution results, and path mappings agree. Different resolver case/symlink/package-instance semantics are not assumed equivalent merely because display paths look alike.

### 5.3 Preserve carriers before normalization loses them

Adopt the review's carrier invariant at the **union constructor**, not only at the final iterator. A semantic `UnionRef` must remain carrier-qualified. Pure type shapes can be shared independently; the union's semantic identity/key must retain any immutable carrier/origin set needed by ordering, signature representation, diagnostics, or generation. A shape-only union key cannot identify distinct observable carrier populations. [A4, suggestion 2]

Audit explicitly:

```text
union member dedup identity
    != general Relate(Identity)
    != display equality
    != pure shape equality
```

An exact repeated semantic member can be removed by its specified construction rule. Other carrier elimination requires a named normalization rule with executable evidence; do not run an unbounded universal-observer equivalence proof at runtime. Preserve evidence for eliminated inputs and enough provenance for the specified diagnostics even when a legitimate language reduction removes their value contribution. This is not a prohibition on all union simplification or an instruction to keep every alias forever.

Where a reduction can select a representative, eliminate a carrier, choose a binder, or synthesize an ordered intersection, supply its defined order **before** that decision. It must use the shared stable-key comparison or a proven order-independent reduction. Sorting the surviving union afterward cannot repair an origin/representative already discarded by first-arrival deduplication.

Keep exact intrinsic/domain simplifications and identity-based membership fast paths cheap. Do not sort an operation proved insensitive to iteration. However, never pass arena-ID storage order into an order-sensitive normalization merely because the finished union view is built later.

Do not add a global mutable list of every source that ever touched a pure shape. Origin-sensitive records are immutable, scoped to their construction/demand, and have the normal dependency/lifetime contract. Concurrent publication may select either allocation of an **identical complete record**; it cannot choose between observably different first-writer origins.

### 5.4 Stable keys independent of loading and resolution progress

Use one production `VerterStableV1`. Its category/sub-tag table and encodings are versioned and explicit, not Rust enum discriminants or host-native layouts. Keys describe representation identity, not full semantic type equivalence.

| Key domain | Required inputs |
|---|---|
| Intrinsics/sentinels | Fixed distinct tags; no source or allocation ordinal. |
| Literals | Canonical scalar values and literal kind, with explicit scalar edge-case handling. Their key never changes because a declaration first mentions the same literal. |
| Authored carriers | Logical source-unit identity, declaration/owner/role anchor, and bound arguments where relevant. |
| Anonymous authored types | Reproducible owner/syntax-role anchor; not an AST allocation ID or editor-history-only identifier. |
| Binders | Stable owner/recursive region plus binder position/role; not a global fresh counter or parameter spelling alone. |
| Synthetic types | Closed normalized constructor/descriptor recipe, stable input references, typed binder anchors, and relevant policy identity. |

A logical source-unit identity comes from the resolver's stable project/library/package-instance namespace and logical resource identity. A temporary checkout path, OS file handle, file-table insertion index, package-cache location, or enumeration rank is not a substitute. Distinct resolved package instances cannot be collapsed merely because name/version text matches. Virtual SFC/generated units use a stable originating source plus virtual role/identity, not a worker-generated temporary filename.

For named declarations, prefer owner/name/role paths and a local ordinal only where the declaration language needs disambiguation. Keep whole-file content hashes and edit versions as freshness evidence rather than ordering all declarations by them. Reproducible source positions can disambiguate a fixed snapshot, but do not treat history-dependent editor anchors as cold-rebuild identities. Anonymous source edits can legitimately change an anchor; the mandatory guarantee concerns fixed logical inputs and pair locality when keys remain unchanged.

Synthetic unordered child collections normalize as sets of stable keys. Ordered collections preserve their semantic sequence. A commutative accumulator alone cannot establish exact set identity, and a digest collision never authorizes merging distinct records. Irrelevant diagnostic history, old allocation chains, and which task supplied an input first are excluded.

Use a fixed, versioned stable fingerprint as an accelerator or arbitrary deterministic primary order within a category, with exact normalized-key comparison on collisions. Endianness, strings, scalar encoding, domain separation, and fingerprint policy are explicit. The whole pair `(fingerprint, exact key)` defines the order when fingerprints are used as a primary. A randomized map seed is permitted for storage buckets but never for semantic sorting. Hash functions and exact-key schemas cannot silently change under `VerterStableV1`.

Two distinguishable members cannot compare equal and inherit input-arrival order from a stable sort. Either interchange is unobservable for the specified carrier domain, or the exact key is missing a discriminator. No final fallback to NodeId, pointer, worker ID, table ordinal, timer, or request counter is allowed—even for rare kinds or collision cases. Inject full collisions and equal-prefix adversaries in tests.

Keys are immutable once published for an identified carrier/recipe. A lazy type is ordered by its closed recipe/anchor, not by whichever subset of its result happens to be forced. When an operation requests a settled type, use the evaluator's complete result; do not mix a partly resolved candidate population with a completed one. A missing stable key is not permission to guess or use allocation order. Ordinary in-scope forms must obtain valid anchors before release; operationally unsettled work remains incomplete.

Recursive representation references terminate at explicit source/recursive-binder anchors or closed deterministic derivations. Local binder numbering must not depend on SCC discovery/DFS completion order. This need not solve arbitrary graph isomorphism: preserve the defined representation's owners and binder roles. Comparators may not force bodies, resolve names, run subtype/identity queries, instantiate new semantic values, or recursively call union reduction. Stable representation-key construction is below those semantic authorities and can be demanded lazily without a dependency cycle.

### 5.5 Parallel execution must preserve semantic selection and grouping

An ordered list is insufficient if the algorithm subsequently accepts whichever candidate task completes first. Parallel execution may evaluate independent facts concurrently, but candidate selection must follow semantic precedence. A later successful candidate cannot be committed until all earlier candidates that could win are conclusively rejected or the language's selection rule otherwise proves that result.

The same principle applies to union representatives, inference-candidate accumulation, declaration merging, recursive obligation results, and cached negative/positive publication. Logical source/argument priority is preserved; completion order is not a new inference rule. Fixed-point computations must use a demonstrated schedule-independent result protocol or their defined deterministic strategy; a racing cycle escape is not a valid answer.

Ordered `ReduceIntersection` is not freely associative. Keep its input recipe's evaluation tree/stages fixed. Do not turn `((A & B) & C)` into `A & (B & C)` by folding completed tasks or using a work-stealing tree reduction. Parallel child evaluation is permitted; the parent receives the specified values in the specified positions. Optimized regrouping requires an established equivalence for that domain, not just an associative container API.

Build global contributor/declaration snapshots from logical membership and precedence before publishing affected complete queries. Use per-symbol populations and dependency boundaries, not an unconditional whole-program scheduling barrier. No consumer uses the subset of contributors that happened to finish first as a complete surface.

This is a deterministic-**result** architecture, not a deterministic scheduler. Keep concurrency and allocation flexible. Do not serialize the interner, all queries, or all file loading solely to make tests pass.

### 5.6 Lazy views and measured cost

Physical union membership may retain compact arena-local order. Restrict raw iteration to audited order-insensitive operations. Use the same representation-only key service for pre-normalization ordered choices and finished `SemanticUnionMembers` views; do not build two competing comparators.

Construct needed multi-member views in request scratch and cache by carrier-qualified union, derived policy, and minimal order domain. Fetch immutable key headers once per batch. Reuse existing stable identity material; no universal 16-byte-per-node column, cryptographic hash on every read, or repeated structural traversal is mandatory. Empty/One and proven scalar paths retain their allocation-free targets.

A resident, valid warm view is not sorted again. Eviction or epoch replacement can require rebuilding, but must reproduce the same order. Measure key-construction work, exact-comparison bytes, cold and warm sorts, collision paths, closure/binder work, retained view bytes, and end-to-end costs. Do not claim the added determinism is free; avoid repeatedly paying for it.

### 5.7 Generation, names, properties, and serialization

Every existing type-text generator, virtual-type producer, public type serializer, diagnostic formatter, and signature-help renderer must use an appropriate deterministic observation order. Future declaration emit inherits this contract before release; this amendment does not require inventing a full emitter now if none exists.

Generation also needs deterministic declaration/member traversal, alias representative selection, imports, synthetic names, recursive labels, and symbol/property projections. A union comparator alone cannot fix these. Preserve authored/member precedence where it affects inference or meaning. For an unordered generated member collection, use stable member identity and a fixed output policy. Do not reorder runtime object initialization, evaluation, or other runtime JavaScript for cosmetic determinism.

Synthetic names derive from logical owners/roles or stable emitted-node keys, with exact collision disambiguation. When counters are required for compact local names, allocate them in deterministic emission traversal within the output scope—not global query/discovery order. Preplan conflicting names within the required output scope so first publisher does not win a short name. Named recursive references use deterministic binder/emission labels.

Declaration output is not merely a pretty printer. It must preserve the signatures, overload/intersection precedence, and intended exported contracts of the semantic result. Where emit is supported, reparse/re-resolve exported observations and check the intended correspondence; separately test the pinned external TypeScript consumer where compatibility is claimed. A different display sort cannot silently change a generated callable contract. Order-policy differences remain explicit through generation and downstream consumers.

Compare canonical semantic observations **and exact generated type artifact bytes**, under fixed formatting/line endings/logical path mappings, in separate tests. Do not sort emitted declarations or signatures in the test normalizer to hide drift. Normalize only specified transport metadata, temporary runtime handles, or configured path relocation; not semantic order or generated names. Persisted semantic record identities use stable encoding; the physical cache file/hash-table layout need not be byte-identical.

### 5.8 Semantic-difference ledger as release authority

Keep four classes: exact agreement; presentation-only difference; demonstrated `VerterStableV1` order-induced difference; and independent semantic difference/incompleteness. Only the third is covered by the owner's custom-order authorization. Consistency is necessary, not a proof of correctness. [A4, suggestion 4]

For an order-induced admission, retain a minimized input, pinned options/compiler/model/implementation hashes, ordered carriers/signatures, the first differing order-sensitive decision, and all affected applicability/return/effect/diagnostic/output observations. Demonstrate causality by changing **only semantic union traversal order** in the same semantic implementation, with authored precedence, inputs, reductions, and options otherwise fixed. Run these counterfactuals in isolated cache namespaces (including parents); changing a leaf order while reusing old parents proves nothing.

The changed order must reproduce the attributed effect; restoring `VerterStableV1` must recover the recorded Verter observation under scheduling perturbations. Inspect the intermediate observations enough to exclude an independent mismatched optionality, relation, binder map, recovery rule, or body context. Merely finding a union in a failing program is not evidence. If the cause cannot be isolated, the mismatch remains unclassified and fails the relevant gate.

Record presentation-only differences separately and verify no binding/callable effect. Other language changes need their own explicit specification/approval. No catch-all allowlist, dynamic golden refresh, or hidden second production algorithm is permitted. A test-only order hook is an experiment over one implementation, not a production fallback.

### 5.9 Mandatory determinism matrix

| Perturbation | Required comparison |
|---|---|
| Same files arrive in forward/reverse/random order | Completed signatures, representatives, inference/effects, diagnostics, and emitted bytes agree. |
| Lazy load versus eager preload; hover/call/emit queried in different orders | No discovery-history effect. |
| An unrelated type/literal is interned or queried first | Unchanged pair ordering and output are preserved. |
| Worker counts 1/2/4/8, randomized delays/work stealing, duplicate publishers | Same result; no first-completed overload or changed fold tree. |
| Cold, warm, partial resident cache, edit/revert, cancelled/retried work, epoch rebuild; a partially loaded persisted cache once one exists (conditional) | Same final logical snapshot gives the same observations and output. |
| Hash seed changes, equal prefixes, forced full stable-fingerprint collisions | Exact ordering and carrier identity remain correct. |
| Duplicate shapes with distinct authored/synthetic origins, anonymous/virtual units, recursive binders | No early carrier loss or discovery-based anchor. |
| Contextual body demands with the same descriptor/type arguments | Correctly distinct coexisting result memo entries. |
| Policy changes with resident parent caches | Parents and leaves use the new policy; formatting-only changes do not rebuild resolution. |
| Logical checkout relocation with fixed mappings and equivalent resolver facts | Stable semantic observations and configured generated bytes. |
| Authored overload/intersection order is deliberately changed | Relevant outcomes may change; the harness must detect, not normalize away, meaningful order. |
| Same output requested with different preceding unrelated outputs | Synthetic names/recursive labels do not depend on a global emission counter. |

**Conditional axis.** The persisted-cache axis of the cold/warm/cache row applies only once the product persists a semantic cache across processes (any semantic query result, node, signature record or proof serialized for a later process). Until then it has no subject and the row is proven over the cache lifecycle the product has: cold and warm, a partial resident cache (a bounded memo past its retention bound), edit/revert, cancelled/retried work and epoch rebuild. The change that introduces cross-process persistence MUST, before it lands, activate the axis: a determinism driver that persists from one process, loads a partial cache in a fresh process, and replays the same logical snapshot, with the same observations and output on both bases.

Attach the earliest divergent intermediate carrier/order/reduction/demand to a failed replay to make the cause localizable. Register seeds and perturbations in V0. V2/V3/V4/V5/V6/V7 each prove the relevant part; V8 runs the combined vertical. If the existing runtime cannot yet run every semantic path concurrently, test the storage/publication primitives and an adversarial completion-order harness now; enabling parallel semantic dispatch later is blocked until that real path passes the same matrix. Do not claim a sequential harness proves an unimplemented scheduler.

## 6. ReduceIntersection and explicit construction inputs

### 6.1 Name and authority

Rename SemanticMeet to `ReduceIntersection` across the query key, family registry, contracts, counters, docs, and consumers before the API spreads. It owns checker intersection reduction; a future mathematical flow lattice can expose its own true meet operation without this naming collision.

Raw interning stays private to algebra/storage. Name the storage primitive `intern_ordered_intersection` or an equally explicit name. Do not leave a generally callable `canonical_intersection` that sorts by node ID and appears to be an alternative semantic authority.

### 6.2 Represent the evaluation operation, not the full syntax tree

Use a compact input reference:

```text
IntersectionInputRef =
    Empty
  | Unary(TypeRef)
  | Binary(TypeRef, TypeRef)
  | Recipe(IntersectionInputId)

IntersectionRecipe =
    OrderedOperands([TypeRef])
  | OrderedSteps([IntersectionTerm])

IntersectionTerm =
    Value(TypeRef)
  | EvaluateSubgroup(IntersectionInputRef, IntersectionPurpose)
```

These are conceptual schemas, not claims about Rust layout. `EvaluateSubgroup` means evaluate that subgroup for its purpose under the context-selected concrete policy before supplying its result as an outer operand. A previously evaluated result can simply be Value(TypeRef), with its evidence retained through the query protocol; it does not need to retain all historical construction steps forever.

A recipe may carry a specific semantic preservation kind only where an actual reduction rule requires it, such as a policy-distinguished empty-object/literal-preserving construction. That distinction must be a typed field participating in relevant identity. A free-form boolean, source-span trick, or first-writer tag is not sufficient.

**A subgroup boundary is not an opaque type.** Its result can subsequently be flattened, distributed, or reduced as the outer operation permits. The point is to preserve the necessary evaluation stage and order, not to prohibit all later simplification.

Do not assume every parenthesis, alias, or source span creates a permanent semantic barrier. Remove syntax-only wrapping. Flatten only groups proven transparent under the operation's closed policy. Preserve nested evaluation when reducing a subgroup before the outer group can affect the result. [A2, grouping recommendation; W3]

The historical 5.8.3 probe illustrates why this distinction matters:

```ts
type L<T extends string> = (number & T) & { x: 1 };
type R<T extends string> = number & (T & { x: 1 });
```

In that probe L is a Never type; R remains an unreduced intersection representation. This is a reduction-state witness, **not a proof that R has inhabitants or that these are different abstract sets**, and it must be rechecked against the chosen oracle. The ordinary callable grouping probes in the same script produced equal outcomes. [P0]

### 6.3 Fast path and key normalization

The binary wrapper must not allocate an input recipe or hash-cons a two-element Vec before trying exact, proven scalar, or memo-hit paths. Unary/binary/flat-list forms normalize to one key representation when they denote the same evaluation operation. Zero-, one-, and two-term OrderedOperands lists normalize to Empty, Unary, and Binary; a two-term sequence containing a meaningful subgroup does not collapse to a flat Binary.

Normalize input recipes syntactically under proven rewrite laws before memoization. Do not run general semantic Identity merely to make an input key. Larger/structured inputs use an interned recipe ID, avoiding a variable-length hot family key. Preserve purpose and semantic context in the key; the context selects the concrete policy exactly once (section 4).

Use one ordered n-ary kernel with a request-local builder for flat groups. Do not eagerly materialize every prefix intersection. Parallel work cannot change the prescribed fold/grouping merely by finishing in a different order (section 5.5). At a meaningful subgroup boundary, evaluate/reuse the subgroup through the same authority, then feed its value/evidence into the outer kernel.

### 6.4 Reduction and identity

Preserve operand order. Remove repeated construction-identical members by first occurrence where the reduction rules allow it; do not deduplicate by the general structural Identity relation. Type-node deduplication, signature-equivalence matching, and proof merging are three different operations.

Retain the full ordered reduction sequence: never/any/error behavior, nullability, literal/domain reductions, discriminants, constraint reductions, and supertype elimination at their specified stages. A list of algebraic identities is not sufficient to determine their interaction. Do not transplant every pass from the old sorting constructor without auditing it.

An immutable form/preservation tag participates consistently in equality/hash. Completion under an environment/policy is carried by evidence, not stamped as mutable first-writer state on a shared node.

### 6.5 Work and expansion

Use domain masks, literal indexes, repeated-member checks, and exact scalar cases before general distribution. Retain factored representations only when a demand can consume them with the same established semantics and dependency proof. Do not advertise a complete concrete query merely because a symbolic product node could be allocated.

Charge actual work through the shared request budget; avoid interning rejected pairs and prefix histories. Retain dependencies used to eliminate operands even when those operands do not appear in the output.

Keep baseline diagnostic complexity thresholds in the versioned semantic policy, not in the fundamental arena/index representation. Increasing supported complexity is a deliberate later policy improvement, not accidental acceptance caused by forgetting a check. General materialized output can still have product size; avoid unnecessary products rather than claiming impossible constant-size explicit output.

## 7. Graph-native relations and recursive obligations

Extend the existing relation engine rather than creating a signature-private comparator. Keep Identity, Subtype, StrictSubtype, Assignable, and Comparable distinct. Do not implement one by silently forwarding to another.

V3 reported that the supplied baseline refuses Subtype and StrictSubtype; V1 verifies the current dispatch before extending it. Its existing coinductive/obligation machinery is the starting point, not something to replace with unconditional “already visited means true.” [S1, S3]

Required coverage for this train includes the structural type forms already emitted by supported signature and flow operations: primitives/literals; unions and intersections; constrained parameters; objects with inherited properties, optionality, visibility and index signatures; callable and constructable objects; arrays/readonly arrays; tuples and variadic rests; instantiated generics; and recursive combinations of these forms.

Mapped, conditional, indexed, or imported forms are settled through the existing semantic evaluator. Where a legitimately symbolic form remains, preserve its symbolic meaning. Concrete in-scope relations must not remain Unknown merely because the implementation stopped at nominal equality.

Use graph-native worklists and reusable per-request scratch storage. Avoid raise-to-TypeExpr/clone/compare/lower round trips. Use exact-node and safe kind-negative fast paths, but never let node equality bypass incompleteness or missing evidence.

Binder comparison must remap constraints, defaults, parameters, receiver types, returns, and predicates consistently. Matching performs a pure comparison, not a mutable inference transaction. Assert that instantiated outputs contain no escaped binder references and that every retained generic reference is owned by an explicit binder region. Relation keys include every material mode and mapping, either explicitly or through fully instantiated operands.

Discharge mutually recursive structural obligations with the existing typed SCC rules and publish only completed results. Keep regular recursive type comparisons distinct from invalid recursive thenables and flow fixed points; they are not the same cycle policy.

A request-local failed attempt can be reused to avoid immediate repeated work, but it is not a globally cached semantic negative. In ordered matching, an unresolved earlier match cannot be skipped when doing so could change the selected representative.

## 8. Signature shape and candidate construction

### 8.1 Shared positional model

Replace, rather than blindly move, the existing private arity helpers. Provide one shape accessor with explicit modes for signature comparison, applicability, tuple projection, and utility inference.

The acceptance contract covers declared minimum versus effective minimum; declared optional parameters versus synthesized optional positions; strict-null behavior; optional tuple elements; array rests; fixed tuple rests; variadic middle segments and required tails; receiver exclusion from positional arity; void-sensitive arity; generic rest instantiation; defaults; and the supported JavaScript signature modes. Preserve parameter names for diagnostics/signature help without using them as type equality accidentally.

The historical tagged checker confirms that defaults, predicates, and additional arity modes exist beyond the abbreviated pseudocode. Re-measure their target behavior rather than borrowing 5.9.3 results blindly. [S7]

Represent a parameter's optional-value semantics explicitly. Two equal-looking optional positions must not share a shape if one adds `undefined` under the active context and the other does not. Optionality is not reconstructed from a missing source span.

### 8.2 Subjects

A direct signature returns One only when its stored kind matches the requested kind. A direct construct signature is not an unconditional Empty; the baseline represents such nodes explicitly. [S2]

Read object and interface signatures from the existing resolved surface, including inherited/merged declarations and visibility of implementation signatures. Do not collect only own declarations or concatenate independently indexed buckets without respecting their ordering authority.

Type parameters use established constraint semantics. Apparent primitive and collection handling uses the resolved global symbols and bound generic substitutions. A built-in interface name is not a license to consult an unrelated shadowing declaration.

Unconstrained dynamic-call permission is a call-resolution rule, not a fabricated ordinary signature. Keep Function/CallableFunction and any-related special cases explicit and verify behavior in composite subjects against the pinned oracle.

Alias and instantiation settlement is a shared worklist with dependency tracking; “unchanged after one expansion” is not itself proof of emptiness, failure, or a cycle.

### 8.3 Unions and intersections

Use revision 2's two-phase union procedure as the readable starting point: common matches first, then the restricted synthesis path. Do not add a Cartesian product of overload choices. Use VerterStableV1 for union-arm traversal while preserving the required first-match rules, authored signature precedence, mapping direction, and return-check modes. [A0: union algorithm]

Complete the protocol around that procedure: defaults, predicates/assertions, receiver comparison direction, instantiation mapper composition, propagated signature flags, and the target's apparent/array-member fallback rules. Constructable composites and mixin constructors are required coverage, not a permanent refusal branch. Upstream source review identified these missing categories; the target oracle fixes their exact behavior. [S4, S7]

Use order-preserving candidate indexes for exact-match rejection and arity/generic compatibility filters. A hash hit still requires exact comparison. Subtype matching retains the defined candidate precedence: an optimization cannot move a later candidate ahead of an earlier possible match.

An intersection's signature list uses the target's signature-equivalence deduplication with the correct representative/provenance retained. It is not unconditional concatenation and is not type-intersection structural deduplication.

Do not eagerly compute composite returns for consumers that need only parameter shapes. Force return/effect recipes exactly when the matching or consumer semantics requires them—for example, a generic exact-match check that includes returns.

### 8.4 Demand-driven results and precise evidence

Candidate enumeration may complete with a closed body-return recipe without inspecting its body. Record the declaration/shape and locator dependencies required to establish that descriptor; ReadSignatureResult records body-content, captured-context, and result dependencies when it forces them. A body-only edit need not invalidate unrelated parameter-shape work, but must invalidate every consumed body result.

Generic exact matching that compares returns is a genuine result demand, not an exception to evidence tracking. An unresolved earlier candidate cannot be skipped merely to find a later successful match. Pure filters reject only proven impossibilities and preserve precedence.

Composite predicates/assertions are not obtained by blindly OR-ing or AND-ing constituent effects. They follow the same shared inference/effect rules as other signatures, and must be validated at the call-to-flow boundary.


## 9. Global contributors and incremental publication

Move population maintenance to artifact ingestion, not to individual semantic reads. Maintain per-file contribution facts and reverse indexes keyed by resolved global symbol identity. Include library files, top-level interfaces in script files, `declare global`, applicable module augmentations, and script/module classification.

`noLib` disables automatic libraries; it does not discard global declarations supplied by the program.

Publish a population only when its membership, file versions, module classification, and indexed contributions describe the same immutable program snapshot. A changed-file queue alone is not a proof of completeness.

Use this publication protocol:

```text
pin program snapshot S and expected population revision E
read changed membership/artifact facts for S
build the contribution delta outside reader-visible state
verify all required artifact versions belong to S
atomically publish the immutable population snapshot complete at E
otherwise abandon publication and leave the newer demand incomplete
```

A concurrent edit cannot be lost between draining a queue and setting a Complete marker. Cancellation, missing artifacts, and failed lowering cannot set Complete. Readers either use a coherent old snapshot or the coherent new snapshot, never a mixture.

A lookup records the relevant symbol's contributor fingerprint, including a proved empty contributor set. Adding/removing the first declaration invalidates prior negative reads. Edits to an unrelated file must not invalidate every augmented primitive signature.

Remove both whole-program `known_canonicals()` scans from the lookup path. Cache reusable generic lowering/substitution results. Preserve declaration merging order, which is not simply lexical sorting by global name. Obtain that order from the existing logical program/declaration authority; do not append contributors in file-read, lowering, lock-acquisition, or queue-drain order and treat arrival as precedence. Stable normalize unordered discovery inputs without overriding configured/authored sequences.

An order view that reads declaration precedence or logical source identity records those exact facts. Semantic union order does not replace declaration-merging order. A reader pinned to an older coherent snapshot may finish; delivery to a newer request must still obey snapshot/version validation.


## 10. Runtime Awaited, lib Awaited, and flow integration

### 10.1 Runtime protocol

Keep an explicit thenability state machine that distinguishes non-thenable values, valid adopted values, malformed thenables with defined recovery, and operationally incomplete evaluation.

Preserve the relevant receiver eligibility checks, dynamic cases, recursive adoption policy, and the correct union-reduction relation. The upstream promised-type implementation has more stages than “Empty means invalid thenable.” [S4]

Use the shared signature authority at the then-member and fulfillment-callback steps. Keep callback parameter extraction, nullish exclusion, and recursion in the runtime protocol. A Promise fast path requires resolved declaration identity and complete dependencies, not only the spelling `Promise`.

### 10.2 Lib conditional protocol

Lib `Awaited<T>` follows conditional-type inference and distribution. It must not reuse the runtime merged-union callback answer as a shortcut. Preserve the proposal's u1/u7/u8/o4 distinction. [A0: runtime/lib protocol and evidence]

Make the non-union assertion local to the distributed inferred-callback-arm step. Do not forbid every union signature query anywhere in evaluation of a lib conditional; an earlier `then` member or a nested type can itself be a union.

Any intrinsic optimization for the standard alias must validate the resolved declaration/library identity and be observationally equivalent to the normal conditional authority. User-defined or replaced library declarations continue through that same normal evaluator, not a second fallback implementation.

### 10.3 Calls and flow

`ResolveCall`/construct resolution must consume the shared candidate list, then apply call-site ordering, applicability/inference, and return/effect demand. Per-arm winners may be retained for explanation when semantically appropriate, but may not remain a competing union-call acceptance algorithm.

Carry predicates and assertions through candidate instantiation and into existing flow narrowing. Reuse FlowReturn for body-derived results and async wrapping. Enumeration must not force every candidate body unless the matching semantics genuinely requires its result.

Flow facts and a checker-ordered intersection are distinct abstractions. A flow solver's join/fixed-point operation must not assume that this ordered reduction is a commutative lattice operator. Where existing flow creates a checker intersection, it must call the shared authority with the relevant order and policy.

Preserve the complete flow/context/substitution identity of call and return demands. Activating non-empty narrowing inputs requires the corresponding key/evidence change; never reuse a sealed-empty flow key for context-dependent answers.

Cut over the existing callable classifier, callable views, signature utility projections, and flow intersection producers before the train is accepted. A thin adapter may remain only if it delegates without semantic decisions.

Keep call-site validity, diagnostics, return type, and narrowing effects distinguishable. A runtime Awaited intrinsic representing the await protocol must never be rebound by the spelling of a user type alias; a lib Awaited conditional remains evaluated through the ordinary alias/conditional authority. Do not turn the local distributed-arm invariant into a global ban on union queries during conditional evaluation.


## 11. Read-hot storage, concurrency, and lifecycle

### 11.1 Initial backend and publication

Retain a private append-only interner adapter, initially backed by a verified, pinned boxcar release using safe APIs. V3 proposed this backend; V0/V2 verify its actual version, target support, MSRV, initialization behavior, and measured fit before relying on it. The correctness contract belongs to Verter's adapter, not to a claim that any crate is intrinsically fastest. [A1]

Keep Empty/One inline. Larger candidate slices, descriptors, provenance, substitution environments, and evidence records use appropriate separate immutable tables. Hash outside shard locks, confirm equality on collisions, fully initialize records before publishing handles, and never hold an interner lock while dispatching semantics.

Use only published handles; do not infer that every index below a concurrent length is initialized. Checked overflow, duplicate publication races, producer cancellation, and panic paths must not expose holes as values. Sixteen dedup shards is an initial benchmark parameter, not an ABI or correctness constant.

### 11.2 Request-scoped borrowed reads

A SemanticReadView pins the compatible graph epoch and snapshot once per request and borrows immutable records. No Arc clone or global proof/node mutex is permitted per candidate or positional parameter read on the targeted warm path. Epoch ownership can be paid at the request boundary; compact local indices can be used inside a validated view.

Instrument memo lookup, validation, input-recipe access, ordering, graph access, descriptor/provenance/evidence access, substitution, and forced bodies separately. A lock-free signature table does not establish a lock-free whole query.

Use worker-local counters with aggregation. Avoid one contended atomic increment on every hot read. Reuse scratch builders and give retained scratch capacity a policy; scratch high-water is real memory.

### 11.3 Coalescing and inference locality

Coalesce compatible in-progress work under the query key **and execution snapshot/epoch/inference domain**. The persistent memo can remain dependency-validated without a global content generation in every key. Sharing a producer across incompatible snapshots is not a valid optimization.

Waiting integrates with typed recursive obligations, cancellation, and producer ownership. Never build an independent map of blocking futures that deadlocks on recursive calls/relations. Cancelling one waiter does not invalidate complete work needed by other active waiters; partial results remain non-admissible.

Provisional inference variables and partial proofs stay inference/request-local. Completed symbolic values must have explicit stable binder ownership; a temporary solver slot is not such a binder.

### 11.4 Required lifetime contract

Tables are append-only **within an epoch**, not immortal across the life of an editor. Every exposed handle is either epoch-qualified or usable only through a lifetime-validated epoch read view. External stable locators are not recycled arena offsets.

V2 must establish roots, ownership, cache eviction, epoch retirement, cancellation/drain behavior, and all new records' participation in that lifecycle. Roots are live project state, intentionally retained cache results, and live readers—not every historical interner entry. No source file, stale proof, or provenance edge may secretly retain an entire retired project forever.

Release requires a real way to retire obsolete storage under sustained edits. Existing graph/region retirement can satisfy this. A coherent graph-epoch replacement that rebuilds from authoritative program artifacts and lazily recomputes demanded semantics can also satisfy it, provided its latency/throughput/memory gates pass. It need not relocate all live handles or preserve every warm cache. Old pinned readers finish against their old epoch; new requests use the coherently published replacement.

A project-close-only reset is not sufficient when an indefinitely open edited project grows without a bound. Merely calling the design “reclamation-ready” does not waive this requirement.

### 11.5 Full compaction is a separate engineering block

Full live-graph compaction/remapping can preserve more warm work but also changes graph, descriptor, proof, cache, and handle machinery together. Do not require it before semantic cutover merely because it is the eventual storage optimization.

V9 owns full compaction or a more efficient regional reclamation mechanism when measurements justify it. It becomes **pre-release mandatory** if V8 cannot pass the registered memory budget, edit-soak/retired-epoch bound, or reclamation tail-latency gate with the simpler mechanism. Otherwise ship the unified semantics with working bounded lifetime, and schedule V9 independently according to measured value.

Memory budgets and benchmark workloads are registered in V0 before observing the new implementation. Count payload, map capacity, slab slack, recipes, provenance, proofs, order views, caches, scratch, and retained old epochs. Report memory pinned by intentionally live readers separately. The required plateau is after obsolete readers and evictable caches are released, not an impossible promise to free live references.

## 12. Work accounting and performance gates

Use the existing connected-work budget. Distinguish requests, cache hits, producer executions, relation obligations, actual pair expansions, and allocated bytes. A warm request still exists; the invariant is that it does not rebuild its completed result.

Do not add private recursion/width caps that silently change semantics. Operation-specific target diagnostics belong to the compatibility policy; cancellation and resource exhaustion are typed operational incompleteness.

Hard structural gates:

| Path | Required property |
|---|---|
| Complete direct Empty/One read | No result heap allocation or candidate-table access; common evidence protocol retained. |
| Repeated complete signature/intersection query | No producer rebuild or new semantic records, aside from explicitly cold diagnostics. |
| Shared body-obligation consumers | Reuse completed return/effect work under the same full demand. |
| Augmented type lookup | No whole-program scan; no unrelated contributor invalidation. |
| Composite construction | No eager overload Cartesian product; no quadratic prefix provenance copying. |
| Concurrent repeated demand | Coalesced computation without recursion deadlock or partial publication. |
| Editor edit/revert soak | Live memory plateaus under the registered workload after retired views are released and the implemented retirement policy runs. |

V0 establishes pinned-hardware release baselines. Report p50/p95/p99 latency for cold project load, warm queries, local edits, declaration/augmentation edits, and cancellation/restart; throughput at 1/2/4/8 workers; allocations; retained memory; and completion/diagnostic agreement rates.

Use the old Verter baseline for regressions and the pinned official compiler for equivalent completed work. A typed gap is not a fast success. Do not compare a partial Verter query to a complete TypeScript project check and label the ratio a compiler speedup. Report intentional VerterStableV1 semantic differences separately from exact-agreement workloads.

Adopt a 5% regression investigation gate for matched pre-existing end-to-end workloads when the difference exceeds the benchmark's measured noise. This is an initial engineering gate, not a prediction. A regression does not get waived because a microbenchmark improved; record the cause and obtain an explicit performance decision. Newly supported work must have its absolute cost and amortization reported separately.

The long-term speed advantage should come from less demanded work, graph-native comparisons, compact keys, lazy recipes, fine invalidation, reusable evidence, and cheap borrowed reads—not from optimistic acceptance, global leaks, or skipped semantic cases.

Additional hard gates: a repeated ready result read performs no historical descriptor-chain walk; simple binary intersection input takes no recipe allocation; a resident valid previously built semantic union view is not sorted again; rendering preference changes rebuild no semantic query; diagnostic materialization at a second call site uses that site's location without rerunning unrelated type work.

The full determinism/generation matrix in section 5.9 is release-gating for touched production paths. Do not game determinism tests by making node allocation serial. Perturb worker scheduling, interner seeds/arrival order, file-ingestion completion order, and failed/cancelled retries. Equivalent completed requests must agree even when their performance differs. Benchmarks include order-heavy generic unions and long descriptor-instantiation sequences, not just direct signatures.

For full-check comparisons, pin the official compiler's actual version/binary and comparable options, library scope, diagnostics demand, and work completed. The 5% threshold above is an investigation trigger for matched pre-existing work, not a promised speedup or permission to suppress new correct work.


## 13. Implementation train and repository touchpoints

Amend the D12 charter explicitly. V0–V8 are the semantic-authority train; V9 is conditional storage work. Foundations can land incrementally, but after a consumer cutover there is one production semantic implementation. Shadow comparisons, alternate iteration orders used to diagnose mismatches, and reference models belong in tests.

V3 inspected the supplied `1ba2d15c8` baseline in `semantic_query.rs`, `project_semantic_dispatch/{dispatch_txn.rs,relation.rs,canonical_algebra.rs,build.rs}`, and `semantic_query/flow_return_result.rs`. These are starting touchpoints, not a claim that the live PR has not changed. New names below are proposed module boundaries. Do not create a module simply to match the spelling if the repository already has its rightful authority. [A1]

### V0 — Evidence lock and acceptance registration

**Depends on:** nothing.

Check in this contract, exact input hashes, chosen compiler binary/source/lib hashes, harness/model hashes, seeds, raw observations, semantic serializer, causality-gated policy-difference ledger, and benchmark definitions. Register section 5.9 scheduling/history perturbations, exact generated-output checks, and the full reachable stable-key variant/encoding table. Audit union member deduplication before order views, logical source/virtual-unit identity, and declaration precedence before semantic cutover. Register memory limits, soak workloads, and relative/absolute performance measurements before implementation results are known. Pin legal/licensing attribution for adapted upstream code.

Recover the reported corpus from the originating worktree/repository if available. If it is unavailable, generate a new identified corpus against the verified target and keep the prior report labelled unverified; do not manufacture a reproduction claim or block all independent foundations indefinitely. The stated 7.0.2 target is not verified until its actual artifacts are identified.

**Gate for semantics depending on evidence:** reproducible unnormalized observations; all required cases have explicit expected behavior or an approved versioned policy difference. Include union-valued then, construct/mixin, defaults/predicates, both substitution stages, preserved/transparent groups, and all reported residual families. “100% of the model's printable outputs” is not the gate.

### V1 — Options, contexts, outcomes, and admission

**Depends on:** V0 contracts; independent infrastructure may proceed while corpus work continues.

Implement real effective tsconfig option plumbing, exact context interning, single context-owned immutable policy sets with private leaf projections, complete body/result demand identity, compact family keys, Ready/Incomplete separation, compact evidence/diagnostic recipes, and dependency-rooted synthetic admission. Bump incompatible cache schemas. Verify carried baseline audit findings rather than relying on stale line numbers.

**Gate:** option matrix, effective-config equivalence, cross-project isolation, policy projection/parent-cache invalidation, contextual result-demand separation, warm invalidation, separate call-site diagnostic locations, no incomplete/provisional inference result admitted, complete direct Empty/One paths preserve evidence without result allocation.

### V2 — Records, substitutions, and epoch-safe storage

**Depends on:** V1.

Implement input shapes, descriptors, explicit binder spaces, result recipes, normalized declaration maps, frozen call substitutions, mapped constituent edges, provenance, inline set cardinalities, and request read views. Establish actual lifetime ownership/retirement support. Keep full compaction separate unless measurements already require it.

**Gate:** measured layouts, exact recipe interning, schedule-independent logical carrier/binder identities, no observable first-writer state, capture-free composition laws, no double substitution, bounded descriptor indirection, warm mapping reuse, valid body identity, safe concurrent publication, stale-handle rejection, and no per-candidate global lock/Arc clone.

### V3 — Complete global contributor snapshots

**Depends on:** V1; can run alongside V2.

Maintain per-file script/module/global/lib contribution facts at artifact ingestion. Publish coherent complete populations atomically; retain per-symbol positive and negative dependencies. Delete lookup-time whole-program scans.

**Gate:** adding/removing the first contributor, script-to-module changes, renamed symbols, noLib/user globals, option/library changes, cancelled lowering, concurrent updates, forward/reverse/random artifact completion with stable logical declaration precedence, and no unrelated-symbol invalidation.

### V4 — Semantic order, ReduceIntersection, and relations

**Depends on:** V1 and V2's representation contracts.

Implement restricted raw-membership APIs, representation-only stable keys, carrier-preserving union normalization, lazy VerterStableV1 views and exact collision handling. Supply stable order before any order-sensitive representative elimination; preserve input-defined reduction grouping under parallel child evaluation. Rename the intersection authority, add inline/recipe inputs with stage-preserving semantics, and implement graph-native required relations using existing recursive obligations. Audit construction identity versus relation identity versus signature matching.

Subblocks can be: representation audit; stable key/view foundation; primitive relation/reducer foundation; ordered recipe evaluation; structural/binder relation closure; SCC interaction and required union reductions. Signatures and relations have runtime recursion, so tests must cover that even though implementation proceeds in blocks.

**Gate:** order laws, pair locality, complete variant/collision coverage, load/query/history/schedule determinism, display independence, grouping preservation without syntax-history retention, no structural-intersection-dedup shortcut, no assignability-as-subtype shortcut, no in-scope structural Unknown workaround, collision injection, and no prefix/product allocation explosion.

### V5 — SignaturesOfType and demand-driven results

**Depends on:** V2, V3, V4.

Implement one positional model and one discovery authority for Call/Construct over required direct/apparent/constraint/inherited/alias/union/intersection subjects. Add common-match and restricted synthesis union phases, array-member categories, constructor/mixin semantics, result forcing, and predicate/assertion propagation. Preserve authored candidate precedence; union-arm traversal uses VerterStableV1.

**Gate:** raw semantic candidate observations, binder/default/receiver/rest/optionality correctness, correct representative and mapped constituent provenance, lazy bodies, no escaped inference variables, distinct empty/incomplete cases, correct precedence despite delayed earlier candidates, complete contextual result keys, and causally proven—not merely labelled—custom-order differences.

### V6 — Calls, construction, utilities, and flow cutover

**Depends on:** V5.

Migrate ResolveCall/ResolveOverloadSet/construct resolution, callable views/classification, signature utilities, flow intersection producers, and return/effect consumers. Keep contextual applicability/inference in its existing owner. Remove independent per-arm union-call acceptance and private callable/intersection synthesis.

**Gate:** dependency-rooted synthetic results warm, both call sites get correct diagnostics, generic return maps compose once, predicates/assertions affect flow correctly, body/type/options edits invalidate the right work, annotated/shape-only demands do not force unnecessary bodies, all migrated consumers agree on their common semantic inputs, and existing generated type/virtual output paths satisfy byte-level determinism plus the supported exported-contract checks.

### V7 — Runtime/lib Awaited and wrapper integration

**Depends on:** V5, V6.

Migrate the runtime thenability protocol and both signature-reading steps to shared discovery/shape access. Keep lib Awaited as normal conditional inference/distribution and the runtime intrinsic separate. Preserve existing shared async/generator/async-generator wrapper ownership through FlowReturn and resolved library identity.

**Gate:** union and optional then, incompatible receivers, dynamic callbacks, malformed/non-thenable distinction, recursive thenables, constrained/open generics, custom/noLib definitions, runtime/lib distinctions, and call→return/effect→async→await→flow end-to-end cases. Generator and async-generator canaries must not regress while Awaited changes.

### V8 — Differential, concurrency, performance, lifetime, and deletion

**Depends on:** V0–V7.

Run the actual Rust implementation against the locked observation corpus; add real-project and deterministic replay suites. Publish completion rates, semantic difference ledger, latency distributions, allocation/work profiles, concurrency outcomes, and sustained-edit memory results. Remove obsolete producers and update registries, skills, charters, manifests, and audits.

**Gate:** section 5.9 end-to-end matrix, complete order-difference causal evidence, exact existing generated-type artifacts, zero unexplained mismatches; no ordinary in-scope typed-gap substitute; no stale warm answers; no permanent competing authority; no ordering-dependent flakiness; registered performance investigations resolved; real retirement behavior and memory limits satisfied.

If lifecycle or retirement latency fails, V9 becomes a prerequisite to release, not a waived future concern. Otherwise the semantic train can ship without full live-graph compaction.

### V9 — Measured storage optimization

**Depends on:** V2 lifecycle contracts; uses V8 profiles or earlier equivalent measurements.

Implement graph compaction, regional retirement, or live-result migration only to solve measured memory/latency/cold-rebuild costs. Preserve epoch identity, root closure, record reference remapping, evidence validity, and old-reader safety. Keep it mechanically isolated from semantic changes.

**Gate:** reclaimed bytes and tail latency improve on matched workloads; fresh/warm/replay semantics remain unchanged; no stale IDs or hidden roots; no added per-read tax that defeats the purpose. Required before release only when V8 cannot otherwise pass; not an indefinite exemption from bounded storage.

## 14. Mandatory test matrix

| Area | Cases that must be executable |
|---|---|
| Representation/order | Section 5.9 matrix, carrier preservation before reduction, pair locality, reversed intersections, fixed evaluation grouping under parallel completion, union permutations with fixed origins, relocation, collision injection, named/anonymous/virtual sources, recursive binder anchors, and query/ingestion/worker schedule permutations. |
| Policy/demand identity | Single-owner derived keys, changed policy with parent caches resident, equal descriptor + type arguments under different body/context/flow demands, Return/Effects/Both projections, and identical demand reuse. |
| Generation | Exact type artifact bytes under fixed output options, stable aliases/names/imports/property views/recursive labels, no global emission counter, and supported reparse/exported-contract checks without normalizing away semantic order. |
| Equality/relations | Separate identity/subtype/strict-subtype/assignability outcomes; constraints and defaults; recursive structural objects; optional properties; index and call/construct signatures; private/protected origins. |
| Parameters | Declared vs synthesized optionality, explicit undefined, strict-null toggle, optional tuple elements, empty/fixed/array/middle-variadic rests and required tails, receiver placement, void-sensitive minimum. |
| Substitution | Distinct outer and call-site maps, explicit type arguments, Identity symbolic maps, capture avoidance, defaults referencing other binders, receiver/rest/predicate mapping, normalized nested instantiations, no historical-chain warm traversal. |
| Candidates | Common and synthesized unions, multiple overloaded arms, alpha-renamed binders, generic defaults/returns/predicates, specialized overload order, inherited declarations, direct constructors and constructor intersections. |
| Collections/apparent | Mutable/readonly arrays and tuples, empty tuple, generic Array contributors, same-member array-union fallback, primitive wrappers, Object/object/{}, Function composites, noLib and custom global declarations. |
| Awaited | All supplied grid and spot rows, all ten residuals, union-valued then, optional then, incompatible this, callback any/noncallable/nullish, recursive thenables, runtime/lib divergence, symbolic generic inference. |
| Flow integration | Union/intersection call returns, body-derived and annotated returns, generic substitutions, predicates/assertions, optional calls, async wrapping and await after calls, relevant narrowing inputs and repeated demands. |
| Cache validity | Fresh=warm=replay; independent construction schedules; changed body/type/library/options/order/global membership; two projects sharing declarations; negative-to-positive transitions; failed/cancelled retries. |
| Concurrency/lifecycle | Duplicate intern races, cancelled producer/waiters, recursive single-flight, population publication races, no uninitialized handles, epoch replacement with old reader, snapshot-scoped coalescing, waiter cancellation, ID overflow behavior, sustained unique edits and edit/revert memory plateau. |

The oracle observation format records signature count/order, kind, binders with normalized names but preserved relationships, constraints/defaults, positional types and optionality, declared/effective minima, rest layout, receiver, returns, predicates, diagnostics, and the selected representative. Preserve graph sharing/cycles in the serializer. Do not use TypeScript display strings as semantic identity.

The reported 9,300 cases are a candidate starting corpus only after their artifacts are recovered and verified; otherwise use the newly generated corpus with its own identity. Add adversarial families and larger/more varied generated programs; keep seeds and minimizers in the repository. Acceptance is coverage plus exact behavior, not a favorable percentage on one generator.

Semantic serialization normalizes binder names and epoch-local IDs but preserves relationships, overload/intersection order, optionality, predicates, and relevant provenance. It never erases undefined to turn diagnostic display into a type oracle. Serialize cycles/sharing through stable graph references, not recursive strings that can diverge.

Order-insensitivity is asserted only for operations/policies where it is a law. Do not add a generic commutativity/associativity property test to ReduceIntersection. Use positive transparency cases plus explicit reduction-stage witnesses. Differential tests distinguish union set equivalence from ordered callable effects; sorting all serialized signatures to make a comparison pass is forbidden.


## 15. Deletion and completion contract

Delete NormalizeIntersection and SemanticMeet key/spec/registry/counter names after migration. Remove Awaited-private signature readers and parameter extraction helpers. Delete standalone callable merging, per-consumer union signature synthesis, duplicate utility last-signature logic, and semantic intersection construction outside ReduceIntersection.

Keep raw interning private. Thin adapters may remain only when they make no semantic decision. There is no production TypeScript-order fallback, strict-only shortcut, or authored-only synthetic admission rule. Invalidate incompatible persisted caches and update all family instrumentation and documentation together.

The complete vertical is:

```text
resolved declaration / apparent global / constrained type
  → SignaturesOfType
  → shared positional matching and graph-native relations
  → contextual call/construct inference
  → frozen call substitution
  → shared return/effect demand
  → existing flow and async/generator wrapper authority
  → runtime await OR normal lib conditional inference as appropriate
  → consistent user-visible types, effects, and diagnostics
```

A repeated complete request reuses the corresponding finished work. An edit invalidates the dependencies actually consumed. Semantic ordering and generated type artifacts are stable for the same logical inputs/policies/output options regardless of loading, querying, allocation, cache history, and future parallel scheduling; there is no incidental TypeScript-order requirement. This guarantee starts before carrier elimination and reaches emitted observations. Old storage can be retired without dangling handles. No consumer silently falls back to a private semantic algorithm.

This is the implementation direction recommended by this review. Architecture alone does not establish the world's fastest engine or universal correctness; matched completed-work benchmarks and executable semantic contracts must establish the implementation's performance and behavior.

## Appendix A. Evidence identity and provenance

**V4.1 amendment identity:** the original V4 is A3 (`verter-signature-kernel-v4.md`, SHA-256 `0813708c2a0b2aa4748dc63bede7deac863bc0a98c185fe96287e52aa6f2eea7`); the latest advisory review is A4 (`Pasted markdown(5).md`, SHA-256 `0d9fecf3e61186b82d50aeb0ee46eef8d744f41d04be49c4bfea24f24a758a9f`). The new companion `verter-v4.1-manifest.json` records their exact bytes and the consolidated output/diff. Review-file line numbers refer to its supplied 276-line form.

**§5.9 amendment (23 September 2026, operator-ratified):** the persisted-cache axis of the cold/warm/cache determinism row is conditional on the product persisting a semantic cache across processes. The row is proven over the resident cache lifecycle; the change that introduces cross-process persistence activates the axis before it lands (§5.9, conditional axis).

A4 suggestions 1–4 respectively concern policy ownership, carrier retention, complete contextual result keys, and causal order-difference admission. Sections 4, 5.3, 3.6, and 5.8 implement those intents. Sections 5.2, 5.4–5.7, and 5.9 make the owner's stronger load-/schedule-independence and generation requirement executable. No additional benchmark or compiler execution is claimed for V4.1.

The V4 evidence below is retained as historical provenance, not regenerated evidence:

The prior V4 evidence bundle contains `verter-v4-evidence-manifest.json` and its exact byte hashes. Raw physical line counts below are informational only; text retrieval may count a terminal empty line or add metadata lines. Hashes, not line counts or the phrase “revision 2,” identify an artifact.

| ID | Artifact | SHA-256 | Raw physical lines | Status |
|---|---|---|---:|---|
| A0 | `Pasted markdown(3).md` | `36dea93816275f20fd7a4bda141a4f969e72e7c8c3106511a27cbe0ce550e3cc` | 985 | Supplied V2; claims 9,300 unions / 56,548 observations and 289-row grid; raw corpus not supplied. |
| A1 | `verter-signature-meet-authorities-v3.md` | `bfbd13328c05131462bea4d17f34b84bc8f217cf6c61aa004ffe2415072fa6f1` | 539 | Prior architecture proposal, not implementation evidence. |
| A2 | `Pasted markdown(4).md` | `873393e5a5877eb80b738b42b19512b11b643ebbccd7d8b274bbe4dd21c734f9` | 395 | Attached advisory review; reports seeing a different 645-line predecessor. |
| P0 | `order-probe.cjs`, `order-probe.ts`, `order-probe-results.json` | See manifest | — | Executed illustrative TypeScript 5.8.3 checks only; no Verter execution. |

A0 reports ten Awaited residuals and parameter comparison with undefined removed. Neither this revision nor the local probe closes those cases. The earlier 645-line artifact, 372-measurement harness, full 9,300-union corpus, corresponding 7.0.2 compiler binary/source, and raw reference-model outputs were not verified in this review. V0 obtains or regenerates the necessary evidence with new explicit identities.

### Current external cross-checks

These sources corroborate selected behaviors, not the complete design. URLs are listed as source locators. The retrieved moving Go branch was used for case discovery; it is not the normative locked oracle.

* W1 — TypeScript Handbook, Conditional Types: last-signature inference and conditional distribution. `https://www.typescriptlang.org/docs/handbook/2/conditional-types.html`
* W2 — TypeScript Handbook, Declaration Merging: later overload-group precedence and specialized signatures. `https://www.typescriptlang.org/docs/handbook/declaration-merging.html`
* W3 — Microsoft typescript-go, retrieved `main`, `internal/checker/checker.go`: ordered intersections, reduction stages, ordered union matching, and transparent parenthesized-type lookup. `https://raw.githubusercontent.com/microsoft/typescript-go/main/internal/checker/checker.go`
* W4 — Same repository, `internal/checker/utilities.go`: contextual CompareTypes and ultimate type-ID fallback. V4 does not require this comparator in production. `https://raw.githubusercontent.com/microsoft/typescript-go/main/internal/checker/utilities.go`
* W5 — Same repository, `internal/core/version.go`: retrieved source reports `7.1.0-dev`. `https://raw.githubusercontent.com/microsoft/typescript-go/main/internal/core/version.go`
* W6 — Public D12 PR #575 scope, previously consulted; not re-audited in V4.1. `https://github.com/pikax/verter/pull/575`
* W7 — Official TypeScript 6.0 release notes, “The --stableTypeOrdering Flag,” retrieved 17 September 2026 for V4.1. Documents encounter-order IDs, declaration-output changes, and native parallel-checking motivation. `https://www.typescriptlang.org/docs/handbook/release-notes/typescript-6-0.html#the---stabletypeordering-flag`

### Carried V3 audit references

S1 — V3's pinned `pikax/verter` `1ba2d15c8` relation dispatch audit (`project_semantic_dispatch/relation.rs`).

S2 — V3's pinned `semantic_query.rs` audit: authored-origin admission, direct signature kinds, call keys, and return carriers.

S3 — V3's pinned `dispatch_txn.rs` audit: strictness encoding and relation-cycle machinery. These are carried findings to revalidate at the implementor's actual checkout, not a claim that this review reran those audits.

S4 — Upstream checker categories cited by V3; cross-checked where relevant by W3 in this revision. S7 — V3's historical TypeScript 5.9.3 source cross-check for matching/defaults/predicates/arity; not the normative target expected-output authority.

The implementation evidence lock must replace moving URLs and reported version labels with exact source/library/compiler/harness hashes. Copying this source list does not satisfy V0.

## Appendix B. Checkout binding (Verter `1ba2d15c8` + PR #575 head, verified 17 September 2026)

This appendix binds the contract's proposed names to the live owners at the implementing checkout. The contract's own rule applies: an existing rightful authority is extended, never duplicated to match a spelling. Where the contract names a symbol that does not exist, the row below names the live subject.

| Contract name | Live owner at the checkout | Disposition |
|---|---|---|
| `SemanticMeet` | Does not exist. The intersection authority is `SemanticQueryKey::NormalizeIntersection` (`crates/verter_session/src/semantic_query.rs`), `canonical_algebra::canonical_intersection`, and the dispatch funnel `ProjectSemanticDispatch::intern_normalized_union_or_intersection` (`project_semantic_dispatch/build.rs`). | The "rename" is: introduce `ReduceIntersection`; retire `NormalizeIntersection`; make `canonical_intersection` the private `intern_ordered_intersection` (V4, V8). |
| `stitch_module_augmentations` | `collect_augmentation_contributions` plus `FileArtifactStore::ensure_augmentation_index_populated`. The two whole-program `known_canonicals()` scans are `build.rs::resolve_external_module_augmentation` and `build.rs::runtime_nominal_call_signatures`. | V3 deletes both scans and publishes the population. |
| `crates/verter_semantic/src/analysis/type_expr.rs` | `TypeExpr` lives in `crates/verter_type_expr/src/lib.rs`. | Path only. |
| `SemanticContextId`, `SemanticPolicySet`, compatibility version | Do not exist. Per-family context structs carry `R/T/L/J` env hashes; no policy identity exists. | V1 introduces them. |
| Effective compiler options | `IdeProjectCompilerOptions` (`crates/verter_semantic/src/resolver_core/project_config.rs`) carries no strictness; `verter_workspace/src/engine.rs` hardcodes `type_strict: false`; `StrictFamilyConfig` is fed by the per-host test knob `RelationHostKnobs::strict_family_relax_bits`. `extends` is already resolved by `verter_workspace/src/config.rs::load_compiler_options_inner`. | V1 replaces the knob with effective options and folds them into `type_env_hash`. |
| Authored-origin admission restriction (S2) | Verified live: `semantic_query.rs::AdmissibleCallResult::admits` (type-state seal on `SignatureCandidateOrigin::Authored`) and `build.rs::signature_group_is_rootless` (`cache_suppress`). | V1 replaces both with dependency-rooted admission. |
| Subtype/StrictSubtype refusal (S1) | Verified live: `relation.rs` `RelationKind::Subtype \| StrictSubtype => RelationResult::Unknown`, zero producers. | V4 implements both graph-natively. |
| Arena-order union output | `canonical_algebra.rs` `kept.sort_by_key(\|id\| id.0)` and `project_semantic_dispatch/mod.rs::canonicalize_node_list`; `CompositeOriginCategory` is first-wins under content hash-consing (disclosed in `semantic_query/composite.rs`). | V4 replaces with `VerterStableV1` and carrier-qualified union identity. |
| Signature discovery | No shared authority. Collectors: `build.rs::settle_signature_group` (`ResolveOverloadSet`), `select_signature_function`/`signature_bucket_arity` (utilities), `awaited_call_signatures` + `first_parameter` (Awaited), `apparent_type.rs::callable_anchor`, `walk.rs::value_may_contribute_call_signatures`, `broad_runtime.rs` inline test, `walk.rs::merge_intersection_surfaces_with_graph` (intersection concat). | V5 introduces `SignaturesOfType`; V6/V7 cut consumers over and delete the collectors. |
| Positional model | `call_resolve.rs::call_candidate_arity` (last-required-position rule) versus `relation.rs::relate_function` (non-optional count, no rest, no `this`). `split_this_receiver` exists but is not used by relations or Awaited. | V4 unifies relations; V5 owns the one accessor. |
| Per-arm union call acceptance | `call_resolve.rs` `arm_ordinal`/`arm_states`, `ResolvedCallResult::UnionSelected`. | V6 removes it. |
| Double substitution | `call_resolve.rs` seeds `FlowReturn` with the substitution, re-substitutes the result, re-takes widened, and probes binders: up to three `FlowReturn` executions per generic call. | V6 collapses to one application. |
| Result-demand identity | `FlowReturnKey` has no narrowing/receiver axis; `ResolveCallKey.flow` is pinned `FlowNarrowingKey::empty()` at every production site. | V1 adds `ResultEvaluationContextId`. |
| Awaited protocol | `build.rs` `surface_thenability` / `lib_awaited_surface` (two deliberately divergent `then` readers), `awaited_relation_output` roots on the operand only. | V7. |
| Oracle | The tree's recorded columns cite tsgo `7.0.0-dev.20260526.1`; TypeScript 7.0.2 native is the pinned target and is installed (`typescript@7.0.2`, `@typescript/typescript-win32-x64@7.0.2`). 7.1.0-dev nightlies are out of scope. | V0 ports every recorded column to 7.0.2. |
| D12 charter amendment (section 13) | Implemented charters are frozen acceptance records (program policy, held in the TAMA controller). | The enlargement is recorded in the DAG decision record `2026-09-17-signature-kernel-train` and by the DAG edge `V0 → D12`; `D12.md` is not rewritten. |
