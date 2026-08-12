# Framework and Product Capability Matrix

**Status:** Normative product truth after baseline lock.  
**Current state:** seed rows as ratified at the A0-accepted base; every `Status` cell is exactly `VERIFY` — A1 ratifies no maturity, default, or compatibility cell, and every non-`VERIFY` seed cell (the Svelte experimental maturity/promise, the graph-export maturity/default/promise, the LSP default/promise, and the seeded degradation/zero-work cells) is the Revision 11 plan's own seed, carried unaltered from the A0-accepted base under the authority of that acceptance. A1 contributes execution evidence only: §2.1 maps each row to its canonical selectors and to the externally retained A1 evidence bundle; per-run counts, verdicts, receipts, and recorded pre-existing failures live exclusively in that bundle (`A1/command-proofs/index.md`, `A1/sentinel-verification.md` under the program's external evidence root) and are never inlined here, so this file is byte-stable across evidence re-runs. A3 updates any fail-closed behavior, and A5/A6 finalize the exact post-safety matrix. Affected product blocks cannot start until completed.

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

## 2.1 A1 execution evidence (references only)

A1 proves ONLY that each row's canonical selectors execute their intended targets
with non-zero work. It ratifies nothing in the table above. Every reference below
names an entry in the external A1 evidence bundle; the bundle — not this file —
carries the counts, exit codes, receipts, digests, and recorded pre-existing
failures. Evidence rows are `A1/command-proofs/index.md` row numbers; sentinels
are `A1/sentinel-verification.md` entries.

- **Vue / runtime compile:** the canonical Rust gate (row 01, including the
  in-gate Verter corpus suites and the gate-internal Vue macro-oracle checks);
  the committed official-Vue golden corpus check (row 12) — the golden corpus
  spans three backend trees (`vdom`, `vapor`, `vdom-inline`); the
  official-compiler macro-runtime oracle (row 15). Sentinels A (gate) and C
  (goldens) discriminate these selectors.
- **Vue / IDE companion:** the editor-neutral provider-matrix lane over real
  tsserver, managed tsgo, and relay shared-tsgo routes (row 18, machine receipt
  row 18r); the external-corpus gate lane (row 16, receipt row 16r) executed
  against a classified corpus identified in the bundle only by an anonymous
  label and a content fingerprint. Sentinel D discriminates the corpus-gate
  selector.
- **Vue / imported macro runtime projection:** the official/compiler-sfc
  macro-runtime differential oracle (row 15), re-executed inside the canonical
  gate (row 01).
- **Svelte / native runtime compile:** the pinned-compiler golden checks
  (rows 13, 14), the name-parity corpus check (row 17), the conformance-corpus
  reconciliation (row 19 — the reconciliation binary emits a verdict, not a
  count; the bundle records the independently counted fixture inventory), and
  the live feature-gated oracle harness (row 20). The pin itself is a tree fact
  recorded in `A1/environment.md`.
- **TypeInfo / `TypeAtPosition`:** native suites inside the canonical gate
  (row 01); the JS `@verter/typeinfo` package suite inside the workspace JS run
  (rows 08, 08c).
- **TypeInfo / graph export:** the wire/taxonomy guard suites inside the
  canonical gate (row 01) and the targeted `typeinfo_proto_ts_freshness`
  byte-pin receipt (row 01b — a direct targeted execution, not an inference
  from unfiltered gate coverage).
- **LSP / external TypeScript provider:** the provider-matrix lane (row 18);
  its machine receipt (row 18r) records a `sourceSha` field captured from the
  checkout at run time, and A1's evidence binding requires that field to equal
  the A1 candidate SHA recorded in the external program ledger; the
  external-corpus gate lane (row 16). Sentinel D discriminates the corpus-gate
  selector.
- **CSS / parse/format/index/transform:** the targeted CSS-syntax package
  receipt (row 01c — a direct targeted execution, not an inference from
  unfiltered gate coverage) plus the Svelte conformance golden corpus whose
  committed payloads pin CSS output bytes (row 14). No dedicated standalone
  CSS selector exists in the repo beyond these.

# 3. Rules

- A missing/`VERIFY` row means the capability is not approved for architecture claims or default changes.
- Maturity is operation-specific; framework citizenship does not imply equal maturity.
- Changing a default or compatibility promise requires product/conformance review.
- Experimental behavior cannot be silently used as a stable oracle for another surface.
- Every enabled row links exact tests and benchmark cells.
- Unsupported and partial behavior is part of the public contract, not an implementation accident.

# 4. Proposed AMD-005 framework detail

AMD-005 is not ratified. Its proposed exact framework/profile/route expansion is
machine-readable at
[`../evidence/framework-conformance/capability-matrix.tsv`](../evidence/framework-conformance/capability-matrix.tsv).
Until ratification and BF1 acceptance, those rows do not replace the `VERIFY` seed
truth above. On acceptance they govern Vue RC.3 and Svelte 5.56.8 compiler products;
the seed rows remain historical lineage.
