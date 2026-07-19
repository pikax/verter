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
    let result = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        Some("{ title: String }"),
        Some("['save']"),
        None,
        ComponentWrap::Plain,
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
    // JS + defineOptions/companion default → Object.assign(options, runtime) merge.
    let result = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        None,
        None,
        Some("inheritAttrs: false"),
        ComponentWrap::ObjectAssign,
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
    // Runtime sections live in the second (merged-into) object.
    assert!(result.contains("__name: 'Test'"));
    // Options must NOT be inlined into the runtime object a second time.
    assert_eq!(result.matches("inheritAttrs").count(), 1);
}

#[test]
fn build_wrapper_end_plain_closes_object_without_call() {
    let result = build_setup_wrapper_end(Some("{ msg }"), None, false, false, ComponentWrap::Plain);
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
    let result = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        None,
        None,
        None,
        ComponentWrap::DefineComponent,
    );
    assert!(result.contains("__name: 'Test'"));
    assert!(result.contains("setup(__props) {"));
    assert!(!result.contains("async"));
}

#[test]
fn build_wrapper_start_async() {
    let result = build_setup_wrapper_start(
        "Test",
        true,
        false,
        false,
        None,
        None,
        None,
        ComponentWrap::DefineComponent,
    );
    assert!(result.contains("async setup(__props"));
}

#[test]
fn build_wrapper_start_no_name() {
    let result = build_setup_wrapper_start(
        "",
        false,
        false,
        false,
        None,
        None,
        None,
        ComponentWrap::DefineComponent,
    );
    assert!(!result.contains("__name"));
}

#[test]
fn build_wrapper_start_with_props() {
    let result = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        Some("{ title: String }"),
        None,
        None,
        ComponentWrap::DefineComponent,
    );
    assert!(result.contains("props: { title: String }"));
}

#[test]
fn build_wrapper_start_with_emits() {
    let result = build_setup_wrapper_start(
        "Test",
        false,
        false,
        true,
        None,
        Some("['click']"),
        None,
        ComponentWrap::DefineComponent,
    );
    assert!(result.contains("emits: ['click']"));
    assert!(result.contains("emit: __emit"));
}

#[test]
fn build_wrapper_start_with_expose() {
    let result = build_setup_wrapper_start(
        "Test",
        false,
        true,
        false,
        None,
        None,
        None,
        ComponentWrap::DefineComponent,
    );
    assert!(result.contains("expose: __expose"));
}

#[test]
fn build_wrapper_start_with_expose_and_emit() {
    let result = build_setup_wrapper_start(
        "Test",
        false,
        true,
        true,
        None,
        None,
        None,
        ComponentWrap::DefineComponent,
    );
    assert!(result.contains("expose: __expose, emit: __emit"));
}

#[test]
fn build_wrapper_start_with_options() {
    let result = build_setup_wrapper_start(
        "Test",
        false,
        false,
        false,
        None,
        None,
        Some("inheritAttrs: false"),
        ComponentWrap::DefineComponent,
    );
    assert!(result.contains("inheritAttrs: false"));
    // Options should come before __name
    let opts_pos = result.find("inheritAttrs").unwrap();
    let name_pos = result.find("__name").unwrap();
    assert!(opts_pos < name_pos);
}

#[test]
fn build_wrapper_end_with_return() {
    let result = build_setup_wrapper_end(
        Some("{ msg, count }"),
        None,
        false,
        false,
        ComponentWrap::DefineComponent,
    );
    assert!(result.contains("const __returned__ = { msg, count }"));
    assert!(result.contains("__isScriptSetup"));
    assert!(result.contains("return __returned__"));
    assert!(result.contains("}});"));
    assert!(result.contains("export default __sfc__"));
}

#[test]
fn build_wrapper_end_no_return() {
    let result = build_setup_wrapper_end(None, None, false, false, ComponentWrap::DefineComponent);
    assert!(!result.contains("return"));
    assert!(result.contains("}});"));
}

#[test]
fn build_wrapper_end_with_scope_id() {
    let result = build_setup_wrapper_end(
        None,
        Some("data-v-abc"),
        false,
        false,
        ComponentWrap::DefineComponent,
    );
    assert!(result.contains("__sfc__.__scopeId = \"data-v-abc\""));
}

#[test]
fn build_returned_empty() {
    let bindings = FxHashMap::default();
    assert_eq!(build_returned_object(&bindings, None), "{}");
}

#[test]
fn build_returned_setup_bindings_only() {
    let mut bindings = FxHashMap::default();
    bindings.insert("count", BindingType::SetupRef);
    bindings.insert("msg", BindingType::SetupConst);
    bindings.insert("title", BindingType::Props); // Not included
    let result = build_returned_object(&bindings, None);
    assert!(result.contains("count"));
    assert!(result.contains("msg"));
    assert!(!result.contains("title"));
}

#[test]
fn build_returned_sorted() {
    let mut bindings = FxHashMap::default();
    bindings.insert("zebra", BindingType::SetupConst);
    bindings.insert("alpha", BindingType::SetupRef);
    let result = build_returned_object(&bindings, None);
    let alpha_pos = result.find("alpha").unwrap();
    let zebra_pos = result.find("zebra").unwrap();
    assert!(alpha_pos < zebra_pos);
}

// ── SetupImport filtering in __returned__ ────────────────────

#[test]
fn build_returned_setup_import_excluded_without_template_vars() {
    // SetupImport bindings are included when no template_used_vars is provided
    // (conservative: no template → include all)
    let mut bindings = FxHashMap::default();
    bindings.insert("MyComp", BindingType::SetupImport);
    bindings.insert("msg", BindingType::SetupConst);
    let result = build_returned_object(&bindings, None);
    assert!(result.contains("MyComp"));
    assert!(result.contains("msg"));
}

#[test]
fn build_returned_setup_import_filtered_by_template_vars() {
    // SetupImport bindings are filtered: only included if in template_used_vars
    let mut bindings = FxHashMap::default();
    bindings.insert("UsedComp", BindingType::SetupImport);
    bindings.insert("UnusedImport", BindingType::SetupImport);
    bindings.insert("msg", BindingType::SetupConst);

    let mut vars = FxHashSet::default();
    vars.insert("UsedComp".to_string());

    let result = build_returned_object(&bindings, Some(&vars));
    assert!(
        result.contains("UsedComp"),
        "Used import should be in __returned__"
    );
    assert!(
        !result.contains("UnusedImport"),
        "Unused import should NOT be in __returned__"
    );
    assert!(
        result.contains("msg"),
        "SetupConst bindings are always included"
    );
}

#[test]
fn build_returned_setup_import_empty_template_vars_excludes_all() {
    // With empty template_used_vars set, no SetupImport bindings are included
    let mut bindings = FxHashMap::default();
    bindings.insert("SomeImport", BindingType::SetupImport);
    bindings.insert("msg", BindingType::SetupConst);

    let vars = FxHashSet::default();
    let result = build_returned_object(&bindings, Some(&vars));
    assert!(
        !result.contains("SomeImport"),
        "SetupImport with empty template_used_vars should be excluded"
    );
    assert!(result.contains("msg"));
}

// ── Vapor: __vapor flag ──────────────────────────────────────

#[test]
fn build_wrapper_end_vapor_adds_vapor_flag() {
    let result = build_setup_wrapper_end(
        Some("{ msg }"),
        None,
        true,
        false,
        ComponentWrap::DefineComponent,
    );
    assert!(
        result.contains("__sfc__.__vapor = true"),
        "Vapor mode should set __vapor flag, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_end_non_vapor_no_vapor_flag() {
    let result = build_setup_wrapper_end(
        Some("{ msg }"),
        None,
        false,
        false,
        ComponentWrap::DefineComponent,
    );
    assert!(
        !result.contains("__vapor"),
        "Non-vapor mode should not set __vapor flag, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_end_vapor_with_scope_id_has_both() {
    let result = build_setup_wrapper_end(
        None,
        Some("data-v-abc"),
        true,
        false,
        ComponentWrap::DefineComponent,
    );
    assert!(result.contains("__sfc__.__vapor = true"));
    assert!(result.contains("__sfc__.__scopeId = \"data-v-abc\""));
}
