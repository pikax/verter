# LSP Workstream Ledger

Ordered backlog for the `fix/lsp-provider-parity` LSP-excellence workstream. This is the active-issue sequence (distinct from [`lsp-deferred-overlap-items.md`](./lsp-deferred-overlap-items.md), which tracks items deferred on overlap with the semantic-db-overhaul plan). Top of list = next.

| # | Issue | Status | Design of record | Notes |
|---|---|---|---|---|
| **I0** | **Editor-client local DX** — local build→package→install + tests + docs + "make it work" for the four added editors (Lapce, Zed, Helix, Neovim) | **IN PROGRESS** | per-editor design docs: `lapce-extension-design.md`, `zed-extension-design.md`, `helix-support-design.md`, `neovim-support-design.md` | Trigger: Lapce volt could not be packaged/installed for local testing on Windows. Acceptance bar: scripted install + docs + packaging/contract tests + Neovim CI **and** an attempt at automated Lapce/Zed smokes (accept infeasible if proven). |
| I1 | **IDE error recovery** — broken `.vue`/`.svelte` script/template behaves like a broken `.ts`: native syntax-diagnostic rail ∪ surviving type diagnostics; reference-preserving recovery | QUEUED | [`ide-error-recovery-design.md`](./ide-error-recovery-design.md) | Codex-architect verdict reproduced in the design (§6). `verter_session` edits **authorized as-specced**. Stages S1→S2→S3. |
| I2 | **tsserver carrier membership** — shadow configured-project (or plugin probe) so carriers join the real tsconfig project: Case-V auto-import + cross-file rename | QUEUED | [`tsserver-carrier-membership-design.md`](./tsserver-carrier-membership-design.md) | Codex-architect verdict referenced in the design. Structured to avoid `verter_session` (escalate if unavoidable). Plugin-vs-shadow decided by a stage-1 feasibility probe, not up front. tsserver-only; tsgo unchanged. |

Serial-implementation rule: one landing-bound block active at a time (avoid rebase churn). Issues execute top→bottom. Each issue lands via worktree forked from `fix/lsp-provider-parity` → 3/3 review → §1a verify → land → independent confirm.

_Maintained by the LSP workstream CTO/MoM orchestration. Update status as issues land; add design pointers as designs are adopted._
