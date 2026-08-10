# Framework and Product Capability Matrix

**Status:** Normative product truth after baseline lock.  
**Current state:** template rows marked `VERIFY`; A1 establishes initial command/product truth, A3 updates any fail-closed behavior, and A5/A6 finalize the exact post-safety matrix. Affected product blocks cannot start until completed.

# 1. Row schema

| Framework/product | Operation | Route/backend | Maturity | Default | Semantic profile(s) | Oracle/conformance corpus | Exact unsupported/degradation behavior | Zero-work negative proof | Compatibility promise | Status |
|---|---|---|---|---|---|---|---|---|---|---|

# 2. Seed rows

| Framework/product | Operation | Route/backend | Maturity | Default | Semantic profile(s) | Oracle/conformance corpus | Exact unsupported/degradation behavior | Zero-work negative proof | Compatibility promise | Status |
|---|---|---|---|---|---|---|---|---|---|---|
| Vue | runtime compile | direct Rust | VERIFY | VERIFY | VERIFY | official Vue fixtures + Verter corpus | VERIFY | no IDE/public/native enrichment work | VERIFY | VERIFY |
| Vue | IDE companion | managed/provider | VERIFY | VERIFY | provider-specific | provider + mapping corpus | typed route/capability failure | no runtime constructor projection unless demanded | VERIFY | VERIFY |
| Vue | imported macro runtime projection | CompileTypeInfo | VERIFY | VERIFY | supported normalized profiles | official/compiler-sfc differential | typed degradation/unresolved input | unrelated object members not traversed | VERIFY | VERIFY |
| Svelte | native runtime compile | direct Rust | Experimental (verify current pin) | VERIFY | syntax/toolchain profile | pinned Svelte compiler corpus | typed unsupported/experimental behavior | zero Vue/native compile projection | experimental | VERIFY |
| TypeInfo | `TypeAtPosition` | native | VERIFY | VERIFY | normalized TS profiles | selected TS oracle | typed partial/gap/no-value | no-flow allocates no graph/plan | VERIFY | VERIFY |
| TypeInfo | graph export | public/wire | advanced explicit | off unless requested | profile stamped | protocol/round-trip corpus | size/depth/unsupported failure | simple DTO operations serialize no graph | named compatibility domain | VERIFY |
| LSP | external TypeScript provider | project binding | VERIFY | `auto`/explicit per product | provider profile | capability matrix | actionable incompatible route; no race/fallback | disabled native enrichment is zero-work | provider epoch/profile stamped | VERIFY |
| CSS | parse/format/index/transform | native/external by dialect | VERIFY | VERIFY | dialect profile | dialect/framework corpus | typed unsupported/recovery-incomplete | identical bytes parsed once per residence | VERIFY | VERIFY |

# 3. Rules

- A missing/`VERIFY` row means the capability is not approved for architecture claims or default changes.
- Maturity is operation-specific; framework citizenship does not imply equal maturity.
- Changing a default or compatibility promise requires product/conformance review.
- Experimental behavior cannot be silently used as a stable oracle for another surface.
- Every enabled row links exact tests and benchmark cells.
- Unsupported and partial behavior is part of the public contract, not an implementation accident.
