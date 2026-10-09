# Beta Release Procedure

Step-by-step guide for bumping from alpha to beta (or between beta releases).

The current flow is `pnpm bump` → a `release: v<version>` pull request, whose
CI runs every lane and the `Release Check` rehearsal → squash-merge it with that
exact subject → `release-tag.yml` tags → `release.yml` proves the pull
request's CI and publishes its artifacts; see
[Publishing a Release](ci-cd.md#publishing-a-release). When `release.yml` cannot
publish, publish the same proven artifacts with
`node scripts/release-publish.mjs local` — see
[Publishing locally](ci-cd.md#publishing-locally). The manual version-bump steps
below (sections 1–3) are the pre-`pnpm bump` procedure, kept for reference;
sections 4–5 are how any release lands, through its pull request.

## Prerequisites

- Clean working tree on `main` branch
- All CI checks passing
- The complete release gate and required integration suites pass without waivers

## 1. Version Bump

All version strings live in two places: `Cargo.toml` (workspace) and `package.json` files.

### Rust (single source)

```bash
# Edit workspace version in root Cargo.toml
# Change: version = "0.0.1-alpha.3"
# To:     version = "0.0.1-beta.1"
sed -i 's/0.0.1-alpha.3/0.0.1-beta.1/' Cargo.toml
```

### JavaScript (all packages)

```bash
# Root + all packages + platform packages
find . -name "package.json" \
  -not -path "*/node_modules/*" \
  -not -path "*/.integration-tests/*" \
  -exec sed -i 's/"0.0.1-alpha.3"/"0.0.1-beta.1"/g' {} +
```

### Verify

```bash
# Should show all packages with beta.1 and distTag "beta"
node scripts/check-versions.mjs

# JSON output for CI validation
node scripts/check-versions.mjs --json | jq '.channel'
# Expected: "beta"
```

## 2. Update Documentation Version References

```bash
# Find and update version references in docs
grep -rn "alpha.3" docs/ CLAUDE.md
# Update any hardcoded version strings
```

## 3. Generate Changelog

```bash
# Generate changelog for all commits since last tag
git cliff --tag v0.0.1-beta.1 -o CHANGELOG.md

# Or preview without writing
git cliff --tag v0.0.1-beta.1 --unreleased
```

## 4. Commit and open the release pull request

```bash
git switch -c release/v0.0.1-beta.1
git add -A
git commit -m "release: v0.0.1-beta.1"
git push -u origin release/v0.0.1-beta.1
gh pr create --title "release: v0.0.1-beta.1" --fill
```

The pull request's CI runs every lane plus the `Release Check` rehearsal (every
build, the packaging and the clean-room smoke test). Do not push the release
commit to `main` or tag it by hand: the tag publishes only the artifacts of a
merged release pull request's green CI.

## 5. Merge (tags and triggers the release workflow)

Squash-merge the pull request once `CI Required` is green, with the commit
subject exactly `release: v0.0.1-beta.1`: remove the ` (#N)` GitHub appends,
since `release-tag.yml` tags only an exact release subject. It then tags the squash commit `v0.0.1-beta.1`, and the tag push runs `release.yml`:

1. **validate** — proves the tag is the squash of the release pull request
   whose CI passed for this tree (`scripts/release-proof.mjs`); the tests and
   every build already ran there, as its CI lanes and `Release Check`
2. **publish-npm** — npm with `--tag beta`, from the release pull request's
   CI artifacts
3. **github-release** — GitHub Release with the same binaries
4. **deploy-playground** — Netlify deployment

## 6. Post-Release Verification

```bash
# Verify npm packages
npm view @verter/unplugin version  # Should show 0.0.1-beta.1
npm view @verter/native version

# Verify VS Code marketplace
# Check https://marketplace.visualstudio.com/items?itemName=verter.verter-vscode

# Smoke test
npm create vite@latest test-app -- --template vue-ts
cd test-app
npm install @verter/unplugin@beta
# Add to vite.config.ts, run dev server, verify compilation works
```

## Versioned Surfaces

Do not rely on a hard-coded file count. Version-bearing surfaces include the
workspace `Cargo.toml`, root `package.json`, publishable `packages/*` manifests,
native and `verter-tsc` platform manifests, and the VS Code extension manifest.
`node scripts/check-versions.mjs` is the executable completeness check.

## Pre-publish rehearsal

Do not create a synthetic release tag as a dry run: every `v*` tag triggers the
publishing workflow. Rehearse on the immutable release-candidate SHA instead:

```bash
pnpm install --frozen-lockfile
node scripts/gate.mjs --exhaustive
node scripts/check-versions.mjs --json
```

Only tag the already-reviewed candidate after those checks and the release
review are complete.
