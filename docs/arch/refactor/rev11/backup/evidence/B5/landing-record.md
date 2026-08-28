# B5 — implementation record

Base `264f727cb`. Branch `block/b5` (initial implementation `a9d1547f2` + fix
round 1 (`931e03afd`) + fix round 2, see §Fix round 1 and §Fix round 2 below
— final HEAD SHA is reported in the fix round 2 completion report, not
inlined here to avoid a self-referential hash). Not landed — this branch
does not get merged/pushed by the implementer; a separate landing agent
verifies and fast-forwards.

**This record was revised in place after an independent review of
`a9d1547f2` found 4 blocking defects.** Sections below that described the
pre-fix state (`StandaloneCompiler::compile`'s return type, `compile_client`'s
visibility, the `compile`/`compile_with_parsed` visibility claim) are
corrected to the current, verified state; §Fix round 1 records what changed
and why. Where a pre-fix claim was simply wrong (not superseded, WRONG), it
is marked as corrected rather than silently rewritten.

## What shipped

### 1. One direct compiler core (`crates/verter_compiler/src/standalone.rs`)

`StandaloneCompiler::compile(&self, source: &'a str, request: &CompileRequest,
inputs: DirectExecutionInputs<'a>) -> Result<DirectCompileOutput, DirectCompileError>`
(`standalone.rs:144`, return type revised in fix round 1 — see §Fix round 1
item 2) is the crate's sole raw-source-in/atomic-publish-out entry.
`DirectCompileOutput { artifacts: ArtifactSet, styles: Vec<RuntimeStyleBlock>,
diagnostics: Vec<CompileDiagnostic> }` is the one-shot core's own return
envelope — `artifacts` is still exactly the atomic `publish()` result;
`styles`/`diagnostics` are host-side siblings in every registered route for
both frameworks that B4's sealed `ProductKind`/`ArtifactContribution`/
`publish()` model has no carrier for (see §Fix round 1 item 2 for why this
doesn't reopen the B4 contract). `DirectExecutionInputs` (`standalone.rs:69`)
is the framework-tagged borrowed carrier (`Vue{execution, macros}` /
`Svelte{execution}`); a framework/input-tag mismatch is a typed
`DirectCompileError::FrameworkMismatch` (`standalone.rs:85`, constructed at
`:158`/`:164`), never a panic. `SvelteExecutionInputs` (`standalone.rs:59`) is
new — one field, `css_hash_override: Option<String>`, mirroring
`VueExecutionInputs`'s "resolved fact, not a semantic option" role.

Dispatch (`compile_vue`, `standalone.rs:172`; `compile_svelte`,
`standalone.rs:409`) builds `ProductPlan::from_request` once, compiles only
requested products, and calls `publish()` exactly once over the full
contribution set — including BOTH halves of a dual `RuntimeClient`+
`RuntimeServer` request (fix round 1 item 1). Verified by
`vue_multi_product_request_publishes_both_atomically` (`standalone.rs`) and
the new `vue_dual_runtime_client_and_server_request_publishes_both_atomically`.

Per-product wiring:
- `IdeCompanion`/`Declarations` (Vue): read directly off the `VerterCompileResult`
  `compile_with_parsed` already produced — no second compile.
- `RuntimeClient`/`RuntimeServer` (Vue): converted to the framework-neutral
  `RuntimeCompileOutput` via the existing `vue_result_to_runtime_bundle`
  (`framework_common/vue_bridge.rs:1381`), then composed through the SAME
  `verter_compiler::assembly::vue_module::compose_fragments` the host route
  uses. Script/template maps are decoded via the TRUSTED same-crate
  `oxc_sourcemap::SourceMap::from_json_string` (the same regime
  `assembly::compose::assemble_sequence` already uses for fragment maps),
  not the host's hardened multi-fragment validator — this compile's own maps
  were produced a moment earlier in the same call, not received cross-tool.
  A request planning BOTH `RuntimeClient` and `RuntimeServer` together runs
  `compile_with_parsed` TWICE (`compile_inner` produces exactly one `ssr` mode
  per call — a genuine architectural constraint shared with the host route,
  not something this block invented): once for the primary request (whose
  `ssr` flag is `derive_legacy_vue_options`'s existing `ANY RuntimeServer
  present` derivation), once more for the other kind via
  `single_runtime_product_request`, a narrowed single-product sub-request.
  Style content is collected once, from the primary bundle only (style
  output does not vary with `ssr`).
- `RuntimeClient`/`RuntimeServer` (Svelte, NEW capability): `compile_client`
  (unchanged) produces a `ClientModule` now carrying real `declared_imports`
  (item 4 below); one `ValidatedFragment` represents the whole ESM, published
  with `emitted_imports` equal to the SAME list — never `fragments: vec![]` /
  `emitted_imports: vec![]`. A dual `RuntimeClient`+`RuntimeServer` Svelte
  request loops `compile_client` once per kind (server first, so an SSR
  refusal fails fast before the client half is ever compiled) and publishes
  both as separate contributions when both succeed; today SSR still fails
  closed unconditionally (`svelte/runtime/client_compile.rs`'s existing `ssr`
  gate, untouched), so in practice a dual Svelte request currently always
  fails closed with no partial output — proven by
  `svelte_dual_runtime_client_and_server_request_fails_closed_with_no_partial_output`.
  Non-dual SSR requests reach `compile_client(ssr: true)` unchanged, proven by
  `svelte_runtime_server_request_fails_closed_not_reinterpreted`.
- Any other planned product (`PublicApi`, `Analysis`, Svelte `IdeCompanion`)
  is a typed `DirectCompileError::UnsupportedProduct` returned BEFORE
  `publish()` runs — never a partial `ArtifactSet`.

**Style/CSS output** (fix round 1 item 2): both frameworks' style content is
now surfaced on `DirectCompileOutput.styles: Vec<RuntimeStyleBlock>` — Vue
from `RuntimeCompileOutput.styles` (already populated pre-fix, simply never
read by the direct core), Svelte from `ClientModule.css` converted via the
exact same `RuntimeOutputDescriptor::carrier_source`/`::generated` pattern
`svelte/carrier.rs`'s production host route already uses for this exact
conversion. A style-less component publishes an empty `Vec`. Proven by
`vue_styled_component_publishes_non_empty_styles` /
`svelte_styled_component_publishes_non_empty_styles` (non-empty) and the
existing style-less tests (extended with an `output.styles.is_empty()`
assertion).

**Svelte `Foreign` namespace** (fix round 1 item 2): `direct_svelte_runtime_options`
now threads `SvelteNamespaceRequest::{Html,Svg,MathMl}` to their compiler-internal
`SvelteNamespace` counterparts and returns the new
`DirectCompileError::UnsupportedSvelteNamespace` for `Foreign` — the
compiler-internal `SvelteNamespace` enum has no `Foreign` variant, and tracing
`SvelteOptionAttempt::into_request()`'s full construction chain confirmed
there is no established precedent anywhere (including the production host
route, which refuses unrecognized namespace strings before a `Foreign`
request value can even be constructed) for what `Foreign` should silently
resolve to — refusing is the only choice that doesn't invent behavior. Proven
by `svelte_foreign_namespace_is_refused_not_silently_defaulted`.

**`custom_element_descriptor` non-consumption (documented, not fixed — a
verified pre-existing gap)**: `SvelteCompileRequest.custom_element_descriptor`
is not consumed anywhere in `svelte/carrier.rs` (grepped: zero references),
and `resolve_custom_element` (`svelte/runtime/custom_element.rs`) takes only a
`custom_element_option: bool`, never a descriptor, when no inline
`<svelte:options customElement>` exists. This is identical in BOTH the
production host route and this direct core — B5's charter is "expose the
already-accepted algorithms," and there is no already-accepted algorithm here
to expose. Disposition: **DEFER** — not a B5 regression, needs a
`SvelteCustomElementDescriptor`-consuming implementation in the shared
`resolve_custom_element` path (a change to shared Svelte runtime semantics,
outside this block's no-framework-semantic-repair charter) before either
route can honor it. Documented at `standalone.rs`'s
`direct_svelte_runtime_options` doc comment with the exact grep evidence.

The old `compile_source`/`compile_source_with_parsed`/`StandaloneCompileOutput`/
`StandaloneSourceBytes` are gone — no dual path.

### 2. Vue compose-and-publish split (`crates/verter_compiler/src/assembly/vue_module.rs`, new)

`compose_fragments` (`vue_module.rs:409`, `pub(crate)`) does everything the old
session-side `assemble_vue_main_module` did EXCEPT the final `publish()` call —
script rewrite (via the moved `rewrite_script`, `vue_module.rs:105`,
`pub(crate)`), fragment minting, sequencing — returning owned
`ComposedFragments { fragments, code, source_map, emitted_imports }`
(`vue_module.rs:380`) so a caller can combine it with sibling contributions
before ONE shared `publish()` call. `compose_main_module` (`vue_module.rs:665`,
`pub`) is the single-artifact convenience — compose then publish in one call —
used by `verter_session`'s host composer, which now only builds host-specific
`ExtraFragment` prelude/trailer decoration (style/custom-block imports,
`__file`, HMR, SSR-manifest registration) and delegates
(`crates/verter_session/src/compile.rs`, `assemble_vue_main_module`).

Byte-identity proof: `cargo test -p verter_session --lib compile::` — 76/76
pass, including the differential harness
(`compile::map_equality_tests::genuine_compiler_output_agrees_across_implementations`,
`::every_composing_seed_vector_agrees_across_implementations`,
`::rewrite_geometries_agree_across_implementations`) that compares Verter's
composed output against a real JS toolchain — proving the split produced
byte-identical host output.

`verter_session`'s own `VueMainAssemblyFailure` keeps its exact pre-existing
4-variant public shape (`compile.rs:63`); a new `From<verter_compiler::
assembly::VueMainAssemblyFailure>` (`compile.rs:95`) lifts the shared
composer's narrower 2-variant `Composition`/`Publication` split back into it.

Six `rewrite_script`-specific unit tests (previously white-box tests inside
`verter_session::compile::map_tests`/`map_equality_tests`, since deleted —
`map_tests.rs:1044-1198`, `map_equality_tests.rs:3002-3077` in the pre-image)
moved to `crates/verter_compiler/src/assembly/vue_module.rs`'s own
`#[cfg(test)] mod tests` (`vue_module.rs:693` onward), adapted from
`verter_session`'s `DecodedFragmentMap` fixture shape to plain
`oxc_sourcemap::SourceMap`/`Token` construction — same assertions, same
coverage, now co-located with the moved production code.

### 3. Structural closure — no legacy alternate core can publish

`publish`, `ArtifactContribution` (`assembly/publish.rs:176`, `:19` — both
`pub(crate)` now, was `pub`), and `ProductPlan::from_request`/`::single`
(`assembly/plan.rs:36`, `:84` — `pub(crate)` now) are visible only inside
`verter_compiler`. `assembly/mod.rs`'s `pub use` list no longer re-exports
`publish`/`ArtifactContribution` (`assembly/mod.rs:34,40`) — a `cargo check
--workspace --all-targets` after this change is the structural proof: the
only in-workspace callers were `verter_session::compile` (now migrated to
`compose_main_module`) and `verter_compiler`'s own three `tests/cases/
assembly/*.rs` integration tests, which could not compile against `pub(crate)`
items from an external test binary and were folded into `publish.rs`'s own
`#[cfg(test)]` suite instead (deleted: `tests/cases/assembly/atomic_refusal.rs`,
`exact_product_set.rs`, `final_module_parse.rs`; ported:
`multi_product_request_publishes_exactly_those_products_and_nothing_else`,
`publish.rs:516`, the one scenario not already covered by an existing
internal test). `cargo check --workspace --all-targets` (see §Verification)
is green with these three items `pub(crate)` — no other production or test
code in the workspace constructs an `ArtifactSet` any other way.

**`AssembledArtifact` widened, not narrowed**: a new `dialect() -> FragmentDialect`
accessor (`publish.rs:71`) — the direct core's `IdeCompanion` consumer
(`verter_tsc`) needs the artifact's dialect (`.jsx` vs `.tsx` extension
choice) and has no `ParsedSfc` of its own to re-derive it from; threading it
through `AssembledArtifact` avoids a second parse. Purely additive — no
existing caller's field set changed.

**`compile_client`'s visibility (corrected in fix round 1 — the original
"stays `pub`" decision above was itself the defect, not a documented
deviation)**: the review correctly flagged that `compile_client` staying
fully `pub` was an unclosed shadow direct compiler — it accepts
caller-controlled `SvelteRuntimeOptions` directly and returns a complete
usable `ClientModule` (real code + CSS), reachable from ANY external crate,
not just the two genuine test callers. `svelte/runtime/mod.rs` now re-exports
it through the same closed-by-default two-arm `cfg` pattern as
`compile`/`compile_with_parsed` (§Fix round 1 item 3/4): `pub` under
`#[cfg(any(test, feature = "test-support"))]`, `pub(crate)` otherwise. Of the
two genuine cross-crate callers: `verter_session`'s
`svelte_official_conformance_matrix.rs` genuinely needs the raw
`SvelteRuntimeOptions`-level entry (its `dev`-axis probe sits one layer below
any public `CompileRequest`) and now reaches it via the `test-support`
feature edge added to `verter_session/Cargo.toml`'s `verter_compiler`
dev-dependency; `verter_svelte_conformance`'s `value_wrap_cells.rs` did NOT
need raw access — it was migrated onto `StandaloneCompiler::compile` instead
(§Fix round 1 item 4).

### 4. Svelte declared-import facts (`crates/verter_compiler/src/svelte/runtime/`)

- `ImportPlan::declared_imports()` (`helpers.rs`, new) — the flag +
  runtime-namespace imports as real `DeclaredImport`s.
- `UserImport::declared_imports()` (`client_imports.rs`, new,
  `pub(super)`) — one `DeclaredImport` per bound-name CLAUSE SHAPE (a mixed
  `import Default, { named } from '…'` splits into two entries sharing one
  specifier); the external `imported` name is dropped (`DeclaredImportKind`
  carries only local bound names).
- `ClientModule.declared_imports: Vec<DeclaredImport>` (`client_output.rs`,
  new field) populated once, in `emit_client_module`'s existing `emit()`
  (`client.rs`, right before its `Ok(ClientModule{...})`) from the SAME
  `topology.imports`/`self.plan.user_imports` `emit_imports` already writes
  from — never recovered by reparsing generated code.

## Fix round 1 — 4 blocking defects from independent review

Independent review of `a9d1547f2` returned BLOCKED with 4 defects, all fixed
on top at `cb4f75fcc` (2 commits: `4cf3be728` then `cb4f75fcc`).

1. **Dual-runtime products silently collapsed to one.** A request planning
   both `RuntimeClient` and `RuntimeServer` together (independently
   co-requestable per `CompileRequest`'s own contract) produced only one
   artifact and `UnsupportedProduct`-refused the other. Fixed: both routes
   now compose AND publish both artifacts in the same `publish()` call when
   both are planned — see item 1's dispatch description above.
2. **Svelte route issues**: (a) `SvelteNamespaceRequest::Foreign` silently
   became `None` — now refused via `DirectCompileError::UnsupportedSvelteNamespace`;
   (b) `custom_element_descriptor` non-consumption — confirmed as a genuine
   pre-existing gap shared by both routes, documented with grep evidence,
   dispositioned DEFER (see above); (c) style/CSS output was silently dropped
   for both frameworks — fixed via `DirectCompileOutput.styles`, a sibling to
   the atomic `ArtifactSet`, not a widening of B4's `ProductKind`/
   `ArtifactContribution`/`publish()` contract.
3. **`compile`/`compile_with_parsed` widened to `#[doc(hidden)] pub`
   was an unclosed legacy alternate core** — `#[doc(hidden)]` doesn't restrict
   callers; the original implementation's self-review claim (struck below)
   that this was "an already-established pattern" was itself wrong (the cited
   precedent, `compile_registered_vue_artifact`, has no test-only-visibility
   gate at all — it is genuinely `pub` for a real production reason). Fixed:
   reverted to the closed-by-default two-arm `#[cfg(any(test, feature =
   "test-support"))] pub` / `#[cfg(not(...))] pub(crate)` pattern (both arms
   delegating to a private `_impl` body) — the SAME pattern already
   established in this exact crate for `emit_static_style_object`. Every real
   external caller migrated: `verter_vue_conformance` (`seed_conformance.rs`,
   Cargo.toml `test-support` edge), `verter_shipped_cfg_contract` (migrated to
   `StandaloneCompiler::compile`), `verter_lsp`
   (`kebab_tag_mapping_full_columns.rs`, migrated), `verter_session`
   (`framework_parse_characterization_tests.rs`, Cargo.toml `test-support`
   edge; `compile_tests.rs`'s `compile_multi_root_template_uses_fragment`,
   migrated), `verter_bench` (4 benches/examples, Cargo.toml `test-support`
   edge — **initially placed under `[dependencies]` with the reasoning "no
   reverse dependents, can't leak"; that reasoning was WRONG and is corrected
   in §Fix round 2 below, which moved it to `[dev-dependencies]`**).
4. **`svelte::runtime::compile_client` staying fully `pub` was ALSO an
   unclosed shadow direct compiler** (accepts caller-controlled
   `SvelteRuntimeOptions` directly, returns complete usable JS+CSS). Fixed —
   see the corrected §3 "`compile_client`'s visibility" section above.

Additionally: `cargo clippy --workspace --all-targets -- -D warnings` caught
one `useless_conversion` (`.zip(pending.into_iter())` → `.zip(pending)`) in
the new Svelte dual-runtime loop, fixed in the same round.

## Fix round 2 — 1 remaining defect from a second independent review

A second independent review of `931e03afd` returned BLOCKED with exactly one
remaining defect (everything from fix round 1 confirmed held).

**Defect**: `crates/verter_bench/Cargo.toml` put
`verter_compiler = { ..., features = ["bench", "test-support"] }` under
`[dependencies]` (a NORMAL, non-dev edge), with a comment claiming this was
safe because `verter_bench` is `publish = false` with no reverse dependents.
That reasoning was wrong: Cargo's feature resolver unifies a NORMAL
dependency edge's features across the whole build graph regardless of which
target is actually requested — a normal edge activates the feature for the
shared `verter_compiler` instance that every other workspace member's
`[dependencies]` edge also resolves to, in ANY command that includes
`verter_bench` in the graph (`cargo check --workspace`, `cargo build
--workspace`, no `--all-targets` needed). That reopens exactly the "no
legacy alternate core reachable from a normal workspace build" hole fix
round 1 closed everywhere else.

**Fix**: moved the `verter_compiler` dependency line from `[dependencies]` to
`[dev-dependencies]` in `crates/verter_bench/Cargo.toml`, keeping `bench`
(needed on the normal edge for `template`/`script` module visibility, which
`compile`/`compile_with_parsed`/`compile_client` are unrelated to) on a plain
non-`test-support` normal edge, and adding a second `[dev-dependencies]`
entry carrying `test-support` alone — the same split-edge idiom already used
by `verter_vue_conformance` and `verter_compiler`'s own self-edge (one plain
`[dependencies]` entry with no `test-support`, one `[dev-dependencies]` entry
that does carry it). This is the SAME idiom already established correctly by
`verter_session` (its `verter_scheduler`/`verter_semantic`/`verter_compiler`
dev-dependency edges), `verter_vue_conformance`, and `verter_compiler`'s own
self-edge elsewhere in this codebase — Cargo unifies dev-dependency features
only for a package's own test/bench/example targets, never for its normal
lib/bin build or for sibling workspace members.

**Verification**:
- `cargo check -p verter_bench --all-targets`: clean (benches + examples
  using `test-support` items still compile).
- `cargo check -p verter_bench --example new_impl_check --example profile_ast
  --example vapor_check --bench real_world_compile_bench`: clean (the 4
  named callers individually).
- `cargo tree -p verter_bench -e features -i verter_compiler`: shows
  `test-support` reached only via a `[dev-dependencies]`-labeled edge.
- `cargo tree --workspace -e no-dev,features -i verter_compiler` (excludes
  ALL dev-dependency edges workspace-wide — i.e. exactly the edge set a
  normal/production build graph uses): shows only `bench` and `default` on
  `verter_compiler`, with `test-support` entirely ABSENT — the positive proof
  that a normal build never activates it.
- Workspace-wide grep of every `crates/*/Cargo.toml` for `test-support` under
  a `[dependencies]` or `[build-dependencies]` section (scripted, section-aware):
  zero hits outside `verter_bench`'s already-fixed edge — confirms the
  review's finding was isolated to this one crate.
- `cargo check --workspace --all-targets`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all -- --check`: clean.
- `cargo test -p verter_session --lib compile::` (byte-identity proof):
  **76 passed, 0 failed** — unchanged, held.
- `cargo test -p verter_vue_conformance`: **8 passed, 0 failed** — unchanged, held.
- `cargo test -p verter_svelte_conformance`: **32 passed, 0 failed** — unchanged, held.

`Cargo.lock` was unaffected (moving a dependency between `[dependencies]` and
`[dev-dependencies]` for an already-resolved path dependency changes no
version/resolution, only which build graphs activate its features) — only
`crates/verter_bench/Cargo.toml` changed in this round.

## Verification run and outcome

All commands wrapped in `~/.claude/bin/rust-lock.sh b5-impl -- <cmd>`. Table
below is the FRESH fix-round-1 state (superseding the original-implementation
numbers that were here before).

| Command | Result |
|---|---|
| `cargo check --workspace --all-targets` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `cargo test -p verter_compiler --lib` | **6301 passed, 0 failed, 5 ignored** (18/18 in `standalone::tests`) |
| `cargo test -p verter_session --lib compile::` (byte-identity proof) | **76 passed, 0 failed** |
| `cargo test -p verter_session --lib framework_parse_characterization` | **8 passed, 0 failed** |
| `cargo check -p verter_session --lib --tests --features bf2-authoritative` | clean (proves the `test-support` edge reaches `compile_client` through `svelte_official_conformance_matrix.rs`; this oracle-gated suite needs a live install to actually RUN and is excluded from the default run by design) |
| `cargo test -p verter_tsc` | **14 passed, 0 failed** |
| `cargo test -p verter_lsp --test main kebab_tag_mapping_full_columns` | **3 passed, 0 failed** |
| `cargo test -p verter_shipped_cfg_contract` | 8 passed, **2 deliberate profile-sanity failures** (expected under the `dev` profile this ran under — see the crate's own doc comment; `gate.mjs` runs it under `no-debug-assertions` separately) |
| `cargo test -p verter_vue_conformance` | **8 passed, 0 failed** |
| `cargo test -p verter_svelte_conformance` | **32 passed, 0 failed** |

Not re-run in this fix round: the full `node scripts/gate.mjs` (per the
implementer brief, another agent owns machine-wide gate runs) and
`cargo test -p verter_session --lib` in full (the byte-identity `compile::`
subset and the `framework_parse_characterization_tests` subset — the two
areas fix round 1 actually touched — were run directly instead; the original
implementation's full-suite run, including the 2 pre-existing unrelated
failures documented below, is unchanged by this round's edits).

**The 2 `verter_session` failures are pre-existing and unrelated to B5**:
`typeinfo::typeinfo_tests::vue_macro_codegen::{tsc_class_inference_budget_is_exact_partial_and_non_cacheable,
tsc_class_return_replay_fails_closed_for_unsupported_and_nested_unsafe_inference}`.
Confirmed independently: (a) a concurrent orchestrator process on this
machine (visible via `ps aux` during this session) is actively dispatching a
fix for these exact two tests, attributing them to trunk commit `979123ef4`
("make the TSC expose bundle fail closed on any corrupt identity") — a commit
that landed on trunk BEFORE this branch's own base `264f727cb`, i.e. before
B5 started; (b) the failing test file
(`crates/verter_session/src/typeinfo/typeinfo_tests/vue_macro_codegen.rs`)
has zero references to `standalone`, `StandaloneCompiler`, `compose_main_module`,
or `assemble_vue_main_module` (grepped) — it exercises TSC class-return-type
inference containment, a completely different subsystem from this block's
scope.

Not run: the full `node scripts/gate.mjs` (per the implementer brief, another
agent owns machine-wide gate runs) and the 3
`framework_common::vue_bridge::tests::*` output-validity tests that need the
pinned TypeScript launcher — `node_modules` was absent early in this session
and present later (installed by another concurrent process on the shared
worktree host); the final `cargo test -p verter_compiler --lib` run above,
taken with `node_modules` present, shows all 6296 tests including those
three passing.

## Self-review

- Every ruling item implemented except the one explicitly recorded
  deviation (§3, `compile_client` visibility) — implemented with evidence,
  not silently substituted.
- No stub tests: every new test (`standalone.rs` §tests, `vue_module.rs`
  §tests) asserts on real generated code content (`.contains("_sfc_main")`,
  `.contains("svelte/internal/client")`, artifact cardinality, refusal
  variants), not existence-only checks.
- No dual path: `compile_source`/`compile_source_with_parsed`/
  `StandaloneCompileOutput`/`StandaloneSourceBytes` are deleted; every
  workspace caller migrated to either the atomic `StandaloneCompiler::compile`
  (when it only needed a final artifact — `checker.rs`,
  `verter_shipped_cfg_contract`, `verter_lsp`'s
  `kebab_tag_mapping_full_columns.rs`, `verter_svelte_conformance`'s
  `value_wrap_cells.rs`, `compile_multi_root_template_uses_fragment`) or the
  `test-support`-gated `compile`/`compile_with_parsed`/`compile_client`
  (when it genuinely needed the raw pre-assembly shape — `verter_vue_conformance`,
  `verter_bench`, `verter_session`'s `framework_parse_characterization_tests.rs`
  and `svelte_official_conformance_matrix.rs`), closed to `pub(crate)` in
  every other build via the two-arm `cfg` pattern (§Fix round 1 item 3/4;
  the ORIGINAL self-review's claim that `#[doc(hidden)] pub` was an
  established closed pattern was itself wrong — corrected in fix round 1).
- `git log` on `block/b5` shows 9 incremental commits (7 original + 2 fix
  round 1), not one giant diff.
