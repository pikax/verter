# D2C DEFER ruling and debt row: flow-return audit partiality (rev11.flow)

- Status: accepted
- Date: 2026-09-01
- Adds: new DAG node `D2C` (predecessor `D2B`; explicit predecessor of `D3R`) and its charter `charters/rev11-flow/D2C.md`. Amends no other node's budgets or boundaries.
- Scope: this is the durable debt-row record for the `FLOW-RETURN-AUDIT-PARTIALITY` deferral. This repo's established debt-row convention (see e.g. `RESIDUAL-NON-CALL-ANY-FABRICATION`, `crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:599`) records debt inline where the finding was disposed; this deferral's finding was disposed during roadmap (`roadmap/0.1.0-tama/`) review of the D2B candidate, so its debt row is recorded here as a roadmap decision rather than in a `crates/` test manifest.

## Context

The D2B round-16 architect review (`.feedback/rev11/d2-intent-conformance.md`
consult, `.feedback/rev11/d2-intent-conformance-out.txt` ruling) found that
`RequestKind::FlowReturnInference`'s audit payload (`FlowReturnInferencePayload`,
`crates/verter_audit/src/payloads/flow_return.rs`) reports only three occurrence
counters — `cold_computes`, `budget_exceeded_events`, `cycle_reentry_holds` — none
of which explains WHY a request came back partial or no-value.
`.claude/skills/audit-infrastructure/SKILL.md` documents exactly those three
counters, confirming the gap is real and not merely undocumented.

## Ruling (architecture authority, verbatim)

> ### b. Flow gaps absent from audit — **DEFER**
>
> This is an independently acceptable observability projection: it changes
> neither semantic value nor admission authority and lies outside D2B's declared
> production surfaces. Assign it to a new durable successor:
>
> - **Owner block:** `D2C — Flow-return partiality audit projection`
> - **Debt row:** `FLOW-RETURN-AUDIT-PARTIALITY`
> - **Resolution gate:** D2C must land before `D3R` dispatch; it may not survive
>   to plan close.
> - **Acceptance ID/test:** `D2C-AC2`, `flow_return_audit_explains_partial_cold_recompute`—a
>   reachable production `FlowReturn` guard gap returns its usable partial value
>   twice, both requests are cold, and each `FlowReturnInferencePayload` reports
>   typed partiality plus `GuardNarrowing`; a complete control reports no
>   partiality and its second request is warm.
>
> The current audit contract documents only the three counters
> (`.claude/skills/audit-infrastructure/SKILL.md`), confirming that this is a
> distinct payload-extension outcome. Until the debt row and D2C authority are
> landed, the DEFER disposition itself is incomplete.

## Debt row

- **ID:** `FLOW-RETURN-AUDIT-PARTIALITY`
- **Finding:** `RequestKind::FlowReturnInference`'s audit payload carries occurrence
  counters but no typed reason, so a caller cannot tell WHY a `FlowReturn` request
  was partial (degraded-but-usable) or no-value from the audit record alone.
- **Durable owner block:** `D2C` — "Flow-return partiality audit projection"
  (`charters/rev11-flow/D2C.md`, `authority/dag/rev11-flow.toml`).
- **Resolution gate:** D2C must land before `D3R` dispatch (encoded structurally:
  `D3R`'s DAG predecessors now include `D2C`) and may not survive to plan close.
- **Acceptance ID/test:** `D2C-AC2`, `flow_return_audit_explains_partial_cold_recompute`,
  exactly as quoted in the ruling above.
- **Ruling reference:** `.feedback/rev11/d2-intent-conformance-out.txt` (finding b),
  consult at `.feedback/rev11/d2-intent-conformance.md`, this decision.

## Decision

1. `D2C — Flow-return partiality audit projection` lands as a new DAG node,
   predecessor `D2B`, in `authority/dag/rev11-flow.toml`, with its own charter
   `charters/rev11-flow/D2C.md`.
2. `D3R`'s DAG predecessors gain `D2C` (alongside the existing `D2B`, `C1`), so
   `D3R` cannot become READY until D2C has an implemented ledger row — the
   resolution-gate half of the ruling ("D2C must land before D3R dispatch"),
   held structurally rather than by convention.
3. D2B does NOT own this debt and does not gain a predecessor edge; the finding
   is explicitly outside D2B's declared production surfaces and D2B's own
   acceptance criteria (D2B-AC1–AC4) are unaffected by it.
4. Per repo convention, D2B's charter (`charters/rev11-flow/D2B.md`) is corrected
   in place to append the two round-16 amendments this same review round also
   ruled on (the whole-control-position tri-state boundary and the
   unclassifiable-guard-arm ADOPT-NOW rule) — a separate correction, recorded in
   those charters directly, not part of this DEFER.

## Consequences

- This decision is the durable authority record satisfying the ruling's own
  condition: "Until the debt row and D2C authority are landed, the DEFER
  disposition itself is incomplete." With this document, the new DAG node, and
  the new charter landed together, the DEFER disposition is now complete.
- D2C's own acceptance criteria (D2C-AC1–AC4) are specified in full in its
  charter; only AC2's exact name and proof shape are dictated by this ruling —
  AC1/AC3/AC4 are derived from D2C's stated purpose (a strictly additive,
  read-only projection) per this repo's normal charter-authoring practice.
- No other node's budget, boundary, or predecessor list changes.
