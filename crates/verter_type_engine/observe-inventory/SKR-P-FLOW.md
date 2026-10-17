# Shared disjunction narrowing environments inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `NarrowingWindowMark` (a persistent share of the frame's product state plus the guard ledger length) | The union-of-facts guard (`apply_guard_union`): each alternative and the guard as a whole restore the narrowing overlay to it, re-installing only the roots the ledger window names instead of copying and re-setting every narrowed subject | REQUIRED | One disjunction evaluation; dropped when the guard returns | `src/project_semantic_dispatch/flow_return_disjunction.rs` | always |
| `StandingIndex` (`slots`, `by_subject`, `kills`): the shared prefix environment and each alternative's delta | The union-of-facts guard's per-alternative narrow of each candidate reference: the newest delta fact, else the prefix fact unless a delta kill retracted it | REQUIRED | One disjunction evaluation (the prefix) / one alternative (a delta) | `src/project_semantic_dispatch/flow_return_disjunction.rs` | always |
| `AlternativeUnion.candidates` (the first alternative's references with their per-alternative narrows) | The narrows a union-of-facts guard and an arm join (`join_arm_narrowings`) establish | REQUIRED | One disjunction evaluation / one arm join | `src/project_semantic_dispatch/flow_return_disjunction.rs` | always |
| `union_of_alternatives` subject indexes (`by_subject`, per-alternative `newest`) | An arm join's per-reference union (`join_arm_narrowings`) | REQUIRED | One arm join | `src/project_semantic_dispatch/flow_return_disjunction.rs` | always |
| `ENVIRONMENT_ENTRIES` / `disjunction_work_observed().environment_entries` (narrowing entries written into disjunction environments) | none; the linear disjunction-work test only | OPTIONAL | Thread-local, cumulative for the thread | `src/project_semantic_dispatch/flow_return_disjunction.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))` |
| `SUBJECT_PROBES` / `disjunction_work_observed().subject_probes` (subject-index lookups and insertions) | none; the linear disjunction-work test only | OPTIONAL | Thread-local, cumulative for the thread | `src/project_semantic_dispatch/flow_return_disjunction.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))` |

The `test-support` arm of the two counters' gate is what lets the
`verter_session` flow tests read them across the crate boundary, as the
existing guard-application counter does; both features are off by default,
so a default build compiles the counters and their increments away.
