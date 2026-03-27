---
name: process-feedback
description: "Review .claude/feedback/ files and process them into actionable plans grouped by scope. Use when: user says 'process feedback', 'review feedback', or 'turn feedback into plans'."
---

# Process Agent Feedback into Actionable Plans

## Workflow

1. **Read all feedback files** from `.claude/feedback/`:

   ```bash
   ls .claude/feedback/
   ```

   Read each `feedback-*.md` file.

2. **Categorize entries** by scope (the part of the codebase they affect):
   - `lsp` — `crates/verter_lsp/`
   - `core` — `crates/verter_core/`
   - `diagnostics` — `crates/verter_diagnostics/`
   - `analysis` — `crates/verter_analysis/`
   - `vscode` — `packages/vue-vscode/`
   - `unplugin` — `packages/unplugin/`
   - `ts-core` — `packages/core/`
   - `infra` — CI, builds, tooling, scripts
   - `docs` — documentation, CLAUDE.md, skills

3. **Group entries** — combine entries from different feedback files that touch the same scope. Deduplicate observations that describe the same underlying issue.

4. **Prioritize within each scope**:
   - `[issue]` — bugs and workarounds → highest priority (fix or remove workaround)
   - `[improvement]` — code quality, perf → medium priority
   - `[debt]` — tech debt → lower priority (track, batch later)
   - `[docs]` — missing/outdated docs → lowest priority

5. **Generate plan(s)** — one plan per scope that has actionable items. Each plan should:
   - Have a clear title: `Plan: {scope} improvements from feedback`
   - List concrete tasks with file paths and line numbers where available
   - Mark estimated effort: `[small]` (< 30 min), `[medium]` (30 min - 2 hr), `[large]` (2+ hr)
   - Group tasks by priority within the plan
   - Include a "Validation" section describing how to verify each task

6. **Output format** — present the plans to the user as markdown. If there are multiple scopes, present them as separate sections so the user can choose which to tackle.

7. **Archive processed feedback** — after the user reviews and approves the plans, move processed feedback files to `.claude/feedback/processed/` (create dir if needed):
   ```bash
   mkdir -p .claude/feedback/processed
   mv .claude/feedback/feedback-2026-03-07-*.md .claude/feedback/processed/
   ```

## Plan Template

```markdown
## Plan: {Scope} improvements

### High Priority (issues)

- [ ] [small] Fix {description} — `{file}:{line}`
  - Validation: {how to verify}

### Medium Priority (improvements)

- [ ] [medium] Refactor {description} — `{file}:{line}`
  - Validation: {how to verify}

### Low Priority (debt)

- [ ] [small] Clean up {description} — `{file}:{line}`
  - Validation: {how to verify}

### Docs

- [ ] [small] Document {description} — `{file}`
```

## Rules

- Do NOT create plans for entries that are purely observational with no clear action
- Do NOT create a plan for a scope with only 1 trivial item — mention it inline instead
- If entries reference pre-existing bugs (e.g., "15 pre-existing test failures"), note them but do NOT create fix tasks unless the root causes are identified in the feedback
- Cross-reference with MEMORY.md to avoid duplicating known issues or completed work
- If a feedback entry contradicts something in CLAUDE.md or MEMORY.md, flag it for the user
