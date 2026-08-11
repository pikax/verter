# A2 environment

- Machine: Windows 11 Pro 10.0.26200 (win32)
- rustc: 1.97.1 (8bab26f4f 2026-07-14) — rust-toolchain.toml pinned
- node: v26.5.0
- pnpm: workspace `pnpm install --frozen-lockfile` run in the worktree before the gate
  (fresh worktrees lack node_modules; gate preflight + typescript-plugin dist need it)
- gate prerequisite: `pnpm --filter @verter/language-shared --filter @verter/typescript-plugin build` run before `node scripts/gate.mjs`
- tsgo oracle binary: pnpm-installed `tsgo`, `--version` → `Version 7.0.0-dev.20260526.1`
- Worktree: <REPO>-wt-a2 @ block/a2-u6-harness
- Main checkout <REPO> untouched (clean, 13cedd6fc)
