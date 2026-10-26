## Tagged-Component Publication: Root Admission Commits, Members Backfill

A mixed `Relate` / `FlowReturn` / `ResolveCall` component closes as ONE decision, but it does NOT publish as one indivisible write. The contract is two-tiered:

- **Root admission is the COMMIT BOUNDARY.** The machinery root publishes through its family singleflight (`execute_via_cold_build_helper_capturing_publication`). That publish is the component's linearization point: it represents a successfully completed result whose read set, self-roots and generation stamp are already validated.
- **A root's carrier covers the members already queued.** Every root close (`resolve_call_frame_pop`, `flow_frame_pop`, the relation root's `relation_completed_publication_roots`) publishes on the union of its own roots, its drained members' roots and the roots of every member the transaction queued earlier for the batched publish. The queues keep that union as members queue (`CompletedMembers::self_roots`, `CompletedRelations::self_roots`, one root per distinct `(canonical, hash)` observation in queue order), so a close reads it in place rather than re-reading every queued member: a chain of `n` closes stays linear. Different hashes for one canonical survive both queue insertion and root composition; `semantic_graph_carrier` rejects the torn component with `SelfRootConflict`, admitting neither root nor members. Guards: `a_later_root_carrier_covers_members_queued_earlier_in_the_transaction`, `relation_publication_rejects_conflicting_root_and_queued_member_versions`, `relation_publication_rejects_conflicting_versions_between_queued_members` (the latter two exercise ordinary relation execution and same-version warm controls).
- **Member publication is INDEPENDENTLY FENCED BACKFILL.** `relation_drain_completed_members` / `flow_return_drain_completed_members` / `resolve_call_drain_completed_members` run strictly AFTER that commit and hand each member to `publish_{relation,flow_return,resolve_call}_member_fenced`, which re-checks cancellation and the cold-abort sweep at its own publish. A member the fence refuses releases its inline flight, stays COLD, and recomputes on the next demand. Reduced cache completeness after the commit boundary is not corrupted semantics — every member is separately validated and separately recomputable.

A pending call retains its indexed call-site root independently of its operand
roots. Only an identical `(canonical, hash)` pair is redundant: a retained
operand's older hash and the indexed site's newer hash must both reach
`semantic_graph_carrier`, which refuses them with `SelfRootConflict`. The indexed
serve's `Parse` fact does not replace this whole-hash observation. The pending
state owns these roots until component close. Guard:
`call_site_indexed_version_conflicting_with_a_retained_operand_refuses_carrier`,
including same-version call admission and unchanged value delivery controls.

Two things are NOT weakened by that split, and both admit nothing at all:

- **Pre-linearization cancellation** — a component cancelled before the root's admitted publish releases every flight without publishing anything.
- **Genuine component failure** — any degraded / `Unknown` / budget-exceeded / abandoned-session member poisons the WHOLE component: `ReturnOnly`, no entry, no fact signature, no reverse-index metadata, for the root and every member alike.

There is deliberately no retrospective rollback of a committed root when a later member is refused: rolling back would alter established relation semantics without adding correctness, because the refused member is never readable as a stale value — it is simply absent.

Guards: `mixed_component_member_publish_is_fenced_backfill_behind_a_committed_root`, `pre_linearization_cancellation_admits_nothing`, `genuine_component_failure_admits_neither_root_nor_member` (`crates/verter_session/src/project_semantic_dispatch_tests/flow_return_tests.rs`).

