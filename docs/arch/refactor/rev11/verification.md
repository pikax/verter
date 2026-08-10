# Verter Revision 11 Verification Contract

**Status:** Normative proof authority.  
**Rule:** a claim is accepted only when the exact candidate, exact result contract, and exact input/profile basis produced reproducible evidence.  
**No substitution:** one green fixture, one wall-time number, one warm cache hit, or one reviewer opinion cannot substitute for the required proof family.

# 1. Proof hierarchy

Evidence is ranked in this order:

1. externally observable behavior and official/differential conformance;
2. deterministic direct-versus-prepared-versus-managed equivalence;
3. negative proof that unsupported/partial/stale/cancelled/budgeted work cannot publish as complete;
4. exact work, copy, allocation, queue, and lifetime attribution;
5. absolute SLO and Verter no-regression decision;
6. equivalent-work competitor/Pareto decision;
7. bounded-memory and churn plateau;
8. architecture/dependency/deletion/complexity proof;
9. reviewer convergence on the unchanged candidate SHA.

A faster result does not pass if a higher-ranked proof fails.

# 2. Candidate provenance and non-vacuous execution

Every evidence bundle records:

- entry checkout (for Gate 0 provenance), implementation baseline, candidate SHA, and clean/dirty state;
- Revision 11 authority-package digest and Implementation Lock Record ID;
- Rust toolchain, target, features, linker, codegen/LTO/PGO, allocator, panic mode, and environment;
- Node/runtime/package manager/lockfile, TypeScript/native-TypeScript/provider versions, Vue/Svelte versions, NAPI/WASM runtime, and platform;
- benchmark machine/runner class, CPU topology/governor, memory, OS/kernel, background-load policy, and thermal policy;
- corpus and generated-input fingerprints;
- semantic/output/presentation/serialization/execution/result contract identities;
- cache/prepared state and thread/worker configuration;
- exact commands and raw logs/results.

Each test command must emit:

- discovered case/test count;
- executed count greater than zero;
- pass/fail/skip/ignored count;
- shard/feature/target selection;
- oracle/provider provenance where relevant.

A selector that runs zero cases is a failure. A skipped critical suite is a failure unless the block charter explicitly excludes it and no acceptance claim relies on it.

Sentinel verification is required for critical harnesses: in an isolated non-candidate run, deliberately break one known assertion or test fixture and prove the canonical selector fails.

# 3. Contract and architecture conformance

## 3.1 Dependency direction

Required compile-time or repository-graph checks prove:

- contract/identity/profile crates/modules depend on no compiler, semantic, session, provider, or framework implementation;
- framework compilers may depend on contract/syntax/framework-local code and the sealed semantic projection facade, not QueryRuntime/InputStore/ProviderHub;
- semantic kernel may depend on syntax/index/contracts and dependency-neutral observations, not framework codegen or managed engine;
- direct compiler has no dependency on `Engine`, InputStore, QueryRuntime, ProviderHub, durable stores, or LSP;
- managed services depend inward on direct algorithms/contracts, never the reverse;
- provider/LSP adapters do not become semantic authorities;
- no dependency cycle is hidden through feature flags, build scripts, generated code, or protocol conversion crates.

A machine-readable dependency snapshot is compared before/after every foundational block.

## 3.2 Public and auto-trait contract

Compile tests prove:

- `PreparedCarrier` and local `CompileTypeInfo` are not required to be `Sync`;
- `Send` exists only where safe whole-owner transfer is intentionally supported;
- no `unsafe impl Send/Sync` patches an ownership mismatch;
- OXC/arena types do not escape public direct results or compact executor jobs;
- stable public entity IDs and ephemeral cohort/session handles use distinct types;
- direct APIs expose no workspace/query/provider/cache implementation type;
- external integrations cannot implement alternate semantic resolution/classification behavior.

## 3.3 Clean-cutover proof

For each cutover, evidence includes:

- old declarations and implementation deleted;
- every compiler-reported caller migrated;
- old cache keys/stores, tasks, metrics, flags, re-exports, schema fields, fixtures, and docs deleted or explicitly preserved by a real compatibility contract;
- no runtime switch or fallback selects the old path;
- no source-name scanner exists merely to prevent resurrection;
- dependency graph and reachable production path count;
- source comments in the changed closure rewritten as present-tense invariants.

Text search is supporting evidence only; type/dependency/behavior proof is authoritative.

# 4. Compiler correctness and product contracts

## 4.1 Product matrix

Every supported product contract is tested independently and in valid combinations:

- Vue runtime subtargets currently claimed;
- Vue IDE companion;
- Vue public API/declarations where claimed;
- Svelte runtime/IDE/public products currently claimed;
- required diagnostics policy;
- required IDE projection map;
- optional runtime source-map data and encoded external map;
- provenance and serialization on/off;
- local and imported compile projections;
- zero-projection paths;
- direct one-shot, prepared first/repeat, batch, and managed paths;
- native, NAPI, and WASM where the operation is declared portable.

For every cell, assert:

- code bytes or normalized semantic output;
- required map coverage and source/generated round trips;
- diagnostics code/span/arguments/severity/order;
- exactness/completeness/unsupported/failure outcome;
- dependency and profile basis;
- deterministic stable IDs where exposed;
- absence of products not requested.

## 4.2 Combination law

For products declared independent, requesting `{A,B}` must equal requesting `A` and `B` separately after canonical product ordering, except for intentional shared materialization that changes no result.

For products declared coupled, the contract must name the coupling—for example, an IDE companion and the projection map required to interpret it publish atomically.

A combination must not:

- widen semantics or diagnostics beyond either product contract;
- collapse distinct per-product output/presentation/serialization profiles into one global profile;
- force Vue projection on Svelte;
- generate runtime maps because an IDE projection map is required;
- initialize TypeInfo when the combined plan has zero projection demands;
- create duplicate parses or duplicate subplans for an identical live prerequisite;
- invalidate an unchanged semantic/code subplan because only one product's terminal presentation or serialization changed.

Duplicate product kinds, irrelevant profile fields, and unsupported combinations are rejected before parsing/projecting beyond work already required to classify the request. Canonically equivalent subrequests produce equal subplan identities regardless of caller insertion order.

## 4.3 Direct/managed semantic identity

For the same source/project observations, profiles, and product contracts:

```text
direct complete result
== prepared complete result
== managed cold complete result
== managed warm complete result
```

Equality covers all Verter-owned observable bytes/facts except explicitly ephemeral request/session handles and timing/audit metadata. Stable IDs must still be equal.

## 4.4 Project-aware retry

Test at minimum:

- local root with no missing input;
- one import, multiple sibling imports, re-export chain, package exports/conditions, path mapping, declaration/library file, config inheritance, realpath/case policy;
- multiple independent misses returned in one currently knowable wave;
- transitive misses over multiple waves;
- unavailable input;
- no-progress repeated key;
- wave/key/byte/retry budget exhaustion;
- project/profile basis change between waves;
- cancellation between waves;
- prepared plan reuse without stale AST borrow;
- one-shot convenience documented reparse behavior;
- facts replay from another root/profile/demand rejected before emission.

The final complete result after caller-owned loading must equal a clean compile against the complete captured environment.

# 5. Native TypeInfo and flow semantic conformance

## 5.1 Profile provenance

Every differential row records:

- exact TypeScript compatibility family/version;
- normalized parse/checker/resolver options and profile ID;
- library/environment fingerprints;
- Verter semantic-kernel domain/epoch;
- source fixture and expected exactness;
- whether the outcome is Complete, Partial, Unsupported, NoValue, or Failed.

Unknown/unclassified semantic-affecting options must fail profile construction rather than silently reuse a result.

## 5.2 Effective type operation

`TypeAtPosition` is tested for:

- declaration/contextual type without flow;
- assignments and use-site narrowing;
- `typeof`, equality, truthiness/optional chain, `in`, `instanceof`, discriminants/switch/destructure;
- substitutions/generics and invalidation;
- predicate/assertion effects;
- nominal `unique symbol` identity and overlap;
- `this`, sequence, callback/contextual/call/value interactions in the claimed capability;
- deep/recursive/budgeted/cancelled cases;
- unsupported mechanism as typed non-complete outcome.

No-flow assertion:

```text
TypeAtPosition(no-flow)
=> correct base/contextual result
=> zero FunctionFlowGraph construction
=> zero FlowDemandPlan/domain solver allocation
=> zero duplicate source traversal used only to ask whether flow is needed
```

## 5.3 Structural return law

For every function fixture:

```text
return_result = union(
    effective_type(return_expression_i, program_point_i)
    for every authored return statement i
) + (endpoint_reachable ? undefined : nothing)
```

Test unreachable authored returns, `never` expressions, nested branches, loops, labels, switch, try/catch/finally, throws, and endpoint fallthrough.

Reachability may not remove an authored return contributor. Subject emptiness may not remove an unrelated branch.

## 5.4 Flow mechanism matrices

### Narrowing matrix

Cross:

- construct family;
- positive/negative branch;
- assignment before/inside/after;
- alias and substitution;
- nested logical/conditional/sequence position;
- loop and abrupt completion;
- exact/profile variation;
- cold/warm/recomputed path.

### Closure matrix

Cross:

- direct/transitive capture depth;
- read/write/both;
- creation before/after narrowing;
- invoked/escaped/opaque call;
- sibling/deeper closure;
- declarator, argument, return, condition, sequence, object/array, default initializer, expression statement;
- cold/warm/recomputed path.

### Completion matrix

Cross:

- block/if/loop/label/switch/try/catch/finally;
- normal/return/throw/break/continue;
- labelled/unlabelled;
- finally normal/override;
- endpoint reachable/unreachable;
- selected-domain closure and fixed-point convergence.

## 5.5 Sole-solver proof

After `D2`:

- every production effective-type/flow-return entry reaches the same solver owner;
- old syntax-shaped control types are absent from production code and dependency graph;
- no feature flag, test-only runtime selector, cache key, or fallback can choose a second evaluator;
- derived projections cannot construct a complete result or semantic transfer independently;
- unsupported rows leave explicit gaps in the sole solver;
- every `A6`-ratified Supported/Stable effective-flow capability remains conformant, or an explicit reviewed breaking capability decision is present.

## 5.6 Coverage/admission negative proof

Fault-injection tests force each obligation/domain edge to be skipped or report a gap. Assert:

- complete finalizer cannot construct `CompleteFlowResult`;
- no warm/persistent admission occurs;
- public exactness is not Complete;
- a useful partial, where allowed, records the missing domain/obligation;
- cancellation, stale input, panic, budget exhaustion, and torn basis have the same non-admission property.

# 6. Determinism and equivalence

## 6.1 Determinism domain

Given equal:

- authoritative source/project/configuration observations;
- semantic, output, presentation, serialization, compatibility, and product contracts;
- framework/toolchain identities;
- supported portable execution profile;

the following must be equal:

- generated code bytes;
- required projection map and requested runtime/source-map data;
- diagnostics code/span/typed arguments/rendered text when requested;
- public operation DTOs and graph exports;
- stable entity IDs, dependency fingerprints, exactness, provenance, and canonical serialization;
- cache/persistence payload bytes where canonical byte equality is claimed.

Excluded from semantic equality:

- wall-clock timestamps;
- trace/request IDs;
- queue timing;
- session-local opaque handles;
- non-normative performance counters;
- platform-specific error text not part of a portable contract.

## 6.2 Schedule randomization

Critical deterministic suites run under at least:

- 100 randomized scheduling seeds for pre-merge foundational candidates;
- 1,000 seeds or a coverage-equivalent systematic schedule campaign before `L4` for flight/publication/parallel aggregation code;
- worker counts `1`, `2`, representative mid-width, and configured maximum;
- randomized input insertion and hash-map construction order;
- randomized independent batch/chunk order;
- cold, warm, partial warm, and pressure-evicted states;
- cancellation at instrumented checkpoints;
- native threaded and WASM/local execution where supported.

A smaller count requires a stronger systematic interleaving proof accepted in the block charter.

## 6.3 Stable IDs versus handles

Tests assert:

- stable IDs do not depend on allocation, traversal, worker, hash seed, cache warmth, or serialization insertion order;
- session handles fail validation outside their cohort/generation and are never compared as stable IDs;
- handle reuse cannot alias a live entity;
- graph export IDs remain deterministic across direct/managed and native/WASM when the protocol claims portability.

## 6.4 Canonical serialization

Round-trip and byte-equality tests cover:

- map/set ordering;
- optional/default fields;
- enum discriminants;
- unknown-field policy;
- protobuf map avoidance/sorting on canonical paths;
- canonical error/diagnostic arguments;
- compression settings where compressed bytes are the product;
- malicious/reordered equivalent input payloads.

# 7. Work conservation and zero-work proof

## 7.1 Required counters

At minimum, record actual executions per logical identity for:

- carrier and language-domain parse;
- source-unit preparation and placement reconciliation;
- options/path/source canonicalization and digest construction;
- operation planning and plan hashing;
- projection demand discovery, batch aggregation, route/load, substitution/relation/inference;
- graph build, derived projection build, demand plan, each activated flow domain, fixed-point iteration, syntax reacquisition;
- template/style parse, fact extraction, edit planning, materialization;
- code generation, required projection map, runtime map generation, map encoding;
- diagnostics collection and text rendering;
- provenance, serialization, NAPI/WASM conversion;
- query-identity candidate lookup, candidate validation/rejection, and bounded candidate replacement;
- cache lookup/validation/admission and flight production/join;
- executor job creation and owner-call messages.

## 7.2 Amplification

For family `F` in one declared live ownership window:

```text
amplification(F) = actual executions / distinct demanded logical identities
```

Default required value is `1.0` for exact live parse, plan, projection, graph, CSS parse, output generation, and terminal product construction.

An amplification above `1.0` is accepted only when:

- the identities are genuinely distinct;
- eviction ended the live window and the same-key reparse is visible;
- failure/retry is required by the load protocol;
- the `A6`-locked `performance-gates.toml` cell explicitly proves recomputation is lower total cost than carrying/reusing the value;
- the product contract requests distinct materializations.

“Cheap” or “implementation simplicity” without measured total cost is not an exception.

## 7.3 Query identity versus attempt identity assertions

```text
same QueryIdentity + unrelated snapshot change
=> prior candidate remains discoverable
=> positive/negative facts decide reuse
```

```text
same QueryIdentity + different InputBasisId while a producer is running
=> no semantic-flight join by default
```

```text
changed positive or negative observed fact
=> prior candidate rejected
=> recompute under the new SemanticFlightKey
```

```text
terminal presentation/serialization change only
=> semantic typed candidate reused
=> only terminal materialization recomputed
```

## 7.4 Mandatory zero-work assertions

```text
Vue/Svelte compile with zero native projection demand
=> zero CompileTypeInfo construction
=> zero semantic projection call
```

```text
Svelte runtime compile under current capability
=> zero Vue projection demand
```

```text
same ParseKey in one live ParseOwnerDomainId
=> one parse invocation
```

```text
separate ordinary direct one-shot calls
=> no process-global parse cache/lock required
```

```text
IDE + build for same bytes and parse contract in one shared owner domain
=> one frontend parse
=> different validation/lowering allowed
```

```text
pressure-evicted parse requested concurrently
=> at most one same-key reparse flight in that owner domain
```

```text
IDE companion requested
=> required SourceProjectionMap constructed exactly once
```

```text
runtime external source maps disabled and not required by product
=> zero RuntimeSourceMapData generation
=> zero external map encoding
```

```text
provenance/serialization/rendered diagnostics disabled
=> zero corresponding terminal materialization and retained artifacts
```

```text
CSS exact identity + N consumers
=> one live syntax parse
```

```text
unrequested optional native enrichment
=> zero semantic work, retained values, formatted events, and attributable allocations
```

# 8. Performance decision methodology

## 8.1 Gate immutability

`A6` freezes populated cells in `performance-gates.toml` against the exact post-A3, post-instrumentation implementation baseline before `B1` and every later non-safety foundational cutover. A gate may be recalibrated only when:

- machine/toolchain/corpus/competitor changed materially;
- raw before/after calibration is retained;
- no candidate result was inspected before choosing the new threshold;
- an independent performance reviewer accepts the change;
- the affected block is re-baselined or restarted.

A candidate cannot choose its pass criterion after measurement.

## 8.2 Benchmark cell identity

Each cell fixes:

- source/corpus fingerprint and size distribution;
- framework, product contract, semantic profile, mappings, diagnostics, provenance, serialization;
- direct/prepared/managed state and warm definition;
- threading/execution profile;
- build mode/toolchain/features/allocator;
- boundary surface: Rust, NAPI, WASM, CLI, LSP;
- exact measured statistic and sample policy;
- work/copy/allocation validity assertions;
- absolute and relative gate.

Results from different cells are never combined without a declared aggregation rule.

## 8.3 Sampling and statistics

Default rules are frozen by the `A6` Implementation Lock Record and `performance-gates.toml`:

- at least 30 interleaved measured samples for short cells after declared warmup;
- at least 10 independent long-cell runs when practical;
- bootstrap 95% confidence interval over the declared statistic;
- no-regression upper slowdown bound no greater than `max(3%, 2 × measured noise floor)` unless the locked cell is tighter;
- p95/p99 only with sufficient observations;
- predefined outlier policy; no discretionary deletion after seeing direction;
- machine drift checks using a stable control benchmark;
- report indistinguishable results as indistinguishable.

For process-level peak RSS, isolate processes and record allocator/platform behavior. For CPU, report process CPU and wall time. For parallel cells, report efficiency and work counts.

## 8.4 Required benchmark families

### Direct compiler

- tiny, medium, large, adversarial, and many-file unique corpora;
- one-thread loop and max configured width;
- Vue/Svelte and each claimed runtime/IDE/public product;
- no projection, local projection, imported projection;
- maps/diagnostics/provenance/serialization off/on as separate cells;
- direct one-shot and prepared first/repeat;
- source-only and project load-wave/retry;
- Rust core, NAPI, and WASM separately.

### Managed compiler

- cold content/session and validated warm;
- one-character edits in script/template/style;
- unit move without byte change;
- dependency/config/project edit;
- create/delete/rename/reopen;
- queue/background load and interactive priority;
- pressure eviction and reparse;
- stale/cancelled work.

### TypeInfo and flow

- script/index/kernel baseline;
- no-flow position;
- each activated domain family;
- closure/loop/completion/call/context;
- local/imported Vue projection;
- cold/warm/recomputed/partial;
- deep/budgeted/cancelled;
- derived projection versus direct graph traversal where promotion is proposed.

### CSS

- parse/index/format/Vue/Svelte plans;
- exact shared identity with multiple consumers;
- transformed new identity;
- maps/provenance off/on;
- large/recovery/adversarial dialect fixtures.

### LSP/provider

- edit-to-companion, completion, hover, diagnostics, rename/navigation where claimed;
- provider acknowledgement and stale rejection;
- provider restart/route transition/project reload;
- background indexing under interactive load;
- queue saturation/cancellation;
- native enrichment off/on/delayed/failing.

### Boundaries

- native Rust result;
- diagnostics/rendering;
- map generation versus encoding;
- JSON/protobuf/binary;
- NAPI/WASM host copies and heap delta;
- cancellation/supersession before and during conversion.

## 8.5 Competitor/Pareto proof

A competitor row is valid only when it records:

- exact source revision/version and build flags;
- corpus and target equivalence;
- output validity and supported semantics;
- source maps/diagnostics/imported-type behavior;
- threading and cache state;
- boundary included/excluded;
- raw result and uncertainty.

Verter must first pass absolute SLO and self no-regression. It is blocking-dominated when one valid competitor is materially faster **and** lower peak RSS under the locked tolerance while doing equivalent work.

The `A6` lock sets exact aggregate and strategic-cell tolerances before candidate implementation. The target is to meet or beat the fastest valid equivalent-work Rust implementation on the primary direct-suite aggregate and to avoid material Pareto domination in strategic cells. A candidate miss is blocking and cannot be waived after results are known. If indispensable extra work or a comparison mismatch proves the locked product/equivalence premise false, the project must amend the product/architecture and Implementation Lock Record under the blind recalibration rule, invalidate affected candidate evidence, and restart. Repeated work, weaker semantics, invalid comparison, or unbounded retention are never acceptable premises.

# 9. Allocation, copy, arena, stack, and boundary proof

Every strategic cell reports:

- allocation count and requested bytes;
- live and peak logical bytes by owner;
- source/input bytes copied and number of source-sized buffers;
- output/map/serialization temporary bytes;
- arena capacity versus live payload and oversized pool discards;
- `Arc` clones/atomic traffic/lock contention/channel messages where attributable;
- worker count × reserved stack and measured high-water where practical;
- NAPI/WASM/host heap delta and transfer copies.

Required negative assertions:

- primary direct Rust source is borrowed;
- no AST/source clone exists merely to satisfy `Send`, `Sync`, or `'static`;
- no public output pins an OXC/request arena;
- independently evictable entries do not share a lifetime-pinning arena;
- terminal representations are not built for cancelled/superseded/unrequested products;
- one explicit safe boundary copy is not mislabeled as failure when zero-copy would require unsafe lifetime coupling.

# 10. Bounded-memory and soak contract

## 10.1 Owner metrics

Every retained owner exposes:

- logical live, pinned, and evictable bytes;
- entry/count by compatibility/input generation;
- admission/refusal/hit/miss/validation/eviction;
- pin count/reason, oldest pin age, generation age, last-use age;
- superseded-but-pinned bytes;
- parse/reparse, graph/projection, semantic cohort, interner, queue, flight, provider, tombstone, and audit counts;
- configured soft/hard bound and trim result.

## 10.2 Workloads

Soaks include:

- repeated edits across tiny and large files;
- create/delete/rename/move/reopen and same-content new incarnation;
- project/workspace-folder add/remove/reload;
- dependency/config/library changes;
- provider restart and mode transition;
- TypeInfo/flow query storms;
- cancellation/supersession and abandoned waiters;
- pressure and admission refusal;
- formatter/CSS/compiler/LSP mixed activity;
- idle periods and explicit quiescence/trim protocol.

`L1` minimum durations/work counts are fixed by the `A6` Implementation Lock Record and the accepted `L1` charter before the soak candidate runs.

## 10.3 Acceptance

After warm-up and quiescence:

- logical bytes are within owner budgets plus attributable live pins;
- no statistically meaningful positive long-run slope exists in live logical bytes/counts;
- superseded generations become reclaimable after live readers end;
- all remaining pins have current source/request/provider reasons;
- RSS remains inside the platform/allocator plateau envelope;
- queue/flight/tombstone/interner/audit counts do not grow monotonically;
- clean-equivalence samples remain green;
- no restart is needed for cleanup.

# 11. QueryRuntime, flight, cancellation, and executor proof

Test at minimum:

- warm hit inline with no executor task;
- cold same-key producer with many followers;
- first waiter cancels while followers continue;
- all waiters cancel and producer cooperatively stops;
- follower deadline/budget differs from producer aggregate;
- additional budget while `Running` extends the bounded producer without changing output;
- higher-budget request after budget finalization uses a successor flight;
- ordinary budget never selects an approximation or prunes a required obligation;
- priority elevation and safe lowering;
- producer panic/internal failure/budget/stale/cancel outcome resolves every waiter once and admits nothing;
- no self-wait/cycle deadlock;
- same semantic arguments under incompatible `ResultContractId` do not share a query identity or flight;
- same `QueryIdentity` on different `InputBasisId` values does not join in flight by default;
- a bounded cached candidate produced on an older basis may be found by `QueryIdentity` but is used only after positive/negative fact validation;
- content artifact may join across snapshots when identity is immutable;
- follower validates completed value against its own admissible view;
- shutdown empties flight table and owner queues;
- owner-affine command does not move AST/arena;
- tiny dependent work stays inline;
- chunk/fork threshold, fan-out, queue bound, and interactive capacity under background saturation;
- native threaded and WASM/local state machines produce equivalent outcomes.

Model-based/state-machine tests should cover every legal transition and reject double-finalization, waiter loss, publication after failure, and producer lifetime tied to first requester.

# 12. Incremental, snapshot, and publication proof

## 12.1 Input commits

Assert:

- readers observe complete root before or after commit, never mixed subroots;
- concurrent commits do not lose updates;
- parser/provider/semantic work is absent from writer critical section;
- document ranges apply only to matching incarnation/version;
- version gaps enter explicit unsynchronized state;
- removed inputs immediately leave authority.

## 12.2 Incremental-clean equivalence

For every supported edit class:

```text
incremental final products(final committed inputs)
== clean final products(final committed inputs)
```

Compare code, required mappings, diagnostics, public DTOs, dependencies, exactness, stable IDs, and canonical serialization.

Test script/template/style-only edits, unit move, dependency change, project/config change, external template/style, fallback, pressure eviction, and repeated edit sequences.

## 12.3 Publication

Adversarially interleave:

- rapid edits during compile/provider work;
- stale native-TypeScript/tsserver/extension response;
- provider epoch transition;
- companion ready before map and map ready before companion;
- mapping supersession;
- dependency input change;
- close/reopen same content with new incarnation;
- provider off and native enrichment on/off/delayed/failing;
- queue saturation and cancellation.

No stale, torn, incompletely mapped, or mixed-provider result may publish.

# 13. CSS and formatter proof

For every claimed dialect/operation:

- Native/External/Unsupported capability is explicit;
- exact parse identity and live owner domain recorded;
- formatter output parses under the same frontend and is idempotent;
- comments/trivia/recovery behavior preserved according to contract;
- range formatting changes only a structurally safe range;
- index/navigation facts match syntax/recovery completeness;
- Vue `v-bind`, modules, scoping/keyframes, and Svelte style consumers reuse syntax where bytes/profile match;
- changed transform/preprocessor output receives a new identity;
- semantic transforms refuse when recovery cannot prove structure;
- source-map/provenance zero-work assertions pass;
- direct and managed results are equivalent.

# 14. Failure, trust, and adversarial proof

Test boundaries with malformed, hostile, deep, huge, cyclic, and inconsistent input:

- parser recovery and deterministic parse failure;
- stack-depth/explicit-stack limits;
- semantic recursion/work budgets;
- giant unions/intersections/generic recursion/flow loops;
- huge LoadSet and dependency cycles;
- invalid UTF-8 at byte-oriented boundaries where applicable;
- path traversal, symlink/case/realpath ambiguity, package exports cycles;
- malformed/untrusted persistent or precomputed payloads;
- oversized graph/protocol/diagnostic/map payloads;
- panic in parser/compiler/semantic/provider/FFI adapter;
- provider crash/hang/protocol violation;
- cancellation at all long-loop and boundary checkpoints;
- shutdown during work;
- corrupt cache entry and digest mismatch;
- session handle from wrong generation/cohort.

Requirements:

- no undefined behavior or unsafe lifetime escape;
- no cache/persistence admission after panic/cancel/stale/budget/internal failure;
- no process-global poison that permanently breaks unrelated operations;
- no sensitive ambient filesystem/network/process access from direct/compiler/semantic core;
- typed bounded failure with deterministic code/basis;
- malformed external data is size/integrity/compatibility checked before allocation/semantic use.

# 15. Compatibility and persistence proof

For every retained compatibility domain:

- owner, scope, epoch, schema/algorithm, producer, consumers, and migration policy recorded;
- monotonic epoch behavior proven;
- no duplicate authority required to remain numerically equal;
- internal no-boundary counters deleted;
- old persisted/public payload behavior matches accepted migration/rejection policy;
- semantic/output/presentation/serialization identities are not conflated;
- precomputed facts validate root/batch/profile/kernel/input/dependency/exactness/integrity/size basis;
- compatibility mismatch fails closed before use;
- canonical serialization byte tests pass;
- persistence is disabled for values lacking hermetic complete positive/negative facts.

# 16. Complexity, concepts, and deletion report

Every block reports before/after:

- production owner/service types;
- traits and dynamic dispatch points;
- public/crate-visible entry points;
- caches/maps/interners;
- locks/atomics/concurrent maps/channels;
- queues/pools/background tasks;
- revision/epoch/token/handle types;
- semantic/syntax/control representations;
- source-sized buffers and materialization passes;
- reachable production paths and runtime selectors;
- lines added/deleted and dependencies added/removed;
- tests/guards/docs/comments added/deleted;
- cold/warm latency, CPU, allocations, copies, peak/retained memory.

A net concept increase must correspond to an explicit accepted capability/invariant that the prior model could not represent. Moving complexity behind new names is a failure.

# 17. Orchestration, program-state, stack, and landing proof

Every accepted block proves its delivery process as well as its code:

- Revision 11 package validation passed from the exact extracted package;
- `program-state.toml` contains every DAG block exactly once and validates before start, review, acceptance recommendation, and acceptance;
- only blocks with accepted predecessors become active, except contingent `READY`/`IN_PROGRESS`/`REVIEW` upper layers whose unaccepted predecessors are lower layers in the same validated immutable stack snapshot; no such upper layer reaches acceptance recommendation before predecessor landing/restack, and before `A6` no post-Gate-0 block is active;
- the context packet, charter, program-state, base SHA/tree, candidate SHA/tree, and evidence digests agree;
- one writable worktree/branch/worker owns the mutation surface; clean-tree proof includes generated and untracked files;
- shared generated files, lockfiles, protocols, and dependency-firewall files had one writer lease;
- the orchestrator did not count its own implementation/synthesis as independent review or maintainer acceptance;
- the stack window is bounded, maps every layer to a block/charter, and contains no hidden unaccepted cross-stack dependency;
- every mergeable layer is independently releasable and passes required checks on its cumulative tree;
- private atomic layers remain draft/non-mergeable and never reach trunk independently;
- `D1` is recorded only as a private checkpoint and `D2` is the atomic public landing;
- every lower-layer change records old/new base/tree, patch/range-diff, conflict/manual edits, regenerated outputs, CI reruns, and review reattestation;
- no approval transfers automatically across a restack or candidate change;
- a `LANDABLE` stack lands only its lowest eligible layer and then issues a successor snapshot for remaining dependants; an `ATOMIC_REVIEW` stack lands only its final candidate;
- the actual accepted commit/tree is bound to the reviewed candidate through exact canonical candidate-delta equality on recorded bases, matching generated-output digests, and required post-landing checks; full-tree equality is not assumed after a base advance;
- program state records actual accepted commit/tree, the landing-equivalence digest, and invalidates/restacks remaining dependent work.

Adversarial delivery tests include an out-of-order block start, stale program state, duplicate block entry, lower-layer restack after upper approval, manual rebase conflict, hidden generated diff, two workers targeting one branch, private-layer merge attempt, merge-queue rebase, failed post-merge candidate-delta equivalence, and missing maintainer authority. Each must fail closed.

# 18. Final acceptance matrix

`L4` requires all rows below on one exact candidate SHA/tree, exact base tree, and evidence digest.

| Area | Required proof |
|---|---|
| Authority | Revision 11 digest, accepted ADRs, no contradiction or unresolved public/identity/lifetime gate |
| Candidate | exact provenance, clean tree, non-vacuous canonical commands |
| Dependency | forbidden edges/cycles absent; direct core independent of managed engine |
| Syntax | one shared error-tolerant frontend per language domain; scoped parse owner/reparse proof |
| Compiler | compositional products, borrowed direct core, prepared/resumable transaction, exact mappings |
| TypeInfo | one profile-parameterized kernel/resolver path; sealed compile facade; load waves bounded |
| Flow | one graph authority, one production solver, demand domains, structural returns, closure/completion correctness |
| Completeness | partial/unsupported/stale/cancelled/budgeted/panicked work cannot admit complete |
| Public API | operation DTOs primary, optional bounded graph export, stable IDs separate from handles, no general TypeExpr |
| Query/runtime | final InputStore basis, value facts, FlightCell, bounded executor/owner affinity |
| Incremental | immutable stable-unit reuse and clean final equivalence |
| CSS | one syntax authority per exact identity, deterministic formatter/index, explicit preprocessing |
| Frameworks | typed Vue/Svelte boundaries, no final Any bag, synthetic alternate-shape fixture |
| Providers/LSP | project-scoped non-racing route, atomic companion/map, stale rejection |
| FFI/checker | deterministic safe NAPI/WASM conversion; narrow verter_tsc checker boundary |
| Work | amplification and all zero-work assertions green |
| Performance | absolute SLO, self no-regression, equivalent-work Pareto/competitor gates green |
| Memory | L1 plateau and pin attribution; no restart cleanup |
| Failure | adversarial, panic, cancellation, untrusted decode, deep-input containment |
| Complexity | negative-net architecture or accepted capability rationale; old paths and campaign machinery gone |
| Delivery | validated program state, bounded stack windows, worktree isolation, restack reattestation, atomic private layers, reviewed-to-accepted candidate-delta equivalence |
| Review | required exact-SHA/tree conformance, architecture, and adversarial/performance approvals plus maintainer acceptance |

A row marked “not applicable” requires a contract citation and evidence that the product/capability is unsupported rather than silently skipped.
