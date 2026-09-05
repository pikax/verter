//! Binder-scope, navigator-agreement, environment-sealing, and
//! post-child-refusal coverage for the semantic-operand forcing boundary.
//!
//! Split out of the main operand test module purely for file size; every
//! case here drives the same public `force_semantic_operand` boundary.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, JsdocTypedefBodyLocator, LocatorSymbolSpace,
    TypeBodyPathStep, TypeParamBoundPosition,
};
use verter_type_expr::TopLevelOwnerId;

use crate::request_context::{RequestContext, RequestContextGuard};
use crate::semantic_query::operand::{
    OperandBinderIdentity, SemanticOperandForceRequest, SemanticOperandMintError,
    SemanticOperandParts,
};
use crate::semantic_query::{
    PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId,
};

use super::semantic_operand_tests::{
    assert_infer_ref, assert_operand_error, assert_type_param, force, force_key, locator,
    locator_at, make_host, member_value, mint, request, upsert, upsert_at, whole, OTHER_OWNER,
    OWNER, SOURCE_V1,
};
use super::{ProjectSemanticDispatch, SemanticOperandAuthority};

const NESTED_BINDER_SOURCE: &str = "\
export type K = boolean;\n\
export type P = boolean;\n\
export type T = boolean;\n\
export type Plain = {\n\
  mapped: { [K in \"x\" | \"y\"]: K };\n\
  cond: string extends infer P ? P : never;\n\
  call: <T>(value: T) => T;\n\
};\n";

#[test]
fn nested_binder_positions_under_a_non_generic_container_bind_locally() {
    // `Plain` is a plain, non-generic object: its ROOT introduces no
    // binder. Each selected position nevertheless sits UNDER one — a
    // mapped key binder, a conditional `infer` binder, a generic
    // callable's own type parameter — reached partway down the path.
    //
    // The regression this discriminates is a navigator that inspects only
    // the path's starting node: it would hand the bare descendant
    // (`K` / `P` / `T`) to the lowerer with no ancestor, so the reference
    // would resolve against FILE scope instead of its binder and silently
    // capture the same-named top-level `boolean` aliases this fixture
    // plants — a wrong-COMPLETE answer, not a typed miss.
    let host = make_host();
    upsert(&host, NESTED_BINDER_SOURCE);
    let dispatch = ProjectSemanticDispatch::new(&host);

    let mapped_value = force(
        &dispatch,
        &mint(
            &dispatch,
            locator(
                "Plain",
                Arc::from([
                    TypeBodyPathStep::Member { ordinal: 0 },
                    TypeBodyPathStep::MemberValue,
                    TypeBodyPathStep::MappedValue,
                ]),
            ),
        ),
        ProjectionMode::Identity,
    );
    assert_type_param(&host, mapped_value);

    let conditional_true = force(
        &dispatch,
        &mint(
            &dispatch,
            locator(
                "Plain",
                Arc::from([
                    TypeBodyPathStep::Member { ordinal: 1 },
                    TypeBodyPathStep::MemberValue,
                    TypeBodyPathStep::ConditionalTrue,
                ]),
            ),
        ),
        ProjectionMode::Identity,
    );
    assert_infer_ref(&host, conditional_true);

    let callable_return = force(
        &dispatch,
        &mint(
            &dispatch,
            locator(
                "Plain",
                Arc::from([
                    TypeBodyPathStep::Member { ordinal: 2 },
                    TypeBodyPathStep::MemberValue,
                    TypeBodyPathStep::FunctionReturn,
                ]),
            ),
        ),
        ProjectionMode::Identity,
    );
    assert_type_param(&host, callable_return);
}

#[test]
fn crossing_no_binder_still_keeps_the_direct_sub_expression_route() {
    // The complement of the test above: a path that crosses NO binder must
    // not start capturing its ancestor, or every single-member request
    // would pay for interning the whole enclosing declaration. A mapped
    // type's SOURCE clause is exactly such a binder-free descent: the key
    // binder is not in scope inside the source it iterates.
    let host = make_host();
    upsert(&host, NESTED_BINDER_SOURCE);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let before = host.project_type_store().semantic_graph().node_count();
    let mapped_source = force(
        &dispatch,
        &mint(
            &dispatch,
            locator(
                "Plain",
                Arc::from([
                    TypeBodyPathStep::Member { ordinal: 0 },
                    TypeBodyPathStep::MemberValue,
                    TypeBodyPathStep::MappedSource,
                ]),
            ),
        ),
        ProjectionMode::Identity,
    );
    let after = host.project_type_store().semantic_graph().node_count();
    assert!(matches!(
        host.project_type_store()
            .semantic_graph()
            .node_data(mapped_source)
            .as_deref(),
        Some(SemanticNodeData::Union(_))
    ));
    // Three nodes: the two string-literal arms plus the union. Capturing
    // the ancestor would additionally intern the whole `Plain` object and
    // every sibling member.
    assert_eq!(
        after - before,
        3,
        "a binder-free descent must not intern the enclosing declaration"
    );
}

#[test]
fn nested_binder_capture_starts_at_the_crossed_node_not_the_enclosing_object() {
    // The path DOES cross a binder (the nested mapped key). Capturing the
    // navigation START — the whole `Big` object — would intern every
    // unselected sibling of `mapped`. The ancestor must be the mapped
    // type itself, so a structurally identical standalone mapped force
    // already interned every node the nested selection needs.
    let host = make_host();
    upsert(
        &host,
        "export type K = boolean;\n\
         export type JustMapped = { [K in \"x\"]: K };\n\
         export type Big = {\n\
           keep: { a: string; b: number; c: boolean; d: bigint; e: symbol; f: never };\n\
           mapped: { [K in \"x\"]: K };\n\
         };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let standalone = force(
        &dispatch,
        &mint(
            &dispatch,
            locator("JustMapped", Arc::from([TypeBodyPathStep::MappedValue])),
        ),
        ProjectionMode::Identity,
    );
    assert_type_param(&host, standalone);
    let before = host.project_type_store().semantic_graph().node_count();
    let nested = force(
        &dispatch,
        &mint(
            &dispatch,
            locator(
                "Big",
                Arc::from([
                    TypeBodyPathStep::Member { ordinal: 1 },
                    TypeBodyPathStep::MemberValue,
                    TypeBodyPathStep::MappedValue,
                ]),
            ),
        ),
        ProjectionMode::Identity,
    );
    let after = host.project_type_store().semantic_graph().node_count();
    assert_type_param(&host, nested);
    assert_eq!(
        after - before,
        0,
        "a nested binder capture must not intern unselected siblings of the enclosing object"
    );
}

#[test]
fn conditional_true_without_infer_keeps_the_direct_branch_route() {
    // `ConditionalTrue` crosses a binder only when `extends` declares
    // `infer`. A closed `string extends number ? string : never` has
    // nothing to preserve, so the true branch lowers as a bare `string`
    // rather than internning the conditional's other arms.
    let host = make_host();
    upsert(
        &host,
        "export type Closed = {\n\
           keep: { a: string; b: number; c: boolean; d: bigint };\n\
           cond: string extends number ? string : never;\n\
         };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let before = host.project_type_store().semantic_graph().node_count();
    let true_branch = force(
        &dispatch,
        &mint(
            &dispatch,
            locator(
                "Closed",
                Arc::from([
                    TypeBodyPathStep::Member { ordinal: 1 },
                    TypeBodyPathStep::MemberValue,
                    TypeBodyPathStep::ConditionalTrue,
                ]),
            ),
        ),
        ProjectionMode::Identity,
    );
    let after = host.project_type_store().semantic_graph().node_count();
    assert!(matches!(
        host.project_type_store()
            .semantic_graph()
            .node_data(true_branch)
            .as_deref(),
        Some(SemanticNodeData::Primitive(kind)) if *kind == PrimitiveKind::String
    ));
    assert_eq!(
        after - before,
        1,
        "a closed conditional true-branch must not intern the conditional or unselected siblings"
    );
}

#[test]
fn jsdoc_typedef_path_through_a_binder_binds_locally() {
    // A `@typedef` payload is the same authored type space as a decl body:
    // descending through `infer` must capture that binder, or `P` resolves
    // against the file-scope `boolean` alias planted below.
    let host = make_host();
    upsert(
        &host,
        "export type P = boolean;\n\
         /** @typedef {string extends infer P ? P : never} FromDoc */\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let locator = AuthoredBodyLocator::JsdocTypedefBody(JsdocTypedefBodyLocator {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(OWNER),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("FromDoc"),
            space: LocatorSymbolSpace::Type,
        },
        path: Arc::from([TypeBodyPathStep::ConditionalTrue]),
    });
    let node = force(
        &dispatch,
        &mint(&dispatch, locator),
        ProjectionMode::Identity,
    );
    assert_infer_ref(&host, node);
}

/// Bodies exercised by [`both_locator_navigators_select_the_same_authored_position`].
/// Each entry is `(body, path, label)`; the body is emitted twice — once
/// as a whole declaration body (pre-lowering navigator) and once nested
/// under a generic callable's return position (lowered-graph navigator).
fn navigator_parity_cases() -> Vec<(&'static str, Arc<[TypeBodyPathStep]>, &'static str)> {
    vec![
        (
            "{ a: string }",
            Arc::from([
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberKey,
            ]),
            "member-key-static",
        ),
        (
            "{ a: string; b: number }",
            Arc::from([
                TypeBodyPathStep::Member { ordinal: 1 },
                TypeBodyPathStep::MemberValue,
            ]),
            "member-value",
        ),
        (
            "string | number | boolean",
            Arc::from([TypeBodyPathStep::UnionArm { ordinal: 1 }]),
            "union-arm",
        ),
        (
            "{ a: string } & { b: number }",
            Arc::from([TypeBodyPathStep::IntersectionArm { ordinal: 1 }]),
            "intersection-arm",
        ),
        (
            "Wrap<number>",
            Arc::from([TypeBodyPathStep::TypeArgument { ordinal: 0 }]),
            "type-argument",
        ),
        (
            "[string, number]",
            Arc::from([TypeBodyPathStep::TupleElement { ordinal: 1 }]),
            "tuple-element",
        ),
        (
            "{ a: string }[\"a\"]",
            Arc::from([TypeBodyPathStep::IndexedAccessIndex]),
            "indexed-access-index",
        ),
        (
            "{ a: string }[\"a\"]",
            Arc::from([TypeBodyPathStep::IndexedAccessObject]),
            "indexed-access-object",
        ),
        (
            "{ [Q in \"x\" | \"y\"]: string }",
            Arc::from([TypeBodyPathStep::MappedSource]),
            "mapped-source",
        ),
        (
            "string extends number ? boolean : symbol",
            Arc::from([TypeBodyPathStep::ConditionalCheck]),
            "conditional-check",
        ),
        (
            "string extends number ? boolean : symbol",
            Arc::from([TypeBodyPathStep::ConditionalExtends]),
            "conditional-extends",
        ),
        (
            "string extends number ? boolean : symbol",
            Arc::from([TypeBodyPathStep::ConditionalFalse]),
            "conditional-false",
        ),
        (
            "{ m(a: string): number }",
            Arc::from([
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::FunctionParam { ordinal: 0 },
            ]),
            "member-function-param",
        ),
        (
            "{ m(a: string): number }",
            Arc::from([
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::FunctionReturn,
            ]),
            "member-function-return",
        ),
        (
            "{ [key: string]: number }",
            Arc::from([
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::IndexSignatureKey,
            ]),
            "index-signature-key",
        ),
        (
            "{ [key: string]: number }",
            Arc::from([
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::IndexSignatureValue,
            ]),
            "index-signature-value",
        ),
    ]
}

#[test]
fn both_locator_navigators_select_the_same_authored_position() {
    // Two independent navigators walk the same closed `TypeBodyPathStep`
    // vocabulary: the pre-lowering `TypeExpr` walk (taken when the descent
    // crosses no binder) and the post-lowering graph walk (taken once the
    // descent passes THROUGH a binder-introducing node, so the ancestor is
    // lowered first). Both must land on the SAME authored position for
    // every shared step kind.
    //
    // The fixture emits each body twice: once as a whole declaration body,
    // and once as the return of a GENERIC callable member, which is what
    // makes the second leg cross a binder and take the lowered-graph
    // navigator. The wrapper's own parameter is never referenced, so the
    // two legs address a byte-identical authored sub-expression.
    //
    // The oracle is the selected node's `SemanticNodeData` VARIANT paired
    // with its rendered type — see the `describe` closure below for why
    // the raw interned ids are not comparable across the two legs. The
    // variant is exactly what a text-only comparison cannot see: a
    // carrier (`DeclRef` / `InstantiationRef`), an `Alias` wrapper, and
    // the `Opaque` a failed lowered-graph walk degrades to can all render
    // identically to the materialized node they stand for.
    //
    // Two vocabulary members cannot appear in this table because they are
    // valid ONLY as a path's FIRST step, so neither can ever follow a
    // crossing hop. Their mid-path behaviour is proven separately by
    // `first_step_only_vocabulary_fails_closed_mid_path_on_direct_and_crossing_routes`
    // rather than assumed here.
    for (body, path, label) in navigator_parity_cases() {
        let host = make_host();
        upsert(
            &host,
            &format!(
                "export type Wrap<W> = {{ w: W }};\n\
                 export type Direct = {body};\n\
                 export type Crossed = {{ m: <Bound>(guard: Bound) => {body} }};\n"
            ),
        );
        let dispatch = ProjectSemanticDispatch::new(&host);
        let crossed_path: Arc<[TypeBodyPathStep]> = {
            let mut steps = vec![
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
                TypeBodyPathStep::FunctionReturn,
            ];
            steps.extend(path.iter().cloned());
            Arc::from(steps.into_boxed_slice())
        };
        let select = |symbol: &str, path: Arc<[TypeBodyPathStep]>| match dispatch
            .force_semantic_operand(
                &mint(&dispatch, locator(symbol, path)),
                request(ProjectionMode::Identity),
            ) {
            QueryResult::Value(forced) => Ok(forced.node()),
            QueryResult::Error(error) => Err(error),
            other => panic!("{label}/{symbol}: unexpected outcome {other:?}"),
        };
        // Interning is compound over `(payload, scope)` and the two legs
        // sit under different top-level owners, so equal node IDS are not
        // available as the oracle. The two scope-independent facts that
        // ARE comparable: the node's VARIANT and its rendered type. The
        // variant is what a text-only comparison cannot see — a carrier
        // (`DeclRef` / `InstantiationRef`), an `Alias` wrapper, and the
        // `Opaque(Miss)` a failed lowered-graph walk falls back to can all
        // render identically to the materialized node they stand for.
        let describe = |node: SemanticNodeId| {
            let data = host
                .project_type_store()
                .semantic_graph()
                .node_data(node)
                .unwrap_or_else(|| panic!("{label}: selected node must have data"));
            assert!(
                !matches!(data.as_ref(), SemanticNodeData::Opaque(_)),
                "{label}: a selection must not degrade to an opaque node, got {data:?}"
            );
            let rendered = crate::typeinfo::raise::render_node_display_with_ctx(&host, node)
                .unwrap_or_else(|| panic!("{label}: selection must render"))
                .text;
            (std::mem::discriminant(data.as_ref()), rendered)
        };
        match (
            select("Direct", Arc::clone(&path)),
            select("Crossed", crossed_path),
        ) {
            (Ok(direct), Ok(crossed)) => assert_eq!(
                describe(direct),
                describe(crossed),
                "{label}: the pre-lowering and lowered-graph navigators selected \
                 different nodes"
            ),
            // A step kind both navigators fail closed on (a static member
            // key denotes no authored type expression) must fail closed in
            // BOTH — a route that silently substitutes the member's VALUE
            // for its missing key would answer here.
            (Err(_), Err(_)) => {}
            (direct, crossed) => panic!(
                "{label}: the two navigators disagree on resolvability — \
                 direct={direct:?}, crossed={crossed:?}"
            ),
        }
    }
}

#[test]
fn binder_frame_visibility_separates_constraint_default_and_body_positions() {
    // TypeScript's binder visibility differs per authored position: a
    // constraint sees every sibling parameter, a default sees prior
    // siblings only, and the body sees the final frame. The sealing
    // authority derives that frame from the locator — a caller never
    // supplies it — and the derivation must keep the three frames, and the
    // per-parameter ordinals within a frame, distinct.
    //
    // The classifier assertions are the load-bearing ones: they fail
    // against a derivation that collapses every position onto the body
    // frame, or that drops the ordinal. The family-key assertions below
    // are the end-to-end consequence (the locator path separates these
    // families on its own, so they cannot stand in for the classifier).
    let host = make_host();
    upsert(
        &host,
        "export type Framed<A extends string, B extends A = A> = { a: A; b: B };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let context = ProjectionReductionContext::published(ProjectionMode::Identity);
    let frames: [Arc<[TypeBodyPathStep]>; 4] = [
        Arc::from([]),
        Arc::from([TypeBodyPathStep::TypeParamBound {
            ordinal: 1,
            position: TypeParamBoundPosition::Constraint,
        }]),
        Arc::from([TypeBodyPathStep::TypeParamBound {
            ordinal: 1,
            position: TypeParamBoundPosition::Default,
        }]),
        Arc::from([TypeBodyPathStep::TypeParamBound {
            ordinal: 0,
            position: TypeParamBoundPosition::Constraint,
        }]),
    ];
    let mut binders = Vec::new();
    let mut identities = Vec::new();
    for path in frames {
        let selected = locator("Framed", path);
        binders.push(OperandBinderIdentity::for_locator(&selected));
        let operand = mint(&dispatch, selected);
        let SemanticOperandParts::Authored(authored) =
            operand.parts(SemanticOperandAuthority::mint_for_forcing_boundary())
        else {
            unreachable!()
        };
        identities.push((
            authored.query_identity(),
            force_key(&dispatch, &operand, context),
        ));
    }
    for (left, right) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
        assert_ne!(
            binders[left], binders[right],
            "the authority-derived binder frame must separate {left} from {right}"
        );
        assert_ne!(
            identities[left].0, identities[right].0,
            "binder frames must not alias in the sealed authored identity"
        );
        assert_ne!(
            identities[left].1, identities[right].1,
            "binder frames must not alias in the forced family key"
        );
    }
    // Same frame, same ordinal, different declaration body path: one
    // binder frame, so the derivation must NOT invent a difference.
    assert_eq!(
        OperandBinderIdentity::for_locator(&locator("Framed", Arc::<[TypeBodyPathStep]>::from([]))),
        OperandBinderIdentity::for_locator(&locator(
            "Framed",
            Arc::<[TypeBodyPathStep]>::from([TypeBodyPathStep::Member { ordinal: 1 }])
        )),
    );
}

#[test]
fn republication_inside_the_environment_read_window_refuses_to_seal() {
    // The five environment dimensions must be sealed from ONE published
    // snapshot. Agreement between two composite reads is not sufficient:
    // a republication can land between the halves and produce a spliced
    // tuple whose components still compare equal.
    //
    // The seam performs a REAL unrelated content upsert inside every read
    // window, which advances the workspace publication epoch while leaving
    // the owner's own environment values byte-identical. The seal must
    // refuse as unstable rather than accept the torn window.
    // The seam needs the host from inside a `'static` closure, so the
    // fixture host is shared rather than owned by the test frame.
    let host = Arc::new(make_host());
    upsert(&host, SOURCE_V1);
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let operand = mint(&dispatch, whole("Owned"));

    let seam_host = Arc::clone(&host);
    let republications = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&republications);
    *host.test_force.semantic_operand_env_window_seam.0.lock() = Some(Arc::new(move || {
        let round = counter.fetch_add(1, Ordering::Relaxed);
        upsert_at(
            &seam_host,
            OTHER_OWNER,
            &format!("export type Unrelated = {round};\n"),
        );
    }));

    assert_operand_error(
        dispatch.force_semantic_operand(&operand, request(ProjectionMode::Identity)),
        QueryError::UnstableState { attempts: 3 },
    );
    assert_eq!(republications.load(Ordering::Relaxed), 3);

    // With the window quiet again the same operand seals and forces.
    *host.test_force.semantic_operand_env_window_seam.0.lock() = None;
    let settled = force(&dispatch, &operand, ProjectionMode::Identity);
    assert!(matches!(
        host.project_type_store()
            .semantic_graph()
            .node_data(settled)
            .as_deref(),
        Some(SemanticNodeData::Object(_))
    ));
}

#[test]
fn refusal_after_a_completed_child_still_withholds_the_forced_candidate() {
    // The neighbouring no-warm coverage arms its refusal BEFORE the nested
    // locator lowering runs, so the child never completes. This one fires
    // AFTER the child has finished and published its own candidate, which
    // is the case that could leave a half-finished force behind: the
    // force's own family slot must still publish nothing, and the answer
    // recomputed afterwards must equal a fresh host's.
    //
    // The locator is a NESTED member position: the post-child seam lives on
    // the authored route, after the locator-lowering child a selected
    // position's force depends on (a whole-declaration force takes the
    // declaration Instantiate route and has no locator child to complete).
    for leg in ["cancel", "budget"] {
        let host = make_host();
        upsert(&host, SOURCE_V1);
        let dispatch = ProjectSemanticDispatch::new(&host);
        let operand = mint(&dispatch, member_value("Owned", 0));
        let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
        let forced_key = force_key(&dispatch, &operand, context);

        let ctx = RequestContext::with_kind_timing_and_projection_budget(
            11,
            Arc::from(OWNER),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            64,
        );
        let seam_ctx = Arc::clone(&ctx);
        let cancelling = leg == "cancel";
        *host.test_force.semantic_operand_post_child_seam.0.lock() = Some(Arc::new(move || {
            if cancelling {
                seam_ctx.cancel();
            } else {
                while !seam_ctx.projection_budget.check_projection_op_count() {}
            }
        }));
        let guard = RequestContextGuard::install(Arc::clone(&ctx));

        let result =
            dispatch.force_semantic_operand(&operand, SemanticOperandForceRequest::new(context));
        if cancelling {
            assert!(
                matches!(result, QueryResult::Error(QueryError::Cancelled)),
                "{leg}: expected a typed cancellation, got {result:?}"
            );
        } else {
            assert!(
                matches!(result, QueryResult::Error(QueryError::BudgetExceeded(_))),
                "{leg}: expected a typed budget refusal, got {result:?}"
            );
        }
        assert_eq!(
            host.project_type_store()
                .semantic_graph()
                .slot_candidate_count_for_tests(&forced_key),
            0,
            "{leg}: the refused force must publish no candidate of its own"
        );
        drop(guard);

        // Whatever the independently-complete child left behind, the
        // recomputed answer must equal a fresh host's — a refusal past a
        // completed child may never poison the shared entry it produced.
        let replay = force(&dispatch, &operand, ProjectionMode::Expanded);
        let replay = crate::typeinfo::raise::render_node_display_with_ctx(&host, replay)
            .expect("replayed force must render");
        let fresh_host = make_host();
        upsert(&fresh_host, SOURCE_V1);
        let fresh_dispatch = ProjectSemanticDispatch::new(&fresh_host);
        let fresh = force(
            &fresh_dispatch,
            &mint(&fresh_dispatch, member_value("Owned", 0)),
            ProjectionMode::Expanded,
        );
        let fresh = crate::typeinfo::raise::render_node_display_with_ctx(&fresh_host, fresh)
            .expect("fresh force must render");
        assert_eq!(replay.text, fresh.text, "{leg}: recomputed answer diverged");
        assert_eq!(replay.degraded, fresh.degraded);
    }
}

#[test]
fn sealing_refuses_an_absent_or_wrong_space_anchor() {
    // Sealing an operand for a declaration that does not exist, or for one
    // that lives in the other symbol space than the locator names, is a
    // typed refusal AT THE SEAL. Admitting either would defer the failure
    // to a generic force-time miss, or worse lower a value declaration
    // under a type-space binder frame.
    let host = make_host();
    upsert(
        &host,
        "export const value = 1;\nexport type Alias = string;\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);

    assert!(matches!(
        dispatch.mint_authored_semantic_operand(
            locator_at(OWNER, "Absent", LocatorSymbolSpace::Type, Arc::from([])),
            Arc::from([]),
        ),
        Err(SemanticOperandMintError::MissingAuthoredDeclaration)
    ));
    assert!(matches!(
        dispatch.mint_authored_semantic_operand(
            locator_at(OWNER, "value", LocatorSymbolSpace::Type, Arc::from([])),
            Arc::from([]),
        ),
        Err(SemanticOperandMintError::WrongAnchorSpace {
            expected: LocatorSymbolSpace::Type,
            actual: LocatorSymbolSpace::Value,
        })
    ));
    assert!(matches!(
        dispatch.mint_authored_semantic_operand(
            locator_at(OWNER, "Alias", LocatorSymbolSpace::Value, Arc::from([])),
            Arc::from([]),
        ),
        Err(SemanticOperandMintError::WrongAnchorSpace {
            expected: LocatorSymbolSpace::Value,
            actual: LocatorSymbolSpace::Type,
        })
    ));
    // Positive control: both spaces seal when the locator names the space
    // the declaration actually lives in.
    assert!(dispatch
        .mint_authored_semantic_operand(
            locator_at(OWNER, "Alias", LocatorSymbolSpace::Type, Arc::from([])),
            Arc::from([]),
        )
        .is_ok());
    assert!(dispatch
        .mint_authored_semantic_operand(
            locator_at(OWNER, "value", LocatorSymbolSpace::Value, Arc::from([])),
            Arc::from([]),
        )
        .is_ok());
}

#[test]
fn first_step_only_vocabulary_fails_closed_mid_path_on_direct_and_crossing_routes() {
    // `MergedContributor` and `TypeParamBound` address positions that
    // exist only at a declaration's ROOT: a contributor ordinal indexes a
    // merged declaration's contributor list, and a bound ordinal indexes a
    // declaration header's parameter list. Neither is a sub-expression
    // position, so mid-path both are unresolvable.
    //
    // `both_locator_navigators_select_the_same_authored_position` excludes
    // them from its parity table for that reason. The exclusion is only
    // sound if a stray contributor/bound step really is refused mid-path —
    // a route that quietly reinterpreted one as, say, the enclosing
    // expression itself would return a WRONG authored position rather than
    // a refusal, and the parity table would never see it.
    //
    // That refusal has exactly ONE owner. The pre-lowering `TypeExpr` walk
    // runs over the WHOLE path first — it is also what decides whether a
    // binder is crossed — so it rejects the stray step before the
    // lowered-graph route can be selected at all. The lowered-graph
    // navigator keeps its own fail-closed arm as a backstop, but that arm
    // is unreachable for this input class and is NOT what this test
    // discriminates; do not read the two legs below as covering two
    // independent refusals.
    //
    // Both legs are still load-bearing and each fails on its own if that
    // single refusal is removed: the direct leg pins the plain route, and
    // the crossing leg pins that binder-crossing DETECTION does not
    // smuggle a stray step past the same refusal on its way to deciding it
    // must lower the ancestor instead.
    let host = make_host();
    upsert(
        &host,
        "export type Direct = { a: string };\n\
         export type Crossed = { m: <Bound>(guard: Bound) => { a: string } };\n",
    );
    let dispatch = ProjectSemanticDispatch::new(&host);
    let stray: [(TypeBodyPathStep, &str); 2] = [
        (
            TypeBodyPathStep::MergedContributor { ordinal: 0 },
            "merged-contributor",
        ),
        (
            TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            },
            "type-param-bound",
        ),
    ];
    for (step, label) in stray {
        // Direct leg: `Direct` -> member 0 -> <stray>. The prefix crosses
        // no binder, so the selected position is served directly.
        let direct = vec![TypeBodyPathStep::Member { ordinal: 0 }, step];
        // Crossing leg: the same stray step after a generic callable's
        // return, so the prefix crosses that callable's own binder and
        // the selected position can only be reached by lowering the
        // enclosing declaration.
        let crossed = vec![
            TypeBodyPathStep::Member { ordinal: 0 },
            TypeBodyPathStep::MemberValue,
            TypeBodyPathStep::FunctionReturn,
            TypeBodyPathStep::Member { ordinal: 0 },
            step,
        ];
        for (symbol, path) in [("Direct", direct), ("Crossed", crossed)] {
            let selected = locator(symbol, Arc::from(path.into_boxed_slice()));
            // The refusal may land at either admission point: the seal
            // (a typed mint error) or the force (a typed query error).
            // Both are fail-closed; silently returning a node is not.
            match dispatch.mint_authored_semantic_operand(selected, Arc::from([])) {
                Err(_) => {}
                Ok(operand) => {
                    let outcome = dispatch
                        .force_semantic_operand(&operand, request(ProjectionMode::Identity));
                    assert!(
                        matches!(outcome, QueryResult::Error(_)),
                        "{label}/{symbol}: a first-step-only step used mid-path must fail \
                         closed, got {outcome:?}"
                    );
                }
            }
        }
    }
}
