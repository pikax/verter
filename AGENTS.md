# Verter Agent Guide

This repository keeps most reusable agent context in shared markdown files. Some of those files live under Claude-named paths for historical reasons, but they are project documentation, not Claude-only data.

Use this file as the neutral entry point. Reuse the shared sources below instead of creating duplicate agent-specific copies.

## Shared Sources

- `CLAUDE.md`
  - Canonical high-level reference for architecture principles, critical invariants, build commands, testing rules, agent implementation rules, and commit conventions.
- `.claude/skills/type-resolution/SKILL.md`
  - Type solver, ShallowFileState, ExternalTypeFrontier, canonical cache rules, macro traversal, prepared declarations.
- `.claude/skills/component-meta/SKILL.md`
  - Component-meta native/compat boundary, fallthrough/root inheritance, resolver rules, cache-owned hydration.
- `.claude/skills/compiler-codegen/SKILL.md`
  - Rust compiler pipeline, template codegen (VDOM/IDE), CodeTransform, cached directives, strict slots, style preprocessing, CompileTarget.
- `.claude/skills/host-session/SKILL.md`
  - TypeProvider (TSGO/tsserver), workspace management, async scheduler, SyncCoordinator, ownership lifecycle.
- `.claude/skills/architecture/SKILL.md`
  - High-level module map, TS packages, plugin system, CSS analysis, MCP server, static analysis types.
- `.claude/skills/position-encoding/SKILL.md`
  - Span types, encoding conversions, and path normalization details.
- `.claude/skills/build-and-profiling/SKILL.md`
  - Build order, rebuild strategy, profiling workflow, and MCP server setup.
- `.claude/skills/testing/SKILL.md`
  - Rust and TypeScript test patterns, TDD workflow, sourcemap checks, and general test execution hygiene.
- `.claude/skills/e2e-vscode-testing.md`
  - VS Code extension E2E fixtures, helpers API, warm-session rules, and adding new extension/LSP tests.
- `.claude/skills/rust-performance/SKILL.md`
  - Rust optimization guidance, allocation patterns, and `CodeTransform` usage notes.
- `docs/`
  - User-facing and contributor-facing documentation.

## Neutrality Rules

- Treat `CLAUDE.md` and `.claude/skills/` as shared project references for any coding agent.
- Keep durable project knowledge in shared docs such as `CLAUDE.md`, `docs/`, or the relevant reference file under `.claude/skills/`.
- Keep agent-specific files thin. They should point at shared documentation, not become separate sources of truth.
- `.claude/settings.local.json` is local tool configuration, not repository policy.
- `.feedback/` contains optional working notes and is not committed project documentation.

## Working Rules

- Follow TDD for code changes: write failing tests first, implement the minimum fix, rerun tests, then refactor.
- Default Rust verification command: `cargo test --workspace --tests --verbose`.
- Do not run bare `cargo test --workspace` unless the user explicitly asks for doctests or you changed rustdoc examples. In this repo it also runs doctests and example builds, which are substantially slower than the normal agent verification loop.
- Do not provide time estimates unless the user explicitly asks for one. Plans are expected to be executed fully in one pass, so do not use estimated effort or duration as a reason to skip, defer, or partially implement approved work.
- The codebase expects the best architecture for the problem. Time constraints, breaking-change avoidance, migration breadth, or saying something is "a lot of work" are not valid reasons to weaken the design or deviate from an approved plan.
- Update the **owning** documentation when public behavior, module paths, or APIs change. Update the relevant skill, not CLAUDE.md, unless summaries or pointers change.
- Use conventional commits: `<type>(<scope>): <description>`.
- Load only the specific reference material needed for the task instead of bulk-reading every file.
- When semantic code-navigation tools such as Serena are available, use them for symbol overview, lookup, references, and targeted refactors as described in `CLAUDE.md` "Codebase Navigation".
- Follow the build philosophy and shallow file processing invariant defined in `CLAUDE.md`.
- For component-meta work, follow the rules in `/component-meta` skill.
- For type resolution work, follow the rules in `/type-resolution` skill.
- **Testing hermeticity**: see `CLAUDE.md` "Testing-Hermeticity (MANDATORY)" and `.claude/skills/testing/SKILL.md`.
- **No phase archaeology in code**: see `CLAUDE.md` "No phase archaeology in production code (MANDATORY)".

## Task Routing

- Architecture principles or ownership questions: start with `CLAUDE.md`.
- Type resolution, solver, cross-file types, macro traversal: load `/type-resolution`.
- Component-meta, fallthrough, compat layer: load `/component-meta`.
- Compiler, codegen, template, style, CodeTransform: load `/compiler-codegen`.
- LSP host, TypeProvider, workspace, scheduler: load `/host-session`.
- Module map, TS packages, plugin system, CSS analysis: load `/architecture`.
- Span, offset, URI, or source map work: load `/position-encoding`.
- Build, release, profiling, or MCP work: load `/build-and-profiling`.
- Test design or verification planning: load `/testing`.
- VS Code extension or LSP E2E verification: load `/e2e-vscode-testing`.
- Rust hot paths or allocation-sensitive work: load `/rust-performance`.
