# Resolver-context dispatch: measurement and request snapshot

The type engine reaches its host through `&dyn ResolverContext<C>`. This page
records which context methods are hot per request, what moved off the virtual
boundary because of it, and the before/after evidence. It is a portable
summary; the raw harness runs stay out of the tree.

## How it was measured

- **Worker.** One shared Windows 11 x86_64 developer worker (32 logical CPUs),
  not the benchmark machine. Under the
  [measurement rule](../contributing/semantic-benchmark.md#measurement-rule)
  times and memory were compared on this worker only. This page records the
  machine-independent answers and work counts and, for times and memory,
  only the verdict; the measured figures stay in the run's report.
- **Trees.** *Baseline*: the last main commit before "refactor: extract
  verter_type_engine and verter_semantic_source from verter_session", which
  is "chore(ci): run the validation probe nightly instead of on pull
  requests" (2026-10-06). *Head*: the extraction commit itself (2026-10-06).
  *Candidate*: head plus the request snapshot described below. Each pair was
  measured on the same worker in one session, interleaved (baseline, head,
  baseline, head; then head, candidate, head, candidate).
- **Harness.** `node scripts/benchmark/semantic-perf.mjs --tier quick --arms
  verter,verter-obs,verter-counted`: per scenario one warmup, three measured
  fresh processes and three in-process warm repeats per run. Every run passed
  validation. The tsc arms were not run, because this is a
  Verter-against-Verter comparison.
- **Lanes the harness has no scenario for** (local edit, flow return,
  component metadata) were timed by a throwaway release probe over the
  public host API on each tree, six interleaved processes per tree. Each lane
  uses a fresh host: project setup, cold and warm requests, then an edit (an
  unrelated declaration appended to the demanded module, or for component
  metadata to an imported types module), then the request cold and warm
  again. The component-metadata lane resolves the session crate's vendored
  SFC fixtures (`crates/verter_session/test_fixtures/{table,tabs,editor_toolbar}.vue`).
- **Noise bound.** The harness's descriptive rule: a difference counts only
  when every repetition of one tree beats every repetition of the other by
  more than the timer resolution (1 ms for one request, 2 ms for first type
  handle). Otherwise the verdict is *overlap*. Peak memory is the harness's
  Windows metric, the process's peak private commit.
- **Call counts.** `count_resolver_context_call!` names each port method at
  its implementation. The counters exist only under the default-off
  `semantic-observe` feature: OPTIONAL in
  [the observation policy](../arch/semantic-observe.md), with an inventory
  row in [`crates/verter_type_engine/observe-inventory/`](../../crates/verter_type_engine/observe-inventory/).
  `cargo run -p verter_bench --profile no-debug-assertions --features
  semantic-observe --example resolver_dispatch_profile -- <scenario-dir>...`
  profiles every given benchmark scenario directory
  (`target/semantic-perf/<run>/scenarios/<id>/strict`) plus a flow-return and
  a component-metadata lane. It reports each phase's calls per method beside
  the semantic nodes the phase interned. Counts do not depend on the build
  profile.

## Extraction parity (baseline → head)

Every scenario answered identically, with identical semantic-node,
memo-entry and relation-proof counts. Cold allocation counts (the counting
arm) stay inside their run-to-run spread, and every time and peak-memory
verdict is overlap. No lane
regressed beyond the noise bound, so no deviation is recorded.

| scenario | answer and outcome | semantic nodes | memo entries | relation proofs | time and peak-memory verdict |
| --- | --- | ---: | ---: | ---: | --- |
| library-generic-call | identical | 32 | 37 | 4 | overlap |
| library-map-entries | identical | 45 / 27 | 41 / 29 | 0 | overlap |
| library-promise-then | identical | 157 | 39 | 0 | overlap |
| library-array-map | identical | 134 | 30 | 0 | overlap |
| library-awaited | identical | 9 | 16 | 0 | overlap |
| base-signature-1 | identical | 28 | 29 | 8 | overlap |
| reference-infer-aliases-100 | identical | 721 | 1020 | 102 | overlap |
| reference-infer-depth-10 | identical | 50 | 79 | 2 | overlap |
| infer-pattern-repeat-10 | identical | 96 | 130 | 40 | overlap |
| contravariant-callbacks-10 | identical | 45 | 72 | 20 | overlap |
| overloads-1025 | identical | 3086 | 2071 | 1025 | overlap |
| inference-deposits-1025 | identical | 18 | 23 | 2 | overlap |
| tail-recursive-parse-50 | identical | 486 | 233 | 152 | overlap |
| conditional-chain-97 | identical | 400 | 504 | 1 | overlap |
| alias-chain-50 | identical | 160 | 267 | 0 | overlap |
| template-absorption-400x250 | identical | 918 | 27 | 0 | overlap |
| template-nested | identical | 1024 | 20 | 0 | overlap |
| template-4-spans | identical | 10022 | 20 | 0 | overlap |
| relation-reversed-200 | identical | 632 | 29 / 23 | 20404 / 2018 | overlap |
| relation-aligned-200 | identical | 617 | 424 | 402 | overlap |
| baseline-empty | identical | 5 | 12 | 0 | overlap |

Lanes outside the harness (six processes per tree; answers, node counts and
memo counts are equal in every process):

| lane | answers, nodes, memo entries equal | phases timed | time verdict |
| --- | --- | --- | --- |
| alias-chain-200 | yes | setup, cold, warm, editCold, editWarm | overlap |
| component-meta | yes | setup, cold, warm, editCold, editWarm | overlap |
| contravariant-callbacks-100 | yes | setup, cold, warm, editCold, editWarm | overlap |
| flow-return | yes | setup, cold, warm | overlap |
| library-generic-call | yes | setup, cold, warm, editCold, editWarm | overlap |
| overloads-1025 | yes | setup, cold, warm, editCold, editWarm | overlap |
| relation-aligned-600 | yes | setup, cold, warm, editCold, editWarm | overlap |
| relation-reversed-200 | yes | setup, cold, warm, editCold, editWarm | overlap |
| tail-recursive-parse-500 | yes | setup, cold, warm, editCold, editWarm | overlap |

## Per-method profile (head)

Port calls per phase. `setup` (project load) makes none: it runs before any
resolver context exists.

| lane | phase | semantic nodes added | port calls (before) | calls / node | port calls (after) |
| --- | --- | ---: | ---: | ---: | ---: |
| relation-aligned-3200 | setup | 0 | 0 | — | 0 |
| relation-aligned-3200 | cold | 9614 | 266 | 0.0 | 254 |
| relation-aligned-3200 | warm | 0 | 16 | — | 16 |
| relation-aligned-3200 | edit-cold | 9614 | 293 | 0.0 | 281 |
| relation-aligned-3200 | edit-warm | 0 | 18 | — | 18 |
| relation-reversed-2100 | setup | 0 | 0 | — | 0 |
| relation-reversed-2100 | cold | 6329 | 116019 | 18.3 | 114009 |
| relation-reversed-2100 | warm | 0 | 3930318 | — | 3930315 |
| relation-reversed-2100 | edit-cold | 6315 | 3930498 | 622.4 | 3930486 |
| relation-reversed-2100 | edit-warm | 0 | 3930326 | — | 3930323 |
| template-369x271 | setup | 0 | 0 | — | 0 |
| template-369x271 | cold | 100652 | 215 | 0.0 | 204 |
| template-369x271 | warm | 0 | 23 | — | 23 |
| template-369x271 | edit-cold | 652 | 232 | 0.4 | 222 |
| template-369x271 | edit-warm | 0 | 25 | — | 25 |
| alias-chain-1000 | setup | 0 | 0 | — | 0 |
| alias-chain-1000 | cold | 3007 | 55114 | 18.3 | 53109 |
| alias-chain-1000 | warm | 0 | 16 | — | 16 |
| alias-chain-1000 | edit-cold | 3008 | 59124 | 19.7 | 57119 |
| alias-chain-1000 | edit-warm | 0 | 18 | — | 18 |
| conditional-chain-200 | setup | 0 | 0 | — | 0 |
| conditional-chain-200 | cold | 809 | 12937 | 16.0 | 12530 |
| conditional-chain-200 | warm | 0 | 16 | — | 16 |
| conditional-chain-200 | edit-cold | 810 | 13947 | 17.2 | 13540 |
| conditional-chain-200 | edit-warm | 0 | 18 | — | 18 |
| tail-recursive-parse-1000 | setup | 0 | 0 | — | 0 |
| tail-recursive-parse-1000 | cold | 9033 | 81377 | 9.0 | 77360 |
| tail-recursive-parse-1000 | warm | 0 | 21 | — | 21 |
| tail-recursive-parse-1000 | edit-cold | 8029 | 72554 | 9.0 | 69534 |
| tail-recursive-parse-1000 | edit-warm | 0 | 28 | — | 27 |
| overloads-1025 | setup | 0 | 0 | — | 0 |
| overloads-1025 | cold | 3083 | 19615 | 6.4 | 18582 |
| overloads-1025 | warm | 0 | 16 | — | 16 |
| overloads-1025 | edit-cold | 3083 | 21675 | 7.0 | 20642 |
| overloads-1025 | edit-warm | 0 | 18 | — | 18 |
| contravariant-callbacks-500 | setup | 0 | 0 | — | 0 |
| contravariant-callbacks-500 | cold | 1512 | 40160 | 26.6 | 37151 |
| contravariant-callbacks-500 | warm | 0 | 16 | — | 16 |
| contravariant-callbacks-500 | edit-cold | 1511 | 41672 | 27.6 | 38663 |
| contravariant-callbacks-500 | edit-warm | 0 | 23 | — | 22 |
| reference-infer-aliases-1000 | setup | 0 | 0 | — | 0 |
| reference-infer-aliases-1000 | cold | 7009 | 97054 | 13.8 | 94050 |
| reference-infer-aliases-1000 | warm | 11 | 6178 | 561.6 | 6171 |
| reference-infer-aliases-1000 | edit-cold | 7010 | 102118 | 14.6 | 99113 |
| reference-infer-aliases-1000 | edit-warm | 10 | 13203 | 1320.3 | 13194 |
| flow-return | setup | 0 | 0 | — | 0 |
| flow-return | cold | 25 | 464 | 18.6 | 441 |
| flow-return | warm | 0 | 291 | — | 281 |
| component-meta | setup | 0 | 0 | — | 0 |
| component-meta | cold | 42 | 2295 | 54.6 | 2232 |
| component-meta | warm | 0 | 21 | — | 21 |
| component-meta | edit-cold | 4 | 791 | 197.8 | 780 |
| component-meta | edit-warm | 0 | 21 | — | 21 |

Calls per method in each lane's cold request:

| method | rel-aligned-3200 | rel-reversed-2100 | template-369x271 | alias-1000 | cond-200 | tail-rec-1000 | overloads-1025 | contra-500 | ref-infer-1000 | flow-return | component-meta |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| *semantic nodes added* | 9614 | 6329 | 100652 | 3007 | 809 | 9033 | 3083 | 1512 | 7009 | 25 | 42 |
| `Cancellation::is_cancelled` | 66 | 18032 | 55 | 10025 | 2035 | 30103 | 5166 | 13046 | 17016 | 87 | 421 |
| `IndexedInputs::host_view_env_hashes_for` | 21 | 8003 | 13 | 4008 | 1209 | 9037 | 3083 | 1012 | 8002 | 48 | 140 |
| `FactValidation::current_project_generation` | 24 | 4016 | 22 | 4010 | 814 | 8034 | 2066 | 8018 | 6005 | 61 | 160 |
| `LiveFactValidation::aggregate_clock_reader` | 17 | 6004 | 13 | 3006 | 808 | 9029 | 1033 | 3509 | 6003 | 24 | 210 |
| `FactValidation::aggregate_basis_seed` | 17 | 6004 | 13 | 3006 | 808 | 9029 | 1033 | 3509 | 6003 | 24 | 207 |
| `IndexedInputs::prepared_decl_bundle` | 19 | 8000 | 15 | 5010 | 1010 | 20 | 1034 | 1009 | 9002 | 13 | 35 |
| `IndexedInputs::ensure_indexed_ready_serve` | 14 | 6003 | 13 | 4007 | 807 | 15 | 1034 | 1010 | 7003 | 33 | 264 |
| `IndexedInputs::host_view_project_identity_for` | 11 | 4002 | 7 | 2004 | 605 | 5021 | 2055 | 508 | 4001 | 28 | 70 |
| `FactValidation::validates_fact_signature_with_self_roots` | 3 | 7981 | 0 | 0 | 0 | 6008 | 0 | 1500 | 998 | 6 | 122 |
| `FactValidation::complete_graph_signature` | 12 | 2010 | 11 | 2005 | 407 | 4017 | 1033 | 2009 | 3004 | 15 | 53 |
| `OwnedLowering::contributor_answer` | 4 | 5990 | 3 | 1002 | 202 | 3 | 1 | 1 | 2000 | 3 | 0 |
| `IndexedInputs::semantic_compiler_options_for` | 5 | 3995 | 3 | 1002 | 203 | 4 | 4 | 505 | 2000 | 18 | 0 |
| `IndexedInputs::host_view_project_identity` | 4 | 3994 | 3 | 1002 | 202 | 3 | 3 | 503 | 2000 | 18 | 0 |
| `OwnedLowering::prepared_type_for_projection` | 7 | 2003 | 6 | 2004 | 404 | 10 | 2 | 2 | 3002 | 2 | 0 |
| `OwnedLowering::prepared_value_decl` | 4 | 1999 | 3 | 1002 | 202 | 1003 | 1028 | 4 | 2000 | 17 | 0 |
| `IndexedInputs::host_view_env_hashes` | 4 | 3994 | 3 | 1002 | 202 | 3 | 2 | 2 | 2000 | 2 | 0 |
| `OwnedLowering::augmentation_index` | 4 | 3994 | 3 | 1002 | 202 | 3 | 2 | 2 | 2000 | 2 | 0 |
| `RouteLookup::reverse_dependency_canonicals` | 4 | 1999 | 3 | 1002 | 202 | 3 | 2 | 2 | 2000 | 2 | 0 |
| `IndexedInputs::shallow_file_state` | 2 | 3 | 2 | 1001 | 401 | 3 | 2 | 1001 | 2001 | 4 | 45 |
| `IndexedInputs::normalized_analysis_canonical` | 4 | 6 | 4 | 2002 | 402 | 4 | 0 | 0 | 2002 | 2 | 27 |
| `IndexedInputs::project_stable_key_for_canonical` | 0 | 3992 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 | 0 |
| `IndexedInputs::resolve_project_for_canonical` | 0 | 3992 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 | 0 |
| `FactValidation::compat_token` | 2 | 3 | 2 | 1001 | 201 | 2 | 0 | 1500 | 1001 | 15 | 58 |
| `FactValidation::observe_borrowed_signature` | 2 | 3 | 2 | 1001 | 401 | 3 | 0 | 0 | 2001 | 1 | 40 |
| `RouteLookup::resolve_imported_type_root_with_facts` | 2 | 3 | 2 | 1001 | 401 | 3 | 0 | 0 | 2001 | 1 | 40 |
| `OwnedLowering::deref_authored_body` | 3 | 4 | 3 | 1002 | 202 | 6 | 1026 | 2 | 1002 | 2 | 21 |
| `IndexedInputs::authoritative_current_content_hash` | 2 | 3 | 2 | 1001 | 201 | 2 | 0 | 0 | 1001 | 1 | 11 |
| `IndexedInputs::indexed_for_current_content` | 2 | 3 | 2 | 1001 | 201 | 2 | 0 | 0 | 1001 | 1 | 4 |
| `OwnedLowering::prepared_type_decl` | 2 | 1997 | 2 | 0 | 0 | 1 | 0 | 0 | 0 | 0 | 9 |
| `OwnedLowering::global_contributor_answer` | 0 | 1996 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 | 0 |
| `RouteLookup::lookup_ambient_symbol` | 0 | 1996 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 | 0 |
| `IndexedInputs::declaration_sequence_rank` | 0 | 1995 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `OwnedLowering::refresh_augmentation_keys` | 0 | 1995 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `FactValidation::validates_fact_signature` | 0 | 0 | 0 | 0 | 200 | 1 | 0 | 0 | 1000 | 0 | 91 |
| `ExpressionSourceSelection::indexed_flow_source` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1000 | 0 | 10 | 74 |
| `OwnedLowering::prepare_function_structure` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 500 | 0 | 3 | 0 |
| `Cancellation::cancellation_checkpoint` | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 24 |
| `RouteLookup::resolve_type_dependency_canonical` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 34 |
| `ExecutionSubmission::attach_engine` | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 12 |
| `HostAttachmentPort::host_attachment` | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 12 |
| `IndexedInputs::engine_policy` | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 12 |
| `IndexedInputs::artifact_key_for_current_content` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 19 |
| `OwnedLowering::parse_fact_for_observed_content` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 19 |
| `IndexedInputs::get_whole_hash` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 12 |
| `IndexedInputs::is_request_bound` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 12 |
| `IndexedInputs::observe_materialize_scope` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 10 |
| `ExpressionSourceSelection::indexed_expression_source` | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 1 | 0 | 6 | 0 |
| `FactValidation::current_external_supersession_fingerprint` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 8 |
| `OwnedLowering::script_setup_type_params` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |
| `FactValidation::observe` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| `IndexedInputs::ensure_loaded` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| `OwnedLowering::ordered_sfc_structure` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| `RouteLookup::resolve_type_declaration_for_dep` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |
| `FactValidation::validate_fact_signature` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |

### Which methods are hot

A method is hot when it is called at least once per interned node and its
body is a trivial read. By that rule:

| method | calls per node (cold) | body | decision |
| --- | --- | --- | --- |
| `Cancellation::is_cancelled` (with `cancellation_checkpoint`) | 2–10 | thread-local cancellation-token selection | **moved** |
| `FactValidation::current_project_generation` | 0.6–5 | one atomic load of the project-generation clock | **moved** |
| `LiveFactValidation::aggregate_clock_reader` | 1–5 | built a fresh clock reader (four handle clones) per call | **moved** |
| `FactValidation::aggregate_basis_seed` | 1–5 | the request view's compaction-basis seed | **kept**: recorded deviation, below |
| `IndexedInputs::host_view_env_hashes_for`, `host_view_project_identity_for`, `project_stable_key_for_canonical`, `resolve_project_for_canonical` | 0.6–3 | keyed project-configuration lookups per canonical | kept: not a trivial read |
| `FactValidation::validates_fact_signature_with_self_roots`, `complete_graph_signature` | up to 3 | validation work proportional to the signature | kept: not a trivial read |
| `OwnedLowering::*`, `RouteLookup::*`, `IndexedInputs::ensure_indexed_ready_serve` / `prepared_decl_bundle` | up to 1.5 | owned lowering on a miss, route and contributor lookups | kept: coarse operations |
| `ExecutionSubmission::attach_engine`, `HostAttachmentPort::host_attachment`, `IndexedInputs::engine_policy` | once per request | execution binding | kept |

**Deviation: `aggregate_basis_seed` is not moved.** Its value includes the
request view's population. That follows the request's canonical-completion
overlay, which advances while the request runs. Capturing the seed at
admission would change the fact tracer's compaction basis, a cache-validity
input, which a dispatch change must not do. It stays a port method.

## The request snapshot

`RequestSnapshot<W>` (`crates/verter_type_engine/src/resolver_core/resolver_context.rs`)
is engine-owned and holds handles only: the cancellation checkpoint, the
project-generation clock and the live aggregate clock reader. Each request
lifecycle captures it once when the request is admitted
(`VerterHost::capture_request_snapshot`). `LiveFactValidation::request_snapshot`
serves it, and `FactValidation::request_flags` serves its cancellation and
generation handles; both are admission-boundary captures only.
`ProjectSemanticDispatch` borrows it at construction, threads its flag handles
into every producer and cache-runtime entry point below, and reads plain
fields throughout.

The `Cancellation` port and the `current_project_generation` and
`aggregate_clock_reader` port methods are deleted, so the compiler rejects any
remaining caller. Every read through the snapshot is live: a cancellation, a
project reset or a workspace edit that lands mid-request is observed by the
next read. The engine stays non-generic over the context, and a session-only
edit leaves `verter_type_engine` Fresh.

## After (head → candidate)

The moved methods no longer exist. The accessors that replaced them:

| method (after) | rel-aligned-3200 | rel-reversed-2100 | template-369x271 | alias-1000 | cond-200 | tail-rec-1000 | overloads-1025 | contra-500 | ref-infer-1000 | flow-return | component-meta |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `Cancellation::is_cancelled` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `Cancellation::cancellation_checkpoint` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `FactValidation::current_project_generation` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `LiveFactValidation::aggregate_clock_reader` | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `FactValidation::request_flags` | 79 | 20039 | 67 | 12031 | 2443 | 34121 | 6200 | 18056 | 20018 | 126 | 530 |
| `LiveFactValidation::request_snapshot` | 18 | 6005 | 14 | 3007 | 809 | 9030 | 1034 | 3510 | 6004 | 25 | 222 |

The dispatch now reads the generation and cancellation handles as plain
fields. The memo producer, memo publication and cache-runtime paths receive
the borrowed snapshot's flag handles at their entry points: `acquire_query`,
`begin_query_claim`, `claim_query`, `Subscription::wait`, `settle`, `admit`,
the `warm_publish_one*` admission steps, the `cache_runtime` `lookup` entry
points and the `FlowSliceDriver` all take `&RequestFlags` (and, where a basis
sample is needed, `&RequestSnapshot<W>`) alongside the context, threaded from
the snapshot the dispatch borrowed once at construction. The per-node reads
below those entries are plain field reads with no port dispatch. The two
accessors remain only as admission-boundary captures: the dispatch's one
`request_snapshot` per request, and the test-support entries that capture the
handles once before driving the same protocol. The fact-tracer basis no
longer builds a clock reader per scope.

The table above was taken before that threading landed. Re-measuring the two
self-contained lanes on the threaded tree: `FactValidation::request_flags`
falls to 0 across flow-return (was 126) and to 24 across component-meta (was
530; the remainder is the session-side component-meta host paths' own coarse
per-build reads), and `LiveFactValidation::request_snapshot` keeps only the
per-request dispatch captures and the per-cold-build fact-tracer basis
constructions. The scenario lanes need the semantic-perf scenario
directories and re-measure through the same harness when present.

Every scenario answered identically, with identical semantic-node,
memo-entry and relation-proof counts, and every timing verdict is overlap:

| scenario | answer and outcome | semantic nodes | memo entries | relation proofs | time and peak-memory verdict |
| --- | --- | ---: | ---: | ---: | --- |
| library-generic-call | identical | 32 | 37 | 4 | overlap |
| library-map-entries | identical | 45 / 27 | 41 / 29 | 0 | overlap |
| library-promise-then | identical | 157 | 39 | 0 | overlap |
| library-array-map | identical | 134 | 30 | 0 | overlap |
| library-awaited | identical | 9 | 16 | 0 | overlap |
| base-signature-1 | identical | 28 | 29 | 8 | overlap |
| reference-infer-aliases-100 | identical | 721 | 1020 | 102 | overlap |
| reference-infer-depth-10 | identical | 50 | 79 | 2 | overlap |
| infer-pattern-repeat-10 | identical | 96 | 130 | 40 | overlap |
| contravariant-callbacks-10 | identical | 45 | 72 | 20 | overlap |
| overloads-1025 | identical | 3086 | 2071 | 1025 | overlap |
| inference-deposits-1025 | identical | 18 | 23 | 2 | overlap |
| tail-recursive-parse-50 | identical | 486 | 233 | 152 | overlap |
| conditional-chain-97 | identical | 400 | 504 | 1 | overlap |
| alias-chain-50 | identical | 160 | 267 | 0 | overlap |
| template-absorption-400x250 | identical | 918 | 27 | 0 | overlap |
| template-nested | identical | 1024 | 20 | 0 | overlap |
| template-4-spans | identical | 10022 | 20 | 0 | overlap |
| relation-reversed-200 | identical | 632 | 29 / 23 | 20404 / 2018 | overlap |
| relation-aligned-200 | identical | 617 | 424 | 402 | overlap |
| baseline-empty | identical | 5 | 12 | 0 | overlap |

| lane | answers, nodes, memo entries equal | phases timed | time verdict |
| --- | --- | --- | --- |
| alias-chain-200 | yes | setup, cold, warm, editCold, editWarm | overlap |
| component-meta | yes | setup, cold, warm, editCold, editWarm | overlap |
| contravariant-callbacks-100 | yes | setup, cold, warm, editCold, editWarm | overlap |
| flow-return | yes | setup, cold, warm | overlap |
| library-generic-call | yes | setup, cold, warm, editCold, editWarm | overlap |
| overloads-1025 | yes | setup, cold, warm, editCold, editWarm | overlap |
| relation-aligned-600 | yes | setup, cold, warm, editCold, editWarm | overlap |
| relation-reversed-200 | yes | setup, cold, warm, editCold, editWarm | overlap |
| tail-recursive-parse-500 | yes | setup, cold, warm, editCold, editWarm | overlap |

## Outside this change

`relation-reversed-*` warm requests repeat the cold work. A warm request of
`relation-reversed-2100` makes about 3.9 million port calls and is slower
than its cold request. That is a cache-admission question for the semantic
performance work, not a dispatch cost.
