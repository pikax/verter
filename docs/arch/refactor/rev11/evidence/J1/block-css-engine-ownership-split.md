# Block `block/css-engine-ownership` — proposed split

J1 (`docs/arch/refactor/rev11/charters/J1.md`) is a single ratified charter but its own row/acceptance-ID
structure (§1.1's inventory, §2.1's 24 acceptance IDs) does not fit one landing unit — it spans a Vue
transform-parity rewrite, a full lightningcss removal + NAPI/unplugin cutover, and an independent
byte-exact Svelte grammar convergence, each gated on its own strict evidence. Attempting all of it as one
change risks exactly what the charter's §7 forbids: a partial removal that leaves lightningcss
half-wired. This block executes Slice 1 below and proposes the remaining slices as separate blocks.

## Slice 1 (this block) — `style_planner` absorbs legacy CSS capability
Row 2's required work (J1 §4): `style_planner.rs` reaches full parity with the legacy `css/` module's
Native capabilities plus fixes six confirmed defects (A10d-h), the pipeline-ordering gap (A10a/A10b),
the re-parse-on-unchanged-stage defect (A10i), and directive coverage (A10). Additive only — the legacy
`css/` module and the `lightningcss` dependency are untouched and remain the sole live NAPI-facing route;
no consumer switches yet, so no dual-path is created. Also captures the perf baseline (§2 Bounds,
"Latency") that gates Slice 2's deletion.

Acceptance IDs covered: A10, A10a, A10b, A10d, A10e, A10f, A10g, A10h, A10i, plus the perf-baseline
capture step.

Acceptance IDs explicitly deferred out of Slice 1: A10c (belongs to `verter_css_syntax`), A3, A9, A12,
A13, A14, the allocator-canary bound.

## Slice 2 (separate block) — the cutover
Rows 3/4/14/15/16/17 together, in one change (per §7, no half-wiring): delete
`crates/verter_compiler/src/css/`, remove the `lightningcss` dependency, replace the NAPI
`process_style` wire contract with the three-way `analyze_style`/`prepare_style_for_preprocessor`/
`transform_vue_style` API (A5), replace `extract_css_class_names` with
`StyleSyntaxIr::complete_static_classes()` (A8/row15), and switch both `packages/unplugin` bundler paths
off `compileStyleAsync()`/legacy `processStyle` (A6/A7). Gated on Slice 1 landing (style_planner must
already have full parity) and on the perf-baseline comparison (§2 Bounds).

## Slice 3 (separate block) — Svelte CSS grammar convergence
Row 5 (A11a-f): delete `svelte/runtime/css/parse.rs` and the grammar-owning portions of `types.rs`,
re-home `analyze.rs`/`match.rs`/`hash.rs` as policy-only consumers of `StyleSyntaxIr`, replace
`render.rs`'s printing with `CodeTransform` edits, retire the `validate_svelte_style_ir` double-parse.
Independent of Slices 1/2 modulo `StyleSyntaxIr` extensions it needs; carries its own mandatory
byte-exact `svelte@5.56.3` parity gate and its own stop/escalate trigger (§4) if that parity can't be
met — large and risky enough to warrant its own block rather than folding into Slice 2.

## Slice 4 (separate block, lower risk, can interleave) — closing items
A3 (Sass/Stylus dialect-coverage parity in `verter_css_syntax`), A9 (capability-matrix ratification,
40-row closure), A12 (CodeTransform mapping-coverage test), A13/A14 (preprocessor boundary — `row 18`,
extending `BlockOverrideEntry`), the allocator-canary bound. None of these gate Slices 1-3 landing;
they can run in parallel with them once the requisite crates stabilize.

Status: Slice 1 in progress in this worktree/branch. Slices 2-4 not started.
