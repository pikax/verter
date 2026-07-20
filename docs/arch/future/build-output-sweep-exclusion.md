# Future work — exclude build output from the proactive workspace sweep

Status: **deferred / not yet scheduled.** Low priority, performance/scale improvement, not a
correctness fix. This document captures the full scope so it can be planned properly later; it is
not an approved block.

## Origin

Surfaced while investigating a "no TypeScript inference at all" report on a real Vue monorepo (the
project-selection / multi-claimant fix). During that investigation the LSP was observed performing
roughly **80 wasteful `open_file` + `get_diagnostics` calls on build output** under a package's
`dist/**` directory — compiled `.cjs` / `.mjs` / `.js` artifacts (including a ~166 KB bundled
chunk), not source. They were program-eligible only because a `tsconfig` in that package sets
`allowJs: true`, and the background carrier/source sweep opened them into the provider.

Contribution to the original inference bug: **none** — the two are independent. This is pure waste.

## Why it is worth doing

- **It scales with the project.** ~80 opens on one mid-size package's `dist/`; a large monorepo
  with big build outputs across many packages turns this into hundreds-to-thousands of wasted
  provider opens + diagnostic pulls, all competing for the single provider process. That directly
  works against the north-star goal of stable, snappy sessions for professionals working all day
  in massive (hundreds-to-thousands of SFCs) projects.
- **The project-selection fix slightly worsens it.** Once multi-claimant carriers resolve to a
  `Bound` owner instead of a terminal `Ambiguous`, more `dist/*.js` files (via `allowJs`) become
  sweep-eligible rather than being dropped at an ambiguous-ownership gate. So this is a natural
  fast-follow to that fix, though still not urgent.

## Why the payoff is bounded (be honest about it)

The `dist/` opens are **provider warming**, not diagnostics published to the editor — the user is
not seeing red squiggles on generated code. The benefit is lower CPU/memory and less provider
contention (lighter, more responsive sessions at scale), not the disappearance of a visible defect.
Scope the block on that basis; do not oversell it as a bug fix.

## Design

### The key insight: the proactive sweep is a WARMING optimization

A directory excluded from the proactive sweep is **not** removed from the program. Files under it
still resolve on demand (when a real source file imports them) and when the editor opens them
(`didOpen` is independent of the sweep). Therefore excluding a directory from the sweep is
**correctness-safe** — at worst it means the first cross-file reference into that directory pays a
cold cost instead of being pre-warmed. For build output, nothing references it from source, so even
that cost is theoretical.

This is what makes the cheap approach safe, and it must stay true: the exclusion lives at the
**scan-enumeration boundary only** and must **not** touch ownership
(`WorkspaceSnapshot::configured_owner_resolution_for_file`). A file under `dist/` that is genuinely
a configured member is still a legitimate on-demand query target; only its *proactive* sweeping is
skipped.

### Recommended approach (SMALL, no new dependency)

Add the **unambiguous** build-output directory names to the existing exclusion list:

- `crates/verter_lsp/src/workspace_scanner.rs` — `EXCLUDED_DIRS` (today `["node_modules"]`),
  consumed by `is_excluded_dir()` in the tiered-scan `filter_entry` predicates.
- Add: `dist`, `build`, `out`, `coverage`, `.nuxt`. These are never source directories by
  convention.
- **Do NOT add `lib`.** `src/lib/` is a common *source* directory name; blind-excluding `lib`
  would stop proactively warming real source (a perf regression on the first cross-file reference,
  not a correctness loss, but avoidable). `lib` is the one ambiguous name — leave it out.

A discriminating test: assert a `dist/**/*.js` file is **not** proactively opened while a real
`src/**` source file still is (drive the scanner, inspect the set of proactively-opened paths).

### Alternative considered: full `.gitignore`-awareness (MEDIUM, new dependency)

The fully-principled signal for "this is generated, not source" is `.gitignore` — build output is
gitignored, real source is not, which side-steps the directory-name ambiguity entirely (a
committed, non-gitignored `dist/` would then still be swept, which is arguably correct). But the
workspace walker uses plain `walkdir` (`walkdir = "2.5"`); gitignore-aware walking needs the
`ignore` crate (the ripgrep walker) as a **new dependency**, plus care around: nested `.gitignore`
files, negation patterns, the rare committed-`dist` case, and cross-platform path semantics. That
is a larger, riskier change for marginal additional coverage over the name-list on the common case.

**Recommendation:** ship the name-list first (it captures essentially all the real-world waste at
near-zero cost and risk). Reserve `.gitignore`-awareness for if/when broader gitignore support is
independently wanted in the workspace layer — at which point this exclusion rides on it for free.

## Acceptance criteria (when scheduled)

- Conventional build-output dirs (`dist`, `build`, `out`, `coverage`, `.nuxt`) are not proactively
  opened/diagnosed by the workspace sweep.
- Ownership semantics unchanged: a file under an excluded dir still resolves on demand and on
  `didOpen`; `configured_owner_resolution_for_file` is untouched.
- Discriminating test: `dist/**/*.js` not proactively opened, `src/**` source still is.
- Cross-platform: exclusion matches by directory name component, not a hardcoded path separator.
- Full gate green (`node scripts/gate.mjs`) + the Project-Bound External-TS Contract guard suite
  green (the change must not perturb ownership/serving).

## Effort / priority

- **Effort:** SMALL (name-list) / MEDIUM (gitignore-aware).
- **Priority:** LOW — schedule after the current release blockers; a natural fast-follow to the
  project-selection fix but not gating it.

## Confidentiality

The originating project is a private third-party codebase used only for local stress testing. Do
not name it or its paths in any committed content or fixture; refer to the layout structurally
(`packages/<pkg>/dist/**`). Any test fixture must be hermetic and synthetic.
