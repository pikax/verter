# Tracked debt — FC-B4-002: Vue main-module composer cutover

Disposition: **RESOLVED**. Originally deferred (see git history for this file's prior
revision for the original DEFER record); closed by the fact-threading round that added
`SfcExportPlacement` and completed the cutover this row tracked.

## What was done

`assemble_vue_main_module` (`crates/verter_session/src/compile.rs`) now builds its
script/template/scaffolding pieces as real, individually-validated `ValidatedFragment`s
(deterministic role-based `SourceUnitId`s — see the Addendum below) and calls
`assembly::compose::assemble_sequence`, then publishes the composed artifact through
`assembly::publish` (a single-artifact `ProductPlan::single(...)` + one
`ArtifactContribution`, since this host composer never went through a `CompileRequest`)
for atomicity and final-parse validation. `MapComposer`, `ModuleWriter`, `FragmentWrite`,
and `SegmentOrigin` are deleted — nothing calls them.

The blocking piece this row was deferred on — the script producer declaring the
`__sfc__` rename/export-removal targets as a typed fact instead of `rewrite_script`
text-scanning for them — is done: `verter_compiler::assembly::fragment::SfcExportPlacement`
is declared at every in-scope runtime-emission site (`script/process.rs`'s
`process_script_only`, `process_script_setup`, `emit_minimal_component`; the template-only
synthetic script and `empty_sfc_script_block` in `verter_compiler`'s `compile` module),
threaded through `VerterScriptBlock` → `vue_result_to_runtime_bundle` →
`RuntimeScriptBlock` unchanged, and consumed by `rewrite_script`, which now applies ONLY
the declared ranges and typed-refuses an out-of-bounds or inconsistent fact
(`SfcRewriteRefusal`) rather than falling back to scanning. `literal_occurrences` and every
`.find`-based landmark scan in `map_compose.rs` are deleted.

Resolution-gate items, against the original list:

1. `rewrite_script` returns `(String, Option<String>)` — a JSON map string, not
   `Vec<WireSegment>`. Done.
2. `assemble_vue_main_module` builds real validated `Fragment`s, calls `assemble_sequence`,
   then publishes through `assembly::publish`. Done (see the Addendum below: the intermediate
   `SequencedFragment` shape this item originally described is deleted — the live path passes
   `ValidatedFragment`s straight through, closing the gap the first cutover pass left open).
3. The `Main` slot's data flows through `assembly::publish`'s atomicity/final-parse checks
   before being served — but NOT via the literal wiring this item originally described.
   `virtual_file_pipeline.rs` still calls `assemble_vue_main_module` directly and receives
   `AssembledVueModule`; the `ArtifactSet` is built and unwrapped ONE level down, inside
   `assemble_vue_main_module` itself, which stays the stable, minimal public entry point
   `verter_vue_conformance` and other external callers already depend on. This was a
   deliberate scope judgment (recorded, not silent): synthesizing a `CompileRequest`/
   `ProductPlan` at the `virtual_file_pipeline.rs` call site — which has neither today —
   would have been a materially larger, higher-risk change for no additional guarantee over
   routing the same checks through the existing entry point's own internals.
4. Every test in `map_tests.rs` passes against the new path. Two pinned assertions whose
   OWN premise was the retired text-scan (`provenance_is_tracked_but_never_serialized`,
   which directly constructed `MapComposer`/`ModuleWriter`/`FragmentWrite`/`SegmentOrigin`;
   and the "every literal occurrence, non-identifier-aware, arbitrary text" test) were
   rewritten against the new engine's actual contract rather than dropped — see
   `provenance_never_reaches_the_wire`, `rewrite_applies_only_the_declared_ranges`, and
   `authored_text_matching_the_landmarks_is_left_untouched` in `map_tests.rs`, with the one
   genuinely retired case (`map_equality_tests.rs`'s two-export-statement fixture, which has
   no fact that could declare it faithfully) documented in place, not silently deleted.
5. `MapComposer`, `ModuleWriter`, `FragmentWrite`, `SegmentOrigin`, and `rewrite_script`'s
   text-scan are deleted. Done — `ide/script/options_api.rs` was confirmed out of scope
   (IDE/TSX-only, never feeds `assemble_vue_main_module`) and left untouched.

## Verification

`cargo test -p verter_compiler --lib` / `--test main`, `cargo test -p verter_session --lib`
/ `--test main`: green modulo the pre-existing node_modules-dependent TypeScript-launcher
tests and one pre-existing unrelated failure (`external_template_ide_compile_contains_selected_bytes`,
confirmed failing identically against the pre-cutover commit in an isolated worktree —
IDE-lowering / external-template-source path, untouched by this work).
`cargo clippy -p verter_compiler -p verter_session --all-targets -- -D warnings` and
`cargo fmt --all --check`: clean.

One genuinely new, pre-existing defect surfaced by the newly-added final-parse check (not
introduced by this cutover — confirmed present on the pre-cutover commit): the inline-
template splice (`ct.move_slice` in `verter_compiler`'s `compile` module) abutted the
moved-in render closure directly against the authored setup body with no separator, which
is a genuine ECMAScript syntax error for a tightly-packed body with no trailing
whitespace/semicolon before `</script>` (`const n = 1` immediately followed by `return` —
`1return` is invalid; every prior fixture happened to have `</script>` on its own line,
which is why this went unexercised). Fixed test-first
(`inline_template_splice_is_valid_js_with_no_trailing_separator_in_setup_body`) by moving
the splice through `move_with_prefix(..., "\n")` instead of `move_slice`.

## Addendum — a second review round hardened this cutover further

A second, independent review of the landed cutover raised 6 questions; every one was ruled on
and applied:

- **Panic instead of typed refusal (adopted).** `assemble_vue_main_module` used to
  `.expect()` its own `assemble_sequence`/`publish` calls, reasoning they could only fail on
  an internal construction defect. That reasoning was wrong: this function receives and
  rewrites PRODUCER-SUPPLIED bytes (the real compiled script/template output), so a
  fragment-grammar, composition, or final-parse refusal can genuinely reflect a malformed
  upstream compile. `VueMainAssemblyFailure` (a new public enum covering input-map/rewrite
  failures, fragment-grammar refusals, composition defects, and publication refusals) is now
  the function's error type; every internal fallible call propagates through `?`; the two
  `virtual_file_pipeline.rs` call sites map it to one stable host diagnostic
  (`HOST_MAIN_MODULE_ASSEMBLY_FAILED`, renamed from the now-inaccurate
  `HOST_UNCOMPOSABLE_MAIN_SOURCE_MAP`).
- **`ValidatedFragment`-only composition (adopted).** `assemble_sequence` (and `publish`'s
  `ArtifactContribution.fragments`) used to accept a raw `{code, source_map}` pair
  (`SequencedFragment`, now deleted) — the contract said every fragment must declare and
  pass its grammar, but the live Main path never actually constructed one. Every
  scaffold/content piece (prelude, script, post-script separator, template prelude,
  template, post-template, trailer) is now a real `Fragment` with a deterministic
  role-based `SourceUnitId` (`canonical_id` + role, e.g. `"script"`/`"template"`), validated
  before composing; the SAME validated collection is what `publish`'s atomicity checks run
  against (closing the `fragments: vec![]`/`emitted_imports: Vec::new()` bypass the first
  cutover left). `assemble_sequence`'s signature itself now makes a raw pair a compile
  error — see `tests/cases/compile-fail/assemble_sequence_requires_validated_fragment.rs`.
- **Permissive-TSX-always parsing (adopted for the B4-owned half).** Both fragment
  validation and the final-parse check used to hardcode `SourceType::tsx()` regardless of
  the module's real dialect — silently permissive both ways (TypeScript-only syntax in a
  plain-JS artifact, or a genuinely invalid module that TSX's `Unambiguous` module-kind
  happens not to flag). `FragmentDialect` (JavaScript/Jsx/TypeScript/Tsx/Declaration) is now
  a real field on `Fragment` and `ArtifactContribution`; `assemble_vue_main_module` derives
  it ONCE from `meta.script_lang`/`profile.force_js` (the same inputs
  `virtual_file_pipeline.rs` used to independently re-derive `main_lang` twice) and reuses
  it for every fragment, the final artifact, and the returned `AssembledVueModule::lang`.
  Declared/emitted import facts are populated from `RuntimeScriptBlock::runtime_imports`,
  the template's own `imports`/`ssr_imports`, and the SSR `useSSRContext` import — never
  recovered by reparsing generated text — via an extended `DeclaredImportKind`
  (`SideEffect`/`Default`/`Namespace`/`Named`, since the old shape could not express a
  side-effect-only or default import). Real package-resolution against pinned npm Vue/Svelte
  packages stays explicitly out of scope (BV1's conformance-harness proof, not B4's).
- **Full multi-slot `compile_entry` cutover (deferred, ledger updated, no code change).**
  Replacing every `compile_entry` slot (Script/Template/Style/Custom/IDE, not just Main)
  with one request-wide `ArtifactSet` is the joint EM-040 cutover — B4's own obligation
  (plan/publication mechanism, exact-set refusal) already lives in `assembly::plan`/
  `assembly::publish`. See `emitter-mapping-dispositions.tsv`'s EM-040 row for the precise
  done/remaining split.
- **Inline-template newline injection (no defect, no action).** The separator inserted at
  the render-closure splice point is a structural, unmapped boundary fix, not a Vue-semantic
  decision; reverting it would knowingly restore an invalid assembled module.

A genuinely NEW defect surfaced by the dialect-accurate final-parse check (documented, not a
regression to route around): the frozen conformance vector V19 embeds literal
`export default _sfc_main;` text inside its OWN template body (simulating retired
write-grammar rules W-13/W-13′); composed with this assembler's own trailing
`export default _sfc_main`, the result now correctly fails final-parse as two default
exports — the FORMER hardcoded-permissive-TSX parse never caught this, because TSX's
`Unambiguous` module-kind does not enforce the single-default-export rule an explicit
`Module` dialect does. See
`docs/arch/refactor/rev11/evidence/B4/finding-frozen-w13-superseded-by-render-export-fact.md`'s
addendum and `vector_inventory.rs`'s updated V19 doc note — the existing divergence
exclusion for V19 (already escalated, not a B4 amendment) covers this new outcome the same
way it covered the original text-content divergence.

## Addendum 2 — round-3 (final) review residuals closed

Two small, mechanical residuals from a third review round, both ADOPT-NOW:

- **The last `.expect()` on this path (closed).** `map_compose::rewrite_script` chained its
  own overwrite-only transform onto the caller-supplied script map via
  `ct.chain_source_map(...).expect(...)` — reachable from `assemble_vue_main_module`'s call
  chain and bypassing `VueMainAssemblyFailure` despite `chain_source_map` genuinely returning
  failures (an input map naming a generated position its own text tiling cannot resolve). A
  new `SfcRewriteRefusal::ChainFailed(SourceMapChainError)` variant carries it; the call site
  now propagates with `?`. This closes the one remaining gap in the "every internal fallible
  call propagates through `?`" claim in Addendum 1 above — that claim is now fully true, not
  just aspirational. A discriminating regression test
  (`chain_source_map_failure_is_a_typed_refusal_not_a_panic` in `map_tests.rs`) proves the
  typed refusal fires; it necessarily calls `rewrite_script` directly rather than through
  `assemble_vue_main_module`'s public raw-JSON-map entry point, because
  `map_input::validate_and_decode`'s own generated-position bound check (step 1.24) already
  rejects, over the identical text, any segment that would trip `chain_source_map`'s bound
  check — the two checks are independently implemented but provably equivalent, so this
  failure mode is genuinely unreachable through the one production call site today. The typed
  refusal is still the correct contract: `rewrite_script`'s own signature does not, and must
  not, structurally assume its `map` argument was already validated against `code`.
- **The name-keyed scanner test deleted.** `crates/verter_compiler/tests/cases/assembly/
  no_generated_reparse.rs` — introduced by this branch, not a pre-existing landed guard — was
  a file-content scanner grepping production source for a hardcoded literal-string needle
  list, a direct violation of "Landed guards are structural, never name-keyed file scanners"
  and "Carrier Geometry From Registered Facts" (CLAUDE.md). Deleted entirely, along with its
  `mod` wiring in `tests/cases/assembly/mod.rs`. The invariant it defended (no generated-text
  reparsing deciding an export-binding identity) is already covered structurally by two
  functional tests that predate and are independent of the deleted scanner:
  `render_export_binding_follows_declared_fact_not_generated_text`
  (`crates/verter_session/src/compile/compile_tests.rs`) and
  `rewrite_applies_only_the_declared_ranges` (`crates/verter_session/src/compile/map_tests.rs`)
  — both confirmed still present and passing after the deletion.

A third finding from the same round — a real, narrow gap where `assemble_vue_main_module`'s
own `emitted_imports` and its `publish`-validated `fragments` are literally the same data at
this one call site, making the undeclared-helper check tautological against composed scaffold
TEXT drifting from the declared fact list — is recorded, per the ruling, as new debt row
`debt-FC-B4-003-scaffold-text-import-fact-drift.md` (disposition DEFER), not fixed in this
round.

## Owner

B4 / `verter_compiler::assembly`.

## Acceptance ID

`FC-B4-002`.
