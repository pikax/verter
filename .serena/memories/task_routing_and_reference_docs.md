# Task Routing and Reference Docs

Use `AGENTS.md` as the neutral entry point. It points to shared project references and says not to duplicate agent-specific sources of truth.

Start with these docs by task area:
- Architecture principles or ownership questions: `CLAUDE.md` and `.claude/skills/architecture/SKILL.md`.
- Type resolution, solver, cross-file types, ShallowFileState, ExternalTypeFrontier, macro traversal, prepared declarations: `.claude/skills/type-resolution/SKILL.md`.
- Component-meta, fallthrough/root inheritance, compat/native boundary, resolver rules, cache-owned hydration: `.claude/skills/component-meta/SKILL.md`.
- Compiler, codegen, template, style preprocessing, `CodeTransform`, cached directives, strict slots, `CompileTarget`: `.claude/skills/compiler-codegen/SKILL.md`.
- LSP host, TypeProvider, workspace management, async scheduler, SyncCoordinator, ownership lifecycle: `.claude/skills/host-session/SKILL.md`.
- Span, offset, URI, path normalization, source maps: `.claude/skills/position-encoding/SKILL.md`.
- Build order, rebuild strategy, profiling, MCP server setup: `.claude/skills/build-and-profiling/SKILL.md`.
- Test design, TDD workflow, hermeticity, sourcemaps, server cleanup: `.claude/skills/testing/SKILL.md`.
- VS Code extension or LSP E2E verification: `.claude/skills/e2e-vscode-testing.md`.
- Rust hot paths/allocation-sensitive work: `.claude/skills/rust-performance/SKILL.md`.

Load only the specific reference material needed for the task. Do not bulk-read every skill.

Special current-context note:
- For component-meta performance work, also check memory `in_flight_component_meta_performance_plan.md` and then inspect `D:\tmp\verter-component-meta-performance-plan.md` for the current orchestration state before changing code.