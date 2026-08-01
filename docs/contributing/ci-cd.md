# CI/CD

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter uses GitHub Actions for continuous integration, testing, and releases.

## Workflows

### CI (`ci.yml`)

Runs on push to `main` and on pull requests. Uses [dorny/paths-filter](https://github.com/dorny/paths-filter) for change detection to only run relevant jobs:

- **Rust changes** (`crates/**`, `Cargo.toml`, etc.) -- `rust-fmt`, `rust-clippy`, `rust-build-configs`, `rust-test`
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
  +-- test                                  <- blocking; gates publishing AND the release
  +-- build-native      (matrix: 7 targets) <- parallel
  +-- build-lsp         (matrix: 7 targets) <- parallel
  +-- build-tsc         (matrix: 7 targets) <- parallel
  +-- build-mcp         (matrix: 7 targets) <- parallel
  +-- build-wasm                            <- parallel
  +-- build-editor-lsp
        +-- editor-helix / editor-lapce / editor-zed / editor-neovim

build-vsix (needs: validate, test, build-lsp, build-native)
  +-- github-release (needs: validate, build-native, build-lsp, build-mcp, build-wasm, build-vsix)
  +-- publish-vscode (needs: validate, build-vsix, publish-npm)

publish-crates (needs: validate, test, editor matrix)
publish-npm    (needs: validate, test, editor matrix, build-native, build-lsp, build-mcp, build-tsc, build-wasm)
  +-- integration-test (consumes the published npm packages)
```

**The GitHub Release is gated on builds, not on publishing.** Every asset it
uploads — native bindings, WASM, the `verter-lsp` and `verter-mcp` binaries
and the
platform VSIXes — is build output, so a failed npm or Marketplace publish no
longer withholds the release and its downloadable assets; publishing runs in
parallel and is retried on its own. Packaging the VSIXes is therefore a build
job (`build-vsix`); `publish-vscode` only pushes the prebuilt artifact, and stays
ordered after `publish-npm` so the extension never lands on the Marketplace
before the packages of the same version reach the registry. The release remains
test-gated transitively, through `build-vsix`.

Consequence to be aware of: the release (and the `CHANGELOG.md` commit it pushes
to `main`) can now exist for a version whose npm publish failed. That is the
intended trade — the assets are the durable artifact, and a failed publish job is
re-runnable — but it means a red `publish-npm` needs acting on, not ignoring.

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

**Binary build matrices:** `build-lsp` (the `verter-lsp` server), `build-mcp`
(the `verter-mcp` MCP server) and `build-tsc` (`verter-tsc`) cover the same 7
targets as `build-native`. Each names its artifacts after the npm platform
package they feed (`lsp-<npm-pkg>`, `mcp-<npm-pkg>`, e.g. `lsp-linux-x64-gnu`),
so `publish-npm` stages them by directory name;
`build-vsix` maps its five VSIX targets onto those same artifacts (the two
musl legs serve the npm channel only -- the VSIX has no musl target).

**Publishing process:**

1. **Rust crates** -- only `verter_compiler` is published to crates.io (binding crates are consumed via npm)
2. **npm platform packages** -- published first (e.g., `@verter/native-darwin-arm64`, `@verter/lsp-linux-x64-gnu`, `@verter/mcp-linux-x64-gnu`). The directory list is derived, not hand-listed per family: `scripts/publish-platform-dirs.mjs` prints every `packages/<pkg>/npm/<platform>` dir in the publish set, and the workflow loops over that output
3. **npm packages** -- published in topological order via `scripts/check-versions.mjs`; the publish set is derived from the product dependency closure by `scripts/lib/publish-set.mjs` (marketplace-only packages such as `verter-vscode` are excluded)
4. **GitHub Release** -- created with the changelog (via git-cliff) and the staged binary assets

**Release assets (28).** Each one is staged under an explicit, platform-qualified
name before `gh release create` runs, and the step writes the full list -- name
and size -- to the workflow run summary:

| Family                        | Count | Asset names                              |
| ----------------------------- | ----- | ---------------------------------------- |
| Native bindings               | 7     | `verter-native.<triple>.node`            |
| LSP server                    | 7     | `verter-lsp-<platform>[.exe]`            |
| MCP server                    | 7     | `verter-mcp-<platform>[.exe]`            |
| VS Code extension             | 5     | `verter-vscode-<target>.vsix`            |
| WASM                          | 2     | `verter_wasm_bg.wasm`, `verter_wasm.js`  |

Staging **fails the job** on a missing source, a duplicate asset name, or a
family whose count is short -- a partial release is a failed release. The summary
is written before that check, so a failed run still shows what it managed to
stage. Two things deliberately do *not* ship as assets: the `native-loader`
artifact (`index.js`, an npm-only file that a blanket extension sweep used to
attach as an opaque asset) and the relay shim inside the LSP artifacts (a VSIX
internal). `verter-tsc` is npm-only -- its only consumption path is `npx` inside
a Node project, whereas `verter-lsp` and `verter-mcp` must be launchable by
editors and agent hosts on machines with no Node at all.

### Release Tag (`release-tag.yml`)

Triggered on every push to `main`. Turns a version commit into the tag that triggers `release.yml`:

1. Exits cleanly unless the HEAD commit message matches `release: v<version>`
2. Reads the workspace version from the tree and fails if it disagrees with the message
3. Exits cleanly if the tag `v<version>` already exists (idempotency)
4. Verifies the full release surface (`scripts/set-version.mjs --check` and `scripts/check-versions.mjs`)
5. Creates and pushes the annotated tag `v<version>`

See [Publishing a Release](#publishing-a-release) for the full flow.

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

Releases start from a local version bump and end with an automatic tag:

1. Run `pnpm bump`. The script computes the next version from the conventional
   commits since the last `v*` tag (via `git-cliff --bumped-version` when
   git-cliff is installed, otherwise from the commit types directly: `feat` ->
   minor, `fix`/`perf` -> patch, breaking changes -> major; a pre-release stays
   in its channel and increments its counter). Overrides: `pnpm bump -- <version>`
   for an explicit version, `pnpm bump -- --prerelease <alpha|beta|rc>` for a
   pre-release channel, `pnpm bump -- --dry-run` to print without changing
   anything.
2. `pnpm bump` writes the version across the whole release surface with
   `scripts/set-version.mjs`: the `Cargo.toml` workspace version (which every
   crate inherits), `Cargo.lock`, and every package in the npm publish set —
   the publishable `packages/*` packages plus the platform sub-packages under
   `packages/{native,verter-lsp,verter-mcp,verter-tsc}/npm/*`. The target set comes from
   `scripts/lib/publish-set.mjs`, the same authority the release workflow
   publishes from; private packages are never touched.
3. `pnpm bump` requires `scripts/check-versions.mjs` to pass, refuses to run
   on a dirty tree, and refuses a version that is not greater than the current
   one. On success it creates exactly one commit, `release: v<version>`. It
   never creates a tag and never pushes.
4. Review the commit and push it to `main`.
5. The `release-tag.yml` workflow detects the version commit on `main` — the
   commit message must match `release: v<version>` and agree with the
   workspace version in the tree, and the tag must not exist yet. It re-verifies
   the whole surface (`set-version.mjs --check` and `check-versions.mjs`), then
   creates and pushes the annotated tag `v<version>`. For any other commit —
   including the CHANGELOG commit the release workflow pushes — it is a no-op.
6. The tag push triggers the `release.yml` workflow, which publishes
   everything.

### Version Checking

```bash
node scripts/check-versions.mjs          # Human-readable output
node scripts/check-versions.mjs --json   # JSON for CI consumption
```

This script compares local versions against published versions, detects pre-release channels, and computes the topological publish order. The publish set is not hand-maintained: `scripts/lib/publish-set.mjs` derives it from the product roots (`@verter/typeinfo`, `@verter/component-meta`, `@verter/unplugin`, `verter-tsc`, `verter-vscode`) by walking runtime dependency fields (`dependencies` + `optionalDependencies` + `peerDependencies`) across workspace packages. It throws if a package in the closure is `private` (marketplace-only packages exempt) or if a dependency cycle exists.

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
