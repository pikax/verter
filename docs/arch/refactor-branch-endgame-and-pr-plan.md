# `refactor/semantic-db-overhaul` — Endgame & PR Plan

> User-directed (2026-06-30). The authoritative, **tracked** (git-durable, account- and checkout-independent) record of how this branch reaches `main`. The orchestration may be driven by an agent on ANY account, so this lives in git, not in any one account's memory. Status of the parselower stages derives from the master plan + git at `HEAD`; this doc owns the END-GAME sequencing and the hard gates.

## Hard order (do NOT reorder)

1. **Finish the full PARSELOWER arc** on `refactor/semantic-db-overhaul`:
   - **Stage 9** — materialize-fence → GREEN (in progress: intrinsic-C done at fence 5; §5a converts the 5 `framework_surface` sites to fence 0; §5c enables the fence + §7 audit; §5d ff-merges to `refactor`).
   - **Stage 10** — TypeExpr compat removal (codex reuse-vs-rescope on the shelved `mom/stage10-typeexpr-compat-removal-design` @ `9062d4baf`).
   - **Stage 11** — TypeExpr quarantine/rename (e.g. `TypeSyntax`) + hard guards; codex-architect-ruled after Stage 10 lands. Goal = ZERO `TypeExpr` as semantic AUTHORITY.
   - **Stage 12** — profile-gated perf compaction (only where profiling proves a hotspot; may legitimately be EMPTY).
2. **POST-STAGE-12 CHECKPOINT — HARD STOP.** After Stage 12 lands, **STOP and ASK the user**: continue vs merge the sibling branches. The checkpoint is **ONLY after Stage 12** — NOT after Stage 9 or Stage 11. Do NOT auto-proceed.
3. **If MERGE — a proper merge of two big sibling branches into `refactor`:**
   - **Svelte native compiler** = `feat/framework-adapters-clean`, based on `docs/arch/multi-framework-adapters-plan.md` + `docs/arch/svelte-native-compiler-plan.md` (those plan docs live IN that branch — read them FROM `feat/framework-adapters-clean` for full merge context).
   - **LSP performance** branch = not in `origin` as of 2026-06-30; the user will signal the exact branch (likely related to `lsp-perf/integration` / `docs/arch/perf-lsp-orchestration-plan.md`).
   - The merge manager(s) MUST first obtain each branch's source design plans before merging.
4. **PR PREP** (this branch is ~2686 commits ahead of `main`):
   - Scrub `Co-Authored-By` / Claude attribution from commit messages (~154 commit-bodies carry it).
   - Top-notch documentation.
   - Squash the ballooned per-block commits (impl + per-finding fix + doc-precision rounds — allowed by the rules at the time) into a reasonable number of clean logical commits.
5. **PR + merge `refactor/semantic-db-overhaul` → `main`.**

## HARD GATE — U-blocks come ONLY after merge-to-main

**Do NOT advance to ANY U0–U15 native-typeinfo-parity block until the PR is merged to `main`.** The master plan (`semantic-db-overhaul-unified-remaining-plan.md`) frames U0–U15 as the immediate "remaining work"; this directive RE-SEQUENCES that — the current branch ships to `main` FIRST; the U* parity work is the next phase.
