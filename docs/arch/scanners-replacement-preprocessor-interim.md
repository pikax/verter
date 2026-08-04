# Scanners replacement — preprocessor override interim contract

Interim state of the legacy unplugin preprocessor override lane (the
`applyBlockOverrides` / `BlockPreprocessor` surface) between the scanners
replacement train and the deferred P-C1 typed block-content cutover.
Authority: T-B train-end adjudication challenge, Q1 replacement ruling.

## The torn-lane contract (deliberate, not a regression)

"Legacy callbacks remain live" means the callback SURFACE is not deleted
mid-deferral — it does **not** mean every block kind recompiles successfully.
The three arms, each pinned by a discriminating test in
`crates/verter_session/src/lib_tests.rs`:

| Arm | Behavior | Pinning test |
| --- | --- | --- |
| Template/script CONTENT overrides | Typed refusal: `HostError::ExternalBlockContentDeferred` with acceptance `B-23`. Never a success; synthetic-carrier splice/reparse is unrepresentable. | `torn_lane_template_and_script_overrides_refuse_typed_b23` |
| Style overrides (and custom blocks routed through style analysis) | LIVE success WITHOUT carrier reparse: compiled CSS layers into the per-profile compile slot; the scheduler source snapshot is untouched. | `torn_lane_style_override_lives_without_carrier_reparse` |
| JS `BlockPreprocessor` / `applyBlockOverrides` SURFACE | Remains reachable (NAPI, WASM, unplugin) until P-C1/B-83; routes style→live, content→typed B-23. | `torn_lane_block_override_surface_routes_style_live_and_content_typed` (host boundary); `packages/unplugin/src/core/compiler.spec.ts` (JS surface presence) |

"Legacy green" therefore must NOT be read as "all override kinds succeed":
only the style/custom arm succeeds; content-bearing template/script arms are
fail-closed B-23 until B-23-GREEN + B-83 deletion land as one later atomic
unit (P-C1). No dual real/deferred content path may be introduced
mid-deferral.

## B-26 — raw style geometry removed from the retained style path

`apply_style_overrides` no longer performs a raw `</style` delimiter search
(`rest.find("</style")`, formerly `host_upsert.rs`, ledger symbol
`manual_style_close_search`). Source-map remapping now takes the SELECTED
style block's `content_span` from the registered inventory
(`RegisteredFileStructure` → `CarrierBlockInventory`), projected by style
ordinal — the same ordinal domain the style analyses are built from. If the
inventory has no entry for an ordinal the remap fails closed (empty original
content, identity remap), never a source scan.

Evidence (mutation/regression):
`lib_tests::style_override_remap_uses_selected_inventory_block_span_not_style_close_scan`
— a style body containing the literal `</styleguide </sty` where the parser's
raw-text rule keeps the block open but a raw scan mis-splits; reverting to a
scan (or perturbing the inventory-span selection) turns the test RED. The
corresponding B-26 ledger row was removed because the capability no longer
exists in the tree.

## B-44 — partial close; storage + surface explicitly deferred to P-C1

Closed now (deleted, zero production callers):
`crates/verter_session/src/host_upsert/block_splice.rs`
(`build_synthetic_source` and its splice/lang-strip helpers). Its ledger row
was removed with it.

NOT closed — explicitly deferred to P-C1: the synthetic content storage
(`ContentOverrideWithParse`, the `content_overrides` compile-cache layer) and
the JS/NAPI/WASM callback surface (`applyBlockOverrides`,
`BlockPreprocessor`, `customBlocks`). Their ledger rows remain open with
disposition `delete`.

## B-83 — not claimed

The callback-surface deletion (B-83) is NOT closed and must not be claimed
until the P-C1 atomic unit. All B-83 ledger rows remain open.

## Ledger note

The two removed rows adjust `statistics`/`set_equality`/`consumer_matrix`
totals (38 → 36) for internal consistency only; a fresh independent
re-discovery of the candidate universe from the fixed tip is owned by the
residual-scanner unit (FL2-B) and supersedes these counts.
