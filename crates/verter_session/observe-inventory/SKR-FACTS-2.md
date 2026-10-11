# Fact reads and final component storage

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `BroadRuntimeClassification.status` and `into_fact` | Runtime constructor publication distinguishes an exact member from an unavailable classification | REQUIRED | Immutable result; follows the answering read and any retained result | `semantic_query` | always |
| `execute_fact` | Fact consumers receive status and value together; execution aborts publish no fact | REQUIRED | One demanded read | `project_semantic_dispatch/projection_fact` | always |
| Carrier-prelude causes on the returning read | Typed status and execution-abort propagation | REQUIRED | Answering read | `project_semantic_dispatch` | always |
| `ComponentMetaResultKey` owner, options, project and split environment axes | Exact final-result identity | REQUIRED | Bounded store slot | `component_meta_result_db` | always |
| `ComponentMetaResultEntry` payload, signature and generation | Warm validation and publication; dependencies remain attached to the result | REQUIRED | Bounded candidate and any returned entry | `component_meta_result_db` | always |
| `ComponentMetaResultDb.inner`, caps, schema and retention account | Candidate selection, invalidation and bounded retention | REQUIRED-lifetime | Host-owned store; candidates retire on replacement, invalidation or FIFO eviction | `component_meta_result_db` | always |
| `ComponentMetaResultDb.live_counter` and `apply_live_delta` | Current store occupancy | REQUIRED-lifetime | Updated on each store mutation; owned by the host store snapshot | `component_meta_result_db` | always |
| `ComponentMetaResultDb.stale_sweeps`, `observe_removed` and retention-refusal debug trace | none; historical eviction and refusal observations | OPTIONAL | Instrumented store lifetime | `component_meta_result_db` | cfg(feature = "semantic-observe") |
| `ComponentMetaEvidence` facts, generation, external-input fingerprint, refusal effects, spent-alone flag, divergence certificate and schedule cut; `ComponentMetaTrace` | Admission consumes this compute's opaque evidence, including the charges and proof frozen when the trace ended | REQUIRED | One traced compute through publication or refusal; the sealed entry keeps the effects | `project_semantic_dispatch/memo` | always |
| Final component read, trace and admit operations | Warm reads validate and adopt every retained dependency into enclosing tracers; rejected candidates contribute no facts; cold publication consumes stable tracer evidence under an unchanged external-input fence | REQUIRED | Request-bound operation | `project_semantic_dispatch/memo` | always |
| `ComponentMetaRefusalLedger` identities, recovery, signature, generation, and retained request-spend effects | Eligible refusals repeat with their original status and replay the compute's own effects; they are not successful candidates and leave `len` unchanged. The reservation counts each retained paid identity at 64 bytes | REQUIRED | Bounded ledger, cap 256; cleared on owner and project invalidation; a failed signature stays until invalidation so a reverted edit restores it | `component_meta_result_db` | always |
| `SharedRefusal`, `Charged` and `ChargedSignature` | A reader holding the recovery or the signature keeps the retention charge | REQUIRED-lifetime | The charge survives ledger invalidation until the last handle drops | `component_meta_result_db` | always |
| Component-meta span (work, bytes, trip, schedule cuts, certificates) | The traced compute's connected charges, copied onto its evidence when the span ends. Root resets do not clear an open span | REQUIRED | The open trace; not a second store after the evidence is sealed | `project_semantic_dispatch/connected_demand` | always |
| Divergence-certificate note | Exact same-path recurrence and a real same-path recursion delivery. A wait cycle, a synthesized same-path stand-in, and a bare recursion bit do not set it | REQUIRED | The open component-meta span; a note outside a trace is not stored | `project_semantic_dispatch/connected_demand` | always |
| `ThreadSpendGuard` spend log | Attributes this thread's request operations and paid identities while a component-meta compute is tracing. Admission reads the copy on the evidence | REQUIRED | The guard; dropped when the compute returns | `request_budget` | always |
| `RequestEntrySnapshot`, `owned_spend`, `spent_alone_since` | Isolation is this compute's recorded spend against the shared request, not the counter delta | REQUIRED | One traced compute | `request_budget` | always |
| `FinalComponentAdmissionProof` | Derived at the publish decision from the compute's evidence | REQUIRED | The admission decision; not stored | `component_meta_result_db` | always |
| Final-component refusal query, read and admit | Same profile, certified divergence and isolated-root refusals replay under their identity; a higher profile, a different root, context or entry allowance, and a stale signature do not | REQUIRED | Request-bound operation | `project_semantic_dispatch/memo` | always |
| `FactValidation.publication_input_fingerprint` | Engine publication binds to the captured request basis rather than sampling a new live basis | REQUIRED | Borrowed request snapshot | `resolver_core/fact_validation_port` | always |
| `QueryBuildOutput.fold_partial` in place of replaceable quality setters | Named partial folds accumulate adopted failures | REQUIRED | Build output | `project_semantic_dispatch/walk` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Final component read/publish facade and admitted-entry carrier | Native final-result cache policy and reconstruction | REQUIRED | Request; admitted entry follows its dependent projection | `component_meta_result_admission` and `component_meta_result_db` | always |
| Captured publication fingerprint and request-port forwarding | Reject stale and unproven request bases; normalize immutable session overlays and exclude additive loading from external supersession | REQUIRED | Request view | `resolver_core/request_store_view` and `resolver_core/request_bound` | always |
| `classify_runtime` fact match | Per-member runtime constructors and typed degradation | REQUIRED | One member classification | `typeinfo/vue_macro_codegen/runtime` | always |
| `demand_fact` per macro projection lane | Each codegen demand publishes its own fact; a value-local reason is an input only of the demand that reads it | REQUIRED | One demand | `typeinfo/vue_macro_codegen/runtime` | always |
| Final component cache hit/miss hooks, facade observation handle, admitted-refusal traces, and the non-eligible refusal traces on both publish decisions | none; request audit and cache history | OPTIONAL | Instrumented request/store lifetime | `component_meta_result_admission` and `host_manage/component_meta_entry` | cfg(feature = "semantic-observe") |

Unit-test and test-support builds retain observation seams needed by behavioral assertions.
Default production builds omit the optional fields and event updates; their
selection has no runtime enabled check.
