//! @ai-generated - Named-symbol operator-reduction bridge characterisation.
//!
//! Characterises the `materialize_through_aliases` reduction bridge: a
//! top-level alias whose body is an `IndexedAccess` (`type X = Y[K]`) or
//! `KeyOf` (`type X = keyof Y`) operator reduces to its member type / key
//! union under the publication modes (`Navigate` / `Shallow` / `Expanded`)
//! through the SHARED `SemanticQueryKey::IndexedAccess` / `KeyOf` reducer
//! (which canonicalises to `ProjectPath` / the keyof builder), and PRESERVES
//! the operator carrier under `Skeleton` (the mode gate) and for symbolic
//! objects the reducer cannot resolve. A written union or intersection body
//! publishes the type the checker constructs from it, reduced by the
//! canonical union / intersection algebra.

use super::support::*;
use verter_type_engine::semantic_query::{
    ProjectionMode, ProjectionReductionContext, SemanticNodeData, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryOutput,
};

const OPERATOR_REDUCTION: &str = include_str!("fixtures/operator_reduction.ts");
const INDEX_SIGNATURES: &str = include_str!("fixtures/index_signatures.ts");

fn upsert(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/operator_reduction.ts", OPERATOR_REDUCTION);
}

fn upsert_index(host: &crate::VerterHost) {
    upsert_ts(host, "/fixtures/index_signatures.ts", INDEX_SIGNATURES);
}

// ── Reduction: operator-bodied aliases reduce to their member/key value ──

#[test]
fn named_resolve_reduces_indexed_access_alias_to_member_type() {
    // `type ConcreteLookup = KeySurface["id"]` reduces to `string` through the
    // shared IndexedAccess reducer — NOT an un-reduced `IndexedAccess` carrier.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/operator_reduction.ts",
        "ConcreteLookup",
        &[],
        ProjectionMode::Expanded,
    );

    assert_primitive(&expr, PrimitiveName::String);
    assert!(
        !matches!(expr, TypeExpr::IndexedAccess { .. }),
        "named resolve must REDUCE the indexed-access alias, not publish the \
         un-reduced carrier: {expr:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn named_resolve_reduces_keyof_alias_to_key_union() {
    // `type ConcreteKeys = keyof KeySurface` reduces to the member-name literal
    // union — NOT an un-reduced `KeyOf` carrier.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, record) = resolve_expr(
        &host,
        "/fixtures/operator_reduction.ts",
        "ConcreteKeys",
        &[],
        ProjectionMode::Expanded,
    );

    assert_literal_union(&expr, &["count", "id"]);
    assert!(
        !matches!(expr, TypeExpr::KeyOf(_)),
        "named resolve must REDUCE the keyof alias, not publish the un-reduced \
         carrier: {expr:?}"
    );
    assert_query_mode(&record, ProjectionModeTag::Expanded);
}

#[test]
fn named_resolve_reduces_indexed_access_under_navigate_and_shallow() {
    // The reduction is gated on the publication modes — Navigate and Shallow
    // reduce a named-member indexed access the same as Expanded (the violation
    // was for all three publication modes).
    let host = make_host_with_footprint();
    upsert(&host);

    // Expanded resolves the terminal scalar.
    let (expanded, _record) = resolve_expr(
        &host,
        "/fixtures/operator_reduction.ts",
        "ConcreteLookup",
        &[],
        ProjectionMode::Expanded,
    );
    assert_primitive(&expanded, PrimitiveName::String);

    // Navigate / Shallow reduce the operator too — the discriminating
    // bug-fix fact is that they no longer publish the un-reduced
    // `IndexedAccess` carrier (the mode-shallow surface they produce is a
    // separate mode contract; the violation was leaving the operator shell).
    for mode in [ProjectionMode::Navigate, ProjectionMode::Shallow] {
        let (expr, _record) = resolve_expr(
            &host,
            "/fixtures/operator_reduction.ts",
            "ConcreteLookup",
            &[],
            mode,
        );
        assert!(
            !matches!(expr, TypeExpr::IndexedAccess { .. }),
            "{mode:?} must reduce the indexed-access operator, not publish the \
             un-reduced carrier: {expr:?}"
        );
    }
}

// ── One-resolver: the FFI evaluate path reduces IDENTICALLY ──

#[test]
fn evaluate_expr_reduces_operator_identically_to_named_resolve() {
    // The public FFI `evaluate_type_expression` entry
    // (`WireOperation::EvaluateExpression`) MUST reduce an operator-bodied
    // expression through the SAME single canonical materializer the
    // named-resolve path uses — not an un-bridged evaluate-local copy.
    //
    // A divergent evaluate path with its OWN `materialize_through_aliases`
    // lacking operator arms would terminate at the carrier-preserving
    // `_ => Ok(current)` arm and return the UN-reduced `IndexedAccess` / `KeyOf`
    // carrier when evaluating `KeySurface["id"]` / `keyof KeySurface`. This test
    // pins that both entry-points share the ONE bridged helper: it FAILS if the
    // evaluate result is an operator carrier and PASSES when it is the reduced
    // terminal.
    let host = make_host_with_footprint();
    upsert(&host);
    let scope = "/fixtures/operator_reduction.ts";

    for mode in [
        ProjectionMode::Expanded,
        ProjectionMode::Navigate,
        ProjectionMode::Shallow,
    ] {
        // Indexed access: `KeySurface["id"]` → `string` on BOTH paths.
        let (named, _r) = resolve_expr(&host, scope, "ConcreteLookup", &[], mode);
        let (evaluated, _r) = evaluate_expr(&host, scope, "KeySurface[\"id\"]", mode);
        assert!(
            !matches!(evaluated, TypeExpr::IndexedAccess { .. }),
            "{mode:?} FFI evaluate must REDUCE the indexed-access operator, not \
             publish the un-reduced carrier: {evaluated:?}"
        );
        assert_eq!(
            evaluated, named,
            "{mode:?} FFI evaluate of `KeySurface[\"id\"]` must reduce IDENTICALLY \
             to the named-resolve of `ConcreteLookup`"
        );

        // keyof: `keyof KeySurface` → the member-name literal union on both.
        let (named_keys, _r) = resolve_expr(&host, scope, "ConcreteKeys", &[], mode);
        let (evaluated_keys, _r) = evaluate_expr(&host, scope, "keyof KeySurface", mode);
        assert!(
            !matches!(evaluated_keys, TypeExpr::KeyOf(_)),
            "{mode:?} FFI evaluate must REDUCE the keyof operator, not publish the \
             un-reduced carrier: {evaluated_keys:?}"
        );
        assert_eq!(
            evaluated_keys, named_keys,
            "{mode:?} FFI evaluate of `keyof KeySurface` must reduce IDENTICALLY to \
             the named-resolve of `ConcreteKeys`"
        );
    }
}

// ── Preservation guard #1: symbolic IndexedAccess (open object) survives ──

#[test]
fn open_typeparam_object_indexed_lookup_preserved_under_publication_reduction() {
    // `type SymbolicLookup<T> = T["id"]` resolved with NO type args keeps the
    // object an open `TypeParam`. The shared reducer cannot resolve a concrete
    // surface, so the bridge MUST preserve the `IndexedAccess` carrier rather
    // than fabricate a member type or collapse to `never` / a miss.
    //
    // Discriminating: if the bridge ignored the reducer's no-progress signal
    // (e.g. forced a member or errored), this would not round-trip back to an
    // IndexedAccess carrier.
    let host = make_host_with_footprint();
    upsert(&host);

    let (expr, _record) = resolve_expr(
        &host,
        "/fixtures/operator_reduction.ts",
        "SymbolicLookup",
        &[],
        ProjectionMode::Expanded,
    );

    assert!(
        matches!(expr, TypeExpr::IndexedAccess { .. }),
        "symbolic `T[\"id\"]` (open object) must preserve the IndexedAccess \
         carrier, got {expr:?}"
    );
}

// ── Preservation guard #2: Skeleton / structural-transit preserves carrier ──

#[test]
fn skeleton_structural_transit_instantiate_preserves_operator_carriers() {
    // The mode gate (`Navigate | Shallow | Expanded`) MUST exclude Skeleton:
    // the BFS / generic-helper traversal mode preserves operator carriers so
    // Conditional branches do not collapse. Two independent assertions:
    //
    // (a) NAMED resolve in `Skeleton` returns the un-reduced `IndexedAccess`
    //     carrier (discriminating against widening / dropping the mode gate —
    //     remove the gate and this reduces to `string` and fails).
    // (b) The bare `Instantiate { context: structural_transit(Skeleton) }`
    //     dispatch — the path the materialization cycle gate drives —
    //     returns the un-reduced operator node (bare Instantiate stays
    //     identity-preserving; the bridge never fires on it).
    let host = make_host_with_footprint();
    upsert_index(&host);

    // (a) named resolve in Skeleton mode.
    let (expr, _record) = resolve_expr(
        &host,
        "/fixtures/index_signatures.ts",
        "NumericLookup",
        &[],
        ProjectionMode::Skeleton,
    );
    assert!(
        matches!(expr, TypeExpr::IndexedAccess { .. }),
        "Skeleton named resolve must PRESERVE the IndexedAccess carrier (the \
         mode gate excludes Skeleton), got {expr:?}"
    );

    // (b) bare structural-transit Skeleton Instantiate.
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch =
        verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(&host_ctx);

    let canonical: Arc<str> = Arc::from("/fixtures/index_signatures.ts");
    let base = dispatch.type_slot_for(
        Arc::clone(&canonical),
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        Arc::from("NumericLookup"),
    );
    let key =
        SemanticQueryKey::Instantiate(verter_type_engine::semantic_query::InstantiateKey::new(
            base,
            Arc::from(Vec::new().into_boxed_slice()),
            dispatch.instantiate_context_for(
                &canonical,
                ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Skeleton),
            ),
        ));
    let node = match dispatch.execute_type_node(key) {
        verter_type_engine::semantic_query::QueryResult::Value(SemanticQueryOutput {
            value,
            ..
        }) => value,
        verter_type_engine::semantic_query::QueryResult::Recursive(value) => value,
        other => panic!("structural-transit Skeleton Instantiate failed: {other:?}"),
    };
    let graph = host.project_type_store().semantic_graph();
    let mut data = graph.node_data(node);
    // Unwrap a single Alias shell if present (bare Instantiate may return the
    // alias wrapper without unwrapping; the carrier underneath must be the
    // operator node, NOT a reduced member).
    if let Some(SemanticNodeData::Alias(inner)) = data.as_deref() {
        data = graph.node_data(*inner);
    }
    assert!(
        matches!(
            data.as_deref(),
            Some(SemanticNodeData::IndexedAccess { .. })
        ),
        "bare structural-transit Skeleton Instantiate must return the \
         un-reduced IndexedAccess carrier, got {:?}",
        data.as_deref()
    );
}

#[test]
fn evaluate_keyof_reduces_on_cold_navigate_dispatch() {
    // DISCRIMINATING (cold-path Navigate keyof): Navigate-mode body lowering
    // deliberately preserves a no-args named reference as a `DeclRef` carrier
    // (cycle-BFS visibility — `lower.rs` carrier-preservation), so a COLD
    // `Instantiate { published(Navigate) }` of `type X = keyof Y` hands
    // `build_key_of` a `DeclRef` operand. `build_key_of`'s own contract
    // (documented at the bridge's KeyOf arm) is that an UN-RESOLVED reference
    // operand returns the deferred `KeyOf` carrier — which the shared
    // materializer bridge then surfaces and reduces. A `DeclRef` operand
    // falling through to the `Opaque(Miss)` arm instead publishes a
    // `semanticMiss` terminal for the whole evaluate.
    //
    // FAILS while `build_key_of` lacks the `DeclRef`/`InstantiationRef`
    // carrier arm (evaluate returns `Unknown { raw: "semanticMiss" }`);
    // PASSES once the carrier defers and the bridge reduces it. The warm
    // path cannot mask this: the keyof evaluate is the FIRST dispatch on a
    // fresh host, so no prior Expanded materialization can satisfy it.
    let host = make_host_with_footprint();
    upsert(&host);
    let scope = "/fixtures/operator_reduction.ts";

    let (named_keys, _r) =
        resolve_expr(&host, scope, "ConcreteKeys", &[], ProjectionMode::Navigate);
    // The named value is anchored to the REAL key union (not merely compared
    // against the evaluate result): pre-fix BOTH cold Navigate paths degraded
    // to the same `semanticMiss` terminal, so a bare parity assert would pass
    // vacuously.
    assert_literal_union(&named_keys, &["count", "id"]);
    let host2 = make_host_with_footprint();
    upsert(&host2);
    let (evaluated_keys, _r) =
        evaluate_expr(&host2, scope, "keyof KeySurface", ProjectionMode::Navigate);
    assert!(
        !matches!(evaluated_keys, TypeExpr::KeyOf(_)),
        "cold Navigate FFI evaluate must REDUCE the keyof operator, not publish \
         the un-reduced carrier: {evaluated_keys:?}"
    );
    assert_eq!(
        evaluated_keys, named_keys,
        "cold Navigate FFI evaluate of `keyof KeySurface` must reduce IDENTICALLY \
         to the named-resolve of `ConcreteKeys` — a `DeclRef` operand must defer \
         to the bridge, not degrade to a semanticMiss terminal"
    );
}

// ── Written unions and intersections publish the type the checker builds ──

/// The checker's lib declarations the semantic benchmark's
/// template-literal scenario carries beside its probe.
const BENCH_LIB_PRELUDE: &str = "type Exclude<T, U> = T extends U ? never : T;\n\
     type Extract<T, U> = T extends U ? T : never;\n\
     type Uppercase<S extends string> = intrinsic;\n\
     type Lowercase<S extends string> = intrinsic;\n\
     type Capitalize<S extends string> = intrinsic;\n\
     type Uncapitalize<S extends string> = intrinsic;\n";

/// `__Probe` in `canonical`, demanded as the semantic benchmark demands it:
/// resolved by name in the host's default mode and projected to its wire
/// answer.
fn published_probe(host: &crate::VerterHost, canonical: &str) -> TypeExpr {
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(canonical, "__Probe", None)
        .into_parts();
    let node = outcome
        .expect("the named demand completes")
        .expect("the probe exists");
    let bytes = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("the answer projects");
    serde_json::from_slice(&bytes).expect("a wire answer")
}

/// The constituents of `answer` as rendered text, sorted: a union's member
/// order is not part of its identity. Any other type is its own single
/// constituent, rendered whole.
fn constituents(answer: &TypeExpr) -> Vec<String> {
    let arms: Vec<&TypeExpr> = match answer {
        TypeExpr::Union(arms) => arms.iter().collect(),
        other => vec![other],
    };
    let mut texts: Vec<String> = arms.into_iter().map(render).collect();
    texts.sort_unstable();
    texts
}

/// `answer` as rendered text.
fn render(answer: &TypeExpr) -> String {
    verter_type_expr::render_type_expr_display(answer)
        .expect("the answer renders")
        .text
}

/// A written union absorbs the literals its primitives and pattern
/// templates already hold, also when a literal or its supertype is written
/// through an alias, and a written intersection of a literal and its
/// primitive is the literal; every other written union publishes exactly as
/// written, in its written order and by its written names. A written
/// composite nested in a published member or element stays shallow there,
/// and walking to it publishes the absorbed type.
///
/// Measured on TypeScript 7.0.2 with `declare const p: <probe>; const s:
/// never = p;` read off the TS2322 message (all four `strictNullChecks` ×
/// `noImplicitAny` settings agree): `"a1" | `a${number}` | "ab"` is `"ab" |
/// `a${number}``, `"a" | string` is `string`, `1 | number | "x"` is `number
/// | "x"`, `string & "a"` is `"a"`, and with `type A = "a"`, `type AB = "a" |
/// "b"`, `type S = string`: `A | string` is `string`, `AB | S | 1` is
/// `string | 1`, while `A | "b"` keeps `A` by its name. A literal named
/// through a long alias chain (`type A0 = "a"; type A1 = A0; …; type A20 =
/// A19`) is absorbed the same way: `A20 | string` is `string`.
#[test]
fn a_written_composite_publishes_the_type_the_checker_constructs() {
    let host = make_host_with_footprint();
    let alias_chain: String = std::iter::once("type A0 = \"a\";\n".to_owned())
        .chain((1..=20).map(|hop| format!("type A{hop} = A{};\n", hop - 1)))
        .collect();
    let absorbed: [(&str, &str, &[&str]); 7] = [
        (
            BENCH_LIB_PRELUDE,
            "\"a1\" | `a${number}` | \"ab\"",
            &["'ab'", "`a${number}`"],
        ),
        ("", "\"a\" | string", &["string"]),
        ("", "1 | number | \"x\"", &["'x'", "number"]),
        ("", "string & \"a\"", &["'a'"]),
        ("type A = \"a\";\n", "A | string", &["string"]),
        (
            "type AB = \"a\" | \"b\";\ntype S = string;\n",
            "AB | S | 1",
            &["1", "string"],
        ),
        (alias_chain.as_str(), "A20 | string", &["string"]),
    ];
    // Unions with no literal to absorb: the declarations beside the probe,
    // the written probe and its published text.
    let kept = [
        ("", "\"a\" | \"b\"", "'a' | 'b'"),
        ("", "\"a\" | (\"b\" | \"c\")", "'a' | 'b' | 'c'"),
        ("", "\"a\" | \"b\" | never", "'a' | 'b' | never"),
        ("", "\"a\" | null | undefined", "'a' | null | undefined"),
    ];
    let mut failures = Vec::new();
    for (index, (prelude, probe, expected)) in kept.iter().enumerate() {
        let canonical = format!("/probes/kept{index}.ts");
        upsert_ts(
            &host,
            &canonical,
            &format!("{prelude}type __Probe = {probe};\n"),
        );
        let published = render(&published_probe(&host, &canonical));
        if published != *expected {
            failures.push(format!(
                "`{probe}`: kept as {expected}, published {published}"
            ));
        }
    }
    // A named arm with nothing to absorb it stays by its name.
    upsert_ts(
        &host,
        "/probes/kept_named.ts",
        "type A = \"a\";\ntype __Probe = A | \"b\";\n",
    );
    let published = constituents(&published_probe(&host, "/probes/kept_named.ts"));
    if published != ["'b'", "A"] {
        failures.push(format!(
            "`A | \"b\"`: kept as [\"'b'\", \"A\"], published {published:?}"
        ));
    }
    for (index, (container, walk)) in [
        ("{ x: \"a\" | string }", "__Probe[\"x\"]"),
        ("(\"a\" | string)[]", "__Probe[number]"),
    ]
    .into_iter()
    .enumerate()
    {
        let canonical = format!("/probes/nested{index}.ts");
        upsert_ts(&host, &canonical, &format!("type __Probe = {container};\n"));
        let (walked, _r) = evaluate_expr(&host, &canonical, walk, ProjectionMode::Navigate);
        if constituents(&walked) != ["string"] {
            failures.push(format!(
                "`{walk}` of `{container}`: the checker answers string, published {}",
                render(&walked)
            ));
        }
    }
    for (index, (prelude, probe, expected)) in absorbed.iter().enumerate() {
        let canonical = format!("/probes/p{index}.ts");
        upsert_ts(
            &host,
            &canonical,
            &format!("{prelude}type __BenchInit = 0;\ntype __Probe = {probe};\n"),
        );
        let published = constituents(&published_probe(&host, &canonical));
        if published != *expected {
            failures.push(format!(
                "`{probe}`: the checker answers {expected:?}, published {published:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));

    // The FFI evaluate entry reduces through the same materializer.
    let (evaluated, _r) = evaluate_expr(
        &host,
        "/probes/p1.ts",
        "\"a\" | string",
        ProjectionMode::Expanded,
    );
    assert_eq!(constituents(&evaluated), ["string"]);

    // `Skeleton` keeps the authored shell.
    let (skeleton, _r) = resolve_expr(
        &host,
        "/probes/p1.ts",
        "__Probe",
        &[],
        ProjectionMode::Skeleton,
    );
    assert_eq!(constituents(&skeleton), ["'a'", "string"]);
}

/// A reducer fault while a written composite is published fails the request,
/// never delivering the unreduced composite as a success: a cancellation that
/// lands on the reduction fails the publication, and the same publication
/// uncancelled absorbs the literal.
#[test]
fn a_reducer_fault_during_composite_publication_fails_the_request() {
    use crate::host_resolve_type_audit::TypeResolutionRequestError;
    use crate::typeinfo::resolve_named_symbol::{
        materialize_through_aliases, resolve_named_symbol_in_current_view,
    };
    use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
    use verter_type_engine::request_context::{RequestContext, RequestContextGuard};

    const FILE: &str = "/probes/fault.ts";
    let host = make_host_with_footprint();
    upsert_ts(&host, FILE, "type __Probe = \"a\" | string;\n");
    let view = crate::typeinfo::current_store_view_for_query(&host).expect("a current view");
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::from_current(&host, &view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let _scope = dispatch.enter_construction_scope();
    // `Skeleton` delivers the written composite unreduced.
    let (written, _mode) = resolve_named_symbol_in_current_view(
        &host,
        &dispatch,
        FILE,
        "__Probe",
        &[],
        ProjectionMode::Skeleton,
    );
    let written = written
        .expect("the skeleton demand completes")
        .expect("the probe exists");
    let scope = verter_type_engine::semantic_query::NodeScopeId::File {
        canonical_id: verter_type_expr::OwnedStr::from(FILE),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: host
            .shallow_file_state(FILE)
            .expect("the probe's file is indexed")
            .whole_hash,
        local_scope: None,
    };

    let cancelled = RequestContext::new(7101, Arc::from(FILE), false, None);
    cancelled.cancel();
    let outcome = {
        let _guard = RequestContextGuard::install(cancelled);
        materialize_through_aliases(&host, &dispatch, written, ProjectionMode::Expanded, &scope)
    };
    assert!(
        matches!(outcome, Err(TypeResolutionRequestError::Cancelled)),
        "a cancelled reduction fails the publication; got {outcome:?}"
    );

    let published =
        materialize_through_aliases(&host, &dispatch, written, ProjectionMode::Expanded, &scope)
            .expect("the uncancelled publication completes");
    assert_ne!(
        published, written,
        "the uncancelled publication absorbs the literal"
    );
}

/// The `null` / `undefined` algebra a written union is constructed under is
/// its file's `strictNullChecks`, as for a written intersection: measured on
/// TypeScript 7.0.2, `"a" | string | null` is `string | null` with
/// `strictNullChecks` on and `string` with it off.
#[test]
fn a_written_union_absorbs_under_its_files_strict_null_checks() {
    for (strict_null_checks, expected) in
        [(true, &["null", "string"][..]), (false, &["string"][..])]
    {
        let host = Arc::new(crate::VerterHost::new_standalone_with_tsconfig_projects(
            crate::HostConfig {
                analysis_level: crate::types::AnalysisLevel::Full,
                audit_enabled: true,
                ..crate::HostConfig::default()
            },
            &[(
                "/snc",
                &format!(
                    r#"{{ "compilerOptions": {{ "strict": true, "strictNullChecks": {strict_null_checks} }} }}"#
                ),
            )],
        ));
        upsert_ts(
            &host,
            "/snc/probe.ts",
            "type __Probe = \"a\" | string | null;\n",
        );
        let published = constituents(&published_probe(&host, "/snc/probe.ts"));
        assert_eq!(
            published, expected,
            "strictNullChecks {strict_null_checks}: published {published:?}"
        );
    }
}

/// Deciding whether a written union may absorb a literal reads each arm
/// once: a union of literals with nothing to absorb them, which publishes as
/// written, classifies four times the arms when it holds four times the
/// literals, never the square of them.
#[test]
fn a_literal_union_decides_absorption_in_linear_work() {
    use verter_type_engine::project_semantic_dispatch::build::written_union_arm_classifications_for_tests as classifications;
    let host = make_host_with_footprint();
    let work = |arms: usize| {
        let canonical = format!("/probes/linear{arms}.ts");
        let probe = (0..arms)
            .map(|arm| format!("\"k{arm}\""))
            .collect::<Vec<_>>()
            .join(" | ");
        upsert_ts(&host, &canonical, &format!("type __Probe = {probe};\n"));
        let before = classifications();
        let published = published_probe(&host, &canonical);
        let classified = classifications() - before;
        assert_eq!(
            constituents(&published).len(),
            arms,
            "a union with nothing to absorb publishes every literal"
        );
        classified
    };
    let small = work(64);
    let large = work(256);
    assert!(small >= 64, "every arm is classified: {small} for 64 arms");
    assert!(
        large <= 5 * small,
        "four times the arms costs about four times the work: {small} for 64 arms, {large} for 256"
    );
}
