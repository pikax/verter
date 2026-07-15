# CI/CD

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter uses GitHub Actions for continuous integration, testing, and releases.

## Workflows

### CI (`ci.yml`)

Runs on push to `main` and on pull requests. Uses [dorny/paths-filter](https://github.com/dorny/paths-filter) for change detection to only run relevant jobs:

- **Rust changes** (`crates/**`, `Cargo.toml`, etc.) -- `rust-fmt`, `rust-clippy`, `rust-test`
- **JS changes** (`packages/**`, `package.json`, etc.) -- `js-build-test`
- **WASM changes** (`crates/verter_compiler/**`, `crates/verter_wasm/**`) -- `wasm-build`

All jobs run independently -- one failing does not block others.

### Benchmark (`benchmark.yml`)

Triggered via `/benchmark` PR comment or manual dispatch. Compares Verter compilation performance against Vue's official compiler.

### LSP Benchmark (`lsp-benchmark.yml`)

Triggered via `/lsp-benchmark` PR comment or manual dispatch. Runs the Verter-vs-Volar LSP benchmark on Linux, macOS, and Windows and reports per-OS values.

### Integration Test (`integration-test.yml`)

Tests Verter against real-world open-source Vue projects to validate compatibility.

**Trigger methods:**

- **Manual** (`workflow_dispatch`) -- select source (artifact/npm) and projects via the Actions tab
- **After Release** (`workflow_call`) -- automatically triggered after successful npm publish
- **PR Comment** -- comment `/integration` on any PR (requires write permission)

**Test matrix includes:**
Vuetify, PrimeVue, Element Plus, Shadcn-vue, and other popular Vue projects.

**Test process for each project:**

1. **Baseline** -- build and test with the standard Vue compiler, record timing
2. **Verter** -- replace `vue()` with `verter()` in Vite config, rebuild and retest
3. **Compare** -- generate performance and compatibility comparison report

Per-project steps retain both baseline and Verter results even when a build or
test command fails, so the aggregate report remains useful. The aggregate PR
check fails when Verter introduces a project failure and reports neutral when
only warnings remain.

### Release (`release.yml`)

Triggered on push of tags matching `v*` (e.g., `v0.0.1-beta.1`, `v1.0.0`).

**Job graph:**

```
validate
  +-- build-native (matrix: 7 targets)   <- parallel
  +-- build-wasm                          <- parallel
        |
        +-- publish-crates (needs: validate)
        |
        +-- publish-npm (needs: validate, build-native, build-wasm)
              |
              +-- github-release (needs: build-native, build-wasm, publish-npm)
              +-- deploy-playground (needs: build-wasm)
```

**Native build matrix:**

| Target                       | Runner         | Method        |
| ---------------------------- | -------------- | ------------- |
| `x86_64-unknown-linux-gnu`   | ubuntu-latest  | Direct        |
| `x86_64-unknown-linux-musl`  | ubuntu-latest  | Cross-compile |
| `aarch64-unknown-linux-gnu`  | ubuntu-latest  | Cross-compile |
| `aarch64-unknown-linux-musl` | ubuntu-latest  | Cross-compile |
| `x86_64-apple-darwin`        | macos-13       | Direct        |
| `aarch64-apple-darwin`       | macos-latest   | Direct        |
| `x86_64-pc-windows-msvc`     | windows-latest | Direct        |

**Publishing process:**

1. **Rust crates** -- only `verter_compiler` is published to crates.io (binding crates are consumed via npm)
2. **npm platform packages** -- published first (e.g., `@verter/native-darwin-arm64`)
3. **npm packages** -- published in topological order via `scripts/check-versions.mjs`
4. **GitHub Release** -- created with changelog (via git-cliff) and all binary assets

### Nightly (`nightly.yml`)

Triggered on push to `main` when `crates/**`, `packages/wasm/**`, or `packages/playground/**` change.

1. Builds WASM via `cargo build --target wasm32-unknown-unknown`, `wasm-bindgen`, and a `wasm-opt` size pass
2. Smoke tests the WASM binary
3. Uploads commit-specific WASM assets to the `nightly` GitHub Release
4. Updates `nightly-manifest.json` (keeps last 50 commits)
5. Cleans up old assets beyond the 50-commit window
6. Builds and deploys the playground to production (via Netlify)

## Versioning

### Pre-release Flow

```
alpha -> beta -> rc -> stable
```

| Version Pattern | npm dist-tag | GitHub Release | Example         |
| --------------- | ------------ | -------------- | --------------- |
| `X.Y.Z-alpha.N` | `alpha`      | prerelease     | `0.0.1-alpha.1` |
| `X.Y.Z-beta.N`  | `beta`       | prerelease     | `0.0.1-beta.1`  |
| `X.Y.Z-rc.N`    | `rc`         | prerelease     | `0.0.1-rc.1`    |
| `X.Y.Z`         | `latest`     | release        | `1.0.0`         |

Pre-releases are published with `--tag <channel>` to avoid polluting the `latest` dist-tag.

### Publishing a Release

1. Update versions in relevant `package.json` files and `Cargo.toml`
2. Commit: `release(all): v0.0.1-beta.1`
3. Tag: `git tag v0.0.1-beta.1`
4. Push: `git push origin v0.0.1-beta.1`
5. The release workflow handles everything else

### Version Checking

```bash
node scripts/check-versions.mjs          # Human-readable output
node scripts/check-versions.mjs --json   # JSON for CI consumption
```

This script compares local versions against published versions, detects pre-release channels, and computes topological publish order from workspace dependencies.

## Build Order

Dependencies must be built in order:

```
native -> lsp -> wasm -> ts packages
```

**Common rebuild sequences:**

| What changed                   | Rebuild commands                                          |
| ------------------------------ | --------------------------------------------------------- |
| Rust crate (`verter_compiler`)     | `pnpm run build:native` then rebuild downstream consumers |
| Rust LSP (`verter_lsp`)        | `pnpm run build:lsp` then restart VS Code extension host  |
| Unplugin (`packages/unplugin`) | `pnpm run build:ts`                                       |
| WASM (for playground)          | `pnpm run build:wasm`                                     |
| Everything                     | `pnpm build` (runs all in correct order)                  |

## Required GitHub Secrets

| Secret                 | Purpose                              |
| ---------------------- | ------------------------------------ |
| `NETLIFY_AUTH_TOKEN`   | Netlify playground deployment        |
| `NETLIFY_SITE_ID`      | Netlify site identification          |
| `CARGO_REGISTRY_TOKEN` | crates.io publishing                 |
| `NPM_TOKEN`            | npm publishing (with `--provenance`) |

The `GITHUB_TOKEN` is automatically provided for GitHub Release creation, nightly asset management, and PR comments.
