# Integration Tests — Agent Guide

Instructions for AI agents working on Verter's integration test infrastructure.

## SYNC RULE (CRITICAL)

**`scripts/integration-test/projects.mjs` and `.github/workflows/integration-test.yml` define the same project matrix.**

When adding, removing, or modifying a project in **either** file, you **MUST** update the other file to match. The fields map as follows:

| JS (`projects.mjs`) | YAML (`integration-test.yml`) |
| ------------------- | ----------------------------- |
| `name`              | `name`                        |
| `repo`              | `repo`                        |
| `branch`            | `branch`                      |
| `buildCmd`          | `build-cmd`                   |
| `testCmd`           | `test-cmd`                    |
| `e2eCmd`            | `e2e-cmd`                     |
| `packageManager`    | `package-manager`             |
| `bundler`           | `bundler`                     |
| `shallow`           | `shallow`                     |

## Shared Repo Architecture Rules

When integration-test work touches host-backed loading, resolution, or cache behavior, follow the root repository build philosophy in [CLAUDE.md](../../CLAUDE.md):

1. one shared file-ready/read/parse/shallow-process path per canonical file
2. no request-local fallback parser/load path
3. no dual-path transition kept alive for safety

Do not add integration-test-only infrastructure that normalizes or depends on a second host-backed read/parse path.

## Running Integration Tests

### Local (primary for development)

```bash
# Single project (fastest feedback)
pnpm integration-test coreui

# With existing build artifacts
pnpm integration-test --skip-build --no-clone coreui

# Multiple projects
pnpm integration-test coreui balancer-frontend-v2 slidev

# All projects
pnpm integration-test

# Inventory local repos under configured roots and stop
pnpm integration-test:discover

# Discover local repos and execute non-manual Tier 2 entries in sandboxes
pnpm integration-test:local
```

### Local inventory rules

- Discovery is repo-root based. Nested packages are treated as surfaces inside the git root unless they force `manual_review`.
- Only deterministic root commands are auto-executed:
  - build: `scripts.build`
  - test: `scripts.test` or `scripts["test:unit"]`
  - typecheck: `tsconfig.json`, then `tsconfig.web.json`, `tsconfig.app.json`, `tsconfig.src.json`
- Local runs copy the repo to `<scratch>/verter-toolchain-runs/<run-id>/sandboxes/<repo>/` before modifying anything.
- Reports live beside the sandboxes under `reports/<repo>/` and include `project.json`, `replacement.json`, `summary.md`, and `typecheck/review-queue.json`.
- Repos classified as `manual_review` must not be auto-executed unless the runner logic is intentionally extended.

### CI

Triggered by:

- PR comment: `/integration`
- Manual dispatch on GitHub Actions
- Release workflow

## Debugging Failures

### Build failure

1. Check `.integration-tests/logs/<project>/verter-build.log`
2. Compare with `baseline-build.log` — if baseline also fails, the issue is project setup, not Verter
3. Search the build log for these error patterns:
   - `Unexpected token` → raw HTML or TS syntax leaked into JS output
   - `<!--` or `-->` in generated code → interstitial comment not removed (see P1 fix pattern)
   - `: string`, `<T>`, `as const` in JS output → TS not stripped (`forceJs` issue, see P2)
   - Garbled identifiers in `props:` section → cross-file type span extraction bug (see P0)
4. Common infrastructure causes:
   - Plugin replacement didn't work (check verification step output)
   - Verter doesn't support a template feature the project uses
   - Dependency version conflict with `@verter/native`

### Test regression

If Verter has **more** test failures than baseline (`TEST REGR` status):

1. Check `.integration-tests/logs/<project>/verter-test.log`
2. Diff against `baseline-test.log` to find which tests regressed
3. These failures indicate Verter's compiler generates incorrect code for some Vue patterns
4. Look at the failing test's component templates for the unsupported pattern

### Replacement verification failed

If "No files contain @verter/unplugin after replacement":

1. The project may use a non-standard import style
2. Check how the project imports its Vue plugin (grep for `@vitejs/plugin-vue` or `rollup-plugin-vue`)
3. The replacement patterns may need to be extended in both `run.mjs` (Node.js `String.replace`) and `integration-test.yml` (bash `sed`)

### Stale native binary (`.node` file)

**Symptom**: `host?.close is not a function` or missing methods on `VerterHost` at runtime.

**Cause**: pnpm's content-addressable store retains old `.node` binaries via hardlinks. `copyRecursive()` may fail silently on locked/hardlinked files (the `catch {}` swallows the error), leaving a stale binary in `.pnpm/` even after the overwrite step reports success.

**Diagnosis**: Run from the project's `node_modules/`:

```bash
node -e "const r = require('@verter/native'); const h = new r.VerterHost({}); console.log(Object.getOwnPropertyNames(Object.getPrototypeOf(h)))"
```

Check that the listed methods include the one reported missing (e.g., `close`).

**Fix**:

1. Rebuild native bindings: `pnpm run build:native`
2. Delete the project's checkout: `rm -rf .integration-tests/repos/<project>/`
3. Re-run: `pnpm integration-test --skip-build --no-clone <project>`

**Prevention**: The overwrite step now verifies native binary file size after copy and retries with direct `fs.copyFileSync` if the initial copy failed.

### Type-check review queue

`vue-tsc` is the baseline. Extra `verter-tsc` diagnostics are persisted, not auto-dismissed:

- `diagnostics.normalized.json` stores parsed raw output for cold/warm runs
- `diagnostics.diff.json` groups shared, Vue-only, and Verter-only diagnostics
- `review-queue.json` stores Verter-only items with statuses like `pending`, `likely_legit`, `verter_bug`, and `env_issue`
- `tool_crash` entries are failures, not cosmetic diffs

## Adding a New Project

### Checklist

1. **Build tool**: Must use Vite (`@vitejs/plugin-vue`), Rollup (`rollup-plugin-vue`), or Nuxt (uses a runtime module override). Webpack and Vue CLI are not supported.
2. **Vue plugin import**: Find the exact import statement. Common patterns:
   - `import vue from '@vitejs/plugin-vue'` (standard)
   - `import Vue from '@vitejs/plugin-vue'` (capital V, e.g., zyronon-douyin)
   - `import vue from 'rollup-plugin-vue'` (rollup projects)
   - Programmatic: `import Vue from '@vitejs/plugin-vue'` in a `.ts` source file (e.g., slidev)
3. **Package manager**: Check for `pnpm-lock.yaml` (pnpm) or `package-lock.json` (npm)
4. **Build command**: Find in `package.json` scripts. For monorepos, use filtered commands (e.g., `pnpm --filter <pkg> build`)
5. **Test command**: Find in `package.json` scripts. Use `""` if no runnable tests exist
6. **Branch**: Check the default branch (`main`, `master`, `dev`, etc.)

### Steps

1. Add entry to `scripts/integration-test/projects.mjs`
2. Add matching entry to `.github/workflows/integration-test.yml` matrix
3. Run `pnpm integration-test <name>` to validate
4. Check that:
   - Clone succeeds
   - Baseline build passes
   - Replacement is verified ("`@verter/unplugin` is present in...")
   - Verter build runs (pass or fail is informational for new projects)

## Infrastructure Fixes

The runner automatically handles several common project setup issues:

### `packageManager` stripping

Projects with a `packageManager` field in `package.json` (e.g., `"pnpm@10.28.0"`) trigger corepack version switching, which fails if the exact version isn't installed. The runner strips this field before installing deps. `COREPACK_ENABLE_STRICT=0` alone is not sufficient.

### `ensureVerterAccessible()` — native binding hoisting fix

Some pnpm monorepos don't properly hoist `@verter/native` into the repo's `node_modules/`. The packages end up at the parent workspace level without the `dist/` directory containing the native `.node` binary. After installing tarballs, the runner checks if `@verter/native/index.js` and `@verter/native/dist/` exist in the repo's `node_modules/`. If not, it copies from source (`packages/native/` and `packages/unplugin/`).

### E2E tests

Projects can define an optional `e2eCmd` field. E2E tests run only on the Verter build (not baseline) after the unit tests complete. E2E logs are saved as `verter-e2e.log`. E2E frameworks used:

- **Playwright** (VitePress, vue-vben-admin): Auto-starts dev server via playwright config
- **Cypress** (slidev): Must start fixture dev server manually before running Cypress

## Common Compiler Bugs by Symptom

When a project's build fails or tests regress, these are the most common root cause patterns:

| Symptom                                           | Root Cause                                              | Example Fix                                             |
| ------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| Garbled prop names in component definition        | Span extraction using wrong source for cross-file types | P0: use `key_name` in `macros.rs`                       |
| Raw HTML in JS output (`<!-- -->`, `&amp;`, etc.) | Template node not overwritten during codegen            | P1: emit removal overwrite in `visit_comment`           |
| TS syntax in JS output (`: string`, `<T>`)        | `forceJs` not set for the bundler framework             | P2: check `meta.framework` in unplugin                  |
| `_ctx.` prefix on globals (`_ctx.String`)         | Missing entry in `is_global()` allowlist                | Add to globals list in binding resolver                 |
| Props missing from runtime declaration            | Type resolution failed for external types               | Check `resolve_external_type` and `key_name` population |
| Build hangs indefinitely                          | CSS preprocessor deadlock in transform hook             | Move preprocessing to style sub-request                 |
| Template expressions at wrong positions           | HTML entity decode shifts byte offsets                  | Pass original source to binding resolver                |

## Common Patterns and Gotchas

### VueMacros wrapping (zyronon-douyin)

```js
import Vue from "@vitejs/plugin-vue";
VueMacros({ plugins: { vue: Vue() } });
```

After replacement: `Vue(` → `verter(` correctly changes the function call. The object key `vue:` stays as-is.

### Programmatic import (slidev)

```js
// packages/slidev/node/vite/vue.ts
import Vue from '@vitejs/plugin-vue'
const VuePlugin = Vue({ ... })
```

This is NOT in a `vite.config.*` file. The CI grep and local `findFiles` must search all `.ts` files, not just config files.

### Monorepo build commands

Some projects have complex build chains. Use the most targeted command:

- `pnpm --filter <pkg> build` for specific workspace packages
- `pnpm run build:packages` to skip non-essential steps (linting, type checking)
- Avoid `npm run build` if it has problematic `prebuild` hooks (check package.json)

### npm vs pnpm differences

- **pnpm**: Needs `public-hoist-pattern` in `.npmrc`, uses `pnpm add -w`, overrides go in `pkg.pnpm.overrides`
- **npm**: Hoists by default, uses `npm install`, overrides go in `pkg.overrides`

### @vitejs/plugin-vue-jsx

The replacement patterns do NOT modify `@vitejs/plugin-vue-jsx` because:

- The import source replacement uses exact match: `'@vitejs/plugin-vue'` (closing quote prevents matching `-jsx`)
- The function rename `Vue(` does not match `VueJsx(` (different characters after `Vue`)

## File Locations

| File                                     | Purpose                                               |
| ---------------------------------------- | ----------------------------------------------------- |
| `scripts/integration-test/run.mjs`       | Local runner script                                   |
| `scripts/integration-test/projects.mjs`  | Project definitions (source of truth)                 |
| `scripts/integration-test/README.md`     | Human documentation                                   |
| `scripts/integration-test/CLAUDE.md`     | This file (agent documentation)                       |
| `.github/workflows/integration-test.yml` | CI workflow (MUST stay in sync with projects.mjs)     |
| `.integration-tests/`                    | Gitignored directory for cloned repos, tarballs, logs |

## Test Result Interpretation

Test results are the primary quality signal for Verter's compiler:

| Scenario                            | Meaning                                                   | Action                             |
| ----------------------------------- | --------------------------------------------------------- | ---------------------------------- |
| Verter tests = baseline tests       | Verter compiles correctly for this project                | None needed                        |
| Verter has fewer failures           | Verter fixed something upstream Vue got wrong (unlikely)  | Investigate                        |
| Verter has more failures            | **Compiler regression** — Verter generates incorrect code | Fix the compiler                   |
| Verter build fails, baseline passes | Verter cannot compile some pattern in this project        | Add support or document limitation |
| Both fail identically               | Project setup issue, not Verter                           | Check project compatibility        |
