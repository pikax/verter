# Parallel Perf / LSP Orchestration Effort — Coordination with semantic-db-overhaul

> **Purpose.** This documents a parallel, multi-track **performance + LSP-correctness** effort running
> alongside the semantic-db-overhaul (currently at **PARSELOWER Stage 1**). It exists so the overhaul's
> agents can SEE and RESPECT the perf work, and vice-versa. **Bidirectional rule:** this effort respects
> the overhaul's origin plan (`docs/arch/semantic-db-overhaul-unified-remaining-plan.md`, authoritative on
> origin); the overhaul respects the changes documented here. Neither clobbers the other; the
> verter_session-touching parts are explicitly gated to fold in at the overhaul's named anchor.

## Base + integration branch
- Perf work was based on `aa58fdd26`; it was being rebased onto the overhaul via the integration branch
  `lsp-perf/integration-overhaul` (**historical; retired / not load-bearing**). The
  remaining work is now folded into the master plan's §4 `UP` block; see
  [`perf-handoff-to-semantic-overhaul.md`](./perf-handoff-to-semantic-overhaul.md).
- A dedicated **rebase/merge/FF manager reconciles the perf work onto the latest `origin/refactor/semantic-db-overhaul`
  AFTER the DX harness lands.** Its mandate:
  - Respect BOTH plans and BOTH sets of changes — **nothing missed** (the overhaul is a massive >1-week
    effort); respect the overhaul's changes and **improve them where possible**, ownership-preserving (no
    mechanical ours/theirs; duplicated ownership ⇒ API cleanup, not an adapter).
  - **Sanity pass for rule violations:** after reconciling, verify NO CRITICAL-rule / architecture-guard
    violations were introduced (the full guard suite + the CLAUDE.md/skill CRITICAL rules) — neither the
    perf work nor the merge may regress an invariant.
  - The merge/rebase/FF itself goes through the **full 3/3 reviewers-fix cycle** (1 claude + 2 codex,
    fix-until-clean, same revision-cap → architect-escalation rule) so it integrates perfectly.

## Tracks — what each changes (do not clobber; coordinate the gated parts)
- **DX** (test-infra, *in progress*): NEW `packages/dx-harness`, `packages/lsp-test-client`,
  `crates/verter_dx_baseline` — a differential DX-inspection harness (verter vs tsgo/tsserver). Mostly new
  files; consumes verter_session PUBLIC APIs only. Step 12 (`verter_lsp` dx_tests) is GATED.
- **F1 retained-close** (*held gated; spec in the master plan UP.G / U11; historical commit `23c3dc260`,
  source branch retired*): host-owned retained-close — retain read-only external deps across document close.
  Touches `verter_lsp` (lifecycle / documents / host_access) **and `verter_session`** (`host_lifecycle.rs`,
  `types.rs`, `host_upsert.rs`, `file_artifact_store.rs`). ⚠ the verter_session part OVERLAPS the overhaul's
  host work — gated into U11.
- **B typeinfo-perf** (*non-gated G8.2/G9.1/G14.1 absorbed; typeinfo-cache items 1–11 are spec-only; source
  branch `lsp-perf/b-typeinfo-perf` retired — it carried the G9.1 realpath memo, NOT the typeinfo-cache
  work*): `verter_lsp` / `verter_scheduler` / `verter_workspace`. Items 1–11 (typeinfo cache) GATED
  (`verter_session`) — split across the master plan's U3 / U10 / U8 / U12 (UP.G index).
- **D compiler-perf** (*in progress*): `verter_compiler` perf phases. Ungated land independently;
  `I3` / `custom_elements` / `F4` GATED (verter_session string-surgery, output-breaking CSS hash).
- **C lsp-perf** (*in progress*): `verter_lsp` perf. Host-API items 16–20 GATED (`verter_session`).
- **L lsp-bugs** (*in progress*): `verter_lsp` correctness fixes (goto-def, auto-import, intellisense
  recovery). Cluster A (re-parse storm), C.2 (ownership), B-protocol GATED (verter_session / scheduler / workspace).

## Gating — verter_session changes coordinate with the overhaul
> **Superseded by the master plan.** The gated `verter_session`-touching perf wins are now folded into §4 of
> [`semantic-db-overhaul-unified-remaining-plan.md`](./semantic-db-overhaul-unified-remaining-plan.md) — F1 →
> U11, C 16–20 → U11, B 1–11 → U3/U10/U8/U12, D I3/custom_elements → U14, D-F4 → UP.D, F2 → U3 (the UP.G
> index lists every one). Every reference branch below is retired; the gating PRINCIPLE is preserved here for
> historical context.

ALL verter_session-touching perf wins (F1's host_lifecycle, B 1–11, C 16–20, D I3/custom_elements/F4) are
GATED: they fold into the overhaul within their owning U-block, breaking verter_session APIs with no
long-lived compat layer (consumers updated in-place). They will **NOT** land before their U-block. The
historical gate was the named anchor `semantic-db/oracle-green-api-freeze`, sequenced **F2 → B → C → D**
(reference; the per-track branches are retired). Pre-authorized by the user, gated on the U-block + a final
re-confirm.

## Integration order (the perf effort's QUEUE-ORDER)
1. **DX harness** first (so later gates use it). 2. **D ungated** compiler/parser. 3. **verter_lsp
candidate** — replay `F1 → B → L → C` (LSP-only), resolving the verter_lsp overlap once. 4. **Semantic
Session Performance Package** (post-anchor). 5. **Final integration gate → main.**

## Process rules the perf managers follow (FYI for parity)
- **Revision cap → architect escalation:** a review→fix loop escalates to a primed codex-architect for a
  BINDING ruling at 5 revisions OR when "dancing around P2" (≥2 same-class P2-only rounds); but P0/P1
  findings keep the review going — never land over a live blocker.
- **Partial-progress checkpointing:** long-running agents record incremental progress (WIP commits +
  `<task>-progress.md`) so recovery from API instability is lossless (resume, never restart).

## Request to the overhaul team
- Respect the perf branches' `verter_lsp` / `verter_compiler` changes; the rebase manager reconciles onto
  your latest tip ownership-preservingly.
- Coordinate at `semantic-db/oracle-green-api-freeze` for the session-package fold.
- Authoritative perf sequencing = this doc. Authoritative overhaul plan =
  `docs/arch/semantic-db-overhaul-unified-remaining-plan.md` (origin).
