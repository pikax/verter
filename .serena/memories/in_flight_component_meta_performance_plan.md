# In-Flight Component-Meta Performance Plan

As of onboarding on 2026-05-02, the user noted that `<scratch>/verter-component-meta-performance-plan.md` is actively being implemented and said it is currently in the middle of Wave 2 implementation. Treat this as ephemeral working context, not durable architecture. Before making component-meta/performance/architecture-guard changes, re-check the plan file, current branch, and git history.

Observed local state during onboarding:
- Repository branch: `integration/component-meta-perf-landing` tracking `origin/integration/component-meta-perf-landing`.
- `git status --short --branch` showed only `.serena/` untracked from onboarding at that moment.
- Recent local commits included component-meta performance work such as indexed-access early-out, lazy projection-rescue, Pick callback-param preservation, and related tests.

Plan summary:
- File: `<scratch>/verter-component-meta-performance-plan.md`.
- Title: `Verter Component-Meta Performance Plan`.
- Scope: component-meta correctness/performance fixes plus architecture cleanup, implemented by an orchestrator with isolated worktrees/sub-agents and cherry-picked/rebased commits.
- Core tiers: Tier A correctness, Tier B performance, Tier C architecture cleanup.
- The plan defines non-negotiable invariants, acceptance gates, cache identity contracts, test harness patterns, rollback/commit slicing, and an orchestration model.

Important Wave/dispatch notes from the plan:
- `B-C0-foundations` is the Wave 2 bundle: Phase 12.A5 hermetic tests, Phase 12.A10 architecture guards, Phase 12.A11 no-phase-archaeology/d-cutover cleanup rule work, plus `WorkspaceAccess::is_workspace_owned` / `is_package_backed` API extensions consumed by Tier B.
- A plan revision on 2026-05-02 defers architecture guard #7 (`no_phase_archaeology_in_production_code`) from `B-C0`/Wave 2 to `B-C6`/Wave 4 because the current integration tree still contains phase-archaeology strings that Wave 4 sweeps.
- Wave-order shipping invariant: A1 ships first, C1/Wave 2 foundations ships second, A2 ships third; Tier B slices stage on `integration/component-meta-perf-landing` and only merge to main after required gates.
- Phase 11b is diagnosis-only and returns `Status: AWAITING_FIX_DECISION`; the orchestrator chooses the Phase 11d fix.

How to use this memory:
- Do not assume the plan file is complete or final. It is the source for current orchestration details; re-read the relevant sections before acting.
- Do not copy the whole plan into durable docs unless the user explicitly asks. Durable architecture belongs in `CLAUDE.md`, `.claude/skills/*`, or `docs/arch/` after the implementation state is settled.
- For component-meta work, load `/component-meta`, `/type-resolution`, `/testing`, and, when architecture/build boundaries are involved, `/architecture` or `/build-and-profiling` as needed.