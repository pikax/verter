//! Unit tests for the client shape classifiers (bind shapes, target-lvalue
//! writability, interpolation shapes) — extracted from the inline module to keep
//! `client_shapes.rs` under the file-size guard.

use super::*;
use crate::svelte::runtime::expr::BindingInfo;

/// A scope graph with a single root holding one `$state` signal binding named
/// `value` (the reactive-signal target a `bind:value` resolves to).
fn signal_value_env() -> (BindingTable, ScopeGraph, ScopeId) {
    let (mut graph, root) = ScopeGraph::with_root();
    let mut bindings = BindingTable::new();
    let id = bindings.push(BindingInfo {
        name: "value".to_string(),
        scope: root,
        kind: BindingRuntimeKind::StateSignal { raw: false },
        state: None,
    });
    graph.declare(root, "value", id);
    (bindings, graph, root)
}

/// Build a real [`AnalyzedExpr`] for `source` through the SAME single-parse analysis
/// path the runtime uses (so the test exercises the actual shared `BindTargetFact`,
/// not a synthetic stand-in).
fn analyzed_expr(source: &'static str, scope: ScopeId) -> AnalyzedExpr<'static> {
    let facts = crate::svelte::runtime::expr::collect_expr_references(source)
        .expect("test bind expression parses cleanly");
    AnalyzedExpr::interned(source, scope, facts)
}

#[test]
fn classify_bind_value_requires_an_explicit_bound_expression_source() {
    // ACCEPTED == EMITTABLE: a `bind:value` with NO bound-expression source
    // (`expr_source: None`) must FAIL CLOSED. Runtime-op collection only emits
    // `$.bind_value` for an `AttrIr::Bind { expr: Some(_) }`; a classifier that
    // accepted a sourceless bind (the old `expr_source.unwrap_or("value")`
    // fabrication) would record a bind shape the emitter then silently drops —
    // an accept-then-drop divergence. The fix makes the absence of a bound
    // expression a refusal at the classifier, so an accepted bind shape ALWAYS
    // has an emittable expression.
    let (bindings, scopes, root) = signal_value_env();
    let locals = rustc_hash::FxHashSet::default();
    let span = Span::new(0, 0);
    let res = classify_bind_shape(
        "value",
        "input",
        /* host_attrs = */ &[],
        /* expr = */ None,
        root,
        &bindings,
        &scopes,
        &locals,
        span,
    );
    assert!(
        matches!(
            res,
            Err(UnsupportedSvelteRuntimeSurface::Binding { ref target, .. }) if target == "value"
        ),
        "a sourceless `bind:value` must fail closed as the `bind:value` surface, got {res:?}"
    );
    // NEGATIVE: it must NOT be accepted as any bind shape (the pre-fix
    // `unwrap_or(\"value\")` accepted it as `ValueSignalIdent`).
    assert!(
        res.is_err(),
        "a sourceless `bind:value` must NOT be accepted (accept-then-drop): {res:?}"
    );
}

#[test]
fn classify_bind_value_accepts_an_explicit_signal_identifier_source() {
    // The positive boundary: an EXPLICIT bound identifier resolving to a signal
    // is accepted (the §1.2 surface + the synthesized-shorthand `value` source
    // both reach the classifier as a `Some(_)` identifier source).
    let (bindings, scopes, root) = signal_value_env();
    let locals = rustc_hash::FxHashSet::default();
    let span = Span::new(0, 0);
    let value_expr = analyzed_expr("value", root);
    let res = classify_bind_shape(
        "value",
        "input",
        /* host_attrs = */ &[],
        /* expr = */ Some(&value_expr),
        root,
        &bindings,
        &scopes,
        &locals,
        span,
    );
    // An explicit `bind:value={value}` to a $state signal is accepted as a DOM
    // value bind carrying the `$.bind_value` routing.
    match res {
        Ok(ClientBindShape::DomBind {
            name,
            routing,
            getset,
            group_key,
        }) => {
            assert_eq!(name, "value");
            assert_eq!(
                routing.helper,
                crate::svelte::bind_contract::RuntimeHelper::Value
            );
            // A bare-identifier signal target synthesizes the lvalue thunks.
            assert_eq!(getset, BindGetSetForm::TargetLvalue);
            // NEGATIVE: a non-`group` DOM bind carries NO group key (the accumulator
            // grouping is `bind:group`-only).
            assert_eq!(group_key, None, "a bind:value carries no group key");
        }
        other => panic!("expected a DomBind(Value) shape, got {other:?}"),
    }
}

#[test]
fn classify_bind_this_requires_a_declared_local_target() {
    // A `bind:this={el}` where `el` IS a declared instance-script local is the
    // supported shape-3 target (accepted); a FREE `bind:this={button}` (no declared
    // local) fails closed (5c) — official accepts it but reserves a fresh local,
    // whereas Verter's element-local allocation would collide with the synthesized
    // DOM local, so the free target is refused to moot the collision.
    let (bindings, scopes, root) = signal_value_env();
    let span = Span::new(0, 0);
    let mut locals = rustc_hash::FxHashSet::default();
    locals.insert("el".to_string());

    // DECLARED target `el` (a bare local, not a binding-table row) — accepted.
    let el_expr = analyzed_expr("el", root);
    let declared = classify_bind_shape(
        "this",
        "div",
        /* host_attrs = */ &[],
        Some(&el_expr),
        root,
        &bindings,
        &scopes,
        &locals,
        span,
    );
    assert_eq!(
        declared,
        Ok(ClientBindShape::This {
            getset: BindGetSetForm::TargetLvalue
        }),
        "a declared `let el;` bind:this target is the supported identifier shape-3"
    );

    // FREE target `button` (undeclared) — fails closed.
    let button_expr = analyzed_expr("button", root);
    let free = classify_bind_shape(
        "this",
        "button",
        /* host_attrs = */ &[],
        Some(&button_expr),
        root,
        &bindings,
        &scopes,
        &locals,
        span,
    );
    assert!(
        matches!(
            free,
            Err(UnsupportedSvelteRuntimeSurface::Binding { ref target, .. }) if target == "this"
        ),
        "a free / undeclared bind:this target must fail closed (5c): {free:?}"
    );
}

/// A scope graph holding ONE binding of the given `kind` named `root` — so the
/// writability decision can be exercised for each binding-runtime kind.
fn env_with_root_kind(kind: BindingRuntimeKind) -> (BindingTable, ScopeGraph, ScopeId) {
    let (mut graph, root) = ScopeGraph::with_root();
    let mut bindings = BindingTable::new();
    let id = bindings.push(BindingInfo {
        name: "root".to_string(),
        scope: root,
        kind,
        state: None,
    });
    graph.declare(root, "root", id);
    (bindings, graph, root)
}

#[test]
fn bind_root_writability_admits_only_assignment_valid_kinds() {
    // The WRITE decision (`bind_root_is_writable_target`) must admit ONLY the
    // assignment-valid roots — a `$state` SIGNAL, a `$.state($.proxy)` reassignable
    // proxy, and a PLAIN local — and must EXCLUDE the read-oriented signal kinds a
    // bind cannot legally reassign: `$derived`, an `{#each}` item, an `{#await}`
    // binding, and a `{@const}` derived. RED before the fix: the write decision reused
    // the read-oriented `is_signal_binding`, which admits `Derived` / `EachSignal` /
    // `AwaitSignal` / `LegacyConstDerived` — so a read-only root was wrongly treated as
    // writable.
    for kind in [
        BindingRuntimeKind::StateSignal { raw: false },
        BindingRuntimeKind::StateSignal { raw: true },
        BindingRuntimeKind::StateProxy,
        BindingRuntimeKind::PlainLocal,
    ] {
        let (bindings, scopes, root) = env_with_root_kind(kind);
        assert!(
            bind_root_is_writable_target(&bindings, &scopes, root, "root"),
            "an assignment-valid root ({kind:?}) must be writable"
        );
    }
    for kind in [
        BindingRuntimeKind::Derived,
        BindingRuntimeKind::EachSignal,
        BindingRuntimeKind::AwaitSignal,
        BindingRuntimeKind::LegacyConstDerived,
    ] {
        let (bindings, scopes, root) = env_with_root_kind(kind);
        assert!(
            !bind_root_is_writable_target(&bindings, &scopes, root, "root"),
            "a read-only signal root ({kind:?}) must NOT be writable (no bind reassignment)"
        );
    }
    // IMPORT roots are NON-writable BY DESIGN (ES import bindings are not
    // reassignable; official rejects the bare bind `constant_binding` and the
    // reassignment `constant_assignment`) — folding either into `PlainLocal`
    // would silently accept `bind:value={imported}` / `imported = v`.
    for kind in [
        BindingRuntimeKind::ComponentImport,
        BindingRuntimeKind::ImportedValue,
    ] {
        let (bindings, scopes, root) = env_with_root_kind(kind);
        assert!(
            !bind_root_is_writable_target(&bindings, &scopes, root, "root"),
            "an import root ({kind:?}) must NOT be a writable bind root"
        );
    }
}

#[test]
fn is_writable_bind_root_admits_only_assignment_valid_kinds() {
    // The writable predicate admits EXACTLY the assignment-valid kinds (a `$state`
    // signal, a reassignable proxy, a plain local) and EXCLUDES the read-oriented signal
    // kinds the read classifier (`is_signal_binding`) admits — `Derived` / `EachSignal` /
    // `AwaitSignal` / `LegacyConstDerived`. A signal being READABLE does not make it a
    // valid bind WRITE target.
    assert!(is_writable_bind_root(BindingRuntimeKind::StateSignal {
        raw: false
    }));
    assert!(is_writable_bind_root(BindingRuntimeKind::StateSignal {
        raw: true
    }));
    assert!(is_writable_bind_root(BindingRuntimeKind::StateProxy));
    // A `BareProxy` is writable at a MEMBER bind target (`o.x = $$value` plain).
    assert!(is_writable_bind_root(BindingRuntimeKind::BareProxy));
    assert!(is_writable_bind_root(BindingRuntimeKind::PlainLocal));
    assert!(!is_writable_bind_root(BindingRuntimeKind::Derived));
    assert!(!is_writable_bind_root(BindingRuntimeKind::EachSignal));
    assert!(!is_writable_bind_root(BindingRuntimeKind::AwaitSignal));
    assert!(!is_writable_bind_root(
        BindingRuntimeKind::LegacyConstDerived
    ));
    // Read-only signal kinds the read classifier admits are NOT writable — the explicit
    // split this predicate enforces.
    assert!(is_signal_binding(BindingRuntimeKind::Derived));
    assert!(!is_writable_bind_root(BindingRuntimeKind::Derived));
    // IMPORT kinds are NON-writable roots by design (non-reassignable ES import
    // bindings; official `constant_binding` / `constant_assignment` rejects) —
    // BOTH the component-default and the general imported-value kind.
    assert!(!is_writable_bind_root(BindingRuntimeKind::ComponentImport));
    assert!(!is_writable_bind_root(BindingRuntimeKind::ImportedValue));
}
