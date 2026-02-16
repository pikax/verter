# Integration Tests

Validates Verter's compiler output by building and testing real-world Vue projects with `@verter/unplugin` replacing `@vitejs/plugin-vue` (or `rollup-plugin-vue`).

> [!WARNING]
> This project is **experimental and under active development**. APIs, architecture, and package boundaries may change without notice.

## Overview

The integration test suite clones open-source Vue projects, builds them once with the stock Vue plugin (baseline), swaps in `@verter/unplugin`, rebuilds and retests, then compares the results. Test pass/fail counts are the primary quality signal for Verter's compiler.

There are two entry points that share the same project list:

| Entry | File | Environment |
|-------|------|-------------|
| **Local runner** | `scripts/integration-test/run.mjs` | Any machine (Windows/macOS/Linux) |
| **CI workflow** | `.github/workflows/integration-test.yml` | GitHub Actions |

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
```

## CLI Options

```
node scripts/integration-test/run.mjs [options] [project-names...]

Options:
  --skip-baseline   Skip baseline build/test (faster iteration)
  --skip-build      Skip building Verter (reuse existing tarballs)
  --no-clone        Skip git clone (reuse existing checkouts)
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

### Plugin Replacement

The script finds all `.ts`, `.js`, `.mjs` files (excluding `node_modules`) that import `@vitejs/plugin-vue` or `rollup-plugin-vue`, then applies these replacements:

- Import source: `'@vitejs/plugin-vue'` → `'@verter/unplugin/vite'`
- Import source: `'rollup-plugin-vue'` → `'@verter/unplugin/rollup'`
- Identifier: `import vue from` → `import verter from`
- Identifier: `import Vue from` → `import verter from`
- Call: `vue(` → `verter(`, `Vue(` → `verter(`

### Workspace Isolation

Cloned projects go into `.integration-tests/repos/` (gitignored). A `pnpm-workspace.yaml` with `packages: []` is placed in `.integration-tests/` to prevent pnpm from linking cloned projects into the Verter monorepo.

## Adding a New Project

1. Check the project uses `@vitejs/plugin-vue` or `rollup-plugin-vue`
2. Identify: package manager, build command, test command, branch, bundler type
3. Add an entry to `scripts/integration-test/projects.mjs`
4. **Also add the matching entry to `.github/workflows/integration-test.yml`** (see SYNC RULE below)
5. Run `pnpm integration-test <name>` to validate

## SYNC RULE

`scripts/integration-test/projects.mjs` and `.github/workflows/integration-test.yml` define the same project matrix. When changing one, you **must** update the other. Field mapping:

| JS (projects.mjs) | YAML (integration-test.yml) |
|-------------------|-----------------------------|
| `name` | `name` |
| `repo` | `repo` |
| `branch` | `branch` |
| `buildCmd` | `build-cmd` |
| `testCmd` | `test-cmd` |
| `packageManager` | `package-manager` |
| `bundler` | `bundler` |

## Interpreting Results

```
Project                B.Build    V.Build    Delta      B.Tests        V.Tests        Status
vuetify                45.2s      38.1s      -15%       OK 1234        OK 1234        OK
element-plus           120.3s     125.8s     +4%        skipped        skipped        SLOWER
my-project             30.1s      ERROR                                               BUILD FAIL
```

| Status | Meaning |
|--------|---------|
| `OK` | Verter build/tests match or improve on baseline |
| `SLOWER` | Verter build/tests pass but take longer |
| `BUILD FAIL` | Verter build failed (baseline succeeded) |
| `TEST REGR` | Verter has **more** test failures than baseline (regression) |
| `TEST FAIL` | Verter tests fail but not worse than baseline |
| `ERROR` | Script error (clone failed, install failed, etc.) |

## Troubleshooting

**"No tarballs found"** — Run without `--skip-build` to build and pack Verter first.

**pnpm workspace errors** — The `.integration-tests/pnpm-workspace.yaml` boundary file may be missing. Delete `.integration-tests/` and re-run.

**Test count shows 0 but tests exist** — The test output parsing only matches `\d+ passed`/`\d+ failed` patterns (vitest/jest). Other test runners may need custom parsing.

**Replacement not found** — The project may use a different import style (e.g., `require()`, dynamic import). Check the project's config file manually and update the replacement patterns if needed.

## Project List

| Project | Repo | Bundler | PM | Tests |
|---------|------|---------|----|-------|
| vuetify | vuetifyjs/vuetify | vite | pnpm | vitest |
| oku-primitives | oku-ui/primitives | vite | pnpm | vitest |
| hoppscotch | hoppscotch/hoppscotch | vite | pnpm | vitest |
| element-plus | element-plus/element-plus | rollup | pnpm | — |
| coreui | coreui/coreui-free-vue-admin-template | vite | npm | — |
| balancer-frontend-v2 | balancer/frontend-v2 | vite | npm | vitest |
| shadcn-vue | unovue/shadcn-vue | vite | pnpm | vitest |
| slidev | slidevjs/slidev | vite | pnpm | vitest |
| zyronon-douyin | zyronon/douyin | vite | pnpm | — |
| primevue | primefaces/primevue | rollup | pnpm | — |
| ant-design-vue | vueComponent/ant-design-vue | vite | npm | jest |
