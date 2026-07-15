use super::*;

// ==================== BindingType ====================

#[test]
fn reactivity_level_static_for_const() {
    assert_eq!(
        BindingType::SetupConst.reactivity_level(),
        ReactivityLevel::Static
    );
    assert_eq!(
        BindingType::LiteralConst.reactivity_level(),
        ReactivityLevel::Static
    );
}

#[test]
fn reactivity_level_dynamic_for_ref() {
    assert_eq!(
        BindingType::SetupRef.reactivity_level(),
        ReactivityLevel::Dynamic
    );
    assert_eq!(
        BindingType::SetupMaybeRef.reactivity_level(),
        ReactivityLevel::Dynamic
    );
    assert_eq!(
        BindingType::Props.reactivity_level(),
        ReactivityLevel::Dynamic
    );
}

#[test]
fn is_setup_true_for_setup_types() {
    assert!(BindingType::SetupConst.is_setup());
    assert!(BindingType::SetupLet.is_setup());
    assert!(BindingType::SetupRef.is_setup());
    assert!(BindingType::SetupReactiveConst.is_setup());
    assert!(BindingType::SetupMaybeRef.is_setup());
    assert!(BindingType::LiteralConst.is_setup());
}

#[test]
fn is_setup_false_for_non_setup() {
    assert!(!BindingType::Props.is_setup());
    assert!(!BindingType::PropsAliased.is_setup());
    assert!(!BindingType::Data.is_setup());
    assert!(!BindingType::Options.is_setup());
}

#[test]
fn is_props_correct() {
    assert!(BindingType::Props.is_props());
    assert!(BindingType::PropsAliased.is_props());
    assert!(!BindingType::SetupConst.is_props());
    assert!(!BindingType::Data.is_props());
}

#[test]
fn needs_value_access_correct() {
    assert!(BindingType::SetupRef.needs_value_access());
    assert!(BindingType::SetupMaybeRef.needs_value_access());
    assert!(!BindingType::SetupConst.needs_value_access());
    assert!(!BindingType::SetupLet.needs_value_access());
    assert!(!BindingType::Props.needs_value_access());
}

// ==================== BindingResolver ====================

fn make_resolver(
    entries: &[(&'static str, BindingType)],
    is_inline: bool,
) -> BindingResolver<'static> {
    let mut map = FxHashMap::default();
    for &(name, bt) in entries {
        map.insert(name, bt);
    }
    BindingResolver::new(map, is_inline)
}

// ---- resolve_prefix ----

#[test]
fn setup_ref_inline_prefix_is_empty() {
    let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);
    assert_eq!(resolver.resolve_prefix("count"), "");
}

#[test]
fn setup_ref_standalone_prefix_is_setup() {
    let resolver = make_resolver(&[("count", BindingType::SetupRef)], false);
    assert_eq!(resolver.resolve_prefix("count"), "$setup.");
}

#[test]
fn props_inline_prefix_is_dunder_props() {
    let resolver = make_resolver(&[("msg", BindingType::Props)], true);
    assert_eq!(resolver.resolve_prefix("msg"), "__props.");
}

#[test]
fn props_standalone_prefix_is_dollar_props() {
    let resolver = make_resolver(&[("msg", BindingType::Props)], false);
    assert_eq!(resolver.resolve_prefix("msg"), "$props.");
}

#[test]
fn props_aliased_prefix_same_as_props() {
    let resolver = make_resolver(&[("m", BindingType::PropsAliased)], true);
    assert_eq!(resolver.resolve_prefix("m"), "__props.");
}

#[test]
fn data_prefix_is_data_in_standalone() {
    let resolver = make_resolver(&[("count", BindingType::Data)], false);
    assert_eq!(resolver.resolve_prefix("count"), "$data.");
}

#[test]
fn data_prefix_is_ctx_in_inline() {
    let resolver = make_resolver(&[("count", BindingType::Data)], true);
    assert_eq!(resolver.resolve_prefix("count"), "_ctx.");
}

#[test]
fn options_prefix_is_options_in_standalone() {
    let resolver = make_resolver(&[("count", BindingType::Options)], false);
    assert_eq!(resolver.resolve_prefix("count"), "$options.");
}

#[test]
fn options_prefix_is_ctx_in_inline() {
    let resolver = make_resolver(&[("count", BindingType::Options)], true);
    assert_eq!(resolver.resolve_prefix("count"), "_ctx.");
}

#[test]
fn unknown_binding_prefix_is_ctx() {
    let resolver = make_resolver(&[], true);
    assert_eq!(resolver.resolve_prefix("unknown"), "_ctx.");
}

#[test]
fn setup_const_inline_prefix_is_empty() {
    let resolver = make_resolver(&[("fn", BindingType::SetupConst)], true);
    assert_eq!(resolver.resolve_prefix("fn"), "");
}

#[test]
fn setup_const_standalone_prefix_is_setup() {
    let resolver = make_resolver(&[("fn", BindingType::SetupConst)], false);
    assert_eq!(resolver.resolve_prefix("fn"), "$setup.");
}

// ---- resolve_suffix ----

#[test]
fn setup_ref_inline_suffix_is_value() {
    let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);
    assert_eq!(resolver.resolve_suffix("count"), ".value");
}

#[test]
fn setup_maybe_ref_inline_suffix_is_value() {
    let resolver = make_resolver(&[("data", BindingType::SetupMaybeRef)], true);
    assert_eq!(resolver.resolve_suffix("data"), ".value");
}

#[test]
fn setup_ref_standalone_suffix_is_empty() {
    let resolver = make_resolver(&[("count", BindingType::SetupRef)], false);
    assert_eq!(resolver.resolve_suffix("count"), "");
}

#[test]
fn setup_const_inline_suffix_is_empty() {
    let resolver = make_resolver(&[("fn", BindingType::SetupConst)], true);
    assert_eq!(resolver.resolve_suffix("fn"), "");
}

#[test]
fn props_inline_suffix_is_empty() {
    let resolver = make_resolver(&[("msg", BindingType::Props)], true);
    assert_eq!(resolver.resolve_suffix("msg"), "");
}

#[test]
fn unknown_binding_suffix_is_empty() {
    let resolver = make_resolver(&[], true);
    assert_eq!(resolver.resolve_suffix("unknown"), "");
}

// ---- collect_binding_patches ----

#[test]
fn collect_patches_setup_ref_inline_adds_value_suffix() {
    let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);
    let alloc = oxc_allocator::Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);

    // Simulate a binding extracted by OXC: "count" at pos 10, len 5
    let bindings = BindingExtractionResult {
        bindings: vec![crate::utils::oxc::Binding {
            name: "count",
            span: crate::common::RelativeSpan::new(10, 15),
            pos: 10,
            ignore: false,
            is_shorthand: false,
        }],
        functions: vec![],
        literals: vec![],
        has_errors: false,
        dynamism: crate::utils::oxc::Dynamism::MaybeDynamic,
    };

    resolver.collect_binding_patches(&bindings, &mut out);

    // SetupRef inline: prefix="" (empty, not pushed), suffix=".value" at pos 15
    assert_eq!(out.prepends.len(), 1);
    assert_eq!(out.prepends[0].0, 15); // pos 10 + len 5
    assert_eq!(out.prepends[0].1, ".value");
}

#[test]
fn collect_patches_props_inline_adds_prefix() {
    let resolver = make_resolver(&[("msg", BindingType::Props)], true);
    let alloc = oxc_allocator::Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);

    let bindings = BindingExtractionResult {
        bindings: vec![crate::utils::oxc::Binding {
            name: "msg",
            span: crate::common::RelativeSpan::new(5, 8),
            pos: 5,
            ignore: false,
            is_shorthand: false,
        }],
        functions: vec![],
        literals: vec![],
        has_errors: false,
        dynamism: crate::utils::oxc::Dynamism::MaybeDynamic,
    };

    resolver.collect_binding_patches(&bindings, &mut out);

    // Props inline: prefix="__props." at pos 5, suffix="" (not pushed)
    assert_eq!(out.prepends.len(), 1);
    assert_eq!(out.prepends[0].0, 5);
    assert_eq!(out.prepends[0].1, "__props.");
}

#[test]
fn collect_patches_ignored_binding_skipped() {
    let resolver = make_resolver(&[("item", BindingType::SetupRef)], true);
    let alloc = oxc_allocator::Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);

    let bindings = BindingExtractionResult {
        bindings: vec![crate::utils::oxc::Binding {
            name: "item",
            span: crate::common::RelativeSpan::new(0, 4),
            pos: 0,
            ignore: true, // v-for local
            is_shorthand: false,
        }],
        functions: vec![],
        literals: vec![],
        has_errors: false,
        dynamism: crate::utils::oxc::Dynamism::Dynamic,
    };

    resolver.collect_binding_patches(&bindings, &mut out);

    // Ignored bindings produce no patches
    assert!(out.prepends.is_empty());
}

#[test]
fn collect_patches_unresolved_adds_ctx_prefix() {
    let resolver = make_resolver(&[], false); // standalone, no bindings registered
    let alloc = oxc_allocator::Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);

    let bindings = BindingExtractionResult {
        bindings: vec![crate::utils::oxc::Binding {
            name: "foo",
            span: crate::common::RelativeSpan::new(0, 3),
            pos: 0,
            ignore: false,
            is_shorthand: false,
        }],
        functions: vec![],
        literals: vec![],
        has_errors: false,
        dynamism: crate::utils::oxc::Dynamism::MaybeDynamic,
    };

    resolver.collect_binding_patches(&bindings, &mut out);

    // Unresolved: prefix="_ctx." at pos 0, suffix="" (not pushed)
    assert_eq!(out.prepends.len(), 1);
    assert_eq!(out.prepends[0].0, 0);
    assert_eq!(out.prepends[0].1, "_ctx.");
}

#[test]
fn collect_patches_multiple_bindings() {
    let resolver = make_resolver(
        &[
            ("count", BindingType::SetupRef),
            ("msg", BindingType::Props),
        ],
        true,
    );
    let alloc = oxc_allocator::Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);

    let bindings = BindingExtractionResult {
        bindings: vec![
            crate::utils::oxc::Binding {
                name: "count",
                span: crate::common::RelativeSpan::new(0, 5),
                pos: 0,
                ignore: false,
                is_shorthand: false,
            },
            crate::utils::oxc::Binding {
                name: "msg",
                span: crate::common::RelativeSpan::new(8, 11),
                pos: 8,
                ignore: false,
                is_shorthand: false,
            },
        ],
        functions: vec![],
        literals: vec![],
        has_errors: false,
        dynamism: crate::utils::oxc::Dynamism::MaybeDynamic,
    };

    resolver.collect_binding_patches(&bindings, &mut out);

    // count: SetupRef inline → suffix ".value" at pos 5
    // msg: Props inline → prefix "__props." at pos 8
    assert_eq!(out.prepends.len(), 2);
    assert_eq!(out.prepends[0], (5, ".value"));
    assert_eq!(out.prepends[1], (8, "__props."));
}

// ==================== is_simple_ident ====================

#[test]
fn is_simple_ident_basic() {
    assert!(is_simple_ident("foo"));
    assert!(is_simple_ident("_bar"));
    assert!(is_simple_ident("$baz"));
    assert!(is_simple_ident("count123"));
    assert!(!is_simple_ident(""));
    assert!(!is_simple_ident("123abc"));
    assert!(!is_simple_ident("foo.bar"));
    assert!(!is_simple_ident("a + b"));
    assert!(!is_simple_ident("foo[0]"));
}

// ==================== resolve_simple_expr ====================

#[test]
fn resolve_simple_expr_setup_ref_standalone() {
    let resolver = make_resolver(&[("count", BindingType::SetupRef)], false);
    assert_eq!(resolver.resolve_simple_expr("count"), "$setup.count");
}

#[test]
fn resolve_simple_expr_setup_ref_inline() {
    let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);
    assert_eq!(resolver.resolve_simple_expr("count"), "count.value");
}

#[test]
fn resolve_simple_expr_props_inline() {
    let resolver = make_resolver(&[("msg", BindingType::Props)], true);
    assert_eq!(resolver.resolve_simple_expr("msg"), "__props.msg");
}

#[test]
fn resolve_simple_expr_unresolved() {
    let resolver = make_resolver(&[], false);
    assert_eq!(resolver.resolve_simple_expr("foo"), "_ctx.foo");
}

#[test]
fn resolve_simple_expr_compound_passthrough() {
    let resolver = make_resolver(&[("a", BindingType::SetupRef)], true);
    assert_eq!(resolver.resolve_simple_expr("a + b"), "a + b");
}

#[test]
fn resolve_simple_expr_trims_whitespace() {
    let resolver = make_resolver(&[("foo", BindingType::SetupConst)], false);
    assert_eq!(resolver.resolve_simple_expr("  foo  "), "$setup.foo");
}

// ==================== Vapor mode bindings ====================

fn make_vapor_resolver(entries: &[(&'static str, BindingType)]) -> BindingResolver<'static> {
    let mut map = FxHashMap::default();
    for &(name, bt) in entries {
        map.insert(name, bt);
    }
    let mut r = BindingResolver::new(map, false);
    r.set_vapor(true);
    r
}

#[test]
fn vapor_setup_ref_prefix_is_ctx() {
    let resolver = make_vapor_resolver(&[("count", BindingType::SetupRef)]);
    assert_eq!(resolver.resolve_prefix("count"), "_ctx.");
}

#[test]
fn vapor_setup_const_prefix_is_ctx() {
    let resolver = make_vapor_resolver(&[("fn", BindingType::SetupConst)]);
    assert_eq!(resolver.resolve_prefix("fn"), "_ctx.");
}

#[test]
fn vapor_setup_let_prefix_is_ctx() {
    let resolver = make_vapor_resolver(&[("x", BindingType::SetupLet)]);
    assert_eq!(resolver.resolve_prefix("x"), "_ctx.");
}

#[test]
fn vapor_props_prefix_is_ctx() {
    let resolver = make_vapor_resolver(&[("msg", BindingType::Props)]);
    assert_eq!(resolver.resolve_prefix("msg"), "_ctx.");
}

#[test]
fn vapor_unresolved_prefix_is_ctx() {
    let resolver = make_vapor_resolver(&[]);
    assert_eq!(resolver.resolve_prefix("unknown"), "_ctx.");
}

#[test]
fn vapor_suffix_is_always_empty() {
    let resolver = make_vapor_resolver(&[("count", BindingType::SetupRef)]);
    assert_eq!(resolver.resolve_suffix("count"), "");
}

#[test]
fn vapor_resolve_simple_expr_uses_ctx() {
    let resolver = make_vapor_resolver(&[("msg", BindingType::SetupRef)]);
    assert_eq!(resolver.resolve_simple_expr("msg"), "_ctx.msg");
}

#[test]
fn vapor_resolve_simple_expr_props_uses_ctx() {
    let resolver = make_vapor_resolver(&[("title", BindingType::Props)]);
    assert_eq!(resolver.resolve_simple_expr("title"), "_ctx.title");
}

// ==================== Reserved word bindings ====================

#[test]
fn resolve_simple_expr_keyword_prop_uses_bracket_notation() {
    let resolver = make_resolver(&[("class", BindingType::Props)], false);
    assert_eq!(resolver.resolve_simple_expr("class"), r#"$props["class"]"#);
}

#[test]
fn resolve_simple_expr_keyword_prop_inline_uses_bracket_notation() {
    let resolver = make_resolver(&[("class", BindingType::Props)], true);
    assert_eq!(resolver.resolve_simple_expr("class"), r#"__props["class"]"#);
}

#[test]
fn resolve_simple_expr_keyword_not_in_bindings_unchanged() {
    let resolver = make_resolver(&[], false);
    assert_eq!(resolver.resolve_simple_expr("class"), "class");
}

#[test]
fn resolve_simple_expr_keyword_for_as_prop() {
    let resolver = make_resolver(&[("for", BindingType::Props)], false);
    assert_eq!(resolver.resolve_simple_expr("for"), r#"$props["for"]"#);
}

#[test]
fn vapor_resolve_simple_expr_keyword_prop_uses_bracket_notation() {
    let resolver = make_vapor_resolver(&[("class", BindingType::Props)]);
    assert_eq!(resolver.resolve_simple_expr("class"), r#"_ctx["class"]"#);
}

// ==================== TSX mode bindings ====================

fn make_tsx_resolver(entries: &[(&'static str, BindingType)]) -> BindingResolver<'static> {
    let mut map = FxHashMap::default();
    for &(name, bt) in entries {
        map.insert(name, bt);
    }
    let mut r = BindingResolver::new(map, true);
    r.set_tsx(true);
    r
}

#[test]
fn tsx_unresolved_binding_prefix_is_instance() {
    let resolver = make_tsx_resolver(&[]);
    assert_eq!(resolver.resolve_prefix("$emit"), "___VERTER___instance.");
}

#[test]
fn tsx_unresolved_dollar_slots_prefix_is_instance() {
    let resolver = make_tsx_resolver(&[]);
    assert_eq!(resolver.resolve_prefix("$slots"), "___VERTER___instance.");
}

#[test]
fn tsx_known_binding_prefix_is_empty() {
    let resolver = make_tsx_resolver(&[("count", BindingType::SetupRef)]);
    assert_eq!(resolver.resolve_prefix("count"), "");
}

#[test]
fn tsx_props_binding_prefix_is_dunder_props() {
    let resolver = make_tsx_resolver(&[("msg", BindingType::Props)]);
    assert_eq!(resolver.resolve_prefix("msg"), "__props.");
}

#[test]
fn tsx_destructured_prop_binding_is_the_real_local() {
    let resolver = make_tsx_resolver(&[("msg", BindingType::PropsDestructured)]);
    assert_eq!(resolver.resolve_prefix("msg"), "");
    assert_eq!(resolver.resolve_simple_expr("msg"), "msg");
    assert!(BindingType::PropsDestructured.is_props());
}

#[test]
fn tsx_unresolved_resolve_simple_expr() {
    let resolver = make_tsx_resolver(&[]);
    assert_eq!(
        resolver.resolve_simple_expr("$emit"),
        "___VERTER___instance.$emit"
    );
}

#[test]
fn tsx_global_stays_bare() {
    let resolver = make_tsx_resolver(&[]);
    assert_eq!(resolver.resolve_prefix("Math"), "");
    assert_eq!(resolver.resolve_prefix("console"), "");
    assert_eq!(resolver.resolve_prefix("undefined"), "");
    assert_eq!(resolver.resolve_prefix("parseInt"), "");
}

#[test]
fn tsx_known_binding_resolve_simple_expr_bare() {
    let resolver = make_tsx_resolver(&[("count", BindingType::SetupConst)]);
    assert_eq!(resolver.resolve_simple_expr("count"), "count");
}

// ==================== $event special variable (#48) ====================

#[test]
fn tsx_dollar_event_prefix_is_empty() {
    // $event is a Vue template special variable, must NOT get ___VERTER___instance. prefix
    let resolver = make_tsx_resolver(&[]);
    assert_eq!(resolver.resolve_prefix("$event"), "");
}

#[test]
fn tsx_dollar_event_resolve_simple_expr_bare() {
    let resolver = make_tsx_resolver(&[]);
    assert_eq!(resolver.resolve_simple_expr("$event"), "$event");
}

#[test]
fn vdom_dollar_event_prefix_is_empty() {
    // In VDOM mode, $event should also not get _ctx. prefix
    let resolver = make_resolver(&[], true);
    assert_eq!(resolver.resolve_prefix("$event"), "");
}

// ==================== Data/Options TSX prefix ====================

#[test]
fn tsx_data_binding_prefix_is_instance() {
    let resolver = make_tsx_resolver(&[("count", BindingType::Data)]);
    assert_eq!(resolver.resolve_prefix("count"), "___VERTER___instance.");
}

#[test]
fn tsx_options_binding_prefix_is_instance() {
    let resolver = make_tsx_resolver(&[("total", BindingType::Options)]);
    assert_eq!(resolver.resolve_prefix("total"), "___VERTER___instance.");
}

#[test]
fn tsx_setup_const_binding_stays_bare() {
    let resolver = make_tsx_resolver(&[("x", BindingType::SetupConst)]);
    assert_eq!(resolver.resolve_prefix("x"), "");
}

// ==================== Const prop overrides ====================

fn make_resolver_with_const_props(
    entries: &[(&'static str, BindingType)],
    is_inline: bool,
    const_props: &[&'static str],
) -> BindingResolver<'static> {
    let mut map = FxHashMap::default();
    for &(name, bt) in entries {
        map.insert(name, bt);
    }
    let const_set: rustc_hash::FxHashSet<&str> = const_props.iter().copied().collect();
    BindingResolver::new_with_const_props(map, is_inline, Some(const_set))
}

/// @ai-generated - Const prop still uses $props. prefix (not $setup.)
#[test]
fn const_prop_still_uses_props_prefix() {
    let resolver = make_resolver_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
    assert_eq!(resolver.resolve_prefix("msg"), "$props.");
}

/// @ai-generated - Const prop still uses __props. prefix in inline mode
#[test]
fn const_prop_still_uses_dunder_props_prefix_inline() {
    let resolver = make_resolver_with_const_props(&[("msg", BindingType::Props)], true, &["msg"]);
    assert_eq!(resolver.resolve_prefix("msg"), "__props.");
}

/// @ai-generated - Const prop has no .value suffix
#[test]
fn const_prop_still_no_value_suffix() {
    let resolver = make_resolver_with_const_props(&[("msg", BindingType::Props)], true, &["msg"]);
    assert_eq!(resolver.resolve_suffix("msg"), "");
}

// ==================== all_bindings_const_props ====================

use crate::utils::oxc::BindingExtractionResult;

fn make_bindings_result<'a>(names: &[(&'a str, bool)]) -> BindingExtractionResult<'a> {
    use crate::common::RelativeSpan;
    use crate::utils::oxc::bindings::Binding;
    BindingExtractionResult {
        bindings: names
            .iter()
            .map(|&(name, ignore)| Binding {
                name,
                span: RelativeSpan::new(0, name.len() as u32),
                pos: 0,
                ignore,
                is_shorthand: false,
            })
            .collect(),
        functions: vec![],
        literals: vec![],
        has_errors: false,
        dynamism: crate::utils::oxc::Dynamism::MaybeDynamic,
    }
}

/// Without const_props data, always returns false (Vue compatibility)
#[test]
fn all_const_props_no_data_returns_false() {
    let resolver = make_resolver(&[("msg", BindingType::Props)], false);
    let bindings = make_bindings_result(&[("msg", false)]);
    assert!(!resolver.all_bindings_const_props(Some(&bindings)));
}

/// With const_props, a const prop expression returns true
#[test]
fn all_const_props_with_const_prop_returns_true() {
    let resolver = make_resolver_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
    let bindings = make_bindings_result(&[("msg", false)]);
    assert!(resolver.all_bindings_const_props(Some(&bindings)));
}

/// Non-const prop expression returns false
#[test]
fn all_const_props_with_non_const_prop_returns_false() {
    let resolver = make_resolver_with_const_props(
        &[("msg", BindingType::Props), ("count", BindingType::Props)],
        false,
        &["msg"], // only msg is const
    );
    let bindings = make_bindings_result(&[("count", false)]);
    assert!(!resolver.all_bindings_const_props(Some(&bindings)));
}

/// Mixed const prop + setup const returns true
#[test]
fn all_const_props_mixed_const_prop_and_setup_const() {
    let resolver = make_resolver_with_const_props(
        &[
            ("msg", BindingType::Props),
            ("LABEL", BindingType::SetupConst),
        ],
        false,
        &["msg"],
    );
    let bindings = make_bindings_result(&[("msg", false), ("LABEL", false)]);
    assert!(resolver.all_bindings_const_props(Some(&bindings)));
}

/// Mixed const prop + reactive ref returns false
#[test]
fn all_const_props_mixed_const_prop_and_ref_returns_false() {
    let resolver = make_resolver_with_const_props(
        &[
            ("msg", BindingType::Props),
            ("count", BindingType::SetupRef),
        ],
        false,
        &["msg"],
    );
    let bindings = make_bindings_result(&[("msg", false), ("count", false)]);
    assert!(!resolver.all_bindings_const_props(Some(&bindings)));
}

/// No bindings data returns false (conservative)
#[test]
fn all_const_props_none_bindings_returns_false() {
    let resolver = make_resolver_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
    assert!(!resolver.all_bindings_const_props(None));
}

/// Empty non-ignored names returns false (literal expression — Vue compat)
#[test]
fn all_const_props_pure_literal_returns_false() {
    let resolver = make_resolver_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
    // All identifiers are ignored (v-for locals, etc.)
    let bindings = make_bindings_result(&[("item", true)]);
    assert!(!resolver.all_bindings_const_props(Some(&bindings)));
}
