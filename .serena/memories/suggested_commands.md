# Suggested Commands

Setup:
- `pnpm install` - install JS/TS dependencies.
- `pnpm install --frozen-lockfile` - verify lockfile sync; CI uses this.

Build:
- `pnpm build` - full build in dependency order: native -> LSP -> WASM -> TS packages -> component-meta.
- `pnpm run build:native` - build NAPI native `.node` bindings via `@verter/native`.
- `pnpm run build:lsp` - build Rust LSP binary (`cargo build -p verter_lsp`).
- `pnpm run build:lsp:release` - optimized LSP binary.
- `pnpm run build:mcp` / `pnpm run build:mcp:release` - build MCP server.
- `pnpm run build:wasm` - build WASM package and copy WASM to playground.
- `pnpm run build:ts` - build TypeScript packages.
- `pnpm run build:playground` - build playground.
- `pnpm --filter <package> build` - build a specific pnpm package.

Development entry points:
- `pnpm watch` - watch-build TS packages for extension dev.
- `pnpm dev-extension` - build LSP, then watch language-shared + VS Code extension + TypeScript plugin.
- `pnpm --filter @verter/playground dev` - run playground dev server.
- `pnpm run docs:dev` or `pnpm --filter docs dev` - run VitePress docs dev server.
- `pnpm clean` - remove common build artifacts.

Testing:
- `cargo test --workspace --tests --verbose` - default Rust workspace verification; skips doctests/examples.
- `cargo test --workspace --doc` - Rust doctests only; use when rustdoc examples changed or explicitly requested.
- `cargo test --package verter_compiler test_name` - specific Rust package/test.
- `pnpm test` - all JS/TS package tests via recursive pnpm.
- `pnpm vitest --run` - all non-watch Vitest tests under root config.
- `pnpm vitest --run path/to/test.spec.ts` - specific TS test file.
- `pnpm run test:e2e` - VS Code extension E2E matrix via package script.
- `pnpm run test:e2e:single` - single VS Code E2E run.

End-of-change checks:
- `cargo test --workspace --tests --verbose`
- `cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings`
- `cargo fmt --all`
- `pnpm install --frozen-lockfile`
- For TypeScript changes, also run `pnpm test` or the focused package test plus any required root-level checks.

Formatting/linting:
- `pnpm run fmt` - JS/TS formatting with `oxfmt`.
- `cargo fmt --all` - Rust formatting.
- `cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings` - Rust lint/fix pass.

Profiling / benchmarks:
- `pnpm run profile:hotpath` - AST-only timing hotspots.
- `pnpm run profile:hotpath:alloc` - AST-only timing + allocation hotspots.
- `pnpm run profile:hotpath:full` - full compile pipeline timing.
- `pnpm run profile:hotpath:full:alloc` - full compile timing + allocations.
- `pnpm --filter @verter/benchmark bench:meta:ui:setup` - set up repo-owned component-meta benchmark corpus.
- `pnpm --filter @verter/benchmark bench:meta:ui -- --backends=verter --scenarios=single_cold --limit=2` - sample component-meta UI benchmark.

MCP server:
- `pnpm run build:mcp`
- `verter-mcp --project-root /path/to/vue-project`
- `verter-mcp --transport http --project-root /path/to/vue-project` (HTTP at `http://localhost:6772/mcp`).

Windows / PowerShell utilities:
- `Get-ChildItem -Force` - list files including hidden.
- `Get-ChildItem -Recurse -Filter <pattern>` - recursive file search by name.
- `rg "pattern"` / `rg --files` - preferred fast search if available.
- `Set-Location <repo-root>` - change directory (the repo root comes from the session, never a literal in this file).
- `git status --short --branch`, `git log --oneline -5`, `git diff --stat` - basic git inspection.

Terminating a test/dev server: this memory carries no kill command. Capture the PID at spawn and
terminate only THAT recorded tree; a port is a diagnostic, not proof of ownership, and a
name/pattern kill (`pkill`, `killall`, `taskkill /F /IM`, `Stop-Process -Name`) reaches the
user's own processes and other agents' servers. Recipe and rationale:
`.claude/skills/testing/SKILL.md` -> Server Cleanup.