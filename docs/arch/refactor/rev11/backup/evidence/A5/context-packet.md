# A5 context packet — verbatim dispatch prompts

**One byte-level normalisation, disclosed rather than silent.** These prompts named the main checkout
and the a5 worktree by absolute machine path (three occurrences). That exact string is a tracked-path
violation the `tracked_paths_no_machine_roots` guard rejects (discovered as A6 discovery D-1), so it is
replaced here with `<MACHINE_ROOT>`, matching the convention A6's own context packet uses. Nothing else
is altered. This repair changes this file's digest; the ledger's `block.A5.context_packet_digest` field
is updated in the same acceptance transition that records this fix.

## 1. Implementer dispatch prompt

```
You are a bounded BLOCK IMPLEMENTER for the Verter Revision 11 architecture program, block A5.

Working directory: this repo (dedicated git worktree, branch work/a5-inventories, based on program/architecture-lock at 147258e0b, which includes A4's landed attribution substrate). Do not touch <MACHINE_ROOT>/verter or any other worktree. You are the only implementer against this worktree.

AUTHORITY (read before acting, in this worktree):
- docs/arch/refactor/rev11/charters/A5.md (ratified charter — binding)
- docs/arch/refactor/rev11/program.md section "A5 — Complete current-owner, identity, profile, compatibility, protocol, and consumer inventories"
- docs/arch/refactor/rev11/governance.md (Foundational review class requirements)
- docs/arch/refactor/rev11/amendments/*.md — skim for anything mentioning A5 (AMD-002/003/004 confirm the lineage A4→A5→A6 but do not add A5-specific obligations beyond program.md; confirm this yourself)
- Predecessor: A4, accepted/landed at 147258e0b (this worktree's base)

OBJECTIVE (from charter + program.md, binding — this is primarily an INVENTORY/DECISION-RECORD block, not a large code-writing block):
Against the exact post-safety, instrumented lineage (post-A4), resolve current authorities and every affected direct consumer; classify all versions/domains and configuration fields; enumerate TypeExpr/component-meta/graph/wire consumers; lock the dependency-direction test strategy; reconcile open changes and surviving instrumentation owners (in particular: decide the fate/ownership of `crates/verter_session/src/loop5_instrumentation.rs` against A4's new `verter_audit::attribution` substrate — do they coexist, does one subsume the other, is there a migration plan — and settle A4's recorded gate-coverage debt for the `attribution`/`compile-fail` features not being compiled by `node scripts/gate.mjs`); decide evidence paths, program-state custody, worktree/branch naming, CI/merge constraints, review contexts, and bounded stack-window policy.

Exit criterion (from program.md): later blocks cannot create a second owner or discover a hidden public/wire consumer mid-cutover by omission.

IN SCOPE: only the deliverables named for A5 in program.md (see "Required evidence" in the charter: resolved current-owner rows; option classification; domains; consumer/protocol map; dependency test plan; evidence custody; program-state workflow; worktree/branch/CI/merge rules; bounded stack-window and review-context policy); evidence/source changes strictly necessary to produce those deliverables; deletion of obsolete evidence-only scaffolding created by this block before you finish.

OUT OF SCOPE: later production ownership or API cutovers; compatibility shims, parallel architecture paths, or speculative services; numeric threshold selection after candidate direction is observed (A6's job). This is NOT a license to actually perform the loop5_instrumentation reconciliation as a code change — your job is to PRODUCE THE DECISION RECORD (own it, resolve it as an inventory/decision artifact) for what A5 is chartered to decide; if the charter's own text implies an actual code migration is needed, treat that as a scope question to flag/RESCOPE rather than silently doing a large migration disguised as "inventory".

ABORT/RESCOPE: if the exact checkout, command target, product capability, current owner, compatibility obligation, or proof boundary differs materially from charter assumptions — STOP, do not improvise a substitute design, and write status RESCOPE_REQUIRED with the exact contradiction to .agent-run/a5-report.yaml. Do not silently widen scope. In particular: if "reconcile ... surviving instrumentation owners" turns out to require nontrivial production code changes beyond a decision record (e.g. actually deleting/merging loop5_instrumentation.rs), STOP and report exactly what's required rather than executing a large unscoped migration.

TDD / EVIDENCE DISCIPLINE (mandatory, no exceptions per CLAUDE.md):
- No stubs, no always-true assertions, no unconditional default returns presented as coverage.
- Every decision record must be evidence-backed (cite actual file/line/owner from the current tree — an inventory of "everything currently true", not aspiration).
- If A5 produces any executable artifact (e.g. a consumer-enumeration script/test, a dependency-direction guard), it must be a real discriminating check, not decorative.

WORK PROCESS:
1. Read the charter and program.md A5 section carefully; enumerate exactly what "Required evidence" items mean concretely in this codebase.
2. Explore the current Rust + TS workspace to build each inventory: current-owner rows (which crate/module owns what, per the "Shared Optimized Codebase" CLAUDE.md section — cross-check against reality, don't just restate CLAUDE.md), TypeExpr/component-meta/graph/wire consumer enumeration (search for actual consumers of the typeinfo wire contract, component-meta public API, etc.), dependency-direction test strategy (what test(s) currently enforce no-upward-deps and what's still missing).
3. Specifically resolve the loop5_instrumentation.rs vs verter_audit::attribution reconciliation as a DECISION (keep both with named boundaries / plan a follow-on migration block / other) — do not silently do a large code migration; if a small, clearly-scoped code change is the natural output of the decision (e.g. adding a doc-comment cross-reference, or wiring attribution into the gate per A4's deferred debt if that's genuinely a small change), it's fine to do it, but keep it bounded and note the judgment call.
4. Decide and document: evidence paths convention (matches A0/A4 precedent — evidence/A5/ + A5-summary.md), program-state custody rules (confirm the orchestrator-writes-ledger convention already in use is what A5 should formalize), worktree/branch naming convention (matches what's already been used: work/a<N>-<slug>, review/a<N>-<role>), CI/merge constraints, review contexts (matches governance.md's Foundational review class), bounded stack-window policy (what does "stack window" mean here — check governance.md/ORCHESTRATOR.md for the term and resolve it concretely, don't invent a definition).
5. Write the evidence: docs/arch/refactor/rev11/evidence/A5-summary.md + evidence/A5/ directory with the concrete inventory artifacts (owner tables, consumer maps, etc. — these can be markdown/TSV as appropriate, matching A4's precedent).
6. Run any targeted tests relevant to what you touched. If you make no source code changes (pure inventory/docs block), say so plainly — do not run cargo test needlessly, but DO verify any decisions against the actual codebase (grep/read, not assumption).
7. Ensure `git status --porcelain` shows only intended changes.
8. You MAY make WIP commits freely as you work (plain descriptive messages, no AI attribution) — do not squash, the orchestrator squashes at landing.
9. Write your final structured report to .agent-run/a5-report.yaml (fields: block, status DONE|BLOCKED|RESCOPE_REQUIRED, changed files, decisions made, evidence paths, discoveries, deletions, open questions for the orchestrator).

Follow "no phase archaeology": do not reference "A5"/"phase"/"block"/"rev11" in source code comments/identifiers under crates/packages/scripts — docs/evidence only.

Follow CLAUDE.md hard rules (no stubs, no co-author lines, no git push).
```

## 2. Fix-pass dispatch prompt (post-adversarial-review corrections)

```
# A5 fix pass — apply adversarial review corrections (evidence-only)

You are working in worktree <MACHINE_ROOT>/verter-a5, branch work/a5-inventories,
HEAD currently at 094060bcc. Do NOT discard existing commits — add new commits on top.

Scope: A5 is a decision-record/inventory block. Charter forbids source changes. This fix pass corrects
**factual accuracy errors** in the evidence markdown/TSV files identified by an independent adversarial
review. Do NOT touch anything under crates/, packages/, scripts/, .github/ — evidence-only, same as the
original block. Do NOT reopen or re-litigate any decision (L1, G1, DD1, R-12, S1, P3) — only correct the
factual claims/citations backing them.

Apply exactly these 9 corrections, each independently verified against source by the reviewer (reproduction
commands given — re-verify each yourself before editing):

## ADV-1 (BLOCKING) — compatibility-domains.md Finding D-3 / A5-summary.md
False claim: "no in-tree producer" for provider_protocol_version = 12.
Reality: `const PROVIDER_PROTOCOL_VERSION: ProviderProtocolVersion = ProviderProtocolVersion(12);` at
crates/verter_protocol/src/consumer_compatibility_manifest.rs:74, consumed at line 109. The committed JSON
is a generated+test-pinned mirror of it (tests `generated_consumer_manifest_is_fresh`,
`public_hash_grammar_and_version_domains_are_closed`), not an independent source.
Fix: rewrite D-3 (and the "Claims this block does not make" section in A5-summary.md, and owner-rows.md
Row 13) to name the real producer at that file:line. Reframe the open question as: does this hand-pinned
const duplicate a compatibility domain owned elsewhere (per ADR-002), and why is it hand-maintained while
component_meta_schema_version is sourced from its owner? That is the real H2 hand-off question — do not
leave "producer missing" language anywhere.

## ADV-2 (BLOCKING) — open-changes.md / R-12
False universal claim: "every unlanded branch is a net deletion against main" as the mechanical test behind
abandon-as-a-class. Reviewer ran A5's own prescribed test over all 469 branches: 468/469 net-delete, but
`port/rust` is a net +370,822 (13,040 files, +3,135,590/-2,764,768).
Fix: either (a) give `port/rust` an explicit individual disposition (reviewer notes it's a 2026-02-07 branch
whose merge-base predates 2de3b2d07, so the existing lineage bound already covers it — verify this yourself
against git log before writing it), or (b) restate R-12's justification on the lineage-bound test rather
than the net-deletion test, so the class claim is accurate. Pick whichever is actually true after you verify
port/rust's merge-base; do not just assert the reviewer's suggestion without checking.

## ADV-3 (MODERATE) — owner-rows.md Row 16
Mis-attributes HostAuditRuntime to crates/verter_session/src/request_context.rs with "18 references".
Reality: real file is host_audit_runtime.rs:66; 22 references in that file, 37 total in tree; no file has 18.
Fix: correct Source field and reference counts in Row 16 (verify counts yourself with grep before writing).

## ADV-4 (MODERATE) — loop5-counter-census.tsv
Two wrong production/test split columns:
- WARM_HIT_FAST_PATH_HITS: says 8 production/0 test; actual 3 production (mod.rs) / 5 test
  (semantic_query_memo/tests.rs). Decision L1b cites this counter by line range — the TSV must match.
- SLOT_BINDING_EXPANDED_INSTANTIATE_CALLS: says 6/1; actual 5/2 (one "production" ref is actually in a
  test file).
Fix both rows' split columns. Verify with grep yourself; do not just copy the reviewer's numbers blindly —
confirm them independently.

## ADV-5 (MODERATE) — option-classification.md
HostConfig claimed "26 rows (25 named + pool group)" at types.rs:500; actual struct has 23 fields. 3 extra
rows double-count nested sub-struct fields (synthesis_steps, walker_pathological_cap from
RecursionBudgetOverrides, and a third field you must locate yourself by reading the struct and the table).
Fix: correct the count and either clearly mark the nested fields as sub-rows (not part of the top-level
23-field tally) or remove them from the top-level count.

## ADV-6 (MINOR) — open-changes.md §5, owner-rows.md Row 1, program-operations-policy.md W-1, A5-summary.md
"Nine live worktrees" is wrong arithmetic — the table itself lists 3 + 7 = 10 (…/verter, …/verter-a5,
…/a4-adv-control, plus 7 under .claude/worktrees/). Fix every place this count is repeated ("nine"/"9") to
say ten (10), consistent with the table's own rows. Do not just fix the heading — grep for all repetitions.

## ADV-7 (MINOR) — dependency-direction.md / A5-summary.md (verter_tsgo_api edge)
The verter_semantic -> verter_workspace -> {verter_scheduler, verter_tsgo_api} chain is real, but
verter_scheduler is an unconditional dependency while verter_tsgo_api is declared under
`[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` in crates/verter_workspace/Cargo.toml — the
consequence ("linking the kernel links a task scheduler and an out-of-process tool client") holds only for
native targets, not wasm32.
Fix: add the platform qualifier wherever this consequence is stated (A5-summary.md, dependency-direction.md
§4). B1's equality-pinned exception should carry the platform qualifier too if referenced there.

## ADV-8 (MINOR) — instrumentation-reconciliation.md §1.1
"95 call sites across six crates (verter_session 50, verter_audit 16, verter_compiler 13, verter_scheduler
6, verter_workspace 5, verter_napi 3, verter_wasm 2)" — that parenthetical lists SEVEN crates, not six.
Fix: change "six crates" to "seven crates" (arithmetic/figures are otherwise correct — verify with the
reviewer's grep command before editing).

## ADV-9 (MINOR) — consumer-protocol-map.md §2
selective_component_meta.proto reported as "25 messages / 3 enums" using top-level-only enum counting, while
the sibling rows (typeinfo 36, component_meta 45) count all enums including nested ones. For
selective_component_meta.proto: ChildKind, PrimitiveKind, BatchExpandError (top-level) + nested `Reason` at
line 263 = 4 if counted the same way as the siblings.
Fix: use ONE consistent counting method across all three proto rows in the table. Either recount the two
sibling rows top-level-only, or change this row to 4 (matching the siblings' "count everything" method) —
verify which method the siblings actually used before picking, and state the method explicitly in the table
header/footnote so it's unambiguous going forward.

## Constraints
- Evidence-only: `git diff --stat` at the end must show changes ONLY under docs/arch/refactor/rev11/evidence/.
- Verify every fact yourself against real source before writing (grep, git log, direct file reads) — do not
  transcribe the reviewer's numbers without independent confirmation, per the "Verification Must Prove
  Execution" rule in CLAUDE.md (negative claims especially — the two BLOCKING findings were both
  under-verified negative claims).
- Commit as many WIP commits as you like, plain descriptive messages, NO AI attribution lines.
- Do not touch any file outside docs/arch/refactor/rev11/evidence/.
- Do not reopen decisions L1/G1/DD1/R-12/S1/P3 — R-12 may get a restated justification (ADV-2) but the
  abandon-as-a-class ruling itself is not up for debate.
- When done, write <MACHINE_ROOT>/verter-a5/.agent-run/a5-fix-report.yaml with:
  fields: block, fixes_applied (list of ADV ids with a one-line description of exactly what changed and
  where), files_changed (list), commits (list of sha:message), verification_notes (how you independently
  confirmed each fact), scope_check (confirm zero files outside docs/arch/refactor/rev11/evidence/ changed).
```
