//! `this` inside a class declaration's members: the class's polymorphic
//! `this` type, bound to the reference a member is read through
//! (`Sub['me']` is `Sub`, `G<string>['me']` is `G<string>`), and read
//! member by member where each member is declared, so a member body
//! reading `this.v` never re-enters its own return.
//!
//! Every expected answer is TypeScript 7.0.2's, measured on this exact
//! fixture with `export const a: null = null! as <probe>;` read off the
//! TS2322 message (`tsc --noEmit --strict --ignoreConfig`).

use super::checker_probe_lane_tests::mismatches;

const FIXTURE: &str = "\
export class D { v = 1; me() { return this; } read() { return this.v; } get g() { return this.v; } chain() { return this.me(); } f = () => this.v; nested() { return () => this; } later() { return this.after(); } after() { return 'a' as string; } }
export class G<T> { t!: T; me() { return this; } read() { return this.t; } }
export class Sub extends D { w = ''; own() { return this.w; } inherited() { return this.v; } }
export class Base2 { b() { return this; } }
export class Mid extends Base2 { }
export class Sub2 extends Base2 { again() { return this.b(); } }
";

/// A member returning `this` returns the class read through the
/// reference it is accessed on: its own class, a subclass that inherits
/// it, or the applied generic.
///
/// Measured on TypeScript 7.0.2: `ReturnType<D['me']>`,
/// `ReturnType<D['chain']>` and `ReturnType<ReturnType<D['nested']>>` are
/// `D`, `ReturnType<Sub['me']>` is `Sub`, `ReturnType<Mid['b']>` is `Mid`,
/// `ReturnType<Sub2['again']>` (an inherited `this`-returning method called
/// off the subclass's own `this`) is `Sub2`, and `ReturnType<G<string>['me']>`
/// is `G<string>`.
#[test]
fn a_member_returning_this_returns_the_receiver() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<D['me']>", "D"),
            ("ReturnType<D['chain']>", "D"),
            ("ReturnType<ReturnType<D['nested']>>", "D"),
            ("ReturnType<Sub['me']>", "Sub"),
            ("ReturnType<Mid['b']>", "Mid"),
            ("ReturnType<Sub2['again']>", "Sub2"),
            ("ReturnType<G<string>['me']>", "G<string>"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A member read or call off `this` reads the member where it is
/// declared: an own field, getter or later-declared method, an inherited
/// field, or a field typed by the class's own parameter.
///
/// Measured on TypeScript 7.0.2: `ReturnType<D['read']>`, `D['g']`,
/// `ReturnType<D['f']>` and `ReturnType<Sub['inherited']>` are `number`;
/// `ReturnType<D['later']>`, `ReturnType<Sub['own']>` and
/// `ReturnType<G<string>['read']>` are `string`.
#[test]
fn a_member_read_off_this_reads_the_declared_member() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("ReturnType<D['read']>", "number"),
            ("D['g']", "number"),
            ("ReturnType<D['f']>", "number"),
            ("ReturnType<Sub['inherited']>", "number"),
            ("ReturnType<D['later']>", "string"),
            ("ReturnType<Sub['own']>", "string"),
            ("ReturnType<G<string>['read']>", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const EXPRESSIONS: &str = "\
export function cls() { return class { v = 1; me() { return this; } read() { return this.v; } get g() { return this.v; } f = () => this.v; early() { return this.late(); } late() { return 'x' as string; } nested() { return () => this.v; } }; }
export class B0 { b = 1; bm() { return this; } }
export function sub() { return class extends B0 { own() { return this.b; } self() { return this.bm(); } }; }
export function named() { return class Named { me() { return this; } }; }
export class GB<T> { t!: T; gm() { return this; } }
export function gsub() { return class extends GB<string> { self2() { return this.gm(); } tt() { return this.t; } }; }
";

/// A class expression's members run against its instance: `this` is the
/// instance (bound to the class a member is read through), a member body
/// reads a sibling field, getter or later-declared method through it, and
/// an inherited member reads off the base.
///
/// Measured on TypeScript 7.0.2: `ReturnType<InstanceType<ReturnType<typeof
/// cls>>['me']>` is `(Anonymous class)`; its `read`, `f` and `nested`
/// returns and its `g` are `number`, its `early` return `string`;
/// `ReturnType<InstanceType<ReturnType<typeof sub>>['own']>` is `number`
/// and its `self` return `(Anonymous class)`; `ReturnType<InstanceType<
/// ReturnType<typeof named>>['me']>` is `Named`; over a generic base,
/// `ReturnType<InstanceType<ReturnType<typeof gsub>>['self2']>` is
/// `(Anonymous class)` and its `tt` return `string`.
#[test]
fn a_class_expression_member_reads_its_instance() {
    let failures = mismatches(
        EXPRESSIONS,
        &[
            (
                "ReturnType<InstanceType<ReturnType<typeof cls>>['me']>",
                "(Anonymous class)",
            ),
            (
                "ReturnType<InstanceType<ReturnType<typeof cls>>['read']>",
                "number",
            ),
            ("InstanceType<ReturnType<typeof cls>>['g']", "number"),
            (
                "ReturnType<InstanceType<ReturnType<typeof cls>>['f']>",
                "number",
            ),
            (
                "ReturnType<InstanceType<ReturnType<typeof cls>>['early']>",
                "string",
            ),
            (
                "ReturnType<ReturnType<InstanceType<ReturnType<typeof cls>>['nested']>>",
                "number",
            ),
            (
                "ReturnType<InstanceType<ReturnType<typeof sub>>['own']>",
                "number",
            ),
            (
                "ReturnType<InstanceType<ReturnType<typeof sub>>['self']>",
                "(Anonymous class)",
            ),
            (
                "ReturnType<InstanceType<ReturnType<typeof named>>['me']>",
                "Named",
            ),
            (
                "ReturnType<InstanceType<ReturnType<typeof gsub>>['self2']>",
                "(Anonymous class)",
            ),
            (
                "ReturnType<InstanceType<ReturnType<typeof gsub>>['tt']>",
                "string",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const LITERALS: &str = "\
export function objFn() { return { v: 1, read() { return this.v; }, get g() { return this.v; }, early() { return this.late(); }, late() { return 'x' as string; }, nested() { return () => this.v; } }; }
export const obj = { v: 1, read() { return this.v; }, get g() { return this.v; }, callM() { return this.later(); }, later() { return 'x' as string; }, viaGetter() { return this.g; }, nested() { return () => this.v; } };
export class S { static k = 1; static readonly rk = 2; static r() { return this.k; } static rr() { return this.rk; } static t() { return 1; } static u() { return this.t(); } static get g() { return this.k; } static vg() { return this.g; } }
";

/// A method or accessor of an object literal runs against the literal, a
/// function's returned literal and a variable's alike, and a static
/// member runs against its class: a member read or call off `this` reads
/// the member the literal (or class) declares.
///
/// Measured on TypeScript 7.0.2: over `objFn`'s literal the `read` and
/// `nested` returns and `g` are `number` and the `early` return is
/// `string`; over `obj` the `read`, `viaGetter` and `nested` returns and
/// `g` are `number` and the `callM` return `string`; over `S` the `r`,
/// `rr`, `u` and `vg` returns are `number`.
#[test]
fn an_object_literal_or_static_member_reads_its_receiver() {
    let failures = mismatches(
        LITERALS,
        &[
            ("ReturnType<ReturnType<typeof objFn>['read']>", "number"),
            ("ReturnType<typeof objFn>['g']", "number"),
            ("ReturnType<ReturnType<typeof objFn>['early']>", "string"),
            (
                "ReturnType<ReturnType<ReturnType<typeof objFn>['nested']>>",
                "number",
            ),
            ("ReturnType<typeof obj.read>", "number"),
            ("typeof obj.g", "number"),
            ("ReturnType<typeof obj.callM>", "string"),
            ("ReturnType<typeof obj.viaGetter>", "number"),
            ("ReturnType<ReturnType<typeof obj.nested>>", "number"),
            ("ReturnType<typeof S.r>", "number"),
            ("ReturnType<typeof S.rr>", "number"),
            ("ReturnType<typeof S.u>", "number"),
            ("ReturnType<typeof S.vg>", "number"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const FILE: &str = "/ws/receivers/members.ts";

/// The flow return of `name`'s member at `member` (its class-body or
/// literal ordinal; `None` for the declaration's own body), projected to
/// its type expression, with the degradation it carries.
fn served_return(
    source: &str,
    name: &str,
    member: Option<u32>,
) -> (
    Option<verter_type_expr::TypeExpr>,
    Option<crate::semantic_query::FlowReturnDegradation>,
) {
    use crate::semantic_query::{
        SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
    };
    let host = std::sync::Arc::new(crate::VerterHost::new_standalone(
        crate::types::HostConfig::default(),
    ));
    let _ = host.upsert(crate::types::UpsertRequest {
        canonical_id: Some(FILE.to_string()),
        input_id: FILE.to_string(),
        source: std::sync::Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(FILE)
            .static_resolution(),
        aliases: Vec::new(),
    });
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = super::ProjectSemanticDispatch::new(&host_ctx);
    let part = match member {
        Some(ordinal) => verter_type_expr::facts::FunctionPartIdentity::Member {
            member_path: std::sync::Arc::from([ordinal]),
        },
        None => verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
    };
    let key = crate::semantic_query::FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            std::sync::Arc::from(FILE),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            std::sync::Arc::from(name),
            part,
            0,
        ),
        normalized_type_args: std::sync::Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(FILE),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    };
    let super::QueryResult::Value(SemanticQueryOutput {
        value: SemanticQueryValue::FlowReturn(result),
        ..
    }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
    else {
        panic!("{name} must produce a value");
    };
    (
        host.project_node_to_type_expr_for_test(result.return_type()),
        result.degradation(),
    )
}

/// A static member's `this` is its class's constructor, and a method of a
/// variable's object literal reads the variable: both are the deferred
/// `typeof` of the declaration, read where a consumer demands it. A
/// literal a function returns has no name to defer to: a method returning
/// that literal's `this` makes the literal its own identity, the
/// checker's one recursive anonymous type, and the return is complete.
///
/// Measured on TypeScript 7.0.2: `ReturnType<typeof SS.s>` is `typeof SS`;
/// `ReturnType<typeof own.me>` is the literal `{ v: number; me(): ...; }`,
/// which `typeof own` names; `ReturnType<typeof selfFn>` is `{ v: number;
/// me(): ...; }`.
#[test]
fn a_receiver_returned_whole_is_the_declared_value() {
    const SOURCE: &str = "\
export class SS { static s() { return this; } }
export const own = { v: 1, me() { return this; } };
export function selfFn() { return { v: 1, me() { return this; } }; }
";
    let typeof_of = |name: &str| {
        verter_type_expr::TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: vec![name.to_string()],
            type_args: Vec::new(),
        })
    };
    assert_eq!(
        served_return(SOURCE, "SS", Some(0)),
        (Some(typeof_of("SS")), None),
        "a static member's `this` is `typeof SS`"
    );
    assert_eq!(
        served_return(SOURCE, "own", Some(1)),
        (Some(typeof_of("own")), None),
        "an object literal method's `this` is `typeof own`"
    );
    let (_, degradation) = served_return(SOURCE, "selfFn", None);
    assert_eq!(
        degradation, None,
        "the recursive anonymous literal is its own identity"
    );
}
