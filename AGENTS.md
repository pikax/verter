# Verter Agent Guide

This repository keeps most reusable agent context in shared markdown files. Some of those files live under Claude-named paths for historical reasons, but they are project documentation, not Claude-only data.

Use this file as the neutral entry point. Reuse the shared sources below instead of creating duplicate agent-specific copies.

## Shared Sources

- `CLAUDE.md`
  - Canonical high-level reference for architecture, repository structure, critical invariants, build commands, testing rules, and commit conventions.
- `.claude/skills/architecture/SKILL.md`
  - Module map, package responsibilities, plugin system, LSP structure, and key file locations.
- `.claude/skills/position-encoding/SKILL.md`
  - Span types, encoding conversions, and path normalization details.
- `.claude/skills/build-and-profiling/SKILL.md`
  - Build order, rebuild strategy, profiling workflow, and MCP server setup.
- `.claude/skills/testing/SKILL.md`
  - Rust and TypeScript test patterns, sourcemap checks, and VS Code extension testing guidance.
- `.claude/skills/rust-performance/SKILL.md`
  - Rust optimization guidance, allocation patterns, and `CodeTransform` usage notes.
- `docs/`
  - User-facing and contributor-facing documentation.

## Neutrality Rules

- Treat `CLAUDE.md` and `.claude/skills/` as shared project references for any coding agent.
- Keep durable project knowledge in shared docs such as `CLAUDE.md`, `docs/`, or the relevant reference file under `.claude/skills/`.
- Keep agent-specific files thin. They should point at shared documentation, not become separate sources of truth.
- `.claude/settings.local.json` is local tool configuration, not repository policy.
- `.claude/feedback/` contains optional working notes and is not committed project documentation.

## Working Rules

- Follow TDD for code changes: write failing tests first, implement the minimum fix, rerun tests, then refactor.
- Update documentation when public behavior, module paths, or APIs change.
- Use conventional commits: `<type>(<scope>): <description>`.
- Load only the specific reference material needed for the task instead of bulk-reading every file.

## Task Routing

- Architecture or ownership questions: start with `CLAUDE.md`, then `.claude/skills/architecture/SKILL.md`.
- Span, offset, URI, or source map work: load `.claude/skills/position-encoding/SKILL.md` before editing.
- Build, release, profiling, or MCP work: load `.claude/skills/build-and-profiling/SKILL.md`.
- Test design or verification planning: load `.claude/skills/testing/SKILL.md`.
- Rust hot paths or allocation-sensitive work: load `.claude/skills/rust-performance/SKILL.md`.
