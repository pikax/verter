# CI/CD

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter uses GitHub Actions for continuous integration, testing, and releases.

## Workflows

### CI (`ci.yml`)

Runs on push to `main` and on pull requests. Uses [dorny/paths-filter](https://github.com/dorny/paths-filter) for change detection to only run relevant jobs:

- **Rust changes** (`crates/**`, `Cargo.toml`, etc.) -- `rust-fmt`, `rust-clippy`, `rust-build-configs`, one provider-free `rust-test-build` archive consumed by `rust-test`, plus independent serial `rust-tsserver-live` and `rust-tsgo-live` provider jobs, the standalone `compiler-contracts` lane, and the Svelte conformance lane
- **Proto changes** -- `proto-fmt` regenerates with the pinned `buf`/`oxfmt` tools and byte-compares the complete committed TypeScript binding tree
- **JS changes** (`packages/**`, `package.json`, etc.) -- `js-build-test`
- **WASM changes** (`crates/verter_compiler/**`, `crates/verter_wasm/**`) -- `wasm-build`

Most jobs run independently. Core nextest alone consumes the shared archive.
Real tsserver/tsgo provider tests run serially with libtest in their own jobs so
third-party engines have explicit initialization and lifecycle ownership.
Compile-fail fixtures run through `node scripts/compile-contracts.mjs`, outside
Rust test discovery; Svelte conformance is also a dedicated Cargo/libtest job.

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
  +-- build-wasm
        +-- test                            <- blocking; gates publishing AND the release
                                               (runs `pnpm test`, whose @verter/wasm suite
                                               loads the wasm artifact build-wasm produced)
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

**Publishing process.** The crates and npm publishes run through
`scripts/release-publish.mjs`, the one publish path CI and a local release
share (see [Publishing locally](#publishing-locally)):

1. **Rust crates** -- `publish-crates`: the crates in `PUBLISHED_CRATES`
   (`scripts/lib/publish-set.mjs`: `verter_span`, then `verter_compiler`; the
   binding crates are consumed via npm), pausing for the crates.io index
   between them. An already-uploaded version is skipped.
2. **Stage** -- `stage --artifacts <dir>`: every platform package's binary
   (named by the package's own `files` list), the wasm build and the
   napi-generated loader are copied from the downloaded build artifacts into
   the tree. Fails closed when any platform package cannot be fed.
3. **Prepare** -- `prepare`: `pnpm run build:ts`, `@verter/native`'s
   `build:types`, and its hermetic packaging guards.
4. **npm packages** -- `publish-npm --dist-tag <tag> --provenance`: platform
   packages first (e.g., `@verter/native-darwin-arm64`,
   `@verter/lsp-linux-x64-gnu`), then the main packages in the topological
   order `scripts/lib/publish-set.mjs` derives from the product dependency
   closure (marketplace-only packages such as `verter-vscode` are excluded).
   Each package is packed once with `pnpm pack` (which resolves `workspace:`
   ranges), shipped binaries get the executable bit inside the tarball, and
   the tarball is published. Only "already published" is tolerated; every
   package is attempted before a failure fails the job.
5. **Verify** -- `verify-npm`: every package in the publish set is visible on
   the registry at the released version.
6. **GitHub Release** -- created with the changelog (via git-cliff) and the staged binary assets

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

### Release IDE (`release-ide.yml`)

The editor distribution's lane: every editor Verter supports, on one version,
triggered by an `ide/v*` tag. It publishes the two halves of "using Verter in an
editor":

1. **The VS Code Marketplace.** Five platform VSIXes (linux x64/arm64, darwin
   x64/arm64, win32 x64), each carrying its own `verter-lsp`, `verter-mcp` and
   napi binding, packaged by the same `packages/vue-vscode/package.mjs` the
   monorepo release uses and published with `vsce publish --packagePath` under
   the `vscode-marketplace` environment (`verter.verter-vscode`).
2. **Every other editor.** Helix, Zed, Lapce and nvim have no store between
   them: they launch the engine directly. The GitHub Release for the tag carries
   `verter-lsp-<platform>` and `verter-mcp-<platform>` for all seven targets —
   including the two musl ones, which have no vsce target at all.

**One version for all of them**, because every editor package is a launcher for
the same `verter-lsp` build — the VSIX embeds it, the Zed extension and the
Lapce volt spawn it, Helix and nvim run it directly. `ide/vX.Y.Z` is what makes
"which engine is in my editor?" answerable, and
`scripts/set-ide-version.mjs` writes that version into all three editor
manifests: `packages/vue-vscode/package.json`, `extensions/zed/extension.toml`
and `extensions/lapce/volt.toml`.

Registry publication for Zed (`zed-industries/extensions`) and Lapce
(plugins.lapce.dev) is a roadmap item in their READMEs. When it lands it is a
job in this workflow, against this same tag — not a lane of its own.

It validates that the tag agrees with the editor manifests before it builds
anything, and `scripts/set-ide-version.mjs --check` refuses a prerelease version
there and then: the Marketplace takes plain `MAJOR.MINOR.PATCH` and `vsce` would
otherwise reject it an hour later, after seven cross-compiles.
`workflow_dispatch` runs it as a rehearsal — everything is built and packaged,
nothing is published, because the Marketplace has no unpublish.

The relay shim is a VSIX internal and is deliberately **not** a release asset.
The `verter-lsp` / `@verter/lsp-*` npm packages and the crates stay on the
monorepo's version line and are published by `release.yml`.

### Release Tag (`release-tag.yml`)

Triggered on every push to `main`. Turns a version commit into the tag that
triggers that lane's release workflow. The **scope** of the release commit names
the lane; `scripts/release-lanes.mjs` is the table of them, and the workflow
itself is lane-agnostic:

| Commit subject                | Tag                 | Version source                     | Workflow             |
| ----------------------------- | ------------------- | ---------------------------------- | -------------------- |
| `release: v<version>`         | `v<version>`        | `Cargo.toml [workspace.package]`   | `release.yml`        |
| `release(ide): v<version>`    | `ide/v<version>`    | the editor manifests               | `release-ide.yml`    |

1. Exits cleanly unless the HEAD commit subject matches `release: v<version>` or
   `release(<lane>): v<version>`
2. Asks the lane table where that lane's version lives, reads it from the tree,
   and fails if it disagrees with the message
3. Exits cleanly if the lane's tag already exists (idempotency)
4. Runs that lane's own verification — the monorepo's is
   `scripts/set-version.mjs --check` plus `scripts/check-versions.mjs`; the
   editors' is `scripts/set-ide-version.mjs --check`
5. Creates and pushes the annotated tag

A release commit naming a lane that does not exist **fails** rather than doing
nothing: a typo'd scope is a release nobody notices never happened.

Adding a lane — when a package moves onto its own version line — is an entry in
`scripts/release-lanes.mjs` plus the workflow that listens on its tag. Nothing
in `release-tag.yml` changes. `node scripts/release-lanes.mjs list` prints the
current table.

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

The repository publishes on two independent version lines — the monorepo
(crates.io + npm) and the editor distribution (the Marketplace + the engine
binaries every other editor launches) — and both follow the same shape: bump
locally, push to `main`, `release-tag.yml` tags, the lane's workflow publishes.
`node scripts/release-lanes.mjs list` prints them.

#### The monorepo

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

#### The editor distribution

Every editor package ships on one version. `packages/vue-vscode` is
`private: true` and `MARKETPLACE_ONLY` so it is not in the npm publish set, and
the Zed/Lapce manifests are not npm packages at all — `pnpm bump` moves none of
them:

1. Run `pnpm bump:ide`. It computes the next version from the conventional
   commits touching the editor payload (`crates/`, `packages/`, `extensions/`)
   since the last `ide/v*` tag, or takes an explicit one:
   `pnpm bump:ide -- 0.2.0`. `--dry-run` prints without changing anything. There
   is no `--prerelease`: Marketplace versions are plain `MAJOR.MINOR.PATCH`, and
   VS Code's own pre-release channel is an odd minor, which is just an explicit
   version.
2. It writes the version with `scripts/set-ide-version.mjs` into all three
   editor manifests — `packages/vue-vscode/package.json` (from `MARKETPLACE_ONLY`
   in `scripts/lib/publish-set.mjs`), `extensions/zed/extension.toml` and
   `extensions/lapce/volt.toml` — verifies them, refuses a dirty tree and a
   version that is not greater than the current one, and creates exactly one
   commit: `release(ide): v<version>`. No tag, no push.
3. Review the commit and push it to `main`.
4. `release-tag.yml` tags `ide/v<version>`.
5. The tag triggers `release-ide.yml`: the extension suite runs, seven LSP and
   MCP targets and five napi targets are cross-compiled, five VSIXes are
   packaged and published to the Marketplace, and the GitHub Release for the tag
   carries the VSIXes plus the per-platform engine binaries every other editor
   launches.

The manifests were versioned separately before the lane existed (the extension
at 0.0.2, the Zed extension and the Lapce volt at 0.1.0). `pnpm bump:ide` takes
the **highest** of them as the current version and says so, so the first unified
release moves all three forward and none of them backwards.

To rehearse it without publishing, dispatch `release-ide.yml` manually — the dry
run builds and packages everything and cannot reach the Marketplace.

### Publishing locally

When `release.yml` cannot finish a release the build matrix already completed
(a red test lane, a publish credential problem), the npm and crates.io publish
runs locally through the SAME code the workflow runs —
`scripts/release-publish.mjs` — against the SAME build artifacts (the run's
`native-*`, `tsc-*`, `lsp-*`, `mcp-*`, `wasm` and `native-loader` artifacts are
retained for 90 days). Nothing is rebuilt from a developer machine: the
binaries are the tag's CI builds, and the TypeScript packages are built from
the tagged commit, which the script requires to be checked out.

```bash
git fetch --tags && git checkout v0.0.1-beta.5   # the tagged commit, clean tree
npm login                                        # a user with publish rights + 2FA
cargo login                                      # a crates.io token (no 2FA involved)

node scripts/release-publish.mjs local           # the whole thing, interactive
node scripts/release-publish.mjs local --run 35334967938   # pick the run explicitly
node scripts/release-publish.mjs local --dry-run --skip-crates   # rehearse: pack + `npm publish --dry-run`
```

`local` runs the preflight (tag = HEAD = workspace version, clean tree,
`set-version.mjs --check`, `npm whoami`), downloads the tag's completed
`release.yml` run with `gh run download` into `.release/<tag>/artifacts/`
(gitignored, reused on the next invocation; `--redownload` refreshes), then
`stage → prepare → publish-npm → verify-npm → publish-crates`. Each step is
also its own subcommand (`node scripts/release-publish.mjs <step>`), so a run
that stopped half way is resumed from the step that failed; a package already
on the registry is skipped, never re-published.

**Two-factor authentication.** npm accepts a one-time password for a short
window, so 44 packages cannot ride one code. `publish-npm --interactive-otp`
(what `local` uses) asks for a code the first time the registry demands one
and again exactly when a code is rejected as expired, retrying the same
package — a batch of publishes shares one code and you type a fresh one only
when npm asks. Pass `--otp <code>` to seed the first batch.

**Executable bits.** A tarball packed on Windows records mode `0644` for every
file, so a `verter-lsp` / `verter-mcp` / `verter-tsc` platform package
published straight from `pnpm publish` installs and then cannot be spawned.
`publish-npm` therefore packs each package to a tarball and sets `0755` on the
shipped binaries inside the archive (the files the platform package's own
`files` list names) before `npm publish <tarball>` — on every host, so CI and a
local release produce the same tarballs.

Not covered by the local path: the GitHub Release with its staged assets, the
`CHANGELOG.md` commit, the platform VSIXes and the Marketplace publish —
`release.yml` owns those.

### Version Checking

```bash
node scripts/check-versions.mjs          # Human-readable output
node scripts/check-versions.mjs --json   # JSON for CI consumption
```

This script compares local versions against published versions, detects pre-release channels, and computes the topological publish order. The publish set is not hand-maintained: `scripts/lib/publish-set.mjs` derives it from the product roots (`@verter/typeinfo`, `@verter/component-meta`, `@verter/unplugin`, `verter-tsc`, `verter-vscode`) by walking runtime dependency fields (`dependencies` + `optionalDependencies` + `peerDependencies`) across workspace packages. It throws if a package in the closure is `private` (marketplace-only packages exempt) or if a dependency cycle exists.

## Build Order

`pnpm build` (host developer build) builds in order:

```
native -> lsp -> ts packages
```

It never builds WASM and never runs `wasm-opt`. `pnpm dist` (publication-ready artifacts) adds a WASM step
between lsp and ts packages, building the full `@verter/wasm` lane (bindgen + cached `wasm-opt`):

```
native -> lsp -> wasm (bindgen + wasm-opt) -> ts packages
```

**Common rebuild sequences:**

| What changed                   | Rebuild commands                                                     |
| ------------------------------ | ---------------------------------------------------------------------|
| Rust crate (`verter_compiler`)     | `pnpm run build:native` then rebuild downstream consumers        |
| Rust LSP (`verter_lsp`)        | `pnpm run build:lsp` then restart VS Code extension host              |
| Unplugin (`packages/unplugin`) | `pnpm run build:ts`                                                   |
| WASM, developer iteration      | `pnpm run build:wasm` (bindgen only, no `wasm-opt`, no playground copy) |
| WASM, publication-ready        | `pnpm --filter @verter/wasm build` (bindgen + cached `wasm-opt`)      |
| Host developer build           | `pnpm build` (native + lsp + ts, in order)                            |
| Publication-ready artifacts    | `pnpm dist` (native + lsp + wasm + ts, in order)                      |

## Required GitHub Secrets

| Secret                  | Purpose                                                     |
| ----------------------- | ----------------------------------------------------------- |
| `NETLIFY_AUTH_TOKEN`    | Netlify playground deployment                               |
| `NETLIFY_SITE_ID`       | Netlify site identification                                 |
| `CARGO_REGISTRY_TOKEN`  | crates.io publishing                                        |
| `NPM_TOKEN`             | npm publishing (with `--provenance`)                        |
| `VSCE_PAT`              | VS Code Marketplace publishing (`verter.verter-vscode`)      |
| `RELEASE_TAG_SSH_KEY`   | the deploy key `release-tag.yml` pushes tags with, so the tag push starts a release workflow (a push made with `GITHUB_TOKEN` never does) |

The `GITHUB_TOKEN` is automatically provided for GitHub Release creation, nightly asset management, and PR comments.
