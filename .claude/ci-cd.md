# CI/CD Documentation

## Overview

Verter uses four GitHub Actions workflows:

| Workflow                             | Trigger                              | Purpose                                                                             |
| ------------------------------------ | ------------------------------------ | ----------------------------------------------------------------------------------- |
| **CI** (`ci.yml`)                    | Push to `main`, PRs                  | Lint, test, build verification                                                      |
| **Nightly** (`nightly.yml`)          | Push to `main` (crates/wasm changes) | Build WASM, upload to GH Release                                                    |
| **Release** (`release.yml`)          | Push tag `v*`                        | Build all platforms, publish to npm/crates.io, deploy playground, create GH Release |
| **Integration Test** (`integration-test.yml`) | Manual, after release, PR comment `/integration` | Test Verter against real-world Vue projects |

## CI Workflow

**File:** `.github/workflows/ci.yml`

Uses [dorny/paths-filter](https://github.com/dorny/paths-filter) for change detection. Only runs jobs relevant to changed files:

- **Rust changes** (`crates/**`, `Cargo.toml`, etc.) → `rust-fmt`, `rust-clippy`, `rust-test`
- **JS changes** (`packages/**`, `package.json`, etc.) → `js-build-test`
- **WASM changes** (`crates/verter_core/**`, `crates/verter_wasm/**`) → `wasm-build`

All jobs run independently — one failing doesn't block others.

## Nightly Workflow

**File:** `.github/workflows/nightly.yml`

Triggered on push to `main` when `crates/**`, `packages/wasm/**`, or `packages/playground/**` change.

### What it does:

1. Builds WASM via `wasm-pack`
2. Smoke tests the WASM binary
3. Uploads commit-specific WASM assets to the `nightly` GitHub Release:
   - `verter_wasm_bg-{sha7}.wasm`
   - `verter_wasm-{sha7}.js`
4. Updates `nightly-manifest.json` (keeps last 50 commits)
5. Cleans up old assets beyond the 50-commit window
6. Builds and deploys the playground to release production (via Netlify)

### Nightly Manifest Format

```json
{
  "latest": "6178ecb",
  "commits": [
    {
      "sha": "6178ecb...",
      "short": "6178ecb",
      "date": "2025-01-15T10:30:00.000Z",
      "message": "feat(core): add v-memo support"
    }
  ]
}
```

The playground version selector fetches this manifest to populate the "Nightly Commits" dropdown.

## Release Workflow

**File:** `.github/workflows/release.yml`

Triggered on push of tags matching `v*` (e.g., `v0.0.1-alpha.1`, `v1.0.0`).

### Job Graph

```
validate
  ├── build-native (matrix: 7 targets)   ← parallel
  └── build-wasm                          ← parallel
        │
        ├── publish-crates (needs: validate)
        │
        └── publish-npm (needs: validate, build-native, build-wasm)
              │
              ├── github-release (needs: build-native, build-wasm, publish-npm)
              └── deploy-playground (needs: build-wasm)
```

### Native Build Matrix

| Target                       | Runner         | Method        |
| ---------------------------- | -------------- | ------------- |
| `x86_64-unknown-linux-gnu`   | ubuntu-latest  | Direct        |
| `x86_64-unknown-linux-musl`  | ubuntu-latest  | Cross-compile |
| `aarch64-unknown-linux-gnu`  | ubuntu-latest  | Cross-compile |
| `aarch64-unknown-linux-musl` | ubuntu-latest  | Cross-compile |
| `x86_64-apple-darwin`        | macos-13       | Direct        |
| `aarch64-apple-darwin`       | macos-latest   | Direct        |
| `x86_64-pc-windows-msvc`     | windows-latest | Direct        |

### Publishing Process

1. **Rust crates**: Only `verter_core` is published to crates.io (binding crates are consumed via npm)
2. **npm platform packages**: Published first (e.g., `@verter/native-darwin-arm64`)
3. **npm packages**: Published in topological order via `scripts/check-versions.mjs`
4. **GitHub Release**: Created with changelog (via git-cliff) and all binary assets

### Changelog Generation

Uses [git-cliff](https://git-cliff.org/) configured in `cliff.toml`. Generates changelogs from conventional commits (see commit convention in CLAUDE.md).

The release workflow:

1. Runs `git-cliff --latest --strip header` for release notes
2. Creates the GitHub Release with the generated notes
3. Pre-release tags (`-alpha`, `-beta`, `-rc`) are marked as prerelease

## Integration Test Workflow

**File:** `.github/workflows/integration-test.yml`  
**Detailed Documentation:** `.github/INTEGRATION_TEST.md`

Tests Verter against real-world open-source Vue projects to validate compatibility and track performance.

### Trigger Methods

1. **Manual** (`workflow_dispatch`): Via Actions tab, select source (artifact/npm) and projects
2. **After Release** (`workflow_call`): Automatically triggered after successful npm publish
3. **PR Comment**: Comment `/integration` on any PR (requires write permission)

### Test Matrix

Currently tests 4 popular Vue projects in parallel:

- **Vuetify** - Material Design component framework
- **PrimeVue** - Rich UI component library
- **Element Plus** - Enterprise-grade components
- **Shadcn-vue** - Modern UI components

### Test Process

For each project:

1. **Baseline** - Build and test with standard Vue compiler, record timing
2. **Verter** - Replace `vue()` with `verter()` in Vite config, rebuild and retest
3. **Compare** - Generate performance and compatibility comparison report

### Outputs

- **Aggregate Summary** - Overall compatibility rate, performance metrics
- **Project Reports** - Detailed per-project build logs and comparisons
- **Artifacts** - Downloadable reports and logs for debugging
- **PR Comments** - Results posted directly to PR (when triggered by comment)

### Comparison Mode

Tests run in **non-blocking comparison mode** during alpha:

- Failures are recorded but don't fail the workflow
- Tracks compatibility progress without blocking releases
- Can switch to strict mode when Verter reaches maturity

### Usage Examples

```bash
# Manual trigger via gh CLI
gh workflow run integration-test.yml -f source=artifact -f projects=vuetify,primevue

# Trigger from PR comment
# Comment: /integration
```

See [.github/INTEGRATION_TEST.md](../.github/INTEGRATION_TEST.md) for detailed documentation.

## Versioning

### Pre-release Support

| Version Pattern | npm dist-tag | GitHub Release | Example         |
| --------------- | ------------ | -------------- | --------------- |
| `X.Y.Z-alpha.N` | `alpha`      | prerelease     | `0.0.1-alpha.1` |
| `X.Y.Z-beta.N`  | `beta`       | prerelease     | `0.0.1-beta.1`  |
| `X.Y.Z-rc.N`    | `rc`         | prerelease     | `0.0.1-rc.1`    |
| `X.Y.Z`         | `latest`     | release        | `1.0.0`         |

Pre-releases are published with `--tag <channel>` to avoid polluting the `latest` dist-tag:

```bash
pnpm publish --access public --tag alpha --no-git-checks  # pre-release
pnpm publish --access public --no-git-checks               # stable
```

### Version Flow

```
0.0.1-alpha.1 → 0.0.1-alpha.2 → ... → 0.0.1-beta.1 → ... → 0.0.1-rc.1 → 0.0.1
```

### Per-Package Versions

- Each `package.json` has its own version
- `Cargo.toml` workspace version applies to all Rust crates
- Single git tag `vX.Y.Z` triggers the release
- `scripts/check-versions.mjs` determines which packages actually need publishing

### Publishing a Release

1. Update versions in relevant `package.json` files and `Cargo.toml`
2. Commit: `release(all): v0.0.1-alpha.1`
3. Tag: `git tag v0.0.1-alpha.1`
4. Push: `git push origin v0.0.1-alpha.1`
5. The release workflow handles everything else

### check-versions.mjs

The script at `scripts/check-versions.mjs` compares local versions against published versions:

```bash
node scripts/check-versions.mjs          # Human-readable output
node scripts/check-versions.mjs --json   # JSON for CI consumption
```

It:

- Reads all non-private package.json files
- Queries npm registry for published versions
- Detects pre-release channels from version strings
- Computes topological publish order from workspace dependencies
- Checks crates.io for Rust crate versions

## Workspace Dependencies

Uses `workspace:^` and `workspace:*` protocol. `pnpm publish` (not `npm publish`) automatically rewrites these to real semver ranges during publishing.

## Required GitHub Secrets

| Secret                 | Purpose                              |
| ---------------------- | ------------------------------------ |
| `NETLIFY_AUTH_TOKEN`   | Netlify playground deployment        |
| `NETLIFY_SITE_ID`      | Netlify site identification          |
| `CARGO_REGISTRY_TOKEN` | crates.io publishing                 |
| `NPM_TOKEN`            | npm publishing (with `--provenance`) |

The `GITHUB_TOKEN` is automatically provided and used for:

- GitHub Release creation/upload
- Nightly asset management
- Pull request comments (preview deploys)

## Playground Version Switching

The playground supports loading different WASM versions at runtime:

- **This Build**: Default — uses the locally bundled WASM
- **Published releases**: Loaded from jsDelivr CDN (`cdn.jsdelivr.net/npm/@verter/wasm@version`)
- **Nightly commits**: Loaded from GitHub Release `nightly` assets

### Technical Approach

wasm-bindgen glue JS has a singleton guard (`if (wasm !== undefined) return wasm;`). To switch versions:

1. Fetch glue JS as text, patch out singleton guard
2. Create Blob URL for fresh module instance
3. Fetch WASM binary as ArrayBuffer
4. Call `mod.default(arrayBuffer)` — bypasses URL resolution entirely

### COEP/COOP

The playground serves `Cross-Origin-Embedder-Policy: require-corp` and `Cross-Origin-Opener-Policy: same-origin`. Cross-origin fetches use `mode: 'cors'`. Both GitHub Releases and jsDelivr serve `Access-Control-Allow-Origin: *`.

If CORS issues arise, fallback: change COEP to `credentialless` in `vite.config.ts`.
