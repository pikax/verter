//! An interface or class declaration with `extends` heritage inherits its
//! bases' signatures by concatenation: its OWN signatures first, then each
//! base's in clause order, with no identical-signature dedup and no mixin
//! composition (TypeScript's `resolveObjectTypeMembers`). Its body is an
//! intersection node, but not an intersection type, so every consumer —
//! call resolution, the signature utilities, the relation engine — reads
//! the heritage order from `SignaturesOfType`, including through an alias
//! of the declaration and after generic instantiation.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture: `tsc --declaration --emitDeclarationOnly --strict` for an
//! inferred function return, and `declare const v: <probe>; export const
//! s: null = v;` read off the TS2322 message for a type probe.

use super::checker_probe_lane_tests::{mismatches, tuple_labels};
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};

const HERITAGE: &str = "\
interface A { (): string }
interface C extends A { (): number }
interface B { (): boolean }
interface D extends A, B {}
declare const c: C; declare const d: D;
export function callC() { return c(); }
export function callD() { return d(); }
interface NA { new (): { a: 1 } }
interface NC extends NA { new (): { c: 1 } }
interface NB { new (): { b: 1 } }
interface ND extends NA, NB {}
interface OA { (x: string): 'a-str'; (x: number): 'a-num' }
interface OC extends OA { (x: string): 'c-str'; (x: boolean): 'c-bool' }
declare const oc: OC;
export function ocStr() { return oc('s'); }
export function ocNum() { return oc(1); }
export function ocBool() { return oc(true); }
interface GA<T> { (): T }
interface GC<T> extends GA<T[]> { (): T }
declare const gc: GC<string>;
export function callGC() { return gc(); }
interface GSA { <T>(x: T): T[] }
interface GSC extends GSA { <T>(x: T): T }
declare const gsc: GSC;
export function callGSC() { return gsc(1 as number); }
type ToC = C;
declare const toc: ToC;
export function callToC() { return toc(); }
type ToGC = GC<string>;
interface HA { (): 'base' }
interface M2 extends HA { (): 'first' }
interface M2 { (): 'second' }
class KI implements A { }
class SB { constructor(x: string) {} }
class SD extends SB { }
interface RT extends A { (): 'x' }
declare const fnStr: () => string;
";

/// A derived declaration's own signatures come before its bases', for call
/// and construct signatures, an own overload set over a base set, generic
/// heritage, generic signatures, an alias of the declaration and a merged
/// declaration with heritage. Call resolution tries them in that order,
/// while `ReturnType` / `InstanceType` / `Parameters` read the LAST one.
///
/// Measured on TypeScript 7.0.2:
/// - `callC()` is `number` and `ReturnType<C>` is `string`; `callD()` is
///   `string` and `ReturnType<D>` is `boolean`;
/// - `InstanceType<NC>` is `{ a: 1; }` and `InstanceType<ND>` is
///   `{ b: 1; }`;
/// - `OC` carries `[c-str, c-bool, a-str, a-num]`: `oc('s')` is `"c-str"`,
///   `oc(1)` is `"a-num"`, `oc(true)` is `"c-bool"`, `ReturnType<OC>` is
///   `"a-num"` and `Parameters<OC>[0]` is `number`;
/// - `gc()` is `string` and `ReturnType<GC<string>>` is `string[]`;
///   `gsc(1 as number)` is `number` and `ReturnType<GSC>` is `unknown[]`;
/// - through `type ToC = C`, `toc()` is `number` and `ReturnType<ToC>` is
///   `string`; `ReturnType<ToGC>` is `string[]`;
/// - `M2` (`extends HA` in its first declaration) lists its merged own
///   signatures before the base's: `ReturnType<M2>` is `"base"`.
#[test]
fn a_derived_declaration_lists_its_own_signatures_before_its_bases() {
    let failures = mismatches(
        HERITAGE,
        &[
            ("ReturnType<C>", "string"),
            ("ReturnType<typeof callC>", "number"),
            ("ReturnType<D>", "boolean"),
            ("ReturnType<typeof callD>", "string"),
            ("InstanceType<NC>", "{ a: 1; }"),
            ("InstanceType<ND>", "{ b: 1; }"),
            ("ReturnType<OC>", "\"a-num\""),
            ("Parameters<OC>[0]", "number"),
            ("ReturnType<typeof ocStr>", "\"c-str\""),
            ("ReturnType<typeof ocNum>", "\"a-num\""),
            ("ReturnType<typeof ocBool>", "\"c-bool\""),
            ("ReturnType<GC<string>>", "string[]"),
            ("ReturnType<typeof callGC>", "string"),
            ("ReturnType<GSC>", "unknown[]"),
            ("ReturnType<typeof callGSC>", "number"),
            ("ReturnType<ToC>", "string"),
            ("ReturnType<typeof callToC>", "number"),
            ("ReturnType<ToGC>", "string[]"),
            ("ReturnType<M2>", "\"base\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `implements` contributes nothing to a class's instance type, and a class
/// extending a class inherits no call or construct signature on its
/// instance (a class declares none); its constructor side keeps the base
/// constructor's parameters. The relation engine reads the heritage list
/// too: each target signature needs a related source signature.
///
/// Measured on TypeScript 7.0.2: `KI extends A ? 'callable' : 'not'` is
/// `"not"`, `ConstructorParameters<typeof SD>` is `[x: string]`,
/// `typeof fnStr extends RT ? 'yes' : 'no'` is `"no"` (`() => string` has
/// no match for `RT`'s own `(): 'x'`), and both `RT extends () => string`
/// and `RT extends () => 'x'` are `"yes"`.
#[test]
fn class_heritage_and_relations_read_the_declarations_signatures() {
    let failures = mismatches(
        HERITAGE,
        &[
            ("KI extends A ? 'callable' : 'not'", "\"not\""),
            ("ConstructorParameters<typeof SD>[0]", "string"),
            ("typeof fnStr extends RT ? 'yes' : 'no'", "\"no\""),
            ("RT extends () => string ? 'yes' : 'no'", "\"yes\""),
            ("RT extends () => 'x' ? 'yes' : 'no'", "\"yes\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const IDENTICAL: &str = "\
interface DA { (x: string): string }
interface DB { (y: string): string }
interface DD extends DA, DB {}
interface DC extends DA { (y: string): string }
interface Root { (a: string): 'A' }
interface Left extends Root { (b1: number): 'B1' }
interface Right extends Root { (b2: boolean): 'B2' }
interface DM extends Left, Right {}
type DMI = Left & Right;
";

/// A declaration keeps every inherited signature, identical ones included;
/// only an intersection type dedups them. Two signatures that differ only
/// in their parameter NAMES are identical to the checker, so the name
/// `Parameters` reports is what shows which one is the last.
///
/// Measured on TypeScript 7.0.2: `Parameters<DD>` is `[y: string]` (both
/// kept, `DB`'s last) while `Parameters<DA & DB>` is `[x: string]` (the
/// repeat dropped); `Parameters<DC>` is `[x: string]` (own `y` first, the
/// base's `x` last). Over the diamond `DM extends Left, Right`, both
/// branches reach `Root`: `ReturnType<DM>` is `"A"` (the root's signature,
/// repeated, is the last) while `ReturnType<Left & Right>` is `"B2"`.
#[test]
fn a_declaration_keeps_identical_inherited_signatures() {
    assert_eq!(
        tuple_labels(IDENTICAL, "Parameters<DD>"),
        [Some("y".to_owned())],
        "both identical base signatures stay, and the second base's is the last"
    );
    assert_eq!(
        tuple_labels(IDENTICAL, "Parameters<DA & DB>"),
        [Some("x".to_owned())],
        "the intersection drops the identical repeat"
    );
    assert_eq!(
        tuple_labels(IDENTICAL, "Parameters<DC>"),
        [Some("x".to_owned())],
        "the own signature comes first even when a base's is identical to it"
    );
    let failures = mismatches(
        IDENTICAL,
        &[("ReturnType<DM>", "\"A\""), ("ReturnType<DMI>", "\"B2\"")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const MIXIN: &str = "\
interface MA { new (...args: any[]): { a: 1 } }
interface MB { new (x: string): { b: 1 } }
interface MX extends MA, MB {}
";

/// The intersection mixin rule does not apply to heritage: a base whose
/// only construct signature is `new (...args: any[]) => X` keeps it, and
/// no base's instance is mixed into another's.
///
/// Measured on TypeScript 7.0.2: `InstanceType<MX>` is `{ b: 1; }` and
/// `ConstructorParameters<MX>` is `[x: string]`, while
/// `InstanceType<MA & MB>` is `{ a: 1; } & { b: 1; }`.
#[test]
fn a_mixin_constructor_base_keeps_its_own_construct_signature() {
    let failures = mismatches(
        MIXIN,
        &[
            ("InstanceType<MX>", "{ b: 1; }"),
            ("ConstructorParameters<MX>[0]", "string"),
            ("InstanceType<MA & MB>", "{ a: 1; } & { b: 1; }"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A heritage body stays one through a member-wise rebuild: substituting a
/// binder inside it keeps its category, so the instantiated declaration
/// still inherits by concatenation — its own signature first, the base's
/// last — instead of turning into an intersection type.
#[test]
fn a_substituted_heritage_body_stays_a_declaration() {
    use crate::semantic_query::composite::{CompositeList, CompositeOriginCategory};
    use crate::semantic_query::{PrimitiveKind, SignatureKind, SignatureReturnCarrier};

    let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
    let dispatch = super::ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let binder = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("C"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: std::sync::Arc::from("T"),
    });
    let returning = |return_type| {
        graph.intern_node(SemanticNodeData::Signature {
            kind: SignatureKind::Call,
            params: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            return_type,
            type_parameters: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            occurrence: None,
            return_carrier: SignatureReturnCarrier::Declared(return_type),
            signature_span: None,
            return_type_span: None,
            predicate: None,
        })
    };
    let callable = |signature| {
        graph.intern_node(SemanticNodeData::Object(crate::test_surface_view! {
            members: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: std::sync::Arc::from(vec![signature].into_boxed_slice()),
            construct_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }))
    };
    let base_signature = returning(string);
    let base = callable(base_signature);
    let own = callable(returning(binder));
    let body = graph.intern_node(SemanticNodeData::Intersection(CompositeList::heritage(
        std::sync::Arc::from(vec![base, own].into_boxed_slice()),
    )));

    let instantiated = dispatch.substitute_semantic_type_param(body, binder, number);
    assert_ne!(instantiated, body, "the binder occurs in the own body");
    assert!(
        matches!(
            graph.node_data(instantiated).as_deref(),
            Some(SemanticNodeData::Intersection(members))
                if members.origin_category() == CompositeOriginCategory::Heritage
        ),
        "a substituted heritage body keeps its category"
    );
    let super::signature_discovery::SharedSignatureNodes::Nodes(signatures) =
        dispatch.shared_signature_nodes(instantiated, SignatureKind::Call)
    else {
        panic!("the instantiated body's signatures settle");
    };
    assert_eq!(signatures.len(), 2);
    assert_eq!(
        signatures.last(),
        Some(&base_signature),
        "own signature first, the base's last"
    );
}

/// The one rebuild rule every order-preserving rebuild site applies: a
/// heritage body rebuilds as a heritage body, every other category as a
/// preserving rebuild.
#[test]
fn a_rebuilt_heritage_body_keeps_its_category() {
    use crate::semantic_query::composite::{
        CompositeList, CompositeOriginCategory, IntersectionKind,
    };
    let arms: std::sync::Arc<[SemanticNodeId]> =
        std::sync::Arc::from(vec![SemanticNodeId(1), SemanticNodeId(2)].into_boxed_slice());
    let rebuilt = |category| {
        CompositeList::<IntersectionKind>::rebuilt_from(category, std::sync::Arc::clone(&arms))
            .origin_category()
    };
    assert_eq!(
        rebuilt(CompositeOriginCategory::Heritage),
        CompositeOriginCategory::Heritage
    );
    assert_eq!(
        rebuilt(CompositeOriginCategory::OrderedCarrier),
        CompositeOriginCategory::PreservingRebuild
    );
}
