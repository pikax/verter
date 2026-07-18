# Vue behavioral multi-mode compare

Compare **Verter** to the **official Vue SFC compiler** (`@vue/compiler-sfc`) on
real OSS trees (default: Vize’s `_fixtures/_git` catalog).

## Contract

| | |
|--|--|
| **Required** | Same *behavior* / structural emit after normalize |
| **Waived** | Local names, whitespace, indentation, comments, patch flags, scope-id *hashes*, `$setup`/`$props` vs `_ctx` (proxy-equivalent) |
| **Not claimed** | Byte-identical output; official Vue *vapor* goldens (Vue 3.5 `compileTemplate({ vapor: true })` still emits VDOM) |

### Modes

| Mode | Official | Verter | Pass means |
|------|----------|--------|------------|
| `client` | `compileTemplate` (VDOM) | `forceVapor: false` Main | Normalized `render` bodies equal |
| `ssr` | `compileTemplate({ ssr: true })` | `ssr: true` Main | Normalized `ssrRender` bodies equal (uses `scripts/ssr-baseline/normalize.mjs` + extra cosmetic strips) |
| `vapor` | — | `forceVapor: true` | Compiles and emits vapor markers (`_template` / `renderEffect`) — **self-health**, not official golden |

## Usage

```bash
# Smoke (create-vue + pinia)
node scripts/vue-behavior-compare/run.mjs \
  --projects create-vue,pinia \
  --modes client,ssr,vapor

# Full populated Vize OSS catalog (~8k SFCs)
node scripts/vue-behavior-compare/run.mjs \
  --root ../vize/tests/_fixtures/_git \
  --modes client,ssr,vapor \
  --json target/vue-behavior-compare/full-oss.json

# Or via package script
pnpm run compare:vue-behavior -- --projects reka-ui --limit 200
```

### Options

- `--root` — directory of project folders (default `$VIZE_ROOT/tests/_fixtures/_git` or `../vize/...`)
- `--projects a,b` — only these folders
- `--modes client,ssr,vapor`
- `--limit N`
- `--json path`
- `--samples N` — mismatch/error samples in the report
- `--verbose`

## Interpreting results

1. **`verter_err` with `XUnresolvedImportedMacroType`**  
   Isolated `VerterHost` compile without installed package deps / project graph.
   Official `compileScript` often *skips* unresolved type args; Verter HostBacked
   treats them as hard errors. Install fixture deps first
   (`node scripts/vize-fixture-compare/install-all-fixture-deps.mjs`) when
   judging host-mode *compile health*, not AST goldens.

2. **`mismatch` with nearly identical samples**  
   Residual cosmetic AST shape (hoist array vs single node, empty scope-id arg).
   Prefer **runtime** goldens (`renderToString` / unplugin drop-in proof) for true
   behavior when AST normalize is incomplete.

3. **High client mismatch, high SSR match**  
   Expected: SSR normalize is mature; client normalize is intentionally coarse.
   Runtime HTML parity (earlier exhaustive unplugin matrix) remains the behavior bar.

## Related

- `scripts/ssr-baseline/` — older SSR-only code compare
- `scripts/vize-fixture-compare/` — fixture dependency mass-install helper for the OSS fixture checkouts
- Drop-in unplugin SSR proof — runtime `renderToString` equality
