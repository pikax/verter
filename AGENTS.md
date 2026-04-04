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
- Follow the shared build philosophy below for type work and any host-backed architecture changes.
- For `component-meta` native/compat boundaries, treat `CLAUDE.md`'s `Component-Meta Native Vs Compat (CRITICAL)` section as the canonical rule set. In particular: Rust is the semantic authority, `component-meta` makes one async native request per query, JS adapts the returned payload but does not become a second resolver/expander, and native should send the smallest semantically complete payload it can.
- For `component-meta` type work, use cached lookup/eval state only. Do not add AST/source-walk fallback to recover or expand types after the cache-owning pass.
- For `component-meta` registry publication, stay shallow and demand-driven: load only the symbols required by the current query, and expand only when a cached lookup result is actually needed.
- For `component-meta` cross-file resolution, deepen in one place only: follow the active declaration route for the requested symbol/query and do not branch into unrelated sibling symbols/files.
- For `component-meta` companion/type-target selection, keep canonicalization shallow too: choosing between runtime and declaration companions may probe cached raw source existence, but must not build export analysis, snapshots, or eval envs just to pick the target file.
- For `component-meta` metadata or fallthrough projection, reuse the already-resolved state and the captured store/session view. Do not bounce back out to a fresh top-level snapshot/query when a resolved query is already in hand.
- For `component-meta` imported-type hydration, treat the imported dependency cache as the only source of file state after shallow seeding. Resolver paths must not call raw snapshot/source builders to recover missing imported data; if the cache does not own the needed snapshot/env yet, stay shallow and stop.
- For `component-meta` imported-eval collection, keep one strategy only: lazy/BFS over the active symbol route. Do not add eager collector modes, source-text fallback parsing, or alternate collection branches that widen traversal.

## Shared Build Philosophy

Follow the same end-state philosophy as `binary-exploring-lamport.md`.

Core rules carried forward:

1. Read, parse, shallow-process, and cache each canonical file once per content hash through one shared host path.
2. Store the full shallow symbol inventory up front, then process only the requested items on demand.
3. Same-file closure stays local to the owning file.
4. Cross-file deepening happens in one place only, one import level at a time.
5. The builder/solver reads only from cached lookup state; it does not reopen file loading or routing.
6. The entire design is demand-driven and query-scoped.
7. The final implementation lands as one clean cutover, not as a merged dual-path transition.
8. Component-meta, LSP, MCP, and other host-backed consumers must share the same file-ready/read/parse/shallow-process lifecycle.

If a change conflicts with these rules, prefer changing ownership, deleting the legacy path, or breaking an internal interface over preserving a second resolver/load path.

## Shallow File Processing Core Invariant

The shallow file process is a core architectural invariant and must be preserved.

When a canonical file is processed, the host stores its shallow symbol inventory once. That inventory is the authoritative index later stages query.

At minimum, the shallow state must classify and retain:

- imports
- exports and reexports
- type declarations
- interfaces
- enums
- classes
- variables/constants
- functions/method signatures
- `typeof`-relevant value declarations
- local symbol dependency edges
- cross-file dependency edges

Design rule:

- processing a file means collecting and indexing its symbols, not eagerly evaluating them
- later stages look up the indexed items they need and only process those items on demand
- no stage should need to rescan the raw file to rediscover symbols that shallow processing already captured

Performance consequence:

- very high performance comes from targeted demand after broad shallow indexing, not from repeated partial reparsing

## Task Routing

- Architecture or ownership questions: start with `CLAUDE.md`, then `.claude/skills/architecture/SKILL.md`.
- Span, offset, URI, or source map work: load `.claude/skills/position-encoding/SKILL.md` before editing.
- Build, release, profiling, or MCP work: load `.claude/skills/build-and-profiling/SKILL.md`.
- Test design or verification planning: load `.claude/skills/testing/SKILL.md`.
- Rust hot paths or allocation-sensitive work: load `.claude/skills/rust-performance/SKILL.md`.
