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
| `ComponentMetaEvidence` facts, generation and external-input fingerprint; `ComponentMetaTrace` | Admission consumes opaque, completely traced evidence and validates the exact owner root and publication fence | REQUIRED | One traced compute through publication or refusal | `project_semantic_dispatch/memo` | always |
| Final component read, trace and admit operations | Warm reads validate every retained dependency; cold publication consumes stable tracer evidence under an unchanged external-input fence | REQUIRED | Request-bound operation | `project_semantic_dispatch/memo` | always |
| `FactValidation.publication_input_fingerprint` | Engine publication binds to the captured request basis rather than sampling a new live basis | REQUIRED | Borrowed request snapshot | `resolver_core/fact_validation_port` | always |
| `QueryBuildOutput.fold_partial` in place of replaceable quality setters | Named partial folds accumulate adopted failures | REQUIRED | Build output | `project_semantic_dispatch/walk` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Final component read/publish facade and admitted-entry carrier | Native final-result cache policy and reconstruction | REQUIRED | Request; admitted entry follows its dependent projection | `component_meta_result_admission` and `component_meta_result_db` | always |
| Captured publication fingerprint and request-port forwarding | Reject stale and unproven request bases; normalize immutable session overlays and exclude additive loading from external supersession | REQUIRED | Request view | `resolver_core/request_store_view` and `resolver_core/request_bound` | always |
| `classify_runtime` fact match | Per-member runtime constructors and typed degradation | REQUIRED | One member classification | `typeinfo/vue_macro_codegen/runtime` | always |
| Final component cache hit/miss hooks, facade observation handle and refusal traces | none; request audit and cache history | OPTIONAL | Instrumented request/store lifetime | `component_meta_result_admission` | cfg(feature = "semantic-observe") |

Unit-test builds retain observation seams needed by behavioral assertions.
Default production builds omit the optional fields and event updates; their
selection has no runtime enabled check.
