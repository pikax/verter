# Go-to-definition overhaul — orchestration handoff (Phases 3 → 6/7)

This is the live handoff prompt for a fresh **pure-orchestrator** session continuing the go-to-definition overhaul. Phases 1 & 2 are landed and reviewed-clean on branch `fix/goto-definition-overhaul`. Paste the prompt below into a new Claude Code session started in `D:/dev/personal/verter`.

Companion docs in this directory:
- `goto-definition-overhaul-plan.md` — the authoritative phased implementation plan (rev 2).
- `goto-definition-architecture-decision.md` — the binding architecture.
- `goto-definition-review-findings.md` — round-1 review findings + binding OQ resolutions.

---

# Orchestrate the go-to-definition overhaul — continuation (Phases 3 → 6/7)

You are a **PURE ORCHESTRATOR** continuing a locked, reviewed, multi-phase implementation plan. You NEVER write/edit production code, never run the test suite yourself, never review diffs line-by-line yourself — you decompose, brief sub-agents, verify git state, gate every phase on **three independent reviewers**, consult codex on any doubt, and manage the worktree/branch/state. **Load the `/multi-agent-orchestration` skill now and activate Serena (project `verter`).**

**STANDING RULE (from the user):** when a phase **lands** (3/3 reviewers ACCEPT + independent gate green), **STOP and write a handoff prompt like this one for the next fresh agent** — do NOT continue to the next phase in the same session. This keeps each orchestrator's context fresh.

## Current state (verified)
- Repo `D:/dev/personal/verter`; work happens in the dedicated worktree **`D:/wt/goto-def`** on branch **`fix/goto-definition-overhaul`**, **HEAD `277c7f69f`**.
- **Phases 1 & 2 are LANDED, reviewed-clean (3/3 each).** Commit chain (oldest→newest):
  `0b3a63894` (grounding base) → `879044859` docs(plans) → **P1:** `a5f6af4c9`,`4d896b70b`,`54ecd9da7`,`be883288f`,`609c3d17a` → **P2:** `b75de5fd7`,`5e5b07748`,`b8109829a`,`7be04c86e`,`f8eeca03b`,`54ec048d4`,`6da268c2a`,`277c7f69f`.
- **Inputs** (committed in the worktree at `docs/plans/`, and in this repo at `docs/arch/`): the plan (`goto-definition-overhaul-plan.md`), the binding architecture (`goto-definition-architecture-decision.md`), the review findings (`goto-definition-review-findings.md`). The plan is authoritative.

## THE GATE BASELINE (critical, hard-won — bake into every brief)
- **ALWAYS run `cargo test --workspace --tests --no-fail-fast`.** Plain fail-fast TRUNCATES at ~70 of 118 binaries and looks like a pass.
- **8 PRE-EXISTING `verter_session` failures** (stale `fact_dep_signature` source-greps from the in-flight refactor — unrelated): in `fact_matrix`: `compile_tier_signature_carries_{import_ref,member,member_presence,module_augmentation_index_shape,route_surface}`; in `g_family`: `family_a_entries_carry_fact_dep_signature`, `family_a_warm_hit_uses_fact_validation`; in `g_misc0`: `materialise_structure_entry_carries_dep_signature`. **Gate = green EXCEPT these 8, ZERO new.**
- `inline_execute_clears_all_install_tls_slots` = Windows UAC skip (os error 740, "never executed") — not a failure.
- `verter_scheduler::tests::bump_generation_supersede_dispatch_skip_no_spurious_panic` = known load-dependent flake (passes in isolation; spawned as a separate task). If it fails under parallel load, re-run it isolated to confirm; `verter_scheduler` is untouched by this plan.
- `typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output` **PASSES** (P2 fixed the Windows `buf.CMD` resolution). Keep it passing.

## Environmental gotchas (carry into EVERY brief)
- The worktree is a SEPARATE checkout. **Every** build/test/edit command must be `cd D:/wt/goto-def && …` with absolute paths; cwd resets between calls. A missing `cd` silently acts on the main checkout.
- **Serena EDIT tools target the MAIN checkout, not the worktree** → instruct every implementer/fix-agent to use built-in `Edit`/`Write`/`Read` in the worktree (Serena symbolic READ is fine).
- The main repo `D:/dev/personal/verter` is the **user's active checkout**: pre-existing untracked (`.serena/memories/*`, `bindings/`, `external-fix-*.txt`) + **2 external uncommitted verter_session test files** (`slot_binding_graph_tests.rs`, `meta_resolve_tests.rs` — the user's parallel work). NEVER stage/touch these; after every implementer/fix verify the main repo has nothing NEW from the agent.
- Implementers: ONE well-scoped commit per dispatch, **never amend/force/push/`--no-verify`**, stage only intended files by path (never `git add -A`). **NEVER `git push`**; don't merge to `refactor/semantic-db-overhaul`; leave the branch for the user.

## Per-phase workflow (the gate)
1. Read the phase's section from the plan.
2. **Brief** a fresh implementer (Agent tool, `general-purpose`) — self-contained: persona preface + env gotchas + baseline + verbatim scope + explicit legacy deletions + TDD discriminating tests (FAIL pre / PASS post, with negative assertions) + architecture guards + the `--no-fail-fast` gate + report-back shape.
3. **Verify git state yourself**: `cd D:/wt/goto-def && git show --stat HEAD` — commit exists, scoped, main repo clean.
4. **3-reviewer review in parallel** (gemini is NOT installed → 2 codex personas): (A) Claude grounding Agent that reads the live worktree; (B) codex "compiler/implementation-hazards"; (C) codex "architecture-conformance". codex invocation: `"C:/Users/david/AppData/Local/OpenAI/Codex/bin/codex.exe" exec -C D:/tmp --skip-git-repo-check -s workspace-write < prompt.md > out.txt 2>&1` run in background (**never `--sandbox read-only` on Windows — suppresses stdout**). Build each prompt = a persona/scrutiny head + the embedded `git -C D:/wt/goto-def diff <base> <head>` (do NOT make codex read the repo). Grep out for `VERDICT:` and `^\[P[0-3]\]`.
5. **Fix cycle**: a fix-agent consumes ALL THREE reviews, fixes as a NEW commit, comprehensive-audit (fix the whole class, not the named site). Re-review (full 3) until **3/3 ACCEPT or only agreed nits**. Tie-break a genuine reviewer disagreement with a codex tie-breaker (no-compromises framing). The user's "every found case gets a test AND a fix — no deferral" directive applies; "scope" framing sequences a fix, never drops it.
6. **Independent final gate** (sub-agent): run `--no-fail-fast` **SYNCHRONOUSLY in the foreground — do NOT let it background the command and return early** (that produced a bogus "Monitor armed" non-result in P2). Confirm exactly the 8 known-base + 0 new; clippy `--fix … -D warnings` clean; `cargo fmt --all` clean; for TS/e2e phases (5, 6/7) the relevant `pnpm` tests + the VS Code e2e definition suite.
7. **Land** (already on the branch) → log to `D:/wt/goto-def/.feedback/feedback-2026-06-02-gotodef.md` → **STOP and write the next handoff** (standing rule).

## Persona preface — open EVERY sub-agent/reviewer/codex prompt with it verbatim
"You are a best-in-class [compiler/LSP/codegen] engineer with a production-ready mindset. Breaking changes acceptable; performance first-class. The FOUR NEVERS bind everything: never shims, never legacy/dual paths, never stubs, never shortcuts. STOP and report if blocked — do not paper over."

## Escalation & trust-but-verify
- Any architectural doubt / plan deviation → **codex consult** (no-compromises framing: production-ready, breaking changes acceptable, best long-term architecture; embed source via an extraction agent first). Follow its verdict. The P2 "unify don't patch" consult is the template (`D:/tmp/gotodef/p2-consult-*.md`).
- A sub-agent STOP is success → consult/fix-round. Trust but verify EVERY report against git + gate output; when two agents disagree, dispatch a diagnostic for ground truth before acting.

## Lessons from Phases 1–2
- Plan file:line anchors are from grounding `0b3a63894` and have NOT shifted for crates the landed phases didn't touch. Phase 3+ touch `verter_session`/`verter_semantic` — those anchors are still fresh; always re-grep before editing.
- "Unify don't patch": if fix rounds keep finding holes in ONE hand-rolled logic, escalate a unify consult (P2 took 5 rounds until a unify-refactor closed the class).
- Reviewers scrutinizing different branches can both be right — read carefully before treating it as a disagreement.
- A verifier that backgrounds the long test and returns is an INVALID gate — require synchronous runs.

## Remaining phases (plan line ranges)
- **Phase 3 (lines 272–353):** SfcComponentAnchor at `IndexedReady` + delete `find_export_span` heuristic. Touches `verter_semantic` + `verter_session` + TS `analysis.ts`. **Ready-to-use implementer brief below — dispatch it as your first action.** (Note: Phase 3 CREATES the `verter_session::navigation::definition` module with the anchor types; Phase 5 fills in the engine.)
- **Phase 4 (355–442):** `SnapshotId` (content-addressed TSX identity) + host-sourced target-mapper/source-context.
- **Phase 5 (443–620):** `navigation::definition` core + `DefinitionEngine` + `TsNavigationBackend` (+ the 4 one-engine guards). TS/e2e.
- **Phase 6/7 (621–707, MERGED):** route ALL nav surfaces through the engine + delete legacy arbitration in one landable change + docs/skills/CLAUDE.md updates + a `critical_rules_have_guards.rs` registry entry. TS/e2e. Carry-forwards to NOT touch: `Range::default()` in `workspace_symbol.rs` and `extract_component.rs:118`.

## Scaffolding
- `D:/tmp/gotodef/` holds reusable prompt heads + diffs from Phases 1–2.
- Full history: `D:/wt/goto-def/.feedback/feedback-2026-06-02-gotodef.md`.

---

## FIRST ACTION — dispatch the Phase 3 implementer with this brief (Agent tool, general-purpose):

[Open with the persona preface, then:] **Implementer Brief — Phase 3: `SfcComponentAnchor` at `IndexedReady`; delete `find_export_span` heuristic.** Work EXCLUSIVELY in `D:/wt/goto-def` (cd-prefix every command; built-in Edit/Write only — Serena edits hit the main checkout). Read the plan lines 272–353 verbatim; the brief summarizes, the plan governs.

**Baseline:** gate = `cargo test --workspace --tests --no-fail-fast` green EXCEPT the 8 known-base verter_session failures (compile_tier_signature_carries_* ×5, family_a_entries_carry_fact_dep_signature, family_a_warm_hit_uses_fact_validation, materialise_structure_entry_carries_dep_signature), 0 new; UAC-skip + scheduler-flake as documented; typeinfo freshness must stay passing. Your changes add fields to verter_session/verter_semantic structs — verify the failing set stays EXACTLY those 8.

**3A:** add `pub define_options_name_span: Option<verter_span::Span>` to `AnalyzedMacro` (`crates/verter_semantic/src/analysis/types.rs:1306-1365`), populated only for `DefineOptions` — capture the `name:` string-literal **value** span (SFC-absolute) in shallow analysis.

**3B:** create the LSP-free module `verter_session::navigation::definition` (Phase 5 extends it) with serde `SfcComponentAnchor { preferred_span: verter_span::Span, kind: SfcAnchorKind }` and `SfcAnchorKind { DefineOptionsName, ExplicitExportDefault, ScriptSetupStart, TemplateRootStart, FileStart }`; add non-optional `TemplateAnalysisState { NoTemplate, Ready(Arc<TemplateAnalysisSnapshot>) }` on the Vue analysis record (replacing the `Option<Arc<TemplateAnalysisSnapshot>>` on the Vue path). A `.vue` `IndexedReady` must NOT publish with a deferred template; the template-root span comes from SFC block syntax (`cached_parse` `ParsedSfc` `<template>` open-tag start), independent of semantic template analysis.

**3C:** add `pub sfc_component_anchor: Option<SfcComponentAnchor>` to `IndexedReady` (`project_type_store.rs:95-164`) — `Some` for `.vue`, `None` non-SFC; populate at BOTH build sites (`host_manage/prepared_decl.rs:1708` `ensure_indexed_ready`, `host_manage/overlay_materialize.rs:533`) via `compute_sfc_component_anchor(script, template, parsed)` applying priority **1 defineOptions-name → 2 explicit export-default span → 3 `<script setup>` tag start → 4 first template-root tag start → 5 FileStart (truly-empty only)**. Mirror onto `FileAnalysisSnapshot` (TS `sfcComponentAnchor?`, `packages/language-shared/src/analysis.ts:435-449`).

**3D:** rewrite `find_export_span` (`host_manage/analysis_io.rs:1920`) default-export Vue branch (`:1944-1954`) to return the anchor's span; **DELETE** the first-binding → first-macro → `(0,0)` cascade. Non-default Vue bindings + non-SFC unchanged.

**Legacy deletions:** the `:1944-1954` heuristic; the `Option<Arc<TemplateAnalysisSnapshot>>` on the Vue path; any other `bindings.first()`/`macros.first()` default-export synthesis.

**TDD (write FIRST, prove they fail pre-change)** in new `crates/verter_session/src/host_manage/sfc_anchor_tests.rs` (pattern: `VerterHost::new_standalone`+`upsert`, see `vue_sfc_absolute_spans.rs`): `anchor_prefers_define_options_name`; `anchor_explicit_export_default`; `anchor_script_setup_start`; `anchor_template_root_start` (discriminating: template-only → TemplateRootStart, NOT FileStart); `anchor_file_start_only_for_empty`; `find_export_span_default_is_anchor_not_first_binding` (leading `const helper=1` before `defineOptions({name:'Foo'})` → returns `Foo` span not `helper`); `find_export_span_default_never_returns_zero_zero`; `vue_indexed_ready_carries_resolved_template_state`.

**Guards:** `every_vue_record_has_anchor`, `vue_record_template_state_never_deferred` (+ template-only → TemplateRootStart), `ban_first_binding_default_export_heuristic` (source scan asserting the `(0,0)`/first-binding fallbacks are gone) — in `crates/verter_session/tests/architecture_guards.rs`.

**Gate:** `--no-fail-fast` (8 known-base + 0 new) + clippy `-D warnings` + fmt; if TS `analysis.ts` shape changed, run the affected `pnpm test` + `pnpm build:native`. ONE commit, conventional msg, no attribution, no amend/push. Report: commit SHA, per-deliverable file:line, legacy-deletion grep evidence, new-test pre-change failure evidence, guards, verbatim gate totals (exactly the 8), main-repo-clean confirmation, deviations.

Then run the full per-phase gate (3 reviewers → fix cycle → independent `--no-fail-fast` final gate), land, and STOP to write the Phase-4 handoff.
