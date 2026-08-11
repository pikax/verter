# Specification amendment 1 — transient carrier enumeration

Supersedes the corresponding text in `A2C-SPEC-V3.md`: section 8's carrier list and final paragraph, section 10's root-adapter bullet, and section 13's test list and mutation items. All other specification text stands unchanged.

## Verdict

The implementer is correct. This is a specification defect, not a contract change and does not require maintainer ratification.

The verified success path is:

`FlowEvaluationOutcome.flow_gap`
→ `FlowReturnPendingState.flow_gap`
→ `DrainedFlowReturnMember.flow_gap`
→ `FlowDischargeEntry.flow_gap`
→ `DrainedFlowReturnMember.flow_gap`
→ `CompletedFlowReturnMember.flow_gap`
→ sealed adapter
→ existing admitted/public `FlowReturnResult`

Evidence:

- The pending state is created for a non-root flow frame at [flow_return.rs:1319](/<REPO>-wt-a2c/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1319).
- All three root domains drain it into `DrainedFlowReturnMember`: [relation.rs:1177](/<REPO>-wt-a2c/crates/verter_session/src/project_semantic_dispatch/relation.rs:1177), [flow_return.rs:1372](/<REPO>-wt-a2c/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1372), and [call_resolve.rs:387](/<REPO>-wt-a2c/crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:387).
- The drained member enters fixed-point discharge at [call_resolve.rs:498](/<REPO>-wt-a2c/crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:498) and [relation.rs:1991](/<REPO>-wt-a2c/crates/verter_session/src/project_semantic_dispatch/relation.rs:1991).
- Successful discharge converts it to `CompletedFlowReturnMember` at [relation.rs:1729](/<REPO>-wt-a2c/crates/verter_session/src/project_semantic_dispatch/relation.rs:1729).

`call_resolve.rs:387` is therefore a required transfer site. By contrast, `call_resolve.rs:763` is only the abort/cleanup path; it consumes the collection for flight cleanup and needs no special `flow_gap` logic.

## Exact amendment

Replace section 8’s text from “Add to `FlowEvaluationOutcome`…” through the affected-carriers list with:

```markdown
Add to [FlowEvaluationOutcome](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:246) and every transient pending/SCC transfer carrier:

```rust
flow_gap: Option<FlowGap>
```

`flow_gap` is an unchanged sidecar across pending-ledger drain, mixed-component fixed-point discharge, and completed-member routing. Copy it at every carrier construction and transfer. The discharge algorithm must not recompute, merge, clear, or derive it from the discharged `FlowReturnResult`.

The affected carriers and required transfer sites are:

- `FlowEvaluationOutcome` in `project_semantic_dispatch/flow_return.rs`;
- [FlowReturnPendingState](<REPO>/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:631);
- [DrainedFlowReturnMember](<REPO>/crates/verter_session/src/project_semantic_dispatch/relation.rs:313), including every construction from `FlowReturnPendingState` in `relation.rs`, `flow_return.rs`, and `call_resolve.rs`;
- [FlowDischargeEntry](<REPO>/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1911), including every construction from a root or drained member and every transfer back to that drained member in `relation.rs`, `flow_return.rs`, and `call_resolve.rs`;
- [CompletedFlowReturnMember](<REPO>/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1836), including construction from `DrainedFlowReturnMember` in `relation.rs` and direct construction for an inline SCC root in `flow_return.rs`;
- `FlowRootClose::Complete` in `project_semantic_dispatch/flow_return.rs`.

For section 8, production edits are permitted only in:

- `crates/verter_semantic/src/analysis/flow/completion.rs`;
- `crates/verter_session/src/cache_runtime/flow_slice_node.rs`;
- `crates/verter_session/src/project_semantic_dispatch/flow_return.rs`;
- `crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs`;
- `crates/verter_session/src/project_semantic_dispatch/relation.rs`;
- `crates/verter_session/src/project_semantic_dispatch/call_resolve.rs`.
```

Keep the existing “Do not add the field…” sentence unchanged.

Replace section 8’s final paragraph with:

```markdown
Under A2C, one sealed adapter owns both exits from the transient carrier graph: the machinery-root `FlowRootClose::Complete` exit and each `CompletedFlowReturnMember` conversion before `publish_scc_member_batch` constructs an admitted `PendingFlowReturnMember`. At both exits it drops `flow_gap` and returns the existing `FlowReturnResult` unchanged. A3 changes only this adapter to match `FlowGap::AbruptCompletion` and retract/non-admit. A3 receives no graph, skeleton, event, region, edge, or endpoint accessor.
```

## Dependent amendments

Section 10’s reasoning remains valid, but replace its root-adapter bullet with:

```markdown
- the A2C sealed adapter discards it at both transient exits—machinery-root return and completed-member backfill—before public return or admission;
```

Section 13 does need a discriminator for the omitted path. Add this test to `u6_flow_a2c_non_interference_tests.rs`:

```markdown
- `a2c_flow_gap_survives_all_pending_scc_drain_domains`
```

Add immediately after the test list:

```markdown
`a2c_flow_gap_survives_all_pending_scc_drain_domains` must drive a real non-root flow member carrying the exact G10 gap through each pending-ledger drain domain: a flow-return root, a relation root, and a resolve-call root. It must not construct the transfer carriers directly. For each domain, assert that the exact `FlowGap` reaches `CompletedFlowReturnMember` unchanged, and that the A2C sealed adapter then preserves the existing public result and admission behavior.
```

Add mutation item 4:

```markdown
4. In turn, replace the `flow_gap` transfer with `None` at each of the three `DrainedFlowReturnMember` construction sites in `relation.rs`, `flow_return.rs`, and `call_resolve.rs`. Each mutation must independently fail `a2c_flow_gap_survives_all_pending_scc_drain_domains`.
```

No other specification section depends on the carrier enumeration being closed. The worktree was not changed.

__DONE__
