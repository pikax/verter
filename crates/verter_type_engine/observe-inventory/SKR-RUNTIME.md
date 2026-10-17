# Continuation-runtime polling inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `Program::poll_subscription` (replaces the blocking `Program::wait`) | `SemanticExecution::poll` resolves the chain tip's outside producer, or returns `Pending` with the poll's waker registered | REQUIRED | One subscription | `src/semantic_execution.rs` | always |
| `SemanticExecution.root` | Later polls of an open drive continue its root; cleared when the poll is `Ready` | REQUIRED | One drive, first poll to `Ready` | `src/semantic_execution.rs` | always |
| `SemanticExecution.request_context` | Installed through `RequestContextLike::install_tls` around each step when bound | REQUIRED | The execution | `src/semantic_execution.rs` | always |
| `PARK_WAKER` (thread-local unparking waker) | `SemanticExecution::drive` polls with it and parks between polls | REQUIRED | The thread | `src/semantic_execution.rs` | always |
| `ExecutionCounters::pending_polls` | none — read by the runtime's tests | OPTIONAL (counter) | The execution | `src/semantic_execution.rs` | `cfg(any(test, feature = "semantic-observe"))`; absent from default builds |
| `InflightState.wakers` | `FlightCell::wake_subscribers` wakes every poller registered while the flight was unresolved | REQUIRED | One flight, until it resolves | `src/semantic_query_memo/inflight.rs` | always |
| `Subscription.parked` (wait-for edge, wait start, registered waker) — the wait start moved from a local of `Subscription::wait` | The edge carries the existing wait-cycle refusal across `Pending`; the start feeds the existing `waits_ms` stat; the waker is removed from the flight when the subscription resolves or drops | REQUIRED | One parked subscription | `src/semantic_query_memo/producer.rs` | always |
| `PendingClaim.attempt` (now taken when the subscription resolves) | The claim continues in `QueryProgram.retries` after a restart | REQUIRED | One pending claim | `src/project_semantic_dispatch/query_frames.rs` | always |
| `TypeofBuild.body` / `TypeofBuild.then` (`TypeofBody`, `TypeofThen`, `SignatureLowering`) | `drain_typeof` resumes the body lowering or initializer evaluation it holds, and lowers a function value's remaining signatures one at a time | REQUIRED | One `typeof` frame | `src/project_semantic_dispatch/build.rs` | always |
| `IndexedValueRun` (path, waiting records, next action) | `drain_indexed_value` resumes an initializer's indexed evaluation at the leaf whose `typeof` it waited on; the inline evaluator runs the same run to completion | REQUIRED | One indexed evaluation | `src/project_semantic_dispatch/flow_return.rs` | always |
| `ExpressionSourceRun` (route state, serve, pending-obligation mark, observation carried between steps) | `drain_semantic_expression_source` re-opens the probe's observation each step (`ObservedRead::resume`) and settles it once, as the inline probe does | REQUIRED | One `typeof` frame | `src/project_semantic_dispatch/flow_return.rs` | always |
| `FrameDelivery` | A `typeof` build consumes its delivery inside its own evaluation's observation | REQUIRED | One step | `src/project_semantic_dispatch/query_frames.rs` | always |
| `ConditionalBuild` (answers, awaited read, its claim) | `drain_conditional` runs a conditional's decision again with every answer its frame holds; the claim continues in `QueryProgram.retries` | REQUIRED | One conditional frame | `src/project_semantic_dispatch/query_frames.rs` | always |
| `ProjectSemanticDispatch.decision_seam` (`DecisionSeam`: answers, need, claim, frame depth, in-place nesting) | `read_at_decision_seam` answers the running decision's instantiation and `typeof` reads, or records its need; `nested_decision_query` marks the queries it evaluates in place | REQUIRED | One decision run | `src/project_semantic_dispatch/query_frames.rs` | always |
| `ProjectionSeam.awaiting_decision` / `ProjectionSeam.decided` | `drain_projection` hands a delivered decision to the conditional that needed it | REQUIRED | One projection run | `src/project_semantic_dispatch/locator_view_worklist.rs` | always |

No production trace or charge history is added. The one new counter is gated behind `semantic-observe`.
