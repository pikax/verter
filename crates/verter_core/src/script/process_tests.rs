use super::*;

#[test]
fn build_wrapper_start_basic() {
    let result = build_setup_wrapper_start("Test", false, false, false, None, None, None);
    assert!(result.contains("__name: 'Test'"));
    assert!(result.contains("setup(__props) {"));
    assert!(!result.contains("async"));
}

#[test]
fn build_wrapper_start_async() {
    let result = build_setup_wrapper_start("Test", true, false, false, None, None, None);
    assert!(result.contains("async setup(__props"));
}

#[test]
fn build_wrapper_start_no_name() {
    let result = build_setup_wrapper_start("", false, false, false, None, None, None);
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
    );
    assert!(result.contains("props: { title: String }"));
}

#[test]
fn build_wrapper_start_with_emits() {
    let result =
        build_setup_wrapper_start("Test", false, false, true, None, Some("['click']"), None);
    assert!(result.contains("emits: ['click']"));
    assert!(result.contains("emit: __emit"));
}

#[test]
fn build_wrapper_start_with_expose() {
    let result = build_setup_wrapper_start("Test", false, true, false, None, None, None);
    assert!(result.contains("expose: __expose"));
}

#[test]
fn build_wrapper_start_with_expose_and_emit() {
    let result = build_setup_wrapper_start("Test", false, true, true, None, None, None);
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
    );
    assert!(result.contains("inheritAttrs: false"));
    // Options should come before __name
    let opts_pos = result.find("inheritAttrs").unwrap();
    let name_pos = result.find("__name").unwrap();
    assert!(opts_pos < name_pos);
}

#[test]
fn build_wrapper_end_with_return() {
    let result = build_setup_wrapper_end(Some("{ msg, count }"), None, false);
    assert!(result.contains("const __returned__ = { msg, count }"));
    assert!(result.contains("__isScriptSetup"));
    assert!(result.contains("return __returned__"));
    assert!(result.contains("}});"));
    assert!(result.contains("export default __sfc__"));
}

#[test]
fn build_wrapper_end_no_return() {
    let result = build_setup_wrapper_end(None, None, false);
    assert!(!result.contains("return"));
    assert!(result.contains("}});"));
}

#[test]
fn build_wrapper_end_with_scope_id() {
    let result = build_setup_wrapper_end(None, Some("data-v-abc"), false);
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

// ── Vapor: __vapor flag ──────────────────────────────────────

#[test]
fn build_wrapper_end_vapor_adds_vapor_flag() {
    let result = build_setup_wrapper_end(Some("{ msg }"), None, true);
    assert!(
        result.contains("__sfc__.__vapor = true"),
        "Vapor mode should set __vapor flag, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_end_non_vapor_no_vapor_flag() {
    let result = build_setup_wrapper_end(Some("{ msg }"), None, false);
    assert!(
        !result.contains("__vapor"),
        "Non-vapor mode should not set __vapor flag, got:\n{}",
        result
    );
}

#[test]
fn build_wrapper_end_vapor_with_scope_id_has_both() {
    let result = build_setup_wrapper_end(None, Some("data-v-abc"), true);
    assert!(result.contains("__sfc__.__vapor = true"));
    assert!(result.contains("__sfc__.__scopeId = \"data-v-abc\""));
}
