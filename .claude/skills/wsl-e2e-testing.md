# WSL E2E Testing (Linux CI Reproduction)

E2E tests that pass on Windows may fail on Linux/CI due to timing differences (background scanner races, TSGO sync ordering). WSL reproduces CI behavior locally.

## Prerequisites (One-Time Setup)

### 1. Linux System Packages

```bash
sudo apt update
sudo apt install -y build-essential curl git xvfb \
  libnspr4 libnss3 libatk1.0-0 libcups2t64 libgtk-3-0 libxss1 \
  libasound2t64 libgbm1 libxshmfence1 libdrm2 libxkbfile1 libnotify4 \
  libsecret-1-0 libxtst6 libatspi2.0-0 libuuid1 libxrandr2 libxdamage1 \
  libxcomposite1 libxfixes3 libxkbcommon0 libpango-1.0-0 libcairo2
```

### 2. Rust Toolchain

```bash
curl https://sh.rustup.rs -sSf | sh -s -- -y
source "$HOME/.cargo/env"
```

### 3. Node/pnpm Shims (WSL Interop Fix)

WSL often picks up Windows `node.exe` via PATH interop. The ci-bin shims force native Linux binaries:

```bash
mkdir -p ~/ci-bin

cat > ~/ci-bin/node <<'EOF'
#!/usr/bin/env bash
exec /usr/bin/node "$@"
EOF

cat > ~/ci-bin/npm <<'EOF'
#!/usr/bin/env bash
exec /usr/bin/node /usr/lib/node_modules/npm/bin/npm-cli.js "$@"
EOF

cat > ~/ci-bin/npx <<'EOF'
#!/usr/bin/env bash
exec /usr/bin/node /usr/lib/node_modules/npm/bin/npx-cli.js "$@"
EOF

cat > ~/ci-bin/pnpm <<'EOF'
#!/usr/bin/env bash
exec /usr/bin/node /usr/lib/node_modules/corepack/dist/corepack.js pnpm "$@"
EOF

chmod +x ~/ci-bin/node ~/ci-bin/npm ~/ci-bin/npx ~/ci-bin/pnpm
```

**Sanity check** (run after every WSL session start):

```bash
export PATH="$HOME/ci-bin:$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/sbin"
node --version   # should be native Linux node (v20+)
pnpm --version   # should resolve via corepack
cargo --version  # should be Linux rustc
```

### 4. Clone the Repo Inside WSL

```bash
cd ~
# <windows-checkout-root> is the WSL mount of your Windows checkout, e.g. /mnt/c/path/to/verter
git clone <windows-checkout-root> verter-ci-repro
cd ~/verter-ci-repro
```

The origin points to the Windows repo, so `git fetch origin` pulls the latest branch state.

## Running E2E Tests

### Step 1: Set PATH

Every WSL session needs this (or add to `~/.bashrc`):

```bash
export PATH="$HOME/ci-bin:$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/sbin"
```

### Step 2: Sync Changes from Windows

```bash
cd ~/verter-ci-repro
git fetch origin
git checkout -B <branch-name> origin/<branch-name>

# Copy any unstaged changes from the Windows working tree
# (<windows-checkout-root> is the WSL mount of your Windows checkout)
cp <windows-checkout-root>/crates/verter_lsp/src/server.rs crates/verter_lsp/src/server.rs
cp <windows-checkout-root>/crates/verter_lsp/src/tsgo/ipc.rs crates/verter_lsp/src/tsgo/ipc.rs
# ... copy other changed files as needed
```

### Step 3: Build Linux Artifacts

```bash
cd ~/verter-ci-repro

# Build LSP binary (Linux ELF, not Windows .exe)
cargo build -p verter_lsp

# Install JS dependencies
pnpm install --frozen-lockfile

# Build TS packages needed by the extension
pnpm --filter @verter/language-shared build
pnpm --filter @verter/typescript-plugin build
pnpm --filter verter-vscode build:dev

# Compile E2E test TypeScript
pnpm --filter verter-vscode exec tsc -p tsconfig.test.json
```

### Step 4: Run Fixtures

Run fixtures **sequentially** (never in parallel — VS Code instances conflict):

```bash
# Barrel exports (barrel re-export type resolution)
E2E_FIXTURE=barrel-exports E2E_TYPE_PROVIDER=tsgo \
  xvfb-run -a pnpm --filter verter-vscode test:e2e:run

# Single project (full suite, import rewrite validation)
E2E_FIXTURE=single-project E2E_TYPE_PROVIDER=tsgo \
  xvfb-run -a pnpm --filter verter-vscode test:e2e:run

# Path aliases (tsconfig paths + per-owner config)
E2E_FIXTURE=path-aliases E2E_TYPE_PROVIDER=tsgo \
  xvfb-run -a pnpm --filter verter-vscode test:e2e:run

# Focused test (single test file within a fixture)
E2E_FIXTURE=single-project E2E_TYPE_PROVIDER=tsgo \
  VERTER_E2E_ONLY=hover.test.js \
  xvfb-run -a pnpm --filter verter-vscode test:e2e:run
```

### Step 5: Log Inspection

```bash
# Tee output for later grep
E2E_FIXTURE=single-project E2E_TYPE_PROVIDER=tsgo \
  xvfb-run -a pnpm --filter verter-vscode test:e2e:run 2>&1 \
  | tee /tmp/single-project-tsgo.stdout.log

# Check for specific TS errors (should find nothing)
grep -n "1192\|2607\|2786" /tmp/single-project-tsgo.stdout.log

# Check barrel hover content
grep -n "boolean\|showOverlay" /tmp/barrel-exports-tsgo.stdout.log

# Extension/server logs
cat /tmp/verter-e2e-single-project@tsgo.log
cat /tmp/verter-e2e-barrel-exports@tsgo.log
```

## Fixture × Provider Matrix

| Fixture           | tsgo        | tsserver | What it validates                                                  |
| ----------------- | ----------- | -------- | ------------------------------------------------------------------ |
| `single-project`  | CI-critical | Baseline | Full LSP stack: hover, completion, definition, diagnostics, rename |
| `barrel-exports`  | CI-critical | Baseline | Barrel re-export type resolution, eager sync                       |
| `path-aliases`    | CI-critical | Baseline | tsconfig `paths`, per-owner config timing                          |
| `monorepo`        | Control     | Baseline | Cross-package imports, workspace folders                           |
| `composite-paths` | Control     | Baseline | Project references + path aliases                                  |

## Acceptance Criteria

### `single-project@tsgo`

- No `TS1192` (module has no default export — import rewrite regression)
- No `TS2607` / `TS2786` (JSX namespace leakage from `.vue.tsx`)
- P6 reports only `thisDoesNotExist` (not valid props flagged as errors)
- All hover/completion/definition tests pass

### `barrel-exports@tsgo`

- `:show` hover includes `showOverlay` and `boolean`
- `:zIndex` prop-name hover shows `number`
- `label` attr hover includes `string`
- Barrel go-to-definition reaches terminal `.vue` file

### `path-aliases@tsgo`

- `@/` imports resolve without `TS2307`
- Hover on aliased imports shows typed results

## Troubleshooting

### "Cannot find module" errors in test output

The compiled `out-test/` directory may have stale files. Clean and rebuild:

```bash
rm -rf packages/vue-vscode/out-test
pnpm --filter verter-vscode exec tsc -p tsconfig.test.json
```

### Double-nested `suite/suite/` directory

If you accidentally `cp -r` the suite directory, it creates `suite/suite/`. Fix:

```bash
rm -rf packages/vue-vscode/e2e/suite/suite
rm -rf packages/vue-vscode/out-test/e2e/suite/suite
pnpm --filter verter-vscode exec tsc -p tsconfig.test.json
```

### WSL node picks up Windows node.exe

Symptom: `Error: Cannot find module 'D:\usr\bin\npm'`
Fix: Ensure `~/ci-bin` is first in PATH (see Step 1).

### GPU errors in output

`ERROR:components/viz/service/main/viz_main_impl.cc` messages are harmless — `xvfb-run` provides a virtual display but GPU acceleration is unavailable. Tests still run correctly.

### Decoration timeout failures

`Prop Constness Decorations "before all"` timeouts are a known flaky test in WSL/CI environments. Not related to LSP correctness — decorations depend on VS Code's internal scheduling which is slower under xvfb.
