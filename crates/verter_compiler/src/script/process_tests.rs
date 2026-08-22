use super::*;

// ── Component wrapper gate (official @vue/compiler-sfc non-inline) ──
//
// Official gates the wrapper on the script `lang` and the presence of a
// companion `export default` / `defineOptions`:
// - TS (`lang="ts"`/`"tsx"`) → `/*@__PURE__*/_defineComponent({ ... })`
// - JS, no companion default / defineOptions → plain object literal
// - JS with companion default / defineOptions → `Object.assign({ ... }, { ... })`

#[test]
fn wrap_gate_ts_always_define_component() {
    assert_eq!(component_wrap(true, false), ComponentWrap::DefineComponent);
    assert_eq!(component_wrap(true, true), ComponentWrap::DefineComponent);
}

#[test]
fn wrap_gate_js_plain_without_options() {
    assert_eq!(component_wrap(false, false), ComponentWrap::Plain);
}

#[test]
fn wrap_gate_js_object_assign_with_options() {
    assert_eq!(component_wrap(false, true), ComponentWrap::ObjectAssign);
}

#[test]
fn build_wrapper_start_plain_js_no_define_component() {
    // JS <script setup> with runtime props/emits still emits a PLAIN object
    // (official emits _defineComponent only for TS).
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        Some("{ title: String }"),
        Some("['save']"),
        None,
        false,
        false,
        false,
        ComponentWrap::Plain,
        false,
        false,
    );
    assert!(
        result.starts_with("const __sfc__ = {\n"),
        "plain JS wrapper should be a bare object literal, got:\n{}",
        result
    );
    assert!(
        !result.contains("_defineComponent"),
        "plain JS wrapper must not reference _defineComponent, got:\n{}",
        result
    );
    assert!(result.contains("__name: 'Test'"));
    assert!(result.contains("props: { title: String }"));
    assert!(result.contains("emits: ['save']"));
    assert!(result.contains("setup(__props) {"));
}

#[test]
fn build_wrapper_start_object_assign_merges_options() {
    // JS + defineOptions → Object.assign(<raw defineOptions expr>, runtime).
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        None,
        None,
        Some("{ inheritAttrs: false }"),
        false,
        false,
        false,
        ComponentWrap::ObjectAssign,
        false,
        false,
    );
    assert!(
        result
            .starts_with("const __sfc__ = /*@__PURE__*/Object.assign({ inheritAttrs: false }, {\n"),
        "merge wrapper should Object.assign the options object, got:\n{}",
        result
    );
    assert!(
        !result.contains("_defineComponent"),
        "merge wrapper must not reference _defineComponent, got:\n{}",
        result
    );
    // Runtime sections live in the last (merged-into) object.
    assert!(result.contains("__name: 'Test'"));
    // Options must NOT be inlined into the runtime object a second time.
    assert_eq!(result.matches("inheritAttrs").count(), 1);
}

#[test]
fn build_wrapper_start_object_assign_companion_default_target() {
    // JS + companion `export default <expr>` → `__default__` is the merge target.
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        None,
        None,
        None,
        true,
        false,
        false,
        ComponentWrap::ObjectAssign,
        false,
        false,
    );
    assert!(
        result.starts_with("const __sfc__ = /*@__PURE__*/Object.assign(__default__, {\n"),
        "companion default must be the Object.assign target, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_start_object_assign_both_sources_official_order() {
    // JS + companion default + defineOptions → Object.assign(__default__, <expr>, runtime).
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        None,
        None,
        Some("{ inheritAttrs: false }"),
        true,
        false,
        false,
        ComponentWrap::ObjectAssign,
        false,
        false,
    );
    assert!(
        result.starts_with(
            "const __sfc__ = /*@__PURE__*/Object.assign(__default__, { inheritAttrs: false }, {\n"
        ),
        "official order: __default__, defineOptions, runtime, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_end_plain_closes_object_without_call() {
    let (result, _binding_ranges, _export_range) =
        build_setup_wrapper_end(Some("{ msg }"), None, ComponentWrap::Plain);
    assert!(
        result.contains("\n}};\n"),
        "plain wrapper closes the object literal directly, got:\n{}",
        result
    );
    assert!(!result.contains("}});"));
    assert!(result.contains("export default __sfc__"));
}

#[test]
fn build_wrapper_start_basic() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::DefineComponent,
        false,
        false,
    );
    assert!(result.contains("__name: 'Test'"));
    assert!(result.contains("setup(__props) {"));
    assert!(!result.contains("async"));
}

// ── __vapor flag placement (official @vue/compiler-sfc's runtimeOptions) ──
//
// Confirmed directly against the vendored rc.5 compiler source
// (`compileScript`'s non-TS branch: `if (vapor) runtimeOptions +=
// '\n  __vapor: true,'`, unconditional on ssr; the TS `_defineComponent`
// branch: `if (ssr && vapor) runtimeOptions += ...` — a non-SSR TS vapor
// component uses the `defineVaporComponent` helper instead, NOT
// implemented here, since that helper is not threaded through
// `ComponentWrap`) and the pinned rc.5 golden for `basic-interpolation.vue`'s
// vapor cell (`{ __name: '…', __vapor: true, setup(…) {…} }`).

#[test]
fn build_wrapper_start_vapor_js_plain_inlines_vapor_flag_after_emits() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        Some("{ title: String }"),
        Some("['save']"),
        None,
        false,
        false,
        false,
        ComponentWrap::Plain,
        true,  // is_vapor
        false, // ssr
    );
    assert!(
        result.contains("emits: ['save'],\n  __vapor: true,\n  setup("),
        "__vapor must be inlined right after emits, before setup, got:\n{}",
        result
    );
    assert!(!result.contains("__sfc__.__vapor ="));
}

#[test]
fn build_wrapper_start_non_vapor_js_plain_omits_vapor_flag() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::Plain,
        false,
        false,
    );
    assert!(!result.contains("__vapor"), "got:\n{}", result);
}

#[test]
fn build_wrapper_start_vapor_ts_define_component_ssr_inlines_vapor_flag() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::DefineComponent,
        true, // is_vapor
        true, // ssr
    );
    assert!(
        result.contains("__vapor: true,"),
        "TS + vapor + ssr must inline __vapor: true, got:\n{}",
        result
    );
}

/// TS + vapor + NON-ssr official uses the `defineVaporComponent` helper
/// instead of `_defineComponent({ __vapor: true, ... })` — NOT implemented
/// here (`defineVaporComponent` is not threaded through `ComponentWrap`),
/// so this asserts the current, incomplete `__vapor`-omitting behavior
/// rather than a claim that this shape is correct.
#[test]
fn build_wrapper_start_vapor_ts_define_component_non_ssr_omits_vapor_flag() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::DefineComponent,
        true,  // is_vapor
        false, // ssr
    );
    assert!(
        !result.contains("__vapor"),
        "TS + vapor + non-ssr does not inline __vapor here (the \
         defineVaporComponent-helper gap is a disclosed, out-of-scope \
         residual), got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_start_async() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        true,
        false,
        false,
        false,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::DefineComponent,
        false,
        false,
    );
    assert!(result.contains("async setup(__props"));
}

#[test]
fn build_wrapper_start_no_name() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "",
        false,
        false,
        false,
        false,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::DefineComponent,
        false,
        false,
    );
    assert!(!result.contains("__name"));
}

#[test]
fn build_wrapper_start_with_props() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        Some("{ title: String }"),
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::DefineComponent,
        false,
        false,
    );
    assert!(result.contains("props: { title: String }"));
}

#[test]
fn build_wrapper_start_with_emits() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        true,
        None,
        Some("['click']"),
        None,
        false,
        false,
        false,
        ComponentWrap::DefineComponent,
        false,
        false,
    );
    assert!(result.contains("emits: ['click']"));
    assert!(result.contains("emit: __emit"));
}

#[test]
fn build_wrapper_start_with_expose() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        true,
        false,
        false,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::DefineComponent,
        false,
        false,
    );
    assert!(result.contains("expose: __expose"));
}

#[test]
fn build_wrapper_start_with_expose_and_emit() {
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        true,
        false,
        true,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::DefineComponent,
        false,
        false,
    );
    assert!(result.contains("expose: __expose, emit: __emit"));
}

// ── Non-inline `<script setup>` without an authored `defineExpose()` still
// binds `expose: __expose` and emits a bare `__expose();` call (official
// `buildDestructureElements` / `hasDefineExposeCall || !inlineMode`) ──

#[test]
fn build_wrapper_start_non_inline_no_define_expose_binds_and_emits_bare_expose_call() {
    // Caller-computed non-inline, no-authored-defineExpose case:
    // `bind_expose = has_expose || !inline_template` = `false || true` = `true`;
    // `emit_bare_expose_call = !has_expose && !inline_template` = `true`.
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        true,
        true,
        false,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::Plain,
        false,
        false,
    );
    assert!(
        result.contains("expose: __expose"),
        "non-inline setup without defineExpose() must still bind expose: __expose, got:\n{}",
        result
    );
    assert!(
        result.contains("  __expose();\n\n"),
        "non-inline setup without defineExpose() must emit a bare __expose() call at the top \
         of the setup body, got:\n{}",
        result
    );
    // The bare call comes AFTER the signature closes and BEFORE anything else.
    let sig_end = result.find(") {\n").expect("setup signature closes");
    let call_pos = result.find("__expose();").expect("bare call present");
    assert!(
        call_pos > sig_end,
        "bare __expose() call must follow the setup signature, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_start_inline_no_define_expose_omits_bind_and_bare_call() {
    // Negative control: inline template, no authored defineExpose() — neither
    // the binding nor the bare call are official behavior for inline mode
    // (`bind_expose = false || false = false`; `emit_bare_expose_call = true
    // && false = false`).
    let (result, _binding_range) = build_setup_wrapper_start(
        "",
        false,
        false,
        false,
        false,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::Plain,
        false,
        false,
    );
    assert!(
        !result.contains("expose"),
        "inline setup without defineExpose() must not bind or reference expose, got:\n{}",
        result
    );
    assert_eq!(
        result, "const __sfc__ = {\n  setup(__props) {\n",
        "inline setup without defineExpose() must be the bare signature, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_start_non_inline_with_authored_define_expose_binds_without_duplicate_bare_call() {
    // Positive control: an AUTHORED defineExpose() already binds and invokes
    // __expose itself in the user's own code, so the synthesized bare call
    // must NOT also be emitted (that would double-invoke __expose).
    // `bind_expose = true || true = true`; `emit_bare_expose_call = false &&
    // true = false`.
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        true,
        false,
        false,
        None,
        None,
        None,
        false,
        false,
        false,
        ComponentWrap::Plain,
        false,
        false,
    );
    assert!(result.contains("expose: __expose"));
    assert!(
        !result.contains("__expose();"),
        "an authored defineExpose() must not ALSO get a synthesized bare call, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_start_with_options() {
    // TS + defineOptions → official spread of the raw expression before __name.
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        None,
        None,
        Some("{ inheritAttrs: false }"),
        false,
        false,
        false,
        ComponentWrap::DefineComponent,
        false,
        false,
    );
    assert!(result.contains("...{ inheritAttrs: false },"));
    // Options should come before __name
    let opts_pos = result.find("inheritAttrs").unwrap();
    let name_pos = result.find("__name").unwrap();
    assert!(opts_pos < name_pos);
}

#[test]
fn build_wrapper_start_ts_companion_default_spread() {
    // TS + companion default → `...__default__` spread before runtime options.
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        None,
        None,
        None,
        true,
        false,
        false,
        ComponentWrap::DefineComponent,
        false,
        false,
    );
    assert!(
        result.contains("/*@__PURE__*/_defineComponent({\n  ...__default__,\n"),
        "TS must spread __default__ first, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_start_ts_both_spreads_official_order() {
    // TS + companion default + defineOptions → both spreads, official order.
    let (result, _binding_range) = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        false,
        None,
        None,
        Some("{ inheritAttrs: false }"),
        true,
        false,
        false,
        ComponentWrap::DefineComponent,
        false,
        false,
    );
    let default_pos = result.find("...__default__").unwrap();
    let options_pos = result.find("...{ inheritAttrs: false }").unwrap();
    assert!(
        default_pos < options_pos,
        "official order: __default__ spread before defineOptions spread, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_end_with_return() {
    let (result, _binding_ranges, _export_range) =
        build_setup_wrapper_end(Some("{ msg, count }"), None, ComponentWrap::DefineComponent);
    assert!(result.contains("const __returned__ = { msg, count }"));
    assert!(result.contains("__isScriptSetup"));
    assert!(result.contains("return __returned__"));
    assert!(result.contains("}});"));
    assert!(result.contains("export default __sfc__"));
}

#[test]
fn build_wrapper_end_no_return() {
    let (result, _binding_ranges, _export_range) =
        build_setup_wrapper_end(None, None, ComponentWrap::DefineComponent);
    assert!(!result.contains("return"));
    assert!(result.contains("}});"));
}

#[test]
fn build_wrapper_end_with_scope_id() {
    let (result, _binding_ranges, _export_range) =
        build_setup_wrapper_end(None, Some("data-v-abc"), ComponentWrap::DefineComponent);
    assert!(result.contains("__sfc__.__scopeId = \"data-v-abc\""));
}

#[test]
fn build_returned_empty() {
    let bindings = FxHashMap::default();
    assert_eq!(build_returned_object(&bindings, &[], None, None), "{}");
}

#[test]
fn build_returned_setup_bindings_only() {
    let mut bindings = FxHashMap::default();
    bindings.insert("count", BindingType::SetupRef);
    bindings.insert("msg", BindingType::SetupConst);
    bindings.insert("title", BindingType::Props); // Not included
    let order = ["count", "msg", "title"];
    let result = build_returned_object(&bindings, &order, None, None);
    assert!(result.contains("count"));
    assert!(result.contains("msg"));
    assert!(!result.contains("title"));
}

#[test]
fn build_returned_preserves_declaration_order_not_alphabetical() {
    // Official's non-inline `__returned__` preserves SOURCE-DECLARATION
    // order (JS object insertion order), not an alphabetical sort — proven
    // against the exact rc.5 `props-emit.vue` seed fixture, whose golden
    // is `{ props, emit, onClick }`. `zebra` is declared BEFORE `alpha`
    // here specifically so an alphabetical-sort regression would flip them.
    let mut bindings = FxHashMap::default();
    bindings.insert("zebra", BindingType::SetupConst);
    bindings.insert("alpha", BindingType::SetupRef);
    let order = ["zebra", "alpha"];
    let result = build_returned_object(&bindings, &order, None, None);
    assert_eq!(
        result, "{ zebra, alpha }",
        "must preserve declaration order (zebra before alpha), got: {result}"
    );
}

#[test]
fn build_returned_imports_sort_after_local_declarations_regardless_of_textual_position() {
    // The exact `basic-interpolation.vue` seed-fixture shape: `import {
    // ref } from "vue"` sits textually BEFORE `const count = ref(0)`, yet
    // the rc.5 golden `__returned__` is `{ count, items, ref }` — both local
    // `const`s first, the import LAST. Official's `allBindings` is built by
    // spreading local `scriptBindings`/`setupBindings` first, then merging
    // in used imports via a separate loop that only adds keys not already
    // present — so imports always sort after every local declaration,
    // independent of where the `import` statement sits in the file.
    // `binding_order` here deliberately lists the import FIRST (matching its
    // earlier textual position) to prove this isn't accidentally already
    // correct via textual order.
    let mut bindings = FxHashMap::default();
    bindings.insert("ref", BindingType::SetupImport);
    bindings.insert("count", BindingType::SetupRef);
    bindings.insert("items", BindingType::SetupConst);
    let order = ["ref", "count", "items"]; // textual order: import first

    let vars = FxHashSet::default(); // not referenced in the template
    let runtime_text = "const count = ref(0);\nconst items = [\"a\", \"b\", \"c\"];";

    let result = build_returned_object(&bindings, &order, Some(&vars), Some(runtime_text));
    assert_eq!(
        result, "{ count, items, ref }",
        "local declarations must precede the import despite its earlier textual \
         position, got: {result}"
    );
}

// ── SetupImport filtering in __returned__ ────────────────────

#[test]
fn build_returned_setup_import_excluded_without_template_vars() {
    // Conservative fallback: neither template_used_vars NOR runtime_text is
    // available (both None) → include all (matches `is_specifier_runtime_used`'s
    // own conservative-include rule for that case).
    let mut bindings = FxHashMap::default();
    bindings.insert("MyComp", BindingType::SetupImport);
    bindings.insert("msg", BindingType::SetupConst);
    let order = ["MyComp", "msg"];
    let result = build_returned_object(&bindings, &order, None, None);
    assert!(result.contains("MyComp"));
    assert!(result.contains("msg"));
}

#[test]
fn build_returned_setup_import_included_when_template_used() {
    // Positive case: a SetupImport referenced in the template is included
    // (a case the unconditional-inclusion rule below never had to widen —
    // template usage was always enough).
    let mut bindings = FxHashMap::default();
    bindings.insert("UsedComp", BindingType::SetupImport);
    bindings.insert("msg", BindingType::SetupConst);
    let order = ["UsedComp", "msg"];

    let mut vars = FxHashSet::default();
    vars.insert("UsedComp".to_string());

    let result = build_returned_object(&bindings, &order, Some(&vars), Some(""));
    assert!(
        result.contains("UsedComp"),
        "Template-used import should be in __returned__"
    );
    assert!(result.contains("msg"));
}

#[test]
fn build_returned_setup_import_included_when_script_body_used() {
    // The actual invariant under test: a SetupImport used ONLY in the
    // script body (never referenced in the template — e.g. `import { ref }
    // from "vue"; const count = ref(0)`, the exact `basic-interpolation.vue`
    // seed-fixture shape) is included, matching official's
    // `genSetupReturn` (`allBindings = { ...scriptBindings, ...setupBindings
    // }`, no template-usage filter) — a template-only filter would drop
    // this case, exactly the shape that previously over-elided `ref` from
    // `basic-interpolation.vue`'s `__returned__`.
    let mut bindings = FxHashMap::default();
    bindings.insert("ref", BindingType::SetupImport);
    bindings.insert("count", BindingType::SetupRef);
    let order = ["ref", "count"];

    let vars = FxHashSet::default(); // empty: not referenced in the template
    let runtime_text = "const count = ref(0);";

    let result = build_returned_object(&bindings, &order, Some(&vars), Some(runtime_text));
    assert!(
        result.contains("ref"),
        "A SetupImport used only in the script body must still be in \
         __returned__ per official's unconditional-inclusion rule, got: {result}"
    );
}

#[test]
fn build_returned_setup_import_excluded_when_genuinely_unused() {
    // Negative control: a SetupImport with ZERO runtime references anywhere
    // (script body or template) is excluded — it was already dropped from
    // the import statement itself by `filter_import_specifiers`
    // (`is_specifier_runtime_used`), so referencing it in __returned__ would
    // be a hard `ReferenceError`, not a cosmetic divergence. This is
    // narrower than "unconditional inclusion" and is why
    // `build_returned_object` reuses `is_specifier_runtime_used` rather than
    // dropping the SetupImport filter outright.
    let mut bindings = FxHashMap::default();
    bindings.insert("deadImport", BindingType::SetupImport);
    bindings.insert("msg", BindingType::SetupConst);
    let order = ["deadImport", "msg"];

    let vars = FxHashSet::default();
    let runtime_text = "";

    let result = build_returned_object(&bindings, &order, Some(&vars), Some(runtime_text));
    assert!(
        !result.contains("deadImport"),
        "A genuinely unused SetupImport (dropped from the import statement) \
         must not be referenced in __returned__, got: {result}"
    );
    assert!(result.contains("msg"));
}

// ── Vapor: __vapor flag ──────────────────────────────────────

// `build_setup_wrapper_end` never touches `__vapor` — official
// `@vue/compiler-sfc` builds it into the SAME accumulated `runtimeOptions`
// string as `__name`/`props`/`emits`, spliced into the object literal by
// `build_setup_wrapper_start` (confirmed directly against the vendored
// rc.5 compiler source and the pinned rc.5 golden for
// `basic-interpolation.vue`'s vapor cell) — never a separate trailing
// `__sfc__.__vapor = true` assignment at the wrapper's CLOSE. See
// `build_wrapper_start_vapor_js_inlines_vapor_flag` and its siblings below
// for the coverage this used to (incorrectly) claim here.
#[test]
fn build_wrapper_end_never_touches_vapor_flag() {
    let (result, _binding_ranges, _export_range) =
        build_setup_wrapper_end(Some("{ msg }"), None, ComponentWrap::DefineComponent);
    assert!(
        !result.contains("__vapor"),
        "build_setup_wrapper_end must never reference __vapor (that's \
         build_setup_wrapper_start's job now), got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_end_scope_id_still_a_separate_trailing_assignment() {
    // __scopeId is a DIFFERENT, bundler-level mechanism in real
    // @vitejs/plugin-vue (attachedProps + the _export_sfc helper, not
    // compileScript's runtimeOptions at all) — its existing separate-
    // statement emission here is untouched by the __vapor fix.
    let (result, _binding_ranges, _export_range) =
        build_setup_wrapper_end(None, Some("data-v-abc"), ComponentWrap::DefineComponent);
    assert!(result.contains("__sfc__.__scopeId = \"data-v-abc\""));
    assert!(!result.contains("__vapor"));
}
