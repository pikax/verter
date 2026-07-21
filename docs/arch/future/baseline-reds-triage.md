# Baseline gate-reds triage — release deferral decision support

> Read-only triage of the pre-existing "baseline" gate reds carried into
> `release/clean-review` (they were already red on a parent line, not
> introduced by the merge). Purpose: let the product owner decide fix-now vs
> defer-to-post-release. No red is fixed here — analysis + recommendations only.
> Confidentiality: every fixture referenced below is an in-repo hermetic
> fixture (`single-project`, `svelte-parity`, `tests/.../diagnostics`); no
> external/private corpus is involved.

## Summary

Measured on this machine (node_modules present, real tsserver + tsgo + gated
`typescript@7.0.2` engine available), the enumerated categories surface **38
distinct failing tests** in **9 root-cause groups** (29 real-provider/LSP + 9
non-provider). The published "46" headline likely counts additional per-engine
variants or a wider gate slice; the 9 groups below cover every enumerated
category.

Recommendation split (by test): **7 FIX-NOW, 10 DEFER, 21 VERIFY-AFTER-FEEDING.**

- **FIX-NOW quick wins — 5 groups / 7 reds, together roughly half a day:** the
  Windows tsc-mock harness bug (one line), the single phase-archaeology needle
  (one line), the IDE-codegen characterization re-bless (paste one value), the
  three fs-boundary guards (one 4-file allowlist/route sweep clears all three),
  and the extension Svelte-grammar removal (one edit + a yes/no product call).
- **21 VERIFY-AFTER-FEEDING:** the provider-returns-empty family (empty
  diagnostics/hover/refs/code-actions, tsserver-skewed) lives in the exact
  document-sync / project-membership / secondary-file-warm / carrier-diagnostic
  domain the in-progress provider document-feeding cutover (and the external-TS
  project-binding work) reorganizes. Re-run these after Landing 1/2 before
  spending any fix effort — most should clear as a side effect.
- **Single biggest discrete effort:** completing the **global-components typed
  IDE hover/diagnostic feature** (8 provider reds, both engines) — a real
  cross-engine feature, already mid-development (it just added the codegen
  preamble that broke the characterization pin). The largest *aggregate* red
  count is the 21-test provider-empty family, but that effort is owned by the
  in-progress feeding/external-TS work, not net-new here.
- **One item that is NOT a re-pin and needs a real look:** `verter-tsc`
  diagnostic-set parity — the batch CLI now drops *all* real fixture type
  errors and emits spurious stub module-resolution errors (see G8). Importance
  is high if `verter-tsc` is in release scope.

## Triage table

| # | Red (or group) | Root cause | Effort | Importance | Covered by in-progress? | Recommendation |
|---|---|---|---|---|---|---|
| G1 | Provider-returns-empty family (21): `hover::hover_secondary_files_{tsgo,tsserver}`; `diagnostics::{vue,svelte}_*` invalid-prop / unused-binding / destructured / class-jsx / unused-snippet (8); `external_ts_baseline::vue_carrier_*_tsserver` (3); `import_matrix::{import_core_bundler_edge,import_syntax_passthrough}_tsserver`; `multi_fixture::…no_default_ts1192_tsserver`; `code_action::vue_add_missing_import…_{tsgo,tsserver}`; `carrier_dx_tests::…_tsserver`; `rename::rename_kebab_prop_usage…_tsserver`; `server::…real_tsserver_slot_member_access…` | Provider produces nothing on small hermetic fixtures — "must warm the fixture project", `got []`, `public=[]; provider=[]`, hover `""`, `observed_emission=false`. Symptom of document-sync / project-membership / secondary-(imported)-carrier warm / carrier-diagnostic-flow not committing. tsserver-skewed (many tsgo peers pass). One named contained bug inside: the `ResilientProvider` wrapper's `register_carrier_member` no-op swallows carrier registration (external_ts resilient test). | LARGE (aggregate; owned by in-progress work) | HIGH (core IDE diagnostics/hover/refs on the actively-edited + imported files) | **YES** — provider document-feeding cutover + external-TS project-binding work target this exact domain | **VERIFY-AFTER-FEEDING** |
| G2 | global-components feature (8): `global_component_tag_typed_in_{setup,options}_arm_{tsgo,tsserver}`; `custom_element_tag_stays_fail_open_{tsgo,tsserver}`; `global_component_unknown_tag_fails_closed_{tsgo,tsserver}` | Hover *answers* but degraded: tsgo → `const GlobalCountComp: any` (registration not typed); tsserver → raw `<global-count-comp>` (kebab tag not resolved to the Pascal binding); unknown-tag fail-closed diagnostic missing (`got: []`). A genuine, partially-built cross-engine feature (the codegen preamble in G7 is its in-flight artifact). | LARGE | MEDIUM–HIGH (global-component IDE typing is a real Volar-parity feature) | PARTIAL — same in-dev global-components workstream (not the feeding work) | **DEFER** (verify after global-components lands) |
| G3 | fs-boundary guards (3, one cause): `foundations_guards::{no_std_fs_in_semantic_session_paths, vfs_boundary_is_authoritative}` + `foundations_guards::no_std_fs_outside_native_fs_or_allow_list` | 4 files use `std::fs::` outside `native_fs.rs` without allowlist entries: `real_provider_tests/global_components.rs` (test), `type_provider/project_sync.rs` (prod), `vue_assets.rs` (prod), `verter_tsgo_api/src/fake_engine.rs` (fake). One fix clears all three guards. | QUICK–MODERATE | LOW (portability/VFS hygiene; not user-facing) | NO (project_sync.rs sits in the dir feeding touches, but not fixed by it) | **FIX-NOW** |
| G4 | `foundations_guards::no_oversize_files` | 7 production files > 1500 lines: `verter_compiler/src/strip_types/typescript.rs` (1566), `verter_lsp/src/background_drain.rs` (1534), `verter_lsp/src/documents/mod.rs` (1505), `verter_lsp/src/features/hover.rs` (2007), `verter_lsp/src/server/nav_features.rs` (1558), `verter_lsp/src/server_utils.rs` (1729), `verter_session/src/template_convert.rs` (2149). | MODERATE (split) / QUICK (exempt) | LOW (hygiene) | PARTIAL — 5 of 7 are LSP sync files the feeding cutover rewrites/deletes (`background_drain` is explicitly deleted) | **DEFER** |
| G5 | `architecture_guards::no_phase_archaeology_in_packages_ts_source` | Exactly **1** violation: `packages/dx-harness/src/corpus-gate/receipt.ts:7` carries plan/phase vocabulary. | QUICK (one line) | LOW (source hygiene) | NO | **FIX-NOW** |
| G6 | `client_framework_manifest_ts_freshness::client_framework_manifest_drives_extension_wiring` | `packages/vue-vscode/package.json` ships a Svelte TextMate grammar (`"scopeName": "source.svelte"`, ~line 549-550). The wiring test forbids it ("Verter must not ship a Svelte grammar — rely on the user's Svelte extension"). Not the byte-pin sibling; a source scanner. | QUICK | LOW–MEDIUM (extension packaging; a shipped grammar can conflict with the user's Svelte extension) | NO (Svelte client wiring) | **FIX-NOW** (needs a ship-grammar-or-not product call) |
| G7 | `g_misc0::language_routing_characterization::vue_and_ts_routing_snapshot` | Byte-pin drift: IDE codegen now unconditionally emits the global-components preamble — two extra `@verter/types` imports (`GlobalComponentType`/`GlobalComponentKebabType` types + `globalComponentsNav` value). `EXPECTED_IDE_CODE` was not re-blessed. Intended addition, stale pin. | QUICK (hand-edit the constant) | LOW (characterization snapshot) | YES — coupled to G2 global-components codegen | **FIX-NOW** (re-bless; redo if G2 codegen still shifts) |
| G8 | `diagnostic_set_parity::verter_tsc_diagnostic_set_parity` | NOT a re-pin. `verter-tsc --noEmit` (gated `--api` engine) now **drops every real fixture type error** (25+ pinned TS2322/TS2345/TS2339 → actual 0) and **adds spurious stub errors** (TS2305 "no exported member" + TS2694 "namespace has no exported member" on generated `BaseButton.vue.ts` / `ComposableErrors.vue.ts` stubs). Generated public-API stub cross-file resolution is broken under the current engine wiring. | MODERATE (investigate stub/`--api` resolution; likely a regression, possibly `--api`-backend-perf fallout) | HIGH *if* `verter-tsc` is release-scoped (batch CLI type-checker emits a wrong diagnostic set); otherwise MEDIUM | MAYBE — in-progress verter-tsc `--api` backend perf work | **DEFER** with mandatory investigation before shipping `verter-tsc` |
| G9 | `verter_tsc checker::tests::run_declaration_phase_with_errors_still_postprocesses_emitted_files` | Windows-only test-harness bug. `write_mock_tsc_error_with_emit`'s `#[cfg(target_os="windows")]` branch (`crates/verter_tsc/src/checker.rs:2583`) writes `mock-tsc.ps1` with a literal, unsubstituted `__MOCK_LSP_HANDSHAKE_PS1__` placeholder → invalid PowerShell → the `--lsp --stdio` initialize handshake fails ("tsgo client closed") → declaration stage can't run. The Unix branch (line 2697) correctly does `.replace("__MOCK_LSP_HANDSHAKE_SH__", MOCK_LSP_HANDSHAKE_SH)`; the sibling mock at line 2496 correctly does the ps1 replace. Production code is fine. | QUICK (one line) | LOW (test-harness only) but reds the Windows gate | NO | **FIX-NOW** |

## Todo notes (concrete fixes)

### FIX-NOW

**G9 — tsc Windows mock (one line).** In `crates/verter_tsc/src/checker.rs`,
function `write_mock_tsc_error_with_emit`, the Windows branch `fs::write(&ps1,
r#"…"#)` (around line 2583-2635) must substitute the handshake the same way the
Unix branch and the sibling mock do. Change the raw-string write to
`r#"…"#.replace("__MOCK_LSP_HANDSHAKE_PS1__", MOCK_LSP_HANDSHAKE_PS1)` (matching
the pattern already at line 2496). Verify: the test passes on Windows; unchanged
on Unix.

**G5 — phase-archaeology needle (one line).** `packages/dx-harness/src/corpus-gate/receipt.ts:7`
has plan/phase vocabulary. Strip the phase/cutover/block wording, keep the
technical content. If the token is genuinely load-bearing (an asserted needle),
instead add the file to `PACKAGES_TS_ARCHAEOLOGY_ALLOWLIST` with a rationale.
Re-run `no_phase_archaeology_in_packages_ts_source`.

**G7 — routing characterization re-bless.** In
`crates/verter_session/tests/cases/g_misc0/language_routing_characterization.rs`,
update the `EXPECTED_IDE_CODE` constant to include the two new global-components
imports (the `left` value in the failure diff): add
`GlobalComponentType as ___VERTER___GlobalComponentType, GlobalComponentKebabType as ___VERTER___GlobalComponentKebabType`
to the `import type … from "@verter/types"` line and
`, globalComponentsNav as ___VERTER___globalComponentsNav` to the value import
line. No update-env-var exists for this test — it is a hand-edit. Only re-bless
once the G2 global-components codegen preamble is finalized (redo if it shifts).

**G3 — fs-boundary guards (one 4-file sweep, clears 3 reds).** For each of the 4
offenders, either route the I/O through `verter_workspace::WorkspaceAccess`
(preferred for the two production files `type_provider/project_sync.rs` and
`vue_assets.rs`) or add an allowlist entry with rationale. The test/fake files
(`real_provider_tests/global_components.rs`, `verter_tsgo_api/src/fake_engine.rs`)
are legitimate allowlist material — add them to
`crates/verter_workspace/tool-output-allowlist.toml` (Guard-1) and the
`D14_ALLOW_LIST` (D14). Guard-2 (`vfs_boundary_is_authoritative`) reads the same
set, so all three go green together. Confirm the two prod files are genuinely
tool-output/non-semantic before allowlisting rather than routing.

**G6 — extension Svelte grammar.** Decision required: does Verter ship a Svelte
TextMate grammar or rely on the user's Svelte extension (the test asserts the
latter)? If relying on the user's extension (current design), remove the
`source.svelte` grammar block from `packages/vue-vscode/package.json` (grammars
array, ~line 549-550) and re-run `client_framework_manifest_drives_extension_wiring`.
If shipping the grammar is intentional, the test assertion at
`client_framework_manifest_ts_freshness.rs:137` must be flipped instead — but
that contradicts the stated design, so confirm with the product owner first.

### DEFER

**G2 — global-components typed IDE hover/diagnostics (largest discrete effort).**
The feature must: resolve a globally-registered component tag (both Pascal and
kebab) to its typed binding in hover (not `any`, not the raw tag string) across
both tsgo and tsserver, and emit a fail-closed diagnostic for unknown/custom-element
tags. Currently the registration flows into codegen (the G7 preamble:
`GlobalComponentType`/`globalComponentsNav`) but the type does not resolve
through the providers. This is an active feature, not a regression — defer to the
global-components workstream and re-verify these 8 tests when it lands. Owner:
the global-components feature owner.

**G4 — oversize files.** Low-priority hygiene. Prefer splitting along sensible
boundaries over blanket exemptions. Note 5 of the 7 offenders are LSP document-sync
files (`background_drain.rs`, `documents/mod.rs`, `features/hover.rs`,
`server/nav_features.rs`, `server_utils.rs`) that the provider document-feeding
cutover rewrites or deletes — defer these until after that lands (splitting them
now would be thrown away). The 2 non-LSP offenders
(`verter_compiler/src/strip_types/typescript.rs`,
`verter_session/src/template_convert.rs`) can be split independently, or all 7
exempted via `guard6_exemptions()` if the sizes are accepted. Post-release.

**G8 — verter-tsc diagnostic-set parity (investigate; not a re-pin).** The drift
is a broken run, not a schema change: the batch CLI drops the fixtures' real
assignability/type errors and surfaces module/namespace-resolution errors
(TS2305/TS2694) on the generated `.vue.ts` public-API stubs — the stub carriers
no longer resolve their cross-file imports under the gated `--api` engine.
Investigate whether this is (a) stub-generation regression, (b) `--api`-backend
wiring drift from the in-progress verter-tsc perf work, or (c) a gated-engine
version behavior change. Do NOT re-pin `EXPECTED` to the broken set. If
`verter-tsc` ships in this release, this is release-blocking (HIGH) and needs
fixing now; if `verter-tsc` is out of release scope, defer with a debt row and
re-verify after the `--api` backend perf work lands. Owner: verter-tsc/host-mode-perf.

### VERIFY-AFTER-FEEDING

**G1 — provider-returns-empty family (21).** Do not fix individually yet. These
are all "the provider committed nothing" on small hermetic fixtures — the
document-sync / project-membership / secondary-(imported)-file-warm /
carrier-diagnostic-flow domain that the provider document-feeding cutover
(`provider-document-feeding-architecture.md`) and the external-TS project-binding
work reorganize (single reconciler as sole wire-writer, P1.5 import-neighbor
warm, truthful per-document convergence, MembershipReconciler fold-in, carrier
API sync). Re-run the full set after Landing 1 and again after Landing 2. Then
triage only the residue. Confidence MODERATE, not certain — likely-residual
items to check individually even after feeding lands:
- The `ResilientProvider::register_carrier_member` no-op (external_ts resilient
  test) is a named, contained bug: the production wrapper the LSP binary installs
  swallows carrier registration so it never reaches the inner tsserver provider.
  If the feeding reconciler rework does not subsume it, it is an independent
  ~MODERATE fix (override the wrapper method to forward to the inner provider).
- `rename_kebab_prop_usage_spans_script_and_template_tsserver` asserts cross-file
  find-references completeness (parent template usage currently missing) — a
  references-breadth concern that may need its own check even if warm/commit is
  fixed.
