# Integration Tests

Validates Verter against the full Vue toolchain. Tier 1 runs the curated integration matrix, and Tier 2 inventories local Vue repos under configurable roots to decide whether Verter should replace editor tooling, `vue-tsc`, the TS plugin, the build plugin, or the Nuxt module.

> [!WARNING]
> This project is **experimental and under active development**. APIs, architecture, and package boundaries may change without notice.

## Overview

The integration test suite has two layers:

- **Tier 1**: curated open-source projects cloned into `.integration-tests/repos/`, built once with stock Vue tooling, then re-run with Verter.
- **Tier 2**: local git repos discovered under configurable roots, classified by recipe, then executed in disposable sandboxes under `<scratch>/verter-toolchain-runs/<run-id>/`.

There are two entry points that share the same project list:

| Entry            | File                                     | Environment                       |
| ---------------- | ---------------------------------------- | --------------------------------- |
| **Local runner** | `scripts/integration-test/run.mjs`       | Any machine (Windows/macOS/Linux) |
| **CI workflow**  | `.github/workflows/integration-test.yml` | GitHub Actions                    |

Both read their project matrix from `scripts/integration-test/projects.mjs`.

## Quick Start

```bash
# Run all projects (builds Verter first)
pnpm integration-test

# Run a single project
pnpm integration-test coreui

# Skip baseline (faster iteration on Verter changes)
pnpm integration-test --skip-baseline coreui

# Reuse existing tarballs and checkouts
pnpm integration-test --skip-build --no-clone coreui

# Inventory local repos only
pnpm integration-test:discover

# Discover + execute Tier 2 local repos in disposable sandboxes
pnpm integration-test:local

# Limit local discovery to specific roots / repos
node scripts/integration-test/run.mjs --discover-local --discover-only --roots <repos-root> --repo-filter "vue|nuxt"
```

## CLI Options

```
node scripts/integration-test/run.mjs [options] [project-names...]

Options:
  --skip-baseline   Skip baseline build/test (faster iteration)
  --fast            Use the debug native build for faster local iteration
  --skip-build      Skip building Verter (reuse existing tarballs)
  --no-clone        Skip git clone (reuse existing checkouts)
  --discover-local  Inventory local Vue repos under the configured roots
  --discover-only   Write discovery artifacts and exit
  --local-only      Execute local discovered repos without running the matrix
  --roots <paths>   Semicolon/comma-separated discovery roots (required; or set VERTER_LOCAL_REPO_ROOTS)
  --out <path>      Output directory for local discovery/execution artifacts
  --repo-filter <r> Regex filter applied to discovered repo paths
  --run-id <id>     Override the local run id used in the output path
  --concurrency <n> Run N projects in parallel (default: 1)
  --help            Show help and available projects
```

## How It Works

```
1. Build Verter native bindings + unplugin
2. Pack tarballs → .integration-tests/tarballs/

For each project:
3. Clone repo → .integration-tests/repos/<name>/
4. Install dependencies (pnpm or npm)
5. Baseline build + test
6. Install Verter tarballs + add overrides
7. Replace @vitejs/plugin-vue → @verter/unplugin/vite in config files
8. Verify replacement (fail if not found)
9. Verter build + test
10. Compare results
```

For local discovery runs:

```
1. Recursively scan git repos under the configured roots
2. Detect Vue toolchain surfaces at repo scope
3. Pick deterministic root commands only:
   - build: scripts.build
   - test: scripts.test or scripts["test:unit"]
   - typecheck: tsconfig.json → tsconfig.web.json → tsconfig.app.json → tsconfig.src.json
4. Classify each repo into one recipe:
   - full_stack
   - typecheck_only
   - editor_only
   - build_only
   - manual_review
5. Write discovery.json + discovery.md
6. Execute non-manual repos in sandboxes under <scratch>/verter-toolchain-runs/<run-id>/
7. Persist baseline logs, Verter logs, normalized diagnostics, diffs, review queue, and summary.md
```

## Local Discovery Recipes

| Recipe           | Meaning                                                                                        |
| ---------------- | ---------------------------------------------------------------------------------------------- |
| `full_stack`     | Replace editor recommendations/settings, TS plugin, `vue-tsc`, and build/Nuxt wiring           |
| `typecheck_only` | Replace `vue-tsc` and TS plugin only                                                           |
| `editor_only`    | Replace Vue Official / Volar workspace settings only                                           |
| `build_only`     | Replace Vite/Rollup/Nuxt compiler wiring only                                                  |
| `manual_review`  | Vue-related repo found, but the root build/typecheck surface is ambiguous or non-deterministic |

`manual_review` is intentional. The runner will not guess through mixed-bundler monorepos or toolchain workspaces.

### Plugin Replacement

The script finds all `.ts`, `.js`, `.mjs` files (excluding `node_modules`) that import `@vitejs/plugin-vue` or `rollup-plugin-vue`, then applies these replacements:

- Import source: `'@vitejs/plugin-vue'` → `'@verter/unplugin/vite'`
- Import source: `'rollup-plugin-vue'` → `'@verter/unplugin/rollup'`
- Identifier: `import vue from` → `import verter from`
- Identifier: `import Vue from` → `import verter from`
- Call: `vue(` → `verter(`, `Vue(` → `verter(`

### Workspace Isolation

Cloned projects go into `.integration-tests/repos/` (gitignored). A `pnpm-workspace.yaml` with `packages: []` is placed in `.integration-tests/` to prevent pnpm from linking cloned projects into the Verter monorepo.

Tier 2 local execution never mutates the source repo. Each run copies the repo into a sandbox, runs installs and replacements there, and writes artifacts to a sibling `reports/` directory.

## Local Artifacts

Each local run writes to `<scratch>/verter-toolchain-runs/<run-id>/` by default:

| Path                                                   | Purpose                                                      |
| ------------------------------------------------------ | ------------------------------------------------------------ |
| `discovery.json`                                       | Machine-readable inventory of Tier 1 + Tier 2 repos          |
| `discovery.md`                                         | Human-readable recipe summary grouped by repo classification |
| `sandboxes/<repo>/`                                    | Disposable copy of the source repo used for execution        |
| `reports/<repo>/project.json`                          | Captured manifest entry for that repo                        |
| `reports/<repo>/baseline-*.log`                        | Baseline build/test logs when applicable                     |
| `reports/<repo>/verter-*.log`                          | Verter build/test logs when applicable                       |
| `reports/<repo>/typecheck/diagnostics.normalized.json` | Parsed `vue-tsc` / `verter-tsc` diagnostics                  |
| `reports/<repo>/typecheck/diagnostics.diff.json`       | Shared / Vue-only / Verter-only diagnostic diff              |
| `reports/<repo>/typecheck/review-queue.json`           | Persisted triage queue for Verter-only diagnostics           |
| `reports/<repo>/summary.md`                            | Per-repo execution summary                                   |

Verter-only `.vue` diagnostics are queued for later review instead of failing immediately unless the runner detects a tool crash.

## Adding a New Project

1. Check the project uses `@vitejs/plugin-vue` or `rollup-plugin-vue`
2. Identify: package manager, build command, test command, branch, bundler type
3. Add an entry to `scripts/integration-test/projects.mjs`
4. **Also add the matching entry to `.github/workflows/integration-test.yml`** (see SYNC RULE below)
5. Run `pnpm integration-test <name>` to validate

## SYNC RULE

`scripts/integration-test/projects.mjs` and `.github/workflows/integration-test.yml` define the same project matrix. When changing one, you **must** update the other. Field mapping:

| JS (projects.mjs) | YAML (integration-test.yml) |
| ----------------- | --------------------------- |
| `name`            | `name`                      |
| `repo`            | `repo`                      |
| `branch`          | `branch`                    |
| `buildCmd`        | `build-cmd`                 |
| `testCmd`         | `test-cmd`                  |
| `e2eCmd`          | `e2e-cmd`                   |
| `packageManager`  | `package-manager`           |
| `bundler`         | `bundler`                   |

## Interpreting Results

```
Project                B.Build    V.Build    Delta      B.Tests        V.Tests        Status
vuetify                45.2s      38.1s      -15%       OK 1234        OK 1234        OK
element-plus           120.3s     125.8s     +4%        skipped        skipped        SLOWER
my-project             30.1s      ERROR                                               BUILD FAIL
```

| Status       | Meaning                                                      |
| ------------ | ------------------------------------------------------------ |
| `OK`         | Verter build/tests match or improve on baseline              |
| `SLOWER`     | Verter build/tests pass but take longer                      |
| `BUILD FAIL` | Verter build failed (baseline succeeded)                     |
| `TEST REGR`  | Verter has **more** test failures than baseline (regression) |
| `TEST FAIL`  | Verter tests fail but not worse than baseline                |
| `ERROR`      | Script error (clone failed, install failed, etc.)            |

## Troubleshooting

**"No tarballs found"** — Run without `--skip-build` to build and pack Verter first.

**pnpm workspace errors** — The `.integration-tests/pnpm-workspace.yaml` boundary file may be missing. Delete `.integration-tests/` and re-run.

**Test count shows 0 but tests exist** — The test output parsing only matches `\d+ passed`/`\d+ failed` patterns (vitest/jest). Other test runners may need custom parsing.

**Replacement not found** — The project may use a different import style (e.g., `require()`, dynamic import). Check the project's config file manually and update the replacement patterns if needed.

**Repo landed in `manual_review`** — The root repo is Vue-related but the runner could not pick a single deterministic replacement path. Check `discovery.md` for the recorded reason.

## Project List

| Project              | Repo                                  | Bundler | PM   | Tests  | E2E        |
| -------------------- | ------------------------------------- | ------- | ---- | ------ | ---------- |
| vuetify              | vuetifyjs/vuetify                     | vite    | pnpm | vitest | —          |
| oku-primitives       | oku-ui/primitives                     | vite    | pnpm | vitest | —          |
| hoppscotch           | hoppscotch/hoppscotch                 | vite    | pnpm | vitest | —          |
| element-plus         | element-plus/element-plus             | rollup  | pnpm | —      | —          |
| coreui               | coreui/coreui-free-vue-admin-template | vite    | npm  | —      | —          |
| balancer-frontend-v2 | balancer/frontend-v2                  | vite    | npm  | vitest | —          |
| shadcn-vue           | unovue/shadcn-vue                     | vite    | pnpm | vitest | —          |
| slidev               | slidevjs/slidev                       | vite    | pnpm | vitest | Cypress    |
| zyronon-douyin       | zyronon/douyin                        | vite    | pnpm | —      | —          |
| primevue             | primefaces/primevue                   | rollup  | pnpm | —      | —          |
| ant-design-vue       | vueComponent/ant-design-vue           | vite    | npm  | jest   | —          |
| nuxt-ui              | nuxt/ui                               | nuxt    | pnpm | vitest | —          |
| vue-vben-admin       | vbenjs/vue-vben-admin                 | vite    | pnpm | —      | Playwright |
| vant                 | youzan/vant                           | vite    | pnpm | vitest | —          |
| naive-ui             | tusen-ai/naive-ui                     | vite    | pnpm | vitest | —          |
| tdesign-vue-next     | Tencent/tdesign-vue-next              | vite    | pnpm | vitest | —          |
| radix-vue            | unovue/radix-vue                      | vite    | pnpm | vitest | —          |
| vitepress            | vuejs/vitepress                       | vite    | pnpm | vitest | Playwright |
