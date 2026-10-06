## Tagged-Component Publication: Root Admission Commits, Members Backfill

A mixed `Relate` / `FlowReturn` / `ResolveCall` component closes as ONE decision, but it does NOT publish as one indivisible write. The contract is two-tiered:

- **Root admission is the COMMIT BOUNDARY.** The machinery root publishes through its family singleflight (`execute_via_cold_build_helper_capturing_publication`). That publish is the component's linearization point: it represents a successfully completed result whose read set, self-roots and generation stamp are already validated.
- **Member publication is INDEPENDENTLY FENCED BACKFILL.** `relation_drain_completed_members` / `flow_return_drain_completed_members` / `resolve_call_drain_completed_members` run strictly AFTER that commit and hand each member to `publish_{relation,flow_return,resolve_call}_member_fenced`, which re-checks cancellation and the cold-abort sweep at its own publish. A member the fence refuses releases its inline flight, stays COLD, and recomputes on the next demand. Reduced cache completeness after the commit boundary is not corrupted semantics — every member is separately validated and separately recomputable.

Two things are NOT weakened by that split, and both admit nothing at all:

- **Pre-linearization cancellation** — a component cancelled before the root's admitted publish releases every flight without publishing anything.
- **Genuine component failure** — any degraded / `Unknown` / budget-exceeded / abandoned-session member poisons the WHOLE component: `ReturnOnly`, no entry, no fact signature, no reverse-index metadata, for the root and every member alike.

There is deliberately no retrospective rollback of a committed root when a later member is refused: rolling back would alter established relation semantics without adding correctness, because the refused member is never readable as a stale value — it is simply absent.

Guards: `mixed_component_member_publish_is_fenced_backfill_behind_a_committed_root`, `pre_linearization_cancellation_admits_nothing`, `genuine_component_failure_admits_neither_root_nor_member` (`crates/verter_session/src/project_semantic_dispatch_tests/flow_return_tests.rs`).

