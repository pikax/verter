# Verter Agent Guide

Most reusable agent context lives in shared markdown files. Some sit under Claude-named paths for historical reasons but are project documentation, not Claude-only data.

Use this as the neutral entry point. Reuse the shared sources below instead of creating duplicate agent-specific copies.

## Shared Sources

- `CLAUDE.md` — canonical high-level reference: architecture principles, critical invariants, build commands, testing rules, agent implementation rules, commit conventions.
- `.claude/skills/type-resolution/SKILL.md` — type solver, ShallowFileState, ExternalTypeFrontier, canonical cache rules, macro traversal, prepared declarations.
- `.claude/skills/type-cache-architecture/SKILL.md` — fact-based cache architecture, 5-way env hash split (R21), `FileArtifactStore`, R1–R31 rules, module augmentation, multi-candidate storage, `parse_stable_hash`.
- `.claude/skills/component-meta/SKILL.md` — component-meta native/compat boundary, fallthrough/root inheritance, resolver rules, cache-owned hydration.
- `.claude/skills/compiler-codegen/SKILL.md` — Rust compiler pipeline, template codegen (VDOM/IDE), CodeTransform, cached directives, strict slots, style preprocessing, CompileTarget.
- `.claude/skills/host-session/SKILL.md` — TypeProvider (TSGO/tsserver), workspace management, async scheduler, SyncCoordinator, ownership lifecycle.
- `.claude/skills/architecture/SKILL.md` — high-level module map, TS packages, plugin system, CSS analysis, MCP server, static analysis types.
- `.claude/skills/position-encoding/SKILL.md` — span types, encoding conversions, path normalization.
- `.claude/skills/build-and-profiling/SKILL.md` — build order, rebuild strategy, profiling workflow, MCP server setup.
- `.claude/skills/testing/SKILL.md` — Rust + TypeScript test patterns, TDD workflow, sourcemap checks, test execution hygiene.
- `.claude/skills/e2e-vscode-testing.md` — VS Code extension E2E fixtures, helpers API, warm-session rules, adding new extension/LSP tests.
- `.claude/skills/rust-performance/SKILL.md` — Rust optimization guidance, allocation patterns, `CodeTransform` usage notes.
- `docs/` — user-facing and contributor-facing documentation.

## Neutrality Rules

- Treat `CLAUDE.md` and `.claude/skills/` as shared project references for any coding agent.
- Keep durable project knowledge in shared docs (`CLAUDE.md`, `docs/`, or the relevant `.claude/skills/` reference).
- Keep agent-specific files thin — point at shared documentation, do not become separate sources of truth.
- `.claude/settings.local.json` is local tool configuration, not repository policy.
- `.feedback/` contains optional working notes, not committed project documentation.

## Working Rules

- Follow TDD for code changes: failing tests first, minimum fix, rerun, then refactor.
- Default Rust verification command: `cargo test --workspace --tests --verbose`.
- Do not run bare `cargo test --workspace` unless the user explicitly asks for doctests or you changed rustdoc examples — it also runs doctests and example builds, substantially slower than the normal agent verification loop.
- Do not provide time estimates unless the user explicitly asks. Plans are executed fully in one pass; do not use estimated effort or duration as a reason to skip, defer, or partially implement approved work.
- The codebase expects the best architecture for the problem. Time constraints, breaking-change avoidance, migration breadth, or "a lot of work" are not valid reasons to weaken the design or deviate from an approved plan.
- Update the **owning** documentation when public behavior, module paths, or APIs change. Update the relevant skill, not CLAUDE.md, unless summaries or pointers change.
- Use conventional commits: `<type>(<scope>): <description>`.
- Load only the specific reference material the task needs instead of bulk-reading every file.
- When semantic code-navigation tools such as Serena are available, use them for symbol overview, lookup, references, and targeted refactors as described in `CLAUDE.md` "Codebase Navigation".
- Follow the build philosophy and shallow file processing invariant in `CLAUDE.md`.
- For component-meta work, follow `/component-meta`. For type resolution work, follow `/type-resolution`.
- **Testing hermeticity**: see `CLAUDE.md` "Testing-Hermeticity (MANDATORY)" and `.claude/skills/testing/SKILL.md`.
- **No phase archaeology in code**: see `CLAUDE.md` "No phase archaeology in production code (MANDATORY)".

## Task Routing

- Architecture principles or ownership questions: start with `CLAUDE.md`.
- Type resolution, solver, cross-file types, macro traversal: `/type-resolution`.
- Cache layer keys, env hash split, fact-based cache rules, `FileArtifactStore`, module augmentation: `/type-cache-architecture`.
- Component-meta, fallthrough, compat layer: `/component-meta`.
- Compiler, codegen, template, style, CodeTransform: `/compiler-codegen`.
- LSP host, TypeProvider, workspace, scheduler: `/host-session`.
- Module map, TS packages, plugin system, CSS analysis: `/architecture`.
- Span, offset, URI, or source map work: `/position-encoding`.
- Build, release, profiling, or MCP work: `/build-and-profiling`.
- Test design or verification planning: `/testing`.
- VS Code extension or LSP E2E verification: `/e2e-vscode-testing`.
- Rust hot paths or allocation-sensitive work: `/rust-performance`.
- Driving a large multi-block plan, refactor, migration, or cutover autonomously (orchestrator + sub-agents + dual review): `/multi-agent-orchestration`.
