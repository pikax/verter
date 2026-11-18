use super::*;
use crate::common::RelativeSpan;
use crate::framework::vue::semantic::{FrameworkApi, OriginEvidence};
use crate::utils::oxc::bindings::Binding;
use crate::utils::oxc::Dynamism;

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

// ==================== The table ====================

#[test]
fn a_reported_table_keeps_the_reported_class_with_no_proven_origin() {
    let table = VueBindingTable::from_reported([("count", BindingType::SetupMaybeRef)]);
    assert_eq!(
        table.get("count"),
        Some(VueBindingFact {
            binding: BindingType::SetupMaybeRef,
            reported: BindingType::SetupMaybeRef,
            callee_origin: FrameworkOrigin::Unknown,
        })
    );
    assert_eq!(table.binding("missing"), None);
    assert_eq!(table.len(), 1);
}

// ==================== The view ====================

fn table_of(entries: &[(&'static str, BindingType)]) -> VueBindingTable<'static> {
    VueBindingTable::from_reported(entries.iter().copied())
}

fn view(entries: &[(&'static str, BindingType)], inline: bool) -> VueBindingView<'static> {
    VueBindingView::owning(table_of(entries), inline, None)
}

fn fine_grained_view(entries: &[(&'static str, BindingType)]) -> VueBindingView<'static> {
    let mut view = view(entries, false);
    view.set_fine_grained(true);
    view
}

fn view_with_const_props(
    entries: &[(&'static str, BindingType)],
    inline: bool,
    const_props: &[&'static str],
) -> VueBindingView<'static> {
    VueBindingView::owning(
        table_of(entries),
        inline,
        Some(const_props.iter().copied().collect()),
    )
}

// ---- resolve_prefix ----

#[test]
fn setup_ref_inline_prefix_is_empty() {
    assert_eq!(
        view(&[("count", BindingType::SetupRef)], true).resolve_prefix("count"),
        ""
    );
}

#[test]
fn setup_ref_standalone_prefix_is_setup() {
    assert_eq!(
        view(&[("count", BindingType::SetupRef)], false).resolve_prefix("count"),
        "$setup."
    );
}

// The view carries NO server-specific mode: non-inline server output routes
// through the same table as the non-inline client tree
// (`setup_ref_standalone_*`, `props_standalone_*`). Vue's `processExpression`
// prefixes identifiers from `context.inline` only, never from an SSR flag;
// the `ssrRender` function declares the matching `$props`/`$setup`/`$data`/
// `$options` parameters instead.

#[test]
fn props_inline_prefix_is_dunder_props() {
    assert_eq!(
        view(&[("msg", BindingType::Props)], true).resolve_prefix("msg"),
        "__props."
    );
}

#[test]
fn props_standalone_prefix_is_dollar_props() {
    assert_eq!(
        view(&[("msg", BindingType::Props)], false).resolve_prefix("msg"),
        "$props."
    );
}

#[test]
fn props_aliased_prefix_same_as_props() {
    assert_eq!(
        view(&[("m", BindingType::PropsAliased)], true).resolve_prefix("m"),
        "__props."
    );
}

#[test]
fn data_prefix_is_data_in_standalone() {
    assert_eq!(
        view(&[("count", BindingType::Data)], false).resolve_prefix("count"),
        "$data."
    );
}

#[test]
fn data_prefix_is_ctx_in_inline() {
    assert_eq!(
        view(&[("count", BindingType::Data)], true).resolve_prefix("count"),
        "_ctx."
    );
}

#[test]
fn options_prefix_is_options_in_standalone() {
    assert_eq!(
        view(&[("count", BindingType::Options)], false).resolve_prefix("count"),
        "$options."
    );
}

#[test]
fn options_prefix_is_ctx_in_inline() {
    assert_eq!(
        view(&[("count", BindingType::Options)], true).resolve_prefix("count"),
        "_ctx."
    );
}

#[test]
fn unknown_binding_prefix_is_ctx() {
    assert_eq!(view(&[], true).resolve_prefix("unknown"), "_ctx.");
}

#[test]
fn setup_const_inline_prefix_is_empty() {
    assert_eq!(
        view(&[("fn", BindingType::SetupConst)], true).resolve_prefix("fn"),
        ""
    );
}

#[test]
fn setup_const_standalone_prefix_is_setup() {
    assert_eq!(
        view(&[("fn", BindingType::SetupConst)], false).resolve_prefix("fn"),
        "$setup."
    );
}

#[test]
fn dollar_event_prefix_is_empty() {
    // `$event` is the handler's parameter, not a context property.
    assert_eq!(view(&[], true).resolve_prefix("$event"), "");
}

#[test]
fn inline_attrs_and_slots_are_the_setup_context_destructure() {
    let view = view(&[], true);
    assert_eq!(view.resolve_prefix("$attrs"), "");
    assert_eq!(view.resolve_prefix("$slots"), "");
}

// ---- resolve_suffix ----

#[test]
fn setup_ref_inline_suffix_is_value() {
    assert_eq!(
        view(&[("count", BindingType::SetupRef)], true).resolve_suffix("count"),
        ".value"
    );
}

#[test]
fn setup_ref_standalone_suffix_is_empty() {
    assert_eq!(
        view(&[("count", BindingType::SetupRef)], false).resolve_suffix("count"),
        ""
    );
}

#[test]
fn setup_const_inline_suffix_is_empty() {
    assert_eq!(
        view(&[("fn", BindingType::SetupConst)], true).resolve_suffix("fn"),
        ""
    );
}

#[test]
fn props_inline_suffix_is_empty() {
    assert_eq!(
        view(&[("msg", BindingType::Props)], true).resolve_suffix("msg"),
        ""
    );
}

#[test]
fn unknown_binding_suffix_is_empty() {
    assert_eq!(view(&[], true).resolve_suffix("unknown"), "");
}

// ---- maybe-refs: read through `unref`, written through `.value` ----

#[test]
fn inline_maybe_ref_read_goes_through_unref() {
    // Vue: `<p>{{ data }}</p>` over `const data = useData()` →
    // `_toDisplayString(_unref(data))`.
    let view = view(&[("data", BindingType::SetupMaybeRef)], true);
    assert_eq!(view.resolve_prefix("data"), UNREF_PREFIX);
    assert_eq!(view.resolve_suffix("data"), ")");
    assert_eq!(view.resolve_simple_expr("data"), "_unref(data)");
}

#[test]
fn inline_let_read_goes_through_unref() {
    // Vue reads a `let` binding through `unref`: it may have been assigned
    // a ref.
    let view = view(&[("n", BindingType::SetupLet)], true);
    assert_eq!(view.resolve_simple_expr("n"), "_unref(n)");
}

#[test]
fn inline_maybe_ref_write_goes_through_value() {
    // Vue: `@click="data = 1"` → `data.value = 1`.
    let view = view(&[("data", BindingType::SetupMaybeRef)], true);
    assert_eq!(view.prefix("data", BindingAccess::Write), "");
    assert_eq!(view.suffix("data", BindingAccess::Write), ".value");
    assert_eq!(view.simple_expr("data", BindingAccess::Write), "data.value");
}

#[test]
fn inline_let_write_is_bare() {
    let view = view(&[("n", BindingType::SetupLet)], true);
    assert_eq!(view.simple_expr("n", BindingAccess::Write), "n");
}

#[test]
fn non_inline_maybe_ref_is_read_through_setup() {
    // Non-inline, `$setup` is the proxy that unwraps refs.
    let view = view(&[("data", BindingType::SetupMaybeRef)], false);
    assert_eq!(view.resolve_simple_expr("data"), "$setup.data");
    assert_eq!(
        view.simple_expr("data", BindingAccess::Write),
        "$setup.data"
    );
}

#[test]
fn a_ref_reads_and_writes_through_value() {
    let view = view(&[("count", BindingType::SetupRef)], true);
    assert_eq!(
        view.simple_expr("count", BindingAccess::Read),
        "count.value"
    );
    assert_eq!(
        view.simple_expr("count", BindingAccess::Write),
        "count.value"
    );
}

/// Each class a setup binding can have, read, constructed and written
/// inline, against Vue's `compileScript({ inlineTemplate: true })` output
/// (`@vue/compiler-sfc` 3.6.0-rc.5): `_unref(fmt)(_unref(count))`,
/// `new (_unref(Klass))()`, `fmt.value = 3`, `[fmt.value, count] = …`,
/// `r.value`.
#[test]
fn inline_setup_bindings_by_class_and_access() {
    use BindingAccess::{Construct, Read, Write};
    let view = view(
        &[
            ("maybe", BindingType::SetupMaybeRef),
            ("n", BindingType::SetupLet),
            ("r", BindingType::SetupRef),
            ("c", BindingType::SetupConst),
        ],
        true,
    );
    let rows: &[(&str, BindingAccess, &str)] = &[
        ("maybe", Read, "_unref(maybe)"),
        ("maybe", Construct, "(_unref(maybe))"),
        ("maybe", Write, "maybe.value"),
        ("n", Read, "_unref(n)"),
        ("n", Construct, "(_unref(n))"),
        ("n", Write, "n"),
        ("r", Read, "r.value"),
        ("r", Construct, "r.value"),
        ("r", Write, "r.value"),
        ("c", Read, "c"),
        ("c", Construct, "c"),
        ("c", Write, "c"),
    ];
    for &(name, access, expected) in rows {
        let text = format!(
            "{}{name}{}",
            view.prefix(name, access),
            view.suffix(name, access)
        );
        assert_eq!(text, expected, "{name} {access:?}");
    }
}

/// The view records that it read through `_unref`, so the target imports it
/// exactly when its text calls it: never for writes, refs or constants.
#[test]
fn the_view_records_an_unref_read() {
    let entries = [
        ("maybe", BindingType::SetupMaybeRef),
        ("r", BindingType::SetupRef),
        ("Comp", BindingType::SetupMaybeRef),
    ];
    let written = view(&entries, true);
    written.prefix("maybe", BindingAccess::Write);
    written.prefix("r", BindingAccess::Read);
    assert!(!written.uses_unref());

    let read = view(&entries, true);
    read.prefix("maybe", BindingAccess::Read);
    assert!(read.uses_unref());

    let constructed = view(&entries, true);
    constructed.prefix("maybe", BindingAccess::Construct);
    assert!(constructed.uses_unref());

    let asset = view(&entries, true);
    asset.resolve_asset_ref("Comp");
    assert!(asset.uses_unref());

    let separate = view(&entries, false);
    separate.prefix("maybe", BindingAccess::Read);
    separate.resolve_asset_ref("Comp");
    assert!(!separate.uses_unref());
}

// ---- asset references ----

#[test]
fn inline_asset_that_may_be_a_ref_goes_through_unref() {
    // Vue's `resolveSetupReference`: `setup-let`, `setup-ref` and
    // `setup-maybe-ref` assets are `_unref(Name)` inline.
    let view = view(
        &[
            ("Dyn", BindingType::SetupMaybeRef),
            ("Held", BindingType::SetupRef),
            ("Child", BindingType::SetupConst),
        ],
        true,
    );
    assert_eq!(view.resolve_asset_ref("Dyn"), "_unref(Dyn)");
    assert_eq!(view.resolve_asset_ref("Held"), "_unref(Held)");
    assert_eq!(view.resolve_asset_ref("Child"), "Child");
}

#[test]
fn non_inline_setup_asset_uses_computed_access() {
    let view = view(&[("Child", BindingType::SetupConst)], false);
    assert_eq!(view.resolve_asset_ref("Child"), r#"$setup["Child"]"#);
}

#[test]
fn written_asset_keeps_the_identifier_form() {
    let view = view(&[("Held", BindingType::SetupRef)], true);
    assert_eq!(view.asset_ref("Held", BindingAccess::Write), "Held.value");
}

// ---- template scopes ----

#[test]
fn a_template_scope_shadows_a_binding() {
    let mut scopes = LexicalScopes::new();
    let item_scope = scopes.push(LexicalScopeId::ROOT, ["item"]);
    let table = table_of(&[("item", BindingType::SetupMaybeRef)]);
    let mut view = VueBindingView::new(&table, true, false, None, FxHashSet::default())
        .with_template_scopes(&scopes);

    assert_eq!(view.resolve_simple_expr("item"), "_unref(item)");
    view.push_local_scope(item_scope);
    assert!(view.resolve_local("item"));
    assert_eq!(view.resolve_prefix("item"), "");
    assert_eq!(view.resolve_suffix("item"), "");
    assert_eq!(view.resolve_simple_expr("item"), "item");
    view.pop_local_scope();
    assert!(!view.resolve_local("item"));
    assert_eq!(view.resolve_simple_expr("item"), "_unref(item)");
}

#[test]
fn a_nested_template_scope_sees_its_parents_names() {
    let mut scopes = LexicalScopes::new();
    let outer = scopes.push(LexicalScopeId::ROOT, ["row"]);
    let inner = scopes.push(outer, ["cell"]);
    let table = table_of(&[("row", BindingType::Props)]);
    let mut view = VueBindingView::new(&table, false, false, None, FxHashSet::default())
        .with_template_scopes(&scopes);

    view.push_local_scope(inner);
    assert!(view.resolve_local("row"));
    assert!(view.resolve_local("cell"));
    assert_eq!(view.resolve_prefix("row"), "");
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
    assert_eq!(
        view(&[("count", BindingType::SetupRef)], false).resolve_simple_expr("count"),
        "$setup.count"
    );
}

#[test]
fn resolve_simple_expr_setup_ref_inline() {
    assert_eq!(
        view(&[("count", BindingType::SetupRef)], true).resolve_simple_expr("count"),
        "count.value"
    );
}

#[test]
fn resolve_simple_expr_props_inline() {
    assert_eq!(
        view(&[("msg", BindingType::Props)], true).resolve_simple_expr("msg"),
        "__props.msg"
    );
}

#[test]
fn resolve_simple_expr_unresolved() {
    assert_eq!(view(&[], false).resolve_simple_expr("foo"), "_ctx.foo");
}

#[test]
fn resolve_simple_expr_compound_passthrough() {
    assert_eq!(
        view(&[("a", BindingType::SetupRef)], true).resolve_simple_expr("a + b"),
        "a + b"
    );
}

#[test]
fn resolve_simple_expr_trims_whitespace() {
    assert_eq!(
        view(&[("foo", BindingType::SetupConst)], false).resolve_simple_expr("  foo  "),
        "$setup.foo"
    );
}

// ==================== Fine-grained (Vapor) ====================

#[test]
fn fine_grained_setup_ref_prefix_is_ctx() {
    assert_eq!(
        fine_grained_view(&[("count", BindingType::SetupRef)]).resolve_prefix("count"),
        "_ctx."
    );
}

#[test]
fn fine_grained_setup_const_prefix_is_ctx() {
    assert_eq!(
        fine_grained_view(&[("fn", BindingType::SetupConst)]).resolve_prefix("fn"),
        "_ctx."
    );
}

#[test]
fn fine_grained_setup_let_prefix_is_ctx() {
    assert_eq!(
        fine_grained_view(&[("x", BindingType::SetupLet)]).resolve_prefix("x"),
        "_ctx."
    );
}

#[test]
fn fine_grained_props_prefix_is_dollar_props() {
    // Vue's non-inline Vapor expression transform is `type === "props" ?
    // "$props" : "_ctx"`: props are the one binding kind that does NOT
    // route through `_ctx.`.
    assert_eq!(
        fine_grained_view(&[("msg", BindingType::Props)]).resolve_prefix("msg"),
        "$props."
    );
}

#[test]
fn fine_grained_unresolved_prefix_is_ctx() {
    assert_eq!(fine_grained_view(&[]).resolve_prefix("unknown"), "_ctx.");
}

#[test]
fn fine_grained_suffix_is_always_empty() {
    assert_eq!(
        fine_grained_view(&[("count", BindingType::SetupRef)]).resolve_suffix("count"),
        ""
    );
}

#[test]
fn fine_grained_resolve_simple_expr_uses_ctx() {
    assert_eq!(
        fine_grained_view(&[("msg", BindingType::SetupRef)]).resolve_simple_expr("msg"),
        "_ctx.msg"
    );
}

#[test]
fn fine_grained_resolve_simple_expr_props_uses_dollar_props() {
    assert_eq!(
        fine_grained_view(&[("title", BindingType::Props)]).resolve_simple_expr("title"),
        "$props.title"
    );
}

// ==================== Reserved word bindings ====================

#[test]
fn resolve_simple_expr_keyword_prop_uses_bracket_notation() {
    assert_eq!(
        view(&[("class", BindingType::Props)], false).resolve_simple_expr("class"),
        r#"$props["class"]"#
    );
}

#[test]
fn resolve_simple_expr_keyword_prop_inline_uses_bracket_notation() {
    assert_eq!(
        view(&[("class", BindingType::Props)], true).resolve_simple_expr("class"),
        r#"__props["class"]"#
    );
}

#[test]
fn resolve_simple_expr_keyword_not_in_bindings_unchanged() {
    assert_eq!(view(&[], false).resolve_simple_expr("class"), "class");
}

#[test]
fn resolve_simple_expr_keyword_for_as_prop() {
    assert_eq!(
        view(&[("for", BindingType::Props)], false).resolve_simple_expr("for"),
        r#"$props["for"]"#
    );
}

#[test]
fn fine_grained_resolve_simple_expr_keyword_prop_uses_bracket_notation() {
    assert_eq!(
        fine_grained_view(&[("class", BindingType::Props)]).resolve_simple_expr("class"),
        r#"$props["class"]"#
    );
}

// ==================== Const prop overrides ====================

#[test]
fn const_prop_still_uses_props_prefix() {
    let view = view_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
    assert_eq!(view.resolve_prefix("msg"), "$props.");
}

#[test]
fn const_prop_still_uses_dunder_props_prefix_inline() {
    let view = view_with_const_props(&[("msg", BindingType::Props)], true, &["msg"]);
    assert_eq!(view.resolve_prefix("msg"), "__props.");
}

#[test]
fn const_prop_still_no_value_suffix() {
    let view = view_with_const_props(&[("msg", BindingType::Props)], true, &["msg"]);
    assert_eq!(view.resolve_suffix("msg"), "");
}

// ==================== all_bindings_const_props ====================

fn make_bindings_result<'a>(names: &[(&'a str, bool)]) -> BindingExtractionResult<'a> {
    BindingExtractionResult {
        bindings: names
            .iter()
            .map(|&(name, ignore)| Binding {
                name,
                span: RelativeSpan::new(0, name.len() as u32),
                pos: 0,
                ignore,
                is_expression_local: false,
                is_shorthand: false,
                usage: crate::utils::oxc::ReferenceUse::Read,
            })
            .collect(),
        functions: vec![],
        literals: vec![],
        has_errors: false,
        dynamism: Dynamism::MaybeDynamic,
    }
}

/// Without const_props data, always returns false (Vue compatibility)
#[test]
fn all_const_props_no_data_returns_false() {
    let view = view(&[("msg", BindingType::Props)], false);
    let bindings = make_bindings_result(&[("msg", false)]);
    assert!(!view.all_bindings_const_props(Some(&bindings)));
}

/// With const_props, a const prop expression returns true
#[test]
fn all_const_props_with_const_prop_returns_true() {
    let view = view_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
    let bindings = make_bindings_result(&[("msg", false)]);
    assert!(view.all_bindings_const_props(Some(&bindings)));
}

/// Non-const prop expression returns false
#[test]
fn all_const_props_with_non_const_prop_returns_false() {
    let view = view_with_const_props(
        &[("msg", BindingType::Props), ("count", BindingType::Props)],
        false,
        &["msg"],
    );
    let bindings = make_bindings_result(&[("count", false)]);
    assert!(!view.all_bindings_const_props(Some(&bindings)));
}

/// Mixed const prop + setup const returns true
#[test]
fn all_const_props_mixed_const_prop_and_setup_const() {
    let view = view_with_const_props(
        &[
            ("msg", BindingType::Props),
            ("LABEL", BindingType::SetupConst),
        ],
        false,
        &["msg"],
    );
    let bindings = make_bindings_result(&[("msg", false), ("LABEL", false)]);
    assert!(view.all_bindings_const_props(Some(&bindings)));
}

/// Mixed const prop + reactive ref returns false
#[test]
fn all_const_props_mixed_const_prop_and_ref_returns_false() {
    let view = view_with_const_props(
        &[
            ("msg", BindingType::Props),
            ("count", BindingType::SetupRef),
        ],
        false,
        &["msg"],
    );
    let bindings = make_bindings_result(&[("msg", false), ("count", false)]);
    assert!(!view.all_bindings_const_props(Some(&bindings)));
}

/// No bindings data returns false (conservative)
#[test]
fn all_const_props_none_bindings_returns_false() {
    let view = view_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
    assert!(!view.all_bindings_const_props(None));
}

/// Empty non-ignored names returns false (literal expression — Vue compat)
#[test]
fn all_const_props_pure_literal_returns_false() {
    let view = view_with_const_props(&[("msg", BindingType::Props)], false, &["msg"]);
    // All identifiers are ignored (v-for locals, etc.)
    let bindings = make_bindings_result(&[("item", true)]);
    assert!(!view.all_bindings_const_props(Some(&bindings)));
}

// ==================== Callee provenance ====================

fn origins_of(setup: &str) -> FrameworkOriginTable {
    use oxc_allocator::Allocator;
    use oxc_span::SourceType;
    use verter_semantic::analysis::{build_script_analysis_with_scope, AnalysisScope};
    use verter_semantic::compiler_view::ScriptOriginView;

    let allocator = Allocator::default();
    let analysis =
        build_script_analysis_with_scope(setup, SourceType::ts(), &allocator, AnalysisScope::BUILD);
    super::super::origin::build_origin_table(ScriptOriginView::build(Some(&analysis), None), "vue")
}

/// The table after recording callee origins over `setup`, for bindings the
/// frontend reported as `reported`.
fn recorded(
    setup: &str,
    reported: &[(&'static str, BindingType)],
) -> (VueBindingTable<'static>, FrameworkOriginTable) {
    let origins = origins_of(setup);
    let mut table = table_of(reported);
    table.record_callee_origins(&origins);
    (table, origins)
}

#[test]
fn a_call_of_an_imported_api_records_its_origin_and_keeps_the_class() {
    let (table, origins) = recorded(
        "import { ref as r } from 'vue'\nconst n = r(0)",
        &[("r", BindingType::SetupConst), ("n", BindingType::SetupRef)],
    );
    let fact = table.get("n").expect("binding");
    assert_eq!(fact.binding, BindingType::SetupRef);
    assert_eq!(fact.reported, BindingType::SetupRef);
    assert_eq!(
        fact.callee_origin,
        FrameworkOrigin::Api(FrameworkApi::Ref, OriginEvidence::NamedImport)
    );
    assert_eq!(
        origins.callee_provenance("n"),
        Some(verter_semantic::compiler_view::ResolvedOrigin::Export {
            source: "vue",
            imported: "ref",
            step: verter_semantic::compiler_view::OriginStep::NamedImport,
        })
    );
    // An import is not a call: it records no callee.
    assert_eq!(
        table.get("r").map(|fact| fact.callee_origin),
        Some(FrameworkOrigin::Unknown)
    );
}

#[test]
fn a_namespace_member_call_records_its_origin() {
    let (table, _) = recorded(
        "import * as V from 'vue'\nconst n = V.ref(0)",
        &[("n", BindingType::SetupMaybeRef)],
    );
    let fact = table.get("n").expect("binding");
    assert_eq!(fact.binding, BindingType::SetupMaybeRef);
    assert_eq!(
        fact.callee_origin,
        FrameworkOrigin::Api(FrameworkApi::Ref, OriginEvidence::NamespaceMember)
    );
}

#[test]
fn a_call_of_a_shadowing_function_records_shadowed_and_keeps_the_class() {
    let (table, _) = recorded(
        "function ref() {}\nconst n = ref(0)",
        &[
            ("ref", BindingType::SetupConst),
            ("n", BindingType::SetupMaybeRef),
        ],
    );
    let fact = table.get("n").expect("binding");
    assert_eq!(fact.binding, BindingType::SetupMaybeRef);
    assert_eq!(fact.callee_origin, FrameworkOrigin::Shadowed);
}

#[test]
fn a_let_call_records_its_origin_and_stays_a_let() {
    let (table, _) = recorded(
        "import { ref as r } from 'vue'\nlet n = r(0)",
        &[("n", BindingType::SetupLet)],
    );
    let fact = table.get("n").expect("binding");
    assert_eq!(fact.binding, BindingType::SetupLet);
    assert_eq!(
        fact.callee_origin,
        FrameworkOrigin::Api(FrameworkApi::Ref, OriginEvidence::NamedImport)
    );
}

#[test]
fn a_call_of_a_reactive_alias_records_reactive() {
    let (table, _) = recorded(
        "import { reactive } from 'vue'\nconst rx = reactive\nconst s = rx({})",
        &[("s", BindingType::SetupReactiveConst)],
    );
    assert_eq!(
        table.get("s").map(|fact| fact.callee_origin),
        Some(FrameworkOrigin::Api(
            FrameworkApi::Reactive,
            OriginEvidence::ConstAlias
        ))
    );
}

#[test]
fn a_call_of_an_unknown_function_keeps_its_module_provenance() {
    let (table, origins) = recorded(
        "import { useThing } from './things'\nconst x = useThing()\nconst y = unknownFn()",
        &[
            ("x", BindingType::SetupMaybeRef),
            ("y", BindingType::SetupMaybeRef),
        ],
    );
    assert_eq!(
        table.get("x").map(|fact| fact.callee_origin),
        Some(FrameworkOrigin::Unknown)
    );
    assert_eq!(
        origins.callee_provenance("x"),
        Some(verter_semantic::compiler_view::ResolvedOrigin::Export {
            source: "./things",
            imported: "useThing",
            step: verter_semantic::compiler_view::OriginStep::NamedImport,
        })
    );
    assert_eq!(
        table
            .get("y")
            .map(|fact| (fact.binding, fact.callee_origin)),
        Some((BindingType::SetupMaybeRef, FrameworkOrigin::Unknown))
    );
    assert_eq!(
        origins.callee_provenance("y"),
        Some(verter_semantic::compiler_view::ResolvedOrigin::Undeclared)
    );
}

#[test]
fn a_destructured_call_result_records_no_callee() {
    // `const { a } = useX()` binds a member of the result, not the call's
    // value.
    let (table, origins) = recorded(
        "import { useX } from './x'\nconst { a } = useX()",
        &[("a", BindingType::SetupMaybeRef)],
    );
    assert_eq!(
        table.get("a").map(|fact| fact.callee_origin),
        Some(FrameworkOrigin::Unknown)
    );
    assert_eq!(origins.callee_provenance("a"), None);
}
