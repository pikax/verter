use super::*;

// The prefix and suffix table is `VueBindingView`'s (tested beside it); these
// tests cover what the wrapper adds: the legacy emitters' write-safe access,
// patch collection and the TSX table.

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

// ---- legacy access: every position is write-safe ----

#[test]
fn setup_maybe_ref_inline_suffix_is_value() {
    // The legacy emitters reuse one resolved text as both the value and the
    // assignment target of `v-model`, so a maybe-ref resolves through `.value`.
    let resolver = make_resolver(&[("data", BindingType::SetupMaybeRef)], true);
    assert_eq!(resolver.resolve_prefix("data"), "");
    assert_eq!(resolver.resolve_suffix("data"), ".value");
    assert_eq!(resolver.resolve_simple_expr("data"), "data.value");
}

#[test]
fn setup_ref_inline_asset_keeps_the_identifier_form() {
    let resolver = make_resolver(&[("Comp", BindingType::SetupRef)], true);
    assert_eq!(resolver.resolve_asset_ref("Comp"), "Comp.value");
}

#[test]
fn vapor_mode_routes_through_the_fine_grained_table() {
    let mut resolver = make_resolver(
        &[("msg", BindingType::Props), ("n", BindingType::SetupRef)],
        false,
    );
    resolver.set_vapor(true);
    assert_eq!(resolver.resolve_simple_expr("msg"), "$props.msg");
    assert_eq!(resolver.resolve_simple_expr("n"), "_ctx.n");
}

// ---- collect_binding_patches ----

#[test]
fn collect_patches_setup_ref_inline_adds_value_suffix() {
    let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);
    let alloc = oxc_allocator::Allocator::default();
    let mut out = EditRecorder::new(&alloc);

    // Simulate a binding extracted by OXC: "count" at pos 10, len 5
    let bindings = BindingExtractionResult {
        bindings: vec![crate::utils::oxc::Binding {
            name: "count",
            span: crate::common::RelativeSpan::new(10, 15),
            pos: 10,
            ignore: false,
            is_expression_local: false,
            is_shorthand: false,
            usage: crate::utils::oxc::ReferenceUse::Read,
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
    let mut out = EditRecorder::new(&alloc);

    let bindings = BindingExtractionResult {
        bindings: vec![crate::utils::oxc::Binding {
            name: "msg",
            span: crate::common::RelativeSpan::new(5, 8),
            pos: 5,
            ignore: false,
            is_expression_local: false,
            is_shorthand: false,
            usage: crate::utils::oxc::ReferenceUse::Read,
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
    let mut out = EditRecorder::new(&alloc);

    let bindings = BindingExtractionResult {
        bindings: vec![crate::utils::oxc::Binding {
            name: "item",
            span: crate::common::RelativeSpan::new(0, 4),
            pos: 0,
            ignore: true, // v-for local
            is_expression_local: false,
            is_shorthand: false,
            usage: crate::utils::oxc::ReferenceUse::Read,
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
    let mut out = EditRecorder::new(&alloc);

    let bindings = BindingExtractionResult {
        bindings: vec![crate::utils::oxc::Binding {
            name: "foo",
            span: crate::common::RelativeSpan::new(0, 3),
            pos: 0,
            ignore: false,
            is_expression_local: false,
            is_shorthand: false,
            usage: crate::utils::oxc::ReferenceUse::Read,
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
    let mut out = EditRecorder::new(&alloc);

    let bindings = BindingExtractionResult {
        bindings: vec![
            crate::utils::oxc::Binding {
                name: "count",
                span: crate::common::RelativeSpan::new(0, 5),
                pos: 0,
                ignore: false,
                is_expression_local: false,
                is_shorthand: false,
                usage: crate::utils::oxc::ReferenceUse::Read,
            },
            crate::utils::oxc::Binding {
                name: "msg",
                span: crate::common::RelativeSpan::new(8, 11),
                pos: 8,
                ignore: false,
                is_expression_local: false,
                is_shorthand: false,
                usage: crate::utils::oxc::ReferenceUse::Read,
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
