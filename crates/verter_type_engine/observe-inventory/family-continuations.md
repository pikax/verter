# Family continuation state

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
The interface is described in [root continuations](../../../docs/arch/root-continuations.md).

| Item | Production consumer | Classification | Lifetime | Owner module | Gate |
| --- | --- | --- | --- | --- | --- |
| `FamilyBuild`, `FamilyStep` | Both drivers carry a family build between its stages: the build's output, the query it reads next or the file it reads next, with everything its stages computed | REQUIRED | One family build | `src/project_semantic_dispatch/family_continuation.rs` | always |
| `MacroPayloadBuild`, `MacroPayloadRequest`, `MacroPayloadRooting` | A macro payload resumes after its owner's load and after its payload operation's read from the owner facts and fence its earlier stage recorded; the owner is read once | REQUIRED | One macro payload build | `src/project_semantic_dispatch/continuation_stages.rs` | always |
| `ReduceIntersectionBuild` | An intersection reduction resumes at the step after the nested subgroup it read, with the operands its earlier steps took | REQUIRED | One intersection reduction | `src/project_semantic_dispatch/continuation_stages.rs` | always |
| `ContinuationStage` | The capability every stage runs with; it holds the dispatcher in a field private to its module and exposes no query read, file read or load, producer join or driver | REQUIRED | One stage call; no retained state | `src/project_semantic_dispatch/family_continuation.rs` | always |
| `FrameBuild::Family`, `FrameBuild::FamilySource` | A query frame owns a family build across a dependency's delivery or a file's load, and across thread transfer | REQUIRED | One query frame | `src/project_semantic_dispatch/query_frames.rs` | always |
| `QueryProgram.suspends_loads` | A production poll's drive waits on a file's load as an outside producer; the synchronous entry's drive loads it in place as its external driver | REQUIRED | One semantic task | `src/project_semantic_dispatch/query_frames.rs` | always |
| `EvalStep::Await`, `PendingWait::Load` | The runtime parks a frame on an outside input that is not a demand and steps the same frame again when it resolves | REQUIRED | One parked frame | `src/semantic_execution.rs`, `src/project_semantic_dispatch/query_frames.rs` | always |
| `IndexedReadiness`, `IndexedLoad`, `IndexedLoadOutcome` | The resolver port answers whether a file is servable without loading it, or hands an owned load to wait on whose typed terminal outcome the waiting frame consumes once | REQUIRED | One load wait | `src/resolver_core/request_ports.rs` | always |
| `LoadWait`, `LoadEnded` | A family frame keeps its load's terminal outcome where the runtime records it, so the frame reads the file from that outcome instead of beginning the load again | REQUIRED | One parked family frame | `src/project_semantic_dispatch/query_frames.rs` | always |
| `continuation_builds_begun`, `continuation_stages_resumed` | None; tests compare the stages each driver ran to detect a stage run again, including replay shared by the synchronous and polled entries | OPTIONAL | One semantic graph store | `src/semantic_query_memo/stats.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))` |

The host side of the load port (`HostIndexedLoad` in
`crates/verter_session/src/host_load.rs`) holds the owned scheduler handoff and
integrates the file once Analysis is ready, as `VerterHost::ensure_loaded`
does; it retains nothing beyond the wait and its terminal outcome. The
test-only `loads_begun` counter (`crates/verter_session/src/host_test_force.rs`)
counts the loads a host begins, owned or in place, and exists only under
`cfg(test)`. The test-only
`SourceLoadGate` (`crates/verter_session/src/host_test_force.rs`) holds a
scheduler source read for held-load tests and exists only under `cfg(test)`.

No production decision reads the counters. A default build contains neither
their storage nor their increments or readers. No history or trace is added.
