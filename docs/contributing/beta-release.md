# Beta Release Procedure

Step-by-step guide for bumping from alpha to beta (or between beta releases).

## Prerequisites

- Clean working tree on `main` branch
- All CI checks passing
- All integration tests passing (or exceptions documented)

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

## 4. Commit and Tag

```bash
git add -A
git commit -m "release(all): v0.0.1-beta.1"
git tag v0.0.1-beta.1
```

## 5. Push (triggers release workflow)

```bash
git push origin main
git push origin v0.0.1-beta.1
```

The `release.yml` workflow runs automatically on tag push:

1. **validate** — clippy, fmt, test
2. **build-native** — 7 platform targets (parallel)
3. **build-wasm** — WASM binary (parallel)
4. **publish-crates** — crates.io (after validate)
5. **publish-npm** — npm with `--tag beta` (after native + wasm builds)
6. **github-release** — GitHub Release with binaries
7. **deploy-playground** — Netlify deployment

## 6. Post-Release Verification

```bash
# Verify npm packages
npm view @verter/unplugin version  # Should show 0.0.1-beta.1
npm view @verter/core version
npm view @verter/native version

# Verify VS Code marketplace
# Check https://marketplace.visualstudio.com/items?itemName=verter.verter-vscode

# Verify crates.io
cargo search verter_compiler

# Smoke test
npm create vite@latest test-app -- --template vue-ts
cd test-app
npm install @verter/unplugin@beta
# Add to vite.config.ts, run dev server, verify compilation works
```

## Files That Need Version Bumps

| File                                     | Count     |
| ---------------------------------------- | --------- |
| `Cargo.toml` (workspace)                 | 1         |
| `package.json` (root)                    | 1         |
| `packages/*/package.json`                | 16        |
| `packages/native/npm/*/package.json`     | 7         |
| `packages/verter-tsc/npm/*/package.json` | 4         |
| `extensions/vscode/package.json`         | 1         |
| **Total**                                | ~30 files |

## Dry Run

To validate the CI pipeline without publishing, tag an `alpha.4` first:

```bash
# Bump to alpha.4, commit, tag, push
# Verify the release workflow completes successfully
# Then proceed with beta.1
```
