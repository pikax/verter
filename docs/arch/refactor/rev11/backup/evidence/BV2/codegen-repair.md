# BV2 — VDOM/SSR root-prefix duplicate-ownership repair (owned-scope items 1–8)

Scope: VDOM root-prefix duplicate-ownership repair (item 1), the SSR
comment-only sibling (item 2), and their regression/conformance coverage
(items 3–8). Items 9–10 (declaration-output fidelity, framework-surface
memberless-runtime-macro gap) are a separate implementer's scope and are not
touched here.

## Root-cause confirmation (reproduced in this tree)

Wrote the regression suite FIRST
(`crates/verter_compiler/tests/cases/vdom_ssr_root_prefix_comment_absorption.rs`,
registered in `tests/cases/mod.rs`) and confirmed it failed against
unmodified `HEAD` before any source change:

- 7 VDOM matrix cells panicked at `template/code_gen/types.rs:712:17` —
  `overwrite_segmented precondition violated at [N,M): ReplacedContentSplit`
  — the exact reported panic, across static/dynamic/static+dynamic root
  class, short/long/whitespace-only comment shape, interpolation/static-text/
  directive-free template bodies, a style block present, and both a plain
  `<script>` (Options API) and `<script setup>` with `inline: Some(false)`
  pinned.
- 1 SSR cell (`ssr_only_disabled_comment_zero_effective_roots` — a template
  whose only content is a disabled leading comment) panicked at the same
  `types.rs:712:17` site.
- All other cells (VDOM dev build, VDOM interior/trailing comments, SSR
  nonempty-root, SSR interior/trailing, SSR dev, Vapor) passed unmodified —
  matching the ruling's root-cause scoping exactly.

Full pre-fix run: `/tmp/bv2-pretest2.txt` (9 failed / 4 passed on the first
pass before two of my own test assertions were corrected — see below — then
7 failed / 6 passed on the corrected assertions, all failures at the single
`types.rs:712:17` site).

Two of my own test assertions needed correction before they were trustworthy
negative controls (both false positives from the blanket `!code.contains("<!--")`
check, not real bugs):
- SSR legitimately emits `<!--[-->`/`<!--]-->` fragment markers when a
  (disabled) comment sits beside a single root — mirrors VDOM's
  `DEV_ROOT_FRAGMENT` logic. Fixed by asserting the absence of the authored
  comment TEXT (`"lead"`, `"only comment"`) instead of a blanket `<!--` ban.
- Vapor with `inline: None` hit `VaporInlineNotYetImplemented` (an unrelated
  request-construction refusal, not the bug under test) — fixed by pinning
  `inline: Some(false)` explicitly, matching every other matrix cell.

## VDOM repair (owned-scope item 1)

`crates/verter_compiler/src/template/code_gen/vdom/comment.rs`:
`process_comment`'s disabled-comment branch no longer calls
`out.overwrite(...)`. It returns a new `CommentOutcome` (`Kept` |
`Dropped { start, end }`) — the disabled case returns the span as a FACT,
no overwrite queued. `Kept` carries no payload: nothing in production ever
read the old `Option<ChildRecord>` return value (the caller always
discarded it via `let _ = ...`), and `build_child_records` independently
walks the AST to build the same `ChildRecord`s — carrying an unread
`ChildRecord` through `Kept` would have been dead weight (confirmed by a
`cargo clippy -D warnings` "field `0` is never read" failure when first
tried with the payload).

`crates/verter_compiler/src/template/code_gen/vdom/mod.rs`:
- New `VdomCodeGen::pending_disabled_comment_removals: Vec<(u32, u32)>`.
- `visit_comment` matches the outcome: `Kept` → no-op; `Dropped { start, end }`
  → pushed onto the pending vec (was previously an immediate
  `out.overwrite(...)`).
- New `absorb_pending_comment_removals(&mut self, start, end)`: retains only
  entries NOT wholly contained by `[start, end)`.
- `leave_template` calls `absorb_pending_comment_removals` with the EXACT
  same `(start, end)` pair immediately before EVERY root-prefix/suffix claim
  — the `effective_count == 0` empty-template branch, all four `1`-arm
  sub-branches (v-if, v-memo, directives-wrap, plain block-root — both the
  segmented and non-segmented prefix write), and the multi-root Fragment
  branch's prefix and suffix. This is unconditional across every branch, not
  just the reported segmented one, per the charter's requirement that the
  root-prefix owner subsume contained comments under its ordinary unmapped
  prefix replacements too.
- After the `match effective_count { ... }` block, any
  `pending_disabled_comment_removals` entries NOT absorbed (interior,
  trailing, or otherwise outside every claimed range) are drained into
  ordinary `out.overwrite(start, end, "")` calls — reproducing today's
  plain-deletion behavior for those comments. `overwrites` sorts by start
  before flushing (`CodeGenOutput::apply_to`), so pushing these last is safe.

Structural confinement: the disabled-comment mutation branch is DELETED from
`process_comment` — reading the function body shows no code path in it can
emit an overwrite for the disabled case; it can only return a fact. No
runtime flag, no bypass.

## SSR sibling (owned-scope item 2)

Investigation confirmed the exact shape the charter describes:
`count_effective_roots` (`ssr/mod.rs`) excludes comments; `visit_comment`'s
disabled branch called `out.overwrite_segmented(comment.start, comment.end, ...)`
directly (not a plain `overwrite` — SSR's disabled-comment path was ALREADY
in the strict `segmented_overwrites` channel); and `leave_template`'s
`effective_count == 0` branch claims `overwrite_segmented(root.tag_open.start,
close_end, ...)` — a range that structurally contains a comment-only
template's own comment-deletion entry, both in the SAME channel. Sorted by
start, the wider empty-template claim applies first and converts the whole
range to one `Overwritten` chunk; the narrower comment deletion then hits
`try_overwrite_segmented`'s single-`Original`-chunk precondition and panics
— confirmed exactly via `ssr_only_disabled_comment_zero_effective_roots`.

The nonempty-root branch does NOT collide: it claims only
`[tag_open.start, tag_open.end)` (the bare `<template>` opening tag bytes)
and `[close_start, close_end)` separately — a leading comment sits AFTER
`tag_open.end`, structurally outside that narrow prefix claim. This matches
the charter's statement that SSR does not reproduce the reported collision
for a nonempty root.

Fix (backend-local, SSR's own field/method — no shared state with VDOM, per
"Two Template Codegen Paths"): same principle as VDOM.
`SsrCodeGen::pending_disabled_comment_removals` + its own
`absorb_pending_comment_removals`. `visit_comment`'s disabled branch now
records the pending removal instead of calling `overwrite_segmented`
directly. `leave_template` absorbs at all three claim sites (the
zero-effective-root whole-template claim, and both halves of the
nonempty-root claim, for symmetry/defense even though the latter two are
structurally never contained), then drains any leftover into a plain
`out.overwrite(start, end, "")` after the `if/else`.

No other `overwrite_segmented` call site in SSR was touched — the charter's
owned scope names exactly these three participants
(`count_effective_roots`, `visit_comment`'s disabled branch, and
`leave_template`'s zero-effective-root claim); a broader audit of every
`overwrite_segmented` call in `ssr/mod.rs` found no other collision shape
against a disabled comment (every other segmented claim there operates on
element/interpolation/attribute-local ranges that cannot contain a root-level
comment span).

## `is_interstitial_condition_node`'s comment-removal branch (VDOM) — left as-is

`visit_comment`'s early-return branch for v-if/v-else-chain interstitial
comments (`vdom/mod.rs`, just above the `process_comment` call) still calls
`out.overwrite(comment_node.start, comment_node.end, "")` directly,
unconditional on `comments_enabled`. This is a distinct, pre-existing
mechanism (regression-tested by
`comment_between_v_if_branches_does_not_leak_in_prod` in
`src/compile_tests.rs`) that removes comments BETWEEN v-if/v-else-if/v-else
branch elements — never at the template-root leading/trailing position the
duplicate-ownership conflict occurs at. `leave_template`'s root-prefix/suffix
claims for the `is_v_if` branch cover `[tag_open.start, child.start)` and
`[close_start, close_end)`, where `child` is the FIRST branch element itself
— an interstitial comment (between two branch elements, not before the
first) is never inside either claimed range, so it cannot participate in
this duplicate-ownership shape. Left unmodified per the charter's guidance;
no changes needed to defend it.

## Test coverage added

`crates/verter_compiler/tests/cases/vdom_ssr_root_prefix_comment_absorption.rs`
(13 tests, direct-route `StandaloneCompiler` invocation — satisfies owned-scope
item 7): headline VDOM reproduction (Options API + `<script setup>` non-inline),
VDOM dev negative control, VDOM interior/trailing comments, VDOM root-class
matrix (static/none/dynamic/static+dynamic), VDOM comment-shape matrix
(short/long/whitespace-only), VDOM template-body matrix
(interpolation/static-text/directive-free), VDOM + style block, Vapor negative
control, SSR headline + zero-effective-root + interior/trailing + dev negative
control.

`crates/verter_compiler/src/template/code_gen/vdom/comment.rs` unit tests
updated for the new `CommentOutcome` return type (13 tests, all green);
`comments_disabled_returns_dropped_span_with_no_overwrite` replaces
`comments_disabled_returns_none` and asserts zero overwrites are queued by
`process_comment` itself for the disabled case (discriminating: this test
would fail against the pre-fix `process_comment`, which queued one).

## Verification run

- `cargo test --package verter_compiler --lib` — 6212 passed, 0 failed
  (3 pre-existing environment-only failures in `framework_common::vue_bridge`
  requiring a `pnpm install`-produced `node_modules/typescript/lib/tsc.js`
  this worktree never had — unrelated to this change, confirmed by `ls
  node_modules` returning "No such file or directory"), 5 ignored (pre-existing).
- `cargo test --package verter_compiler --test main` — 554 passed, 0 failed,
  2 ignored (pre-existing).
- `cargo test --package verter_vue_conformance` — 8 passed, 0 failed (locked
  Vue `3.6.0-rc.3` hermetic conformance pack, including
  `vue_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`
  and the committed-corpus/manifest/lockfile freshness guards).
- `cargo clippy --package verter_compiler --lib --tests -- -D warnings` —
  clean.
- `cargo fmt --package verter_compiler -- --check` — clean.

Not run here (deferred to the orchestrator's single combined-tree gate per
the implementer brief): `node scripts/gate.mjs`, the `pikax/vue-benchmarks`
secondary-evidence run (owned-scope item 8) — both require environment/tooling
this worktree does not have installed (`node_modules`, the benchmark repo).

## Ambiguity / interpretation notes

- SSR's `leave_template` nonempty-root absorb calls (open-tag and close-tag
  claims) are provably never contained-in cases for a disabled comment given
  current AST shapes; they were added anyway for defensive symmetry with the
  VDOM repair's "unconditional across every branch" requirement, at
  negligible cost (a `Vec::retain` over an almost-always-empty vec).

## Coverage completion (fix round 2)

An independent three-mandate review (conformance/architecture/adversarial)
found the codegen repair itself sound (architecture + adversarial: no
defects) but the conformance review (finding B1, plus owned-scope items
3/4/6/7) found the 13-test acceptance matrix a sparse sample rather than a
full cross of the charter's axes. This round closes those specific gaps by
adding 15 tests to
`crates/verter_compiler/tests/cases/vdom_ssr_root_prefix_comment_absorption.rs`
(13 → 28 tests total), with no change to the repair itself
(`vdom/comment.rs`, `vdom/mod.rs`, `ssr/mod.rs` are untouched this round).

### 1. Native `compile_bundle` invocation proof (owned-scope item 7 / B1's "native `compileMany`" gap)

`compileMany` is NAPI-only (`crates/verter_napi/src/lib.rs`, out of this
fix round's file scope, and this worktree has no `node_modules`/native
build to exercise it through Node regardless). Traced its call chain
instead: `NapiVerterHost::compile_many` → `VerterHost::compile_many`
(`verter_session::host_compile`, `CompileManyTarget::RuntimeRender` lane) →
`render_only_main` → `compile_entry_runtime_render` →
`compiler.compile_bundle(...)`, where `compiler` is the registry-resolved
`CarrierCompiler` impl (`VueCarrierCompiler` for `.vue`). This is a
genuinely SEPARATE production entry point from `StandaloneCompiler`'s
`CompileRequest::new` route — confirmed by the in-tree doc comment on
`compile_bundle_refuses_explicit_ssr_and_force_vapor`
(`framework_common/vue_bridge.rs`): "This trait method is a SEPARATE
production entry into the shared codegen substrate from
`CompileRequest::new` (the session's per-file compile path routes here
without constructing a `CompileRequest` first)". `StandaloneCompiler`
routes through `compile::compile` → (traced further) `compile_from_parsed`;
`compile_bundle`'s Vue impl routes through `compile_from_parsed_legacy` — a
different top-level function, so the direct-route proof this file already
had did NOT cover this path.

Added three tests driving `CarrierCompiler::compile_bundle` directly
(`native_route_compile_bundle` helper, using the same
`RegisteredSourceAuthority`/`CarrierGrammarAuthority` artifact-construction
pattern the crate's own internal `#[cfg(test)]`-only `artifact_for` helpers
use, reimplemented locally since those are not visible outside the crate's
own unit-test build): `native_route_vdom_leading_comment_static_class_root_production`,
`native_route_ssr_leading_comment_static_class_root_production`,
`native_route_ssr_only_disabled_comment_zero_effective_roots`. The two VDOM/
zero-root-SSR cells panicked against the pre-fix tree (confirmed by
reverting `vdom/comment.rs` + `vdom/mod.rs` + `ssr/mod.rs` to `53d6c3157`
and rerunning — see "TDD verification" below) and pass post-fix, proving
the repair independently of `StandaloneCompiler`.

### 2. Source maps on/off (B1)

Added `compile_client_with_map`/`compile_server_with_map` helpers
(`RuntimeProductRequest.runtime_source_map: true`) plus a standalone
`assert_source_map_token_resolves` decoder (`oxc_sourcemap::OwnedSourceMap`
— the same crate the compiler's own `oxc_sourcemap` re-export and the
`compose_template_virtual_file_tests` module in
`verter_session::host_resolve::virtual_file_pipeline` use for map
decoding; the crate's own `framework_common::sourcemap_e2e_helpers` module
is `#[cfg(test)]`-internal and not reachable from an integration test, so
this is a small standalone reimplementation of its lookup-and-compare
pattern, not a new invented decoder).

- `vdom_leading_comment_source_map_on_resolves_root_tag`: decodes the map,
  finds the first `"div"` occurrence in the generated VDOM code, and asserts
  it resolves to a token whose source position's line contains `"div"` —
  panicked pre-fix (map generation reaches the same `leave_template`/
  `overwrite_segmented` panic site), passes post-fix.
- `vdom_leading_comment_source_map_off_emits_no_map`: negative control,
  `compile_client` (used throughout this file) never requests a map.
- `ssr_leading_comment_source_map_on_resolves_class_attr`: SSR maps the
  `class` ATTRIBUTE NAME token (not every literal inside the merged
  `_mergeProps` object — confirmed by dumping the raw token table before
  picking this target: the `"root"` VALUE string sits in an unmapped gap
  between two mapped anchors), so this asserts against `"class"` instead of
  `"div"`/`"root"`.
- `ssr_leading_comment_source_map_off_emits_no_map`: negative control,
  asserts the produced template's `source_map` field is empty.

### 3. Deeper axis crossing (B1's acceptance-matrix table)

- **Root-level trailing comment** (previously only comment-inside-root was
  tested): `vdom_root_level_trailing_disabled_comment` +
  `ssr_root_level_trailing_disabled_comment` — a disabled comment AFTER the
  single root element closes, sitting in the root's SUFFIX claim range.
- **Comment-shape × root-class × build-mode**:
  `vdom_comment_shape_x_root_class_x_build_mode_matrix` crosses
  short/long/whitespace-only comments against static/dynamic root class
  AND production/development build mode (dev is a per-cell negative control
  proving the shape axis doesn't interact with the dev/prod axis either).
- **Style × SSR**: `ssr_leading_comment_with_style_block` (previously VDOM-only).
- **Script kind × SSR**: `ssr_leading_comment_static_class_root_production_script_setup`
  (previously the `<script setup>` non-inline cell was VDOM-only).
- **No-comment negative controls × build mode × backend**:
  `no_comment_negative_controls_across_backend_and_build_mode` crosses a
  template with NO comment at all against VDOM/SSR/Vapor × production/
  development, proving the absorption machinery is a pure no-op when there
  is nothing to absorb.

### 4. Runtime-link / behavior assertion (B1)

Reused this crate's own established pattern for VDOM/SSR codegen tests
(`template/code_gen/vdom/tests.rs`, `template/code_gen/ssr/tests.rs`):
`code.contains("<exact call-site string>")` against real compiled output,
not merely a parse check. A real Node/Vue execution harness exists
(`packages/framework-conformance-harness`, with one existing Rust caller in
`verter_session::compile::map_equality_tests::nested_v_for_runtime_proof`)
but requires `node_modules` this worktree does not have (`pnpm install` was
never run here) and lives in a package outside this fix round's file scope
to add a second caller to; the `code.contains(...)` pattern is both the
crate's dominant existing convention for this exact test class and fully
verifiable in this environment.

- `vdom_headline_shape_links_against_intended_runtime_helpers`: asserts the
  static-class object is hoisted verbatim
  (`const _hoisted_1 = { class: "root" }`), the non-inline render signature
  is intact, and the root element links against the real
  `_openBlock()`/`_createElementBlock("div", _hoisted_1, "hi")` runtime
  helpers.
- `ssr_headline_shape_links_against_intended_runtime_helpers`: asserts the
  `ssrRender` signature is intact and the root element links against
  `_push(` / `_ssrRenderAttrs(_mergeProps({ class: "root" }, _attrs))`.

### 5. Existing accepted-pack re-confirmation (owned-scope item 6)

Re-ran fresh (post this round's additions) rather than assuming unaffected:

```text
cargo test --package verter_compiler --lib template::code_gen --jobs 4 -- --test-threads=4
977 passed; 0 failed (unchanged from the round-1 evidence run)

cargo test --package verter_compiler --test main --jobs 4 -- --test-threads=4
569 passed; 0 failed; 2 ignored (was 554; +15 = 569, matching the 13→28 test-file growth)

cargo test --package verter_vue_conformance --jobs 4 -- --test-threads=4
(locked Vue 3.6.0-rc.3 hermetic conformance pack — see run log)
```

These ARE the repository's "comment/class/hoisting/mapping suites" for this
block: there is no separately named `BV0`/`BV1` test file — `comment.rs`
(comment handling), `element.rs`/`element_tests.rs`/`props.rs` (class
handling), `mod.rs`'s hoist-reservation tests + `element_tests.rs` (static
hoisting), and `sourcemap_e2e_tests.rs`/this file's new map tests (mapping)
are all under the `template::code_gen` lib-test module re-run above.

### TDD verification for every new test

Confirmed discriminating power by reverting `vdom/comment.rs`, `vdom/mod.rs`,
and `ssr/mod.rs` to their pre-fix content (`git show 4bfb9b15d^:<path>`,
i.e. `53d6c3157`) and rerunning the full 28-test file:

- 13 failed at the exact `types.rs:712:17` `overwrite_segmented`
  precondition panic — the ORIGINAL 13 tests plus this round's
  `native_route_vdom_leading_comment_static_class_root_production`,
  `native_route_ssr_only_disabled_comment_zero_effective_roots`,
  `vdom_headline_shape_links_against_intended_runtime_helpers`,
  `vdom_leading_comment_source_map_on_resolves_root_tag`,
  `vdom_leading_comment_source_map_off_emits_no_map` (map generation itself
  reaches the panic site), and `vdom_comment_shape_x_root_class_x_build_mode_matrix`
  (its static-class production cells) — confirming these are genuine
  regression tests, not coverage theater.
- 15 passed pre-fix, including `native_route_ssr_leading_comment_static_class_root_production`
  (SSR's nonempty-root case never reproduced the collision — matches the
  original evidence), and the new root-level-trailing-comment and
  no-comment-negative-control tests (legitimate axis coverage that was
  never expected to discriminate against this specific bug — they prove the
  new/never-tested cells behave correctly, not that the bug is absent from
  them).
- Restored the three files exactly (`git diff --stat` empty against HEAD
  afterward) and reran the full file: 28/28 pass.

### Not closed this round

- **Owned-scope item 8** (`pikax/vue-benchmarks` confirmation): explicitly
  non-gating per the charter's own wording (N1 in the conformance review);
  still not run — no benchmark repo/tooling in this environment.
- **Items 9/10** (declaration fidelity, framework-surface runtime-macro
  gap) and the phase-archaeology comment cleanup (B4) are a separate fix
  round's scope (`verter_napi`, `verter_session`) — out of this file scope.
- Real Node/Vue runtime execution (vs. the `code.contains(...)`
  runtime-link pattern used here) — see §4 above for why it wasn't added
  this round.
