//! Nominal (`unique symbol`) contracts for the ONE shared relation
//! authority — decided at the public host boundary.
//!
//! TypeScript's only nominal type is `unique symbol`: two declarations
//! denote DIFFERENT types even though both widen to the same `symbol`
//! primitive, and ONE declaration denotes one type however many aliases,
//! imports, re-exports, or namespace qualifiers a reference travelled
//! through. These tests pin that the live nominal relation kinds read the
//! DECLARING identity through the shared dispatch, that a subject whose
//! content the oracle never read stays undecided rather than being answered
//! in either direction.
//!
//! Everything here enters through the public host surface
//! (`get_component_meta`) or the `for_tests` shims over the canonical dispatch — there is no
//! hand-interned node fixture in this file, so every assertion judges the
//! carriers real consumers reach.

use std::sync::Arc;

use verter_session::for_tests::{
    dispatch_execute_relate_verdict_for_tests, dispatch_execute_type_node_for_tests,
    dispatch_lower_type_expr_in_scope_with_context_for_tests,
    dispatch_relation_nominal_identity_for_tests, dispatch_resolve_type_decl_for_tests,
    relate_query_key_for_tests, RelateVerdictForTests,
};
use verter_session::semantic_query::{
    AuthoredPropertyKey, PrimitiveKind, ProjectionReductionContext, RelationKind, SemanticNodeData,
};
use verter_session::{AnalysisLevel, FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::{
    ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TupleElement, TypeExpr, ValueRef,
};

fn make_audit_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            audit_enabled: true,
            footprint_capture: false,
            ..HostConfig::default()
        },
        // Single-threaded scheduler: the flow fixtures below are
        // deterministic cold/warm traces, and a one-thread pool keeps the
        // audit counters they assert against free of cross-pool scheduling
        // noise (the same rationale the in-crate flow suites document).
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    ))
}

fn upsert(host: &VerterHost, canonical: &str, script: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(format!("{script}\nexport {{}};\n").as_str()),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// The nominal declaration corpus every relation fixture below shares: two
/// distinct `unique symbol` declarations, a non-unique `symbol`, an alias
/// chain, and one type imported from a module that does not exist.
const NOMINAL_SOURCE: &str = r#"
import type { AbsentKind } from "./absent-module";
export declare const A_KIND: unique symbol;
export declare const B_KIND: unique symbol;
export declare const C_KIND: unique symbol;
export declare const PLAIN: symbol;
export type KindA = typeof A_KIND;
export type KindAAlias = KindA;
export type KindB = typeof B_KIND;
export type KindC = typeof C_KIND;
export type KindAOrB = KindA | KindB;
export type PlainKind = typeof PLAIN;
export type UnreadableKind = AbsentKind;
export type LoopSelf = LoopOther;
export type LoopOther = LoopSelf;
"#;

/// `Identity` and `Comparable` are both live over the nominal axis and both
/// read the DECLARING identity, not the reference that reached them.
///
/// The rows discriminate the three outcomes that matter: the SAME unique
/// symbol (reached directly and through an alias) holds, two DISTINCT unique
/// symbols provably do not, and a subject the oracle could not read is
/// undecided — never silently answered as either. The undecided row also
/// pins the admission consequence: an undecided relation admits NO
/// candidate, so it can never be served warm as a decision.
#[test]
fn relation_unique_symbol_identity_and_comparability_are_tristate() {
    let host = make_audit_host();
    upsert(&host, "/relation-authority/decl.ts", NOMINAL_SOURCE);
    let node = |name: &str| {
        dispatch_resolve_type_decl_for_tests(&host, "/relation-authority/decl.ts", name)
    };

    // (source, target, Identity verdict, Comparable verdict)
    let rows: [(&str, &str, RelateVerdictForTests, RelateVerdictForTests); 6] = [
        // The same declaration, reached directly.
        (
            "KindA",
            "KindA",
            RelateVerdictForTests::Holds,
            RelateVerdictForTests::Holds,
        ),
        // The same declaration through an alias chain: the alias is not a
        // second nominal type.
        (
            "KindA",
            "KindAAlias",
            RelateVerdictForTests::Holds,
            RelateVerdictForTests::Holds,
        ),
        // Two distinct `unique symbol` declarations. Both widen to `symbol`,
        // so a structural-only oracle would call them one type; the nominal
        // axis is what proves they are two, and the negative `Comparable`
        // verdict is the disjointness proof a narrowing consumer needs.
        (
            "KindA",
            "KindB",
            RelateVerdictForTests::DoesNotHold,
            RelateVerdictForTests::DoesNotHold,
        ),
        // A nominal subject against a NON-unique `symbol`: they overlap (a
        // unique symbol IS a symbol), so comparability holds — while
        // IDENTITY is a structural question this bounded reduction does not
        // answer.
        (
            "KindA",
            "PlainKind",
            RelateVerdictForTests::Undecided,
            RelateVerdictForTests::Holds,
        ),
        // A subject the oracle never read decides nothing in either
        // direction.
        (
            "KindA",
            "UnreadableKind",
            RelateVerdictForTests::Undecided,
            RelateVerdictForTests::Undecided,
        ),
        // The SAME unread subject on both sides is STILL unread when the
        // identity unwrap cannot resolve it at all (an alias cycle: the
        // unwrap walks LoopSelf → LoopOther → LoopSelf and fails). The
        // canonicalization must not fall back to the raw pair and let the
        // fast path answer `Overlaps` from bare node equality on an operand
        // it never read — a positive that would be warm-admissible exactly
        // where the doctrine forbids a fact, and that the slow path (which
        // unwraps first) would answer `Unknown`, making the verdict flip
        // with the relation budget knob.
        (
            "LoopSelf",
            "LoopSelf",
            RelateVerdictForTests::Undecided,
            RelateVerdictForTests::Undecided,
        ),
    ];

    for (source_name, target_name, identity_verdict, comparable_verdict) in rows {
        let source = node(source_name);
        let target = node(target_name);
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                source,
                target,
                RelationKind::Identity
            ),
            identity_verdict,
            "Identity({source_name}, {target_name})"
        );
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                source,
                target,
                RelationKind::Comparable
            ),
            comparable_verdict,
            "Comparable({source_name}, {target_name})"
        );
    }

    // Comparability is a SYMMETRIC question, so it must not depend on which
    // operand the caller wrote first. A composite on the non-nominal side
    // has to distribute BEFORE the nominal side is widened to `symbol`:
    // widening first makes every nominal alternative overlap the widened
    // `symbol` and reports an overlap where the correct answer is a
    // disjointness proof. Both directions of a nominal-union / nominal pair
    // are pinned so a one-sided guard cannot pass.
    for (left_name, right_name, expected) in [
        ("KindAOrB", "KindC", RelateVerdictForTests::DoesNotHold),
        ("KindAOrB", "KindB", RelateVerdictForTests::Holds),
    ] {
        let left = node(left_name);
        let right = node(right_name);
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(&host, left, right, RelationKind::Comparable),
            expected,
            "Comparable({left_name}, {right_name})"
        );
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(&host, right, left, RelationKind::Comparable),
            expected,
            "Comparable({right_name}, {left_name}) must agree with the forward direction"
        );
    }

    // An undecided judgement has no public value form: it admits no
    // candidate, so a second ask recomputes instead of serving a decision
    // that was never made.
    for relation in [RelationKind::Identity, RelationKind::Comparable] {
        let key =
            relate_query_key_for_tests(&host, node("KindA"), node("UnreadableKind"), relation);
        assert_eq!(
            host.project_type_store()
                .semantic_graph()
                .slot_candidate_count_for_tests(&key),
            0,
            "{relation:?} over an unreadable subject must admit no candidate"
        );
    }

    // A DECIDED judgement does admit, and the two relation kinds occupy
    // DISTINCT slots over the same node pair — an `Identity` ask must never
    // be served the `Comparable` answer.
    let (a, b) = (node("KindA"), node("PlainKind"));
    for relation in [RelationKind::Identity, RelationKind::Comparable] {
        let _ = dispatch_execute_relate_verdict_for_tests(&host, a, b, relation);
    }
    let identity_key = relate_query_key_for_tests(&host, a, b, RelationKind::Identity);
    let comparable_key = relate_query_key_for_tests(&host, a, b, RelationKind::Comparable);
    assert_ne!(
        identity_key, comparable_key,
        "the relation kind is part of relation identity"
    );
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&comparable_key),
        1,
        "the decided comparability judgement admits its own candidate"
    );
}

/// A nominal TypeOf carrier is a terminal type at the public component-meta
/// boundary; every downstream resolver must publish the prop without a
/// self-resolution loop or failed publication.
#[test]
fn component_meta_publishes_unique_symbol_typed_prop() {
    let host = make_audit_host();
    for (canonical, source, language) in [
        (
            "/relation-authority/token.ts",
            "export declare const TOKEN: unique symbol;",
            FileLanguage::script_ts(),
        ),
        (
            "/relation-authority/TokenProp.vue",
            "<script setup lang=\"ts\">\nimport { TOKEN, TOKEN as RENAMED } from \"./token\";\ndefineProps<{ token: typeof TOKEN; renamed: typeof RENAMED }>();\n</script>\n<template><div /></template>",
            FileLanguage::vue(),
        ),
    ] {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .expect("component-meta fixture must upsert");
    }

    for _ in 0..2 {
        let meta = host
            .get_component_meta("/relation-authority/TokenProp.vue")
            .expect("component metadata must resolve");
        let prop = meta
            .props
            .iter()
            .find(|prop| prop.name == "token")
            .expect("the unique-symbol-typed prop must publish");
        assert!(!prop.publication.is_failed());
        assert!(prop.publication.source_position().present().is_some());
    }

    // The published TYPE is the discriminating half: `unique symbol` widens
    // to the shared `symbol` primitive, so a surface that lost the carrier
    // would still publish a healthy prop — just the wrong type. The public
    // boundary must hand the consumer the nominal reference.
    let output = host
        .get_component_meta_output("/relation-authority/TokenProp.vue")
        .expect("output materialization must not fail")
        .expect("component resolves");
    let (analysis, _resolution, types) = output.into_parts();
    let index = analysis
        .props
        .iter()
        .position(|prop| prop.name == "token")
        .expect("the unique-symbol-typed prop must publish");
    let lanes = types.into_lanes();
    let published = &lanes.props[index];
    assert!(
        matches!(
            published.materialized_type(),
            Some(TypeExpr::TypeOf(value_ref))
                if value_ref.path == ["TOKEN".to_string()] && value_ref.type_args.is_empty()
        ),
        "the published prop type must stay the nominal `typeof TOKEN` reference, never the \
         widened `symbol` primitive: {:?}",
        published.materialized_type()
    );
    assert_eq!(published.terminal_display().text(), Some("typeof TOKEN"));

    // The carrier's head is the AUTHORED reference, so a renamed import
    // publishes the LOCAL spelling. Canonicalising the head onto the
    // declaring name would hand the consumer `typeof TOKEN` — a symbol the
    // renaming file does not have in scope — while the declaring identity
    // that makes the two ONE type rides the node's payload instead.
    let renamed_index = analysis
        .props
        .iter()
        .position(|prop| prop.name == "renamed")
        .expect("the renamed-import prop must publish");
    let renamed = &lanes.props[renamed_index];
    assert!(
        matches!(
            renamed.materialized_type(),
            Some(TypeExpr::TypeOf(value_ref))
                if value_ref.path == ["RENAMED".to_string()] && value_ref.type_args.is_empty()
        ),
        "the renamed reference must publish its authored spelling: {:?}",
        renamed.materialized_type()
    );
    assert_eq!(renamed.terminal_display().text(), Some("typeof RENAMED"));
}

/// Nominal identity covers MEMBER declarations: a class static or an
/// object-annotation member annotated `unique symbol` is its OWN nominal
/// type, reached through `typeof Tokens.A` / `typeof CONFIG.K` and through
/// an inherited `typeof Derived.A` (which names the DECLARING base class).
///
/// The identity is the declaring anchor plus the member path, so two
/// distinct members of one class are two nominal types and a member never
/// aliases the declaration root or the shared `symbol` primitive. A member
/// whose annotation is NOT authored `unique symbol` still widens — that is
/// the honest projection for a non-nominal member, asserted by the
/// `plainSymbol` control.
#[test]
fn value_member_unique_symbols_carry_their_own_nominal_identity() {
    let host = make_audit_host();
    for (canonical, source, language) in [
        (
            "/relation-authority/member-tokens.ts",
            "export class Tokens {\n\
             static readonly A: unique symbol = Symbol();\n\
             static readonly B: unique symbol = Symbol();\n\
             static readonly plain: symbol = Symbol();\n\
             }\n\
             export class Derived extends Tokens {}\n\
             export class ShadowBase { static readonly K: unique symbol = Symbol(); }\n\
             export class ShadowDerived extends ShadowBase { static readonly K: symbol = Symbol(); }\n\
             export class InheritsOnly extends ShadowBase {}\n\
             export declare const CONFIG: { readonly K: unique symbol; readonly plainSymbol: symbol };\n\
             type Keys = { readonly Aliased: unique symbol };\n\
             export declare const ALIASED: Keys;\n\
             export interface NamedShape { readonly K: unique symbol }\n\
             export declare const FIRST: NamedShape;\n\
             export declare const SECOND: NamedShape;\n\
             type MutableShape = { K: unique symbol };\n\
             export declare const MUTABLE: MutableShape;\n\
             export declare const INTERSECTED: { readonly Left: unique symbol } & { readonly Right: unique symbol };",
            FileLanguage::script_ts(),
        ),
        (
            "/relation-authority/MemberTokenProp.vue",
            "<script setup lang=\"ts\">\nimport { Tokens, Derived, ShadowDerived, InheritsOnly, CONFIG, ALIASED, FIRST, SECOND, MUTABLE, INTERSECTED } from \"./member-tokens\";\ndefineProps<{ fromStatic: typeof Tokens.A; fromOtherStatic: typeof Tokens.B; fromDerived: typeof Derived.A; fromShadow: typeof ShadowDerived.K; fromInherited: typeof InheritsOnly.K; fromObject: typeof CONFIG.K; fromAliased: typeof ALIASED.Aliased; fromNamedFirst: typeof FIRST.K; fromNamedSecond: typeof SECOND.K; fromMutable: typeof MUTABLE.K; fromIntersected: typeof INTERSECTED.Left; plainStatic: typeof Tokens.plain; plainObject: typeof CONFIG.plainSymbol }>();\n</script>\n<template><div /></template>",
            FileLanguage::vue(),
        ),
    ] {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source),
                file_language: language,
                aliases: Vec::new(),
            })
            .expect("member-bound fixture must upsert");
    }

    let lower = |expr: &TypeExpr| {
        dispatch_lower_type_expr_in_scope_with_context_for_tests(
            &host,
            "/relation-authority/member-tokens.ts",
            expr,
            ProjectionReductionContext::structural_transit(),
        )
        .expect("the member typeof fixture must lower")
    };
    let typeof_member = |path: [&str; 2]| {
        TypeExpr::TypeOf(ValueRef {
            path: path.iter().map(|s| s.to_string()).collect(),
            type_args: Vec::new(),
        })
    };
    let static_a = lower(&typeof_member(["Tokens", "A"]));
    let static_b = lower(&typeof_member(["Tokens", "B"]));
    let derived_a = lower(&typeof_member(["Derived", "A"]));
    let shadow_k = lower(&typeof_member(["ShadowDerived", "K"]));
    let inherited_k = lower(&typeof_member(["InheritsOnly", "K"]));
    let object_k = lower(&typeof_member(["CONFIG", "K"]));
    let aliased = lower(&typeof_member(["ALIASED", "Aliased"]));
    let named_first = lower(&typeof_member(["FIRST", "K"]));
    let named_second = lower(&typeof_member(["SECOND", "K"]));
    let mutable_k = lower(&typeof_member(["MUTABLE", "K"]));
    let intersected = lower(&typeof_member(["INTERSECTED", "Left"]));
    let plain_static = lower(&typeof_member(["Tokens", "plain"]));
    let plain_object = lower(&typeof_member(["CONFIG", "plainSymbol"]));

    // Identity presence + distinctness: each unique member is its own
    // nominal type; nothing aliases the shared primitive.
    let identity_of = |node| dispatch_relation_nominal_identity_for_tests(&host, node);
    let a_identity = identity_of(static_a).expect("`typeof Tokens.A` carries nominal identity");
    assert_eq!(
        (a_identity.canonical_id.as_ref(), a_identity.symbol.as_ref()),
        ("/relation-authority/member-tokens.ts", "Tokens"),
        "the identity names the DECLARING class"
    );
    assert_eq!(
        a_identity.member_path.as_ref(),
        &["A".to_string()],
        "the member path names the declaring member"
    );
    assert_eq!(
        identity_of(static_b)
            .map(|identity| identity.member_path.to_vec())
            .as_deref(),
        Some(&["B".to_string()][..]),
        "`typeof Tokens.B` is a DIFFERENT member type"
    );
    assert_eq!(
        identity_of(derived_a).as_ref(),
        Some(&a_identity),
        "an inherited static names the DECLARING base class, not the derived reference"
    );
    let k_identity = identity_of(object_k).expect("`typeof CONFIG.K` carries nominal identity");
    assert_eq!(k_identity.member_path.as_ref(), &["K".to_string()]);
    assert_ne!(a_identity, k_identity, "distinct members never alias");
    let aliased_identity =
        identity_of(aliased).expect("an aliased object member keeps unique-symbol identity");
    assert_eq!(
        aliased_identity.member_path.as_ref(),
        &["Aliased".to_string()]
    );
    assert_eq!(
        aliased_identity.symbol.as_ref(),
        "Keys",
        "a member certified through a named TYPE annotation anchors on the \
         TYPE declaration, never the annotated value"
    );
    let intersected_identity = identity_of(intersected)
        .expect("an intersection annotation member keeps unique-symbol identity");
    assert_eq!(
        intersected_identity.member_path.as_ref(),
        &["Left".to_string()]
    );
    for (node, label) in [(plain_static, "plainStatic"), (plain_object, "plainObject")] {
        assert!(
            identity_of(node).is_none(),
            "`{label}` (a non-unique annotation) carries no nominal identity"
        );
    }

    // A member certified through a NAMED type is ONE nominal type however
    // many values carry the annotation: tsc unifies `typeof FIRST.K` and
    // `typeof SECOND.K` through the single interface member, so minting
    // `(FIRST, [K])` and `(SECOND, [K])` would prove legal code disjoint and
    // a narrowing consumer would read `never` where the checker reads the
    // member's type. Both spellings must carry the SAME declaring identity —
    // the interface's — and the relation must answer accordingly.
    let first_identity =
        identity_of(named_first).expect("`typeof FIRST.K` carries nominal identity");
    assert_eq!(
        (
            first_identity.symbol.as_ref(),
            first_identity.member_path.as_ref()
        ),
        ("NamedShape", &["K".to_string()][..]),
        "the named-type member anchors on the certifying TYPE declaration"
    );
    assert_eq!(
        identity_of(named_second).as_ref(),
        Some(&first_identity),
        "every value carrying the same named-type annotation names ONE \
         member nominal type through it"
    );
    let relation = |source, target, kind| {
        dispatch_execute_relate_verdict_for_tests(&host, source, target, kind)
    };
    assert_eq!(
        relation(named_first, named_second, RelationKind::Identity),
        RelateVerdictForTests::Holds,
        "two spellings of one named-type member are the SAME nominal type"
    );
    assert_eq!(
        relation(named_first, named_second, RelationKind::Comparable),
        RelateVerdictForTests::Holds,
        "a named-type member never proves disjoint against itself through a \
         second annotated value"
    );
    assert_eq!(
        relation(named_first, static_a, RelationKind::Comparable),
        RelateVerdictForTests::DoesNotHold,
        "a named-type member and a DISTINCT member identity stay provably \
         disjoint — one nominal type per declaration, no aliasing either way"
    );

    // The heritage chase stops at a class that DECLARES the member itself:
    // `ShadowDerived` re-declares `K` with a non-nominal annotation (source
    // tsc rejects as a static-side incompatibility, but the pipeline still
    // lowers it), so `typeof ShadowDerived.K` is the override's own type and
    // must NOT inherit `ShadowBase`'s nominal identity — which would let the
    // relation prove the override's actual type disjoint from itself.
    assert!(
        identity_of(shadow_k).is_none(),
        "a re-declared non-unique static shadow carries no nominal identity"
    );
    let inherited_identity = identity_of(inherited_k)
        .expect("a non-shadowing subclass still resolves the base's member identity");
    assert_eq!(
        (
            inherited_identity.symbol.as_ref(),
            inherited_identity.member_path.as_ref()
        ),
        ("ShadowBase", &["K".to_string()][..]),
        "the heritage chase names the DECLARING base when nothing shadows it"
    );

    // The mutable-member guard: tsc widens (and errors on) a mutable
    // `K: unique symbol` member, so a non-`readonly` annotation never
    // certifies — the reference reads the widened primitive.
    assert!(
        identity_of(mutable_k).is_none(),
        "a mutable member annotation never certifies nominal identity"
    );

    // The relation reads the member identity exactly as it reads a
    // declaration root's: same member holds, distinct members are disjoint,
    // a member is disjoint from a distinct DECLARATION, and the plain
    // symbols stay comparable with everything.
    assert_eq!(
        relation(static_a, static_a, RelationKind::Identity),
        RelateVerdictForTests::Holds
    );
    assert_eq!(
        relation(static_a, derived_a, RelationKind::Identity),
        RelateVerdictForTests::Holds,
        "the inherited spelling is the same declaring identity"
    );
    for other in [static_b, object_k] {
        assert_eq!(
            relation(static_a, other, RelationKind::Identity),
            RelateVerdictForTests::DoesNotHold,
            "distinct unique members are different nominal types"
        );
        assert_eq!(
            relation(static_a, other, RelationKind::Comparable),
            RelateVerdictForTests::DoesNotHold,
            "distinct unique members are provably disjoint"
        );
    }
    assert_eq!(
        relation(plain_static, static_a, RelationKind::Comparable),
        RelateVerdictForTests::Holds,
        "a plain `symbol` overlaps every unique member"
    );

    // The PUBLIC boundary keeps the member nominal reference: the published
    // prop type is the authored `typeof` member reference, never the widened
    // `symbol` primitive and never the whole class-static surface.
    let output = host
        .get_component_meta_output("/relation-authority/MemberTokenProp.vue")
        .expect("output materialization must not fail")
        .expect("component resolves");
    let (analysis, _resolution, types) = output.into_parts();
    let lanes = types.into_lanes();
    for (name, expected_display) in [
        ("fromStatic", "typeof Tokens.A"),
        ("fromDerived", "typeof Derived.A"),
        ("fromShadow", "symbol"),
        ("fromInherited", "typeof InheritsOnly.K"),
        ("fromObject", "typeof CONFIG.K"),
        ("fromAliased", "typeof ALIASED.Aliased"),
        ("fromNamedFirst", "typeof FIRST.K"),
        ("fromNamedSecond", "typeof SECOND.K"),
        ("fromMutable", "symbol"),
        ("fromIntersected", "typeof INTERSECTED.Left"),
        ("plainStatic", "symbol"),
        ("plainObject", "symbol"),
    ] {
        let index = analysis
            .props
            .iter()
            .position(|prop| prop.name == name)
            .unwrap_or_else(|| panic!("the `{name}` prop must publish"));
        let published = &lanes.props[index];
        assert_eq!(
            published.terminal_display().text(),
            Some(expected_display),
            "the `{name}` prop must publish `{expected_display}`: {:?}",
            published.materialized_type()
        );
    }
}

/// Composite relation frames must preserve a nominal carrier until the
/// composite has distributed, and inference must see the carrier rather than
/// a premature concrete-kind rejection.
#[test]
fn nominal_assignability_survives_composite_and_inference_frames() {
    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/token.ts",
        "export declare const TOKEN: unique symbol;",
    );
    upsert(
        &host,
        "/relation-authority/composite.ts",
        "import { TOKEN, TOKEN as TOKEN_ALIAS } from \"./token\";\n\
         type Token = typeof TOKEN;\n\
         type TokenAlias = Token;\n\
         type SourceUnion = typeof TOKEN | typeof TOKEN_ALIAS;\n\
         type TargetUnion = Token | string;\n\
         type SourceTuple = [TokenAlias];\n\
         type TargetTuple = [typeof TOKEN];\n\
         type ParameterOf<F> = F extends (value: infer U) => void ? U : never;\n\
         type Inferred = ParameterOf<(value: typeof TOKEN) => void>;\n\
         type ReturnOf<F> = F extends () => infer R ? R : never;\n\
         type InferredReturn = ReturnOf<() => typeof TOKEN>;",
    );
    let node = |name: &str| {
        dispatch_resolve_type_decl_for_tests(&host, "/relation-authority/composite.ts", name)
    };
    let lower = |expr: &TypeExpr| {
        dispatch_lower_type_expr_in_scope_with_context_for_tests(
            &host,
            "/relation-authority/composite.ts",
            expr,
            ProjectionReductionContext::structural_transit(),
        )
        .expect("the relation fixture type must lower")
    };
    let typeof_value = |name: &str| {
        TypeExpr::TypeOf(ValueRef {
            path: vec![name.to_string()],
            type_args: Vec::new(),
        })
    };
    let type_ref = |name: &str| TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::<TypeExpr>::new().into_boxed_slice()),
    };
    let token = lower(&typeof_value("TOKEN"));
    assert!(
        dispatch_relation_nominal_identity_for_tests(&host, token).is_some(),
        "the direct typeof target must carry nominal identity",
    );
    let token_alias = lower(&typeof_value("TOKEN_ALIAS"));
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            token_alias,
            token,
            RelationKind::Assignable,
        ),
        RelateVerdictForTests::Holds,
    );
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            token,
            node("TokenAlias"),
            RelationKind::Identity,
        ),
        RelateVerdictForTests::Holds,
    );
    let source_union = lower(&TypeExpr::Union(Arc::from(
        vec![typeof_value("TOKEN"), typeof_value("TOKEN_ALIAS")].into_boxed_slice(),
    )));
    let target_union = lower(&TypeExpr::Union(Arc::from(
        vec![
            typeof_value("TOKEN"),
            TypeExpr::primitive(PrimitiveName::String),
        ]
        .into_boxed_slice(),
    )));
    let source_tuple = lower(&TypeExpr::Tuple {
        elements: Arc::from(
            vec![TupleElement {
                label: None,
                ty: type_ref("TokenAlias"),
                optional: false,
                rest: false,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    let target_tuple = lower(&TypeExpr::Tuple {
        elements: Arc::from(
            vec![TupleElement {
                label: None,
                ty: typeof_value("TOKEN"),
                optional: false,
                rest: false,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    });

    for (source, target, expected, boundary) in [
        (
            source_union,
            token,
            RelateVerdictForTests::Holds,
            "source-union distribution",
        ),
        (
            token,
            target_union,
            RelateVerdictForTests::Holds,
            "target-union distribution",
        ),
        (
            source_tuple,
            target_tuple,
            RelateVerdictForTests::Holds,
            "tuple-element carrier descent",
        ),
    ] {
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                source,
                target,
                RelationKind::Assignable,
            ),
            expected,
            "{boundary} must not publish a false negative",
        );
    }

    // BOTH inference directions must deposit the CARRIER. A parameter
    // position and a return position reach the deposit under opposite
    // variance, so a pin written for one direction does not cover the
    // other: a covariant deposit of the widened `symbol` would silently
    // hand every `ReturnType`-shaped inference the shared primitive and
    // lose the discriminant.
    for (name, boundary) in [
        ("Inferred", "contravariant inference"),
        ("InferredReturn", "covariant inference"),
    ] {
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                node(name),
                token,
                RelationKind::Identity,
            ),
            RelateVerdictForTests::Holds,
            "{boundary} must deposit the nominal carrier",
        );
    }

    // The carrier's HEAD stays the AUTHORED reference: a renamed import
    // interns its own node (the local spelling every display, locator, and
    // provenance consumer reads) while the nominal relation still answers
    // EQUAL against the direct reference. Canonicalising the head onto the
    // declaring name would publish a symbol that is not in scope here.
    assert_ne!(
        token_alias, token,
        "a renamed reference keeps its own authored head",
    );
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            token_alias,
            token,
            RelationKind::Identity,
        ),
        RelateVerdictForTests::Holds,
        "two spellings of ONE declaration are one type",
    );
    assert_eq!(
        dispatch_relation_nominal_identity_for_tests(&host, token_alias),
        dispatch_relation_nominal_identity_for_tests(&host, token),
        "the DECLARING identity is the same for both spellings",
    );
}

/// Shared required members are processed inside one bounded comparable
/// worklist; they do not open a fresh memoized relation query per member.
///
/// The member VALUES are distinct carrier-backed named types, and the
/// disjoint row proves the descent actually happened: the conflict is two
/// levels down (`b.v` is `number` on one side and `string` on the other), so
/// a comparator that only compared the two root surfaces — or that
/// short-circuited on interned-identical member pairs — would report an
/// overlap. Reaching the conflict requires unwrapping each declaration
/// carrier and descending, and the counter pins that all of it stayed inside
/// ONE relation frame.
#[test]
fn comparable_shared_members_use_one_relation_check() {
    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/bounded.ts",
        "type TextBox = { v: string };\n\
         type OtherTextBox = { v: string };\n\
         type NumberBox = { v: number };\n\
         type LeftShape = { a: TextBox; b: NumberBox; extra: boolean };\n\
         type RightShape = { a: OtherTextBox; b: NumberBox };\n\
         type ConflictShape = { a: TextBox; b: TextBox };",
    );
    let node = |name: &str| {
        dispatch_resolve_type_decl_for_tests(&host, "/relation-authority/bounded.ts", name)
    };
    let graph = host.project_type_store().semantic_graph();

    for (target_name, expected, boundary) in [
        (
            "RightShape",
            RelateVerdictForTests::Holds,
            "distinct-but-compatible member carriers overlap",
        ),
        (
            "ConflictShape",
            RelateVerdictForTests::DoesNotHold,
            "a conflict two levels below the root is still proved",
        ),
    ] {
        let source = node("LeftShape");
        let target = node(target_name);
        let before = graph.stats_snapshot().relation_check_count;
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                source,
                target,
                RelationKind::Comparable,
            ),
            expected,
            "{boundary}",
        );
        assert_eq!(
            graph.stats_snapshot().relation_check_count - before,
            1,
            "member descent must remain inside the root relation budget ({boundary})"
        );
        assert_eq!(
            host.project_type_store()
                .semantic_graph()
                .slot_candidate_count_for_tests(&relate_query_key_for_tests(
                    &host,
                    source,
                    target,
                    RelationKind::Comparable
                )),
            1,
            "the decided judgement admits exactly one candidate ({boundary})"
        );
    }
}

/// An UNREDUCED OPERATOR is UNDECIDED — never the permissive arm, never a
/// proof.
///
/// The oracle's permissive arm is a PROMISE ("no proof of empty overlap
/// exists"); an operand whose content was never read — an indexed access, a
/// mapped type, a `keyof`, a deferred `typeof` — cannot back that promise,
/// so answering `Holds` would convert missing knowledge into a positive,
/// memo-admitted fact. Both wrong directions are pinned per row: `Holds`
/// (a fabricated completeness, warm-admissible through the shared memo) and
/// `DoesNotHold` (a disjointness proof from an unread operand, which the
/// flow consumer publishes as a complete warm `never`). The exact verdict —
/// `Undecided` — also pins the admission consequence below: zero candidates,
/// so an unread operand can never warm-serve either answer.
#[test]
fn comparable_treats_unreduced_operands_as_undecided() {
    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/deferred.ts",
        "type Holder = { kind: \"a\"; other: \"b\" };\n\
         type MappedHolder = { [K in keyof Holder]: Holder[K] };\n\
         type SingleKey = { a: number };\n\
         type LiteralA = \"a\";\n\
         type LiteralB = \"b\";",
    );
    let node = |name: &str| {
        dispatch_resolve_type_decl_for_tests(&host, "/relation-authority/deferred.ts", name)
    };
    // Lowered DIRECTLY, so the operator node itself reaches the oracle
    // rather than a declaration carrier the relation unwrap already
    // flattened on the way in.
    let lower = |expr: &TypeExpr| {
        dispatch_lower_type_expr_in_scope_with_context_for_tests(
            &host,
            "/relation-authority/deferred.ts",
            expr,
            ProjectionReductionContext::structural_transit(),
        )
        .expect("the deferred fixture type must lower")
    };
    let type_ref = |name: &str| TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::<TypeExpr>::new().into_boxed_slice()),
    };
    let indexed = |object: &str, key: &str| TypeExpr::IndexedAccess {
        object: Arc::new(type_ref(object)),
        index: Arc::new(TypeExpr::Literal(verter_type_expr::LiteralValue::String(
            key.to_string(),
        ))),
    };

    for (subject, boundary) in [
        (lower(&indexed("Holder", "kind")), "an indexed access"),
        (lower(&indexed("MappedHolder", "kind")), "a mapped type"),
        (
            lower(&TypeExpr::KeyOf(Arc::new(type_ref("SingleKey")))),
            "a keyof operator",
        ),
    ] {
        for other in ["LiteralA", "LiteralB"] {
            assert_eq!(
                dispatch_execute_relate_verdict_for_tests(
                    &host,
                    subject,
                    node(other),
                    RelationKind::Comparable,
                ),
                RelateVerdictForTests::Undecided,
                "{boundary} is unread content: the oracle reports NO fact, never a \
                 permissive positive and never a proof",
            );
        }
    }

    // The admission consequence: an undecided comparability verdict admits
    // no candidate, so it cannot be warm-served as either answer.
    let subject = lower(&indexed("Holder", "kind"));
    let key =
        relate_query_key_for_tests(&host, subject, node("LiteralB"), RelationKind::Comparable);
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&key),
        0,
        "an unread operand must admit no comparability candidate"
    );
}

/// Readable composite roots are ordinary comparability inputs, not missing
/// nominal identity. Union alternatives are combined by overlap; other
/// composite roots keep the permissive overlap verdict unless a shared leaf
/// proves disjointness.
#[test]
fn comparable_classifies_readable_composite_roots() {
    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/comparable-composites.ts",
        "type StringOrNumber = string | number;\n\
         type LeftAndRight = { left: string } & { right: number };\n\
         type Base = { id: string };\n\
         type TaggedA = Base & { kind: \"a\" };\n\
         type TaggedB = Base & { kind: \"b\" };\n\
         type TaggedAlsoA = Base & { kind: \"a\" };\n\
         type TextArray = string[];\n\
         type TextTuple = [string];\n\
         type Callback = () => string;\n\
         type Label = `id-${string}`;\n\
         type Empty = {};\n\
         type BooleanType = boolean;\n\
         type StringType = string;",
    );
    let node = |name: &str| {
        dispatch_resolve_type_decl_for_tests(
            &host,
            "/relation-authority/comparable-composites.ts",
            name,
        )
    };

    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            node("StringOrNumber"),
            node("BooleanType"),
            RelationKind::Comparable,
        ),
        RelateVerdictForTests::DoesNotHold,
        "a union is disjoint only when every alternative is disjoint",
    );

    for name in ["LeftAndRight", "TextArray", "TextTuple", "Callback"] {
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                node(name),
                node("Empty"),
                RelationKind::Comparable,
            ),
            RelateVerdictForTests::Holds,
            "{name} is readable and has no proved-empty overlap",
        );
    }
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            node("Label"),
            node("StringType"),
            RelationKind::Comparable,
        ),
        RelateVerdictForTests::Holds,
        "a template-literal root remains comparable with string",
    );

    // A COMPOSED root has no members until its arms are merged. Reading
    // only the terminal tag would answer every intersection permissively
    // and lose the discriminated-union proof the oracle exists to supply:
    // `TaggedA` and `TaggedB` share a required `kind` whose literal values
    // conflict, so a narrow to `TaggedB` on a `TaggedA` subject must read
    // `never` on the positive edge rather than publish an inhabited
    // intersection. The positive control pins that the composition is a
    // real member descent and not a blanket negative.
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            node("TaggedA"),
            node("TaggedB"),
            RelationKind::Comparable,
        ),
        RelateVerdictForTests::DoesNotHold,
        "two intersections carrying a conflicting required discriminant are disjoint",
    );
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            node("TaggedA"),
            node("TaggedAlsoA"),
            RelationKind::Comparable,
        ),
        RelateVerdictForTests::Holds,
        "two intersections agreeing on every shared required member overlap",
    );
}

/// The DECLARING identity controls both nominal relations, so an import, a
/// re-export, and a renaming alias all still name ONE type.
///
/// This is the regression the nominal axis exists to prevent: if the
/// nominal identity were the referencing site rather than the declaration,
/// a discriminant imported into a consumer file would compare unequal to
/// itself and every cross-file `unique symbol` narrow would silently
/// invert.
#[test]
fn relation_nominal_identity_survives_import_and_reexport() {
    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/decl.ts",
        "export declare const A_KIND: unique symbol;\n\
         export declare const B_KIND: unique symbol;\n\
         export type DeclaredKindA = typeof A_KIND;",
    );
    upsert(
        &host,
        "/relation-authority/barrel.ts",
        "export { A_KIND, B_KIND } from \"./decl\";",
    );
    upsert(
        &host,
        "/relation-authority/consumer.ts",
        "import { A_KIND as RenamedA, B_KIND } from \"./barrel\";\n\
         export type ViaBarrelKindA = typeof RenamedA;\n\
         export type ViaBarrelKindB = typeof B_KIND;",
    );
    upsert(
        &host,
        "/relation-authority/direct.ts",
        "import { A_KIND } from \"./decl\";\n\
         export type DirectKindA = typeof A_KIND;",
    );

    let node =
        |canonical: &str, name: &str| dispatch_resolve_type_decl_for_tests(&host, canonical, name);
    let declared = node("/relation-authority/decl.ts", "DeclaredKindA");
    let via_barrel = node("/relation-authority/consumer.ts", "ViaBarrelKindA");
    let via_barrel_b = node("/relation-authority/consumer.ts", "ViaBarrelKindB");
    let direct = node("/relation-authority/direct.ts", "DirectKindA");

    // The three references travelled through different modules and a rename;
    // they are the same nominal type.
    for (label, other) in [
        ("re-export + rename", via_barrel),
        ("direct import", direct),
    ] {
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                declared,
                other,
                RelationKind::Identity
            ),
            RelateVerdictForTests::Holds,
            "{label}: the declaring identity controls Identity"
        );
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                declared,
                other,
                RelationKind::Comparable
            ),
            RelateVerdictForTests::Holds,
            "{label}: the declaring identity controls Comparable"
        );
    }
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            via_barrel,
            direct,
            RelationKind::Identity
        ),
        RelateVerdictForTests::Holds,
        "two consumer aliases of one declaration are one type"
    );

    // Travelling the same route does not make two DIFFERENT declarations one
    // type: the identity is the declaration, not the path taken to it.
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            via_barrel,
            via_barrel_b,
            RelationKind::Identity
        ),
        RelateVerdictForTests::DoesNotHold,
        "distinct declarations behind one barrel stay distinct"
    );
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            via_barrel,
            via_barrel_b,
            RelationKind::Comparable
        ),
        RelateVerdictForTests::DoesNotHold,
        "distinct declarations behind one barrel are provably disjoint"
    );

    // The identity the relation reads is the DECLARING one — the consumer's
    // local spelling (`RenamedA`) never becomes the type's name.
    let declared_identity = dispatch_relation_nominal_identity_for_tests(&host, declared)
        .expect("a `unique symbol` reference carries its declaring identity");
    assert_eq!(declared_identity.symbol.as_ref(), "A_KIND");
    assert_eq!(
        declared_identity.canonical_id.as_ref(),
        "/relation-authority/decl.ts",
        "the identity names the DECLARING file"
    );
    assert_eq!(
        dispatch_relation_nominal_identity_for_tests(&host, via_barrel),
        Some(declared_identity.clone()),
        "a renamed re-exported reference resolves to the declaring identity"
    );
    assert_eq!(
        dispatch_relation_nominal_identity_for_tests(&host, direct),
        Some(declared_identity)
    );
}

/// A namespace-QUALIFIED reference (`typeof Ns.KIND`) names the same nominal
/// type as every other route to the declaration.
///
/// The qualified root never appears as a literal symbol in the shallow
/// state, so it is the one reference shape a bare-name-only rule silently
/// drops: the type would fall back to the shared `symbol` primitive and the
/// identity would be gone. The relation over a type-position qualified
/// reference is pinned here; computed-key publication is covered by the
/// existing flow harness.
#[test]
fn relation_nominal_identity_survives_qualified_namespace_reference() {
    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/decl.ts",
        "export declare const A_KIND: unique symbol;\n\
         export declare const B_KIND: unique symbol;\n\
         export type DeclaredKindA = typeof A_KIND;",
    );
    upsert(
        &host,
        "/relation-authority/namespace.ts",
        "import * as Ns from \"./decl\";\n\
         export type ViaNsKindA = typeof Ns.A_KIND;\n\
         export type ViaNsKindB = typeof Ns.B_KIND;",
    );

    let declared =
        dispatch_resolve_type_decl_for_tests(&host, "/relation-authority/decl.ts", "DeclaredKindA");
    let via_ns_a = dispatch_resolve_type_decl_for_tests(
        &host,
        "/relation-authority/namespace.ts",
        "ViaNsKindA",
    );
    let via_ns_b = dispatch_resolve_type_decl_for_tests(
        &host,
        "/relation-authority/namespace.ts",
        "ViaNsKindB",
    );

    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            declared,
            via_ns_a,
            RelationKind::Identity
        ),
        RelateVerdictForTests::Holds,
        "a namespace-qualified reference is the declaring identity"
    );
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            declared,
            via_ns_a,
            RelationKind::Comparable
        ),
        RelateVerdictForTests::Holds,
    );
    assert_eq!(
        dispatch_execute_relate_verdict_for_tests(
            &host,
            via_ns_a,
            via_ns_b,
            RelationKind::Identity
        ),
        RelateVerdictForTests::DoesNotHold,
        "two qualified references to DISTINCT declarations stay distinct"
    );
    assert_eq!(
        dispatch_relation_nominal_identity_for_tests(&host, via_ns_a)
            .as_ref()
            .map(|identity| (identity.canonical_id.as_ref(), identity.symbol.as_ref())),
        Some(("/relation-authority/decl.ts", "A_KIND")),
        "the qualified reference names the DECLARING file and symbol"
    );
}

/// A qualified `typeof` key is classified from the complete resolved path.
/// A nominal prefix must not lend its identity to a trailing property access.
#[test]
fn qualified_typeof_key_does_not_truncate_trailing_segments() {
    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/decl.ts",
        "export declare const A_KIND: unique symbol;\n\
         export declare const B_KIND: unique symbol;",
    );
    upsert(
        &host,
        "/relation-authority/qualified-key.ts",
        "import * as Ns from \"./decl\";",
    );

    let expr = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::synthetic_public_key(
                verter_type_expr::AuthoredPropertyKey::Computed(TypeExpr::TypeOf(ValueRef {
                    path: vec!["Ns".into(), "A_KIND".into()],
                    type_args: Vec::new(),
                })),
                TypeExpr::primitive(PrimitiveName::Number),
                false,
                false,
            )),
            ObjectMember::Property(ObjectProperty::synthetic_public_key(
                verter_type_expr::AuthoredPropertyKey::Computed(TypeExpr::TypeOf(ValueRef {
                    path: vec!["Ns".into(), "A_KIND".into(), "description".into()],
                    type_args: Vec::new(),
                })),
                TypeExpr::primitive(PrimitiveName::String),
                false,
                false,
            )),
        ],
    }));
    let projected = dispatch_lower_type_expr_in_scope_with_context_for_tests(
        &host,
        "/relation-authority/qualified-key.ts",
        &expr,
        ProjectionReductionContext::structural_transit(),
    )
    .expect("the object type must lower");
    let data = host
        .project_type_store()
        .semantic_graph()
        .node_data(projected)
        .expect("the lowered object must be interned");
    let SemanticNodeData::Object(surface) = data.as_ref() else {
        panic!("expected an object, got {data:?}");
    };
    let members = surface.positive_members();
    let qualified_key = &members
        .first()
        .expect("the object must retain its keys")
        .key;
    assert!(
        matches!(qualified_key, AuthoredPropertyKey::UniqueSymbol(identity) if identity.symbol.as_ref() == "A_KIND"),
        "the exact namespace member must retain its declaring identity: {qualified_key:?}"
    );
    let projected_key = &members
        .get(1)
        .expect("the object must retain both keys")
        .key;
    let AuthoredPropertyKey::Computed(projected_node) = projected_key else {
        panic!(
            "the trailing `.description` projection is a computed key over the projected \
             member type, not a unique-symbol marker: {projected_key:?}"
        );
    };
    // The EXACT projected node, not merely "not UniqueSymbol": this host
    // carries no TS lib, so the widened primitive's `Symbol` interface is
    // absent and the honest projection is the interned `Opaque(Miss)` —
    // the raw typeof carrier, the whole declaration surface, or an
    // unresolved marker would each fail here.
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .node_data(*projected_node)
            .as_deref(),
        Some(&SemanticNodeData::Opaque(
            verter_session::semantic_query::QueryError::Miss
        )),
        "the trailing `.description` projection is an exact interned node: {projected_key:?}"
    );
}

/// A pending segment over a nominal carrier WIDENS before it projects:
/// `TOKEN.description` reads the `Symbol` interface member on the widened
/// primitive, never the carrier with segments unconsumed and never the
/// whole declaration surface.
#[test]
fn nominal_carrier_with_pending_segment_widens_and_projects() {
    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/decl.ts",
        "export declare const TOKEN: unique symbol;",
    );
    let lowered = dispatch_lower_type_expr_in_scope_with_context_for_tests(
        &host,
        "/relation-authority/decl.ts",
        &TypeExpr::TypeOf(ValueRef {
            path: vec!["TOKEN".into(), "description".into()],
            type_args: Vec::new(),
        }),
        ProjectionReductionContext::structural_transit(),
    )
    .expect("the pending-segment typeof must lower");
    assert!(
        dispatch_relation_nominal_identity_for_tests(&host, lowered).is_none(),
        "a projected member carries no nominal identity — the carrier must not be \
         published as the answer with segments unconsumed"
    );
    // No TS lib on this host, so the widened primitive's `Symbol` interface
    // is absent: the honest projection is the exact interned `Opaque(Miss)`
    // — NOT the carrier (segments must be consumed), NOT the declaration's
    // whole surface, and NOT an unresolved marker node.
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .node_data(lowered)
            .as_deref(),
        Some(&SemanticNodeData::Opaque(
            verter_session::semantic_query::QueryError::Miss
        )),
        "`TOKEN.description` widens and projects the `Symbol` interface member"
    );
}

/// Mixed concrete kinds: the full cross-kind matrix of the overlap oracle.
/// Every pair of DISTINCT concrete kind classes is table-driven here, in
/// BOTH operand orders, so the `_ => true` fallthrough of
/// `comparable_root_kinds_disjoint` — which governs every cross-kind pair
/// the explicit arms do not name — is pinned by the whole matrix rather
/// than two spot pairs. A wrong future arm addition (or a dropped explicit
/// not-disjoint arm) flips a row in one order or the other.
///
/// `DoesNotHold` rows are pairs with genuinely NO shared inhabitant. `Holds`
/// rows are either genuinely-overlapping pairs or deliberate permissive
/// rows (no proof of empty overlap found) — never a fabricated disjointness
/// proof.
#[test]
fn comparable_mixed_concrete_kinds_are_disjoint() {
    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/mixed-kinds.ts",
        "export declare const A_KIND: unique symbol;\n\
         export type Nom = typeof A_KIND;\n\
         export type Num = number;\n\
         export type Str = string;\n\
         export type Sym = symbol;\n\
         export type ObjPrim = object;\n\
         export type Lit = \"lit\";\n\
         export type Shape = { x: number };\n\
         export type Arr = string[];\n\
         export type Fn = () => void;\n\
         export type Tpl = `a${string}`;",
    );
    let node = |name: &str| {
        dispatch_resolve_type_decl_for_tests(&host, "/relation-authority/mixed-kinds.ts", name)
    };

    // (left kind, right kind, expected Comparable verdict)
    let rows: [(&str, &str, RelateVerdictForTests); 39] = [
        // Nominal × every non-symbol concrete kind — a `unique symbol` is
        // only ever a `symbol`.
        ("Nom", "Num", RelateVerdictForTests::DoesNotHold),
        ("Nom", "Str", RelateVerdictForTests::DoesNotHold),
        ("Nom", "ObjPrim", RelateVerdictForTests::DoesNotHold),
        ("Nom", "Lit", RelateVerdictForTests::DoesNotHold),
        ("Nom", "Shape", RelateVerdictForTests::DoesNotHold),
        ("Nom", "Arr", RelateVerdictForTests::DoesNotHold),
        ("Nom", "Fn", RelateVerdictForTests::DoesNotHold),
        ("Nom", "Tpl", RelateVerdictForTests::DoesNotHold),
        // Nominal × the bare `symbol` primitive: the widened inhabitant.
        ("Nom", "Sym", RelateVerdictForTests::Holds),
        // A concrete primitive against the object-ish kinds.
        ("Num", "Shape", RelateVerdictForTests::DoesNotHold),
        ("Num", "Arr", RelateVerdictForTests::DoesNotHold),
        ("Num", "Fn", RelateVerdictForTests::DoesNotHold),
        ("Str", "Shape", RelateVerdictForTests::DoesNotHold),
        ("Str", "Arr", RelateVerdictForTests::DoesNotHold),
        ("Str", "Fn", RelateVerdictForTests::DoesNotHold),
        ("Sym", "Shape", RelateVerdictForTests::DoesNotHold),
        ("Sym", "Arr", RelateVerdictForTests::DoesNotHold),
        ("Sym", "Fn", RelateVerdictForTests::DoesNotHold),
        // A literal against kinds outside its own base primitive.
        ("Lit", "Num", RelateVerdictForTests::DoesNotHold),
        ("Lit", "Sym", RelateVerdictForTests::DoesNotHold),
        ("Lit", "ObjPrim", RelateVerdictForTests::DoesNotHold),
        ("Lit", "Shape", RelateVerdictForTests::DoesNotHold),
        ("Lit", "Arr", RelateVerdictForTests::DoesNotHold),
        ("Lit", "Fn", RelateVerdictForTests::DoesNotHold),
        // A literal of string base overlaps the string primitive.
        ("Lit", "Str", RelateVerdictForTests::Holds),
        // A template-literal pattern against non-string kinds.
        ("Tpl", "Num", RelateVerdictForTests::DoesNotHold),
        ("Tpl", "Sym", RelateVerdictForTests::DoesNotHold),
        ("Tpl", "ObjPrim", RelateVerdictForTests::DoesNotHold),
        ("Tpl", "Shape", RelateVerdictForTests::DoesNotHold),
        ("Tpl", "Arr", RelateVerdictForTests::DoesNotHold),
        ("Tpl", "Fn", RelateVerdictForTests::DoesNotHold),
        // A template-literal pattern overlaps the string primitive and
        // string literals (`a${string}` admits `"lit"`-shaped strings).
        ("Tpl", "Str", RelateVerdictForTests::Holds),
        ("Tpl", "Lit", RelateVerdictForTests::Holds),
        // The object-ish kinds overlap each other (an object can be
        // array-like AND callable).
        ("ObjPrim", "Shape", RelateVerdictForTests::Holds),
        ("ObjPrim", "Arr", RelateVerdictForTests::Holds),
        ("ObjPrim", "Fn", RelateVerdictForTests::Holds),
        ("Arr", "Shape", RelateVerdictForTests::Holds),
        ("Fn", "Shape", RelateVerdictForTests::Holds),
        ("Arr", "Fn", RelateVerdictForTests::Holds),
    ];

    for (left_kind, right_kind, expected) in rows {
        let (left, right) = (node(left_kind), node(right_kind));
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(&host, left, right, RelationKind::Comparable),
            expected,
            "Comparable({left_kind}, {right_kind})"
        );
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(&host, right, left, RelationKind::Comparable),
            expected,
            "Comparable({right_kind}, {left_kind}) must agree with the forward direction"
        );
    }
}

/// A pending walker segment over a nominal carrier must widen and project
/// the remaining hop. Lowering `typeof TOKEN.description` already pins the
/// eager path; this drives `ProjectPath` over the interned carrier so a
/// walker that treated the terminal as consuming leftover segments would
/// republish the carrier.
#[test]
fn path_walker_pending_segment_over_nominal_carrier_widens() {
    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/walker-decl.ts",
        "export declare const TOKEN: unique symbol;",
    );
    let carrier = dispatch_lower_type_expr_in_scope_with_context_for_tests(
        &host,
        "/relation-authority/walker-decl.ts",
        &TypeExpr::TypeOf(ValueRef {
            path: vec!["TOKEN".into()],
            type_args: Vec::new(),
        }),
        ProjectionReductionContext::structural_transit(),
    )
    .expect("typeof TOKEN must lower to the nominal carrier");
    assert!(
        dispatch_relation_nominal_identity_for_tests(&host, carrier).is_some(),
        "the carrier itself is nominal"
    );
    let walked = dispatch_execute_type_node_for_tests(
        &host,
        verter_session::semantic_query::SemanticQueryKey::ProjectPath {
            base: carrier,
            path: Arc::from(vec![verter_session::semantic_query::PathSegment::Member(
                verter_session::semantic_query::PropertyKey::identifier(Arc::from("description")),
            )]),
            context: ProjectionReductionContext::structural_transit(),
        },
    );
    let walked = match walked {
        verter_session::semantic_query::QueryResult::Value(
            verter_session::semantic_query::SemanticQueryOutput { value, .. },
        ) => value,
        other => panic!("pending-segment projection must produce a node: {other:?}"),
    };
    assert!(
        dispatch_relation_nominal_identity_for_tests(&host, walked).is_none(),
        "the walker must consume the pending segment rather than republish the carrier"
    );
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .node_data(walked)
            .as_deref(),
        Some(&SemanticNodeData::Opaque(
            verter_session::semantic_query::QueryError::Miss
        )),
        "no TS lib: the Symbol interface member is an interned miss, not the carrier"
    );
    // KNOWN LIMIT of this pin: on a host without the TS lib the widened
    // `symbol` surface resolves no `description` member EITHER WAY, so a
    // widened-and-missed walk and an errored-and-missed walk land on the
    // same interned `Opaque(Miss)` — the assertion above proves the carrier
    // was consumed (no identity survives) but cannot separate those two
    // miss provenances. Splitting them needs a lib-bearing host.
}

/// A namespace-qualified nominal reference on a GENERATED surface keeps the
/// NAMESPACE binding in scope.
///
/// A `typeof Ns.A_KIND` reference renders its head verbatim into the
/// generated TypeScript, and the surface's import retention resolves
/// bindings by their LOCAL name. The qualified head stores the JOINED
/// spelling, so the reference-retention rail must record the LEXICAL ROOT
/// (`Ns`) — recording the joined string would strand the rendered
/// `typeof Ns.A_KIND` on a binding no file declares, a spurious wrong-OPEN
/// error in the user's editor. The `defineExpose` leg is the discriminating
/// one: its reference names come from the RESOLVED nominal carrier node
/// (there is no authored macro type-argument list to read them from), so
/// the exposed surface's import exists ONLY through the lexical-root
/// recording. The `defineProps` leg additionally pins the authored-spelling
/// well-formedness on the props lane.
#[test]
fn namespace_qualified_nominal_reference_keeps_the_namespace_import() {
    let host = make_audit_host();
    for (canonical, source, language) in [
        (
            "/relation-authority/ns-decl.ts",
            "export declare const A_KIND: unique symbol;",
            FileLanguage::script_ts(),
        ),
        (
            "/relation-authority/NsTokenProp.vue",
            "<script setup lang=\"ts\">\nimport * as Ns from \"./ns-decl\";\ndefineProps<{ token: typeof Ns.A_KIND }>();\n</script>\n<template><div /></template>",
            FileLanguage::vue(),
        ),
        (
            "/relation-authority/NsExpose.vue",
            "<script setup lang=\"ts\">\nimport * as Ns from \"./ns-decl\";\nconst kind: typeof Ns.A_KIND = Ns.A_KIND;\ndefineExpose({ kind });\n</script>\n<template><div /></template>",
            FileLanguage::vue(),
        ),
        (
            "/relation-authority/NsSurface.svelte",
            "<script context=\"module\" lang=\"ts\">\n  import * as Ns from \"./ns-decl\";\n  export const holder: { t: typeof Ns.A_KIND } = { t: Ns.A_KIND };\n</script>\n<script lang=\"ts\">\n  let { label }: { label: string } = $props();\n</script>\n<div>{label}</div>\n",
            FileLanguage::svelte(),
        ),
    ] {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source),
                file_language: language,
                aliases: Vec::new(),
            })
            .expect("namespace nominal fixture must upsert");
    }
    for consumer in [
        "/relation-authority/NsTokenProp.vue",
        "/relation-authority/NsExpose.vue",
        "/relation-authority/NsSurface.svelte",
    ] {
        host.set_import_dependencies(
            consumer,
            vec![verter_session::DependencyResolution {
                specifier: "./ns-decl".to_owned(),
                resolved_canonical_id: Some("/relation-authority/ns-decl.ts".to_owned()),
                possible_canonical_ids: Vec::new(),
            }],
        );
    }

    let response = host
        .get_public_api_with_mode(
            "/relation-authority/NsTokenProp.vue",
            verter_session::PublicApiMode::Public,
            None,
        )
        .expect("the public-API projection must not fault")
        .expect("a loaded Vue carrier must project");
    let code = response.ts_labeled_code();
    // Structural: the `keyof ({ ... })` wrapper exists only in the GENERATED
    // props intersection — the authored `defineProps<{ token: typeof Ns.A_KIND }>()`
    // echo spells the member without it, so this pin reads the generated type
    // position, not any substring of the echoed script.
    assert!(
        code.lines()
            .any(|line| line.contains("keyof ({ token: typeof Ns.A_KIND })")),
        "the generated props type renders the nominal member in type position:\n{code}"
    );
    assert!(
        code.lines().any(|line| line.starts_with("import ")
            && line.contains("* as Ns")
            && line.contains("./ns-decl")),
        "the rendered `typeof Ns.A_KIND` needs the NAMESPACE binding imported:\n{code}"
    );

    // The discriminating leg: the expose surface's reference names come from
    // the resolved carrier node alone.
    let exposed = host
        .get_public_api_with_mode(
            "/relation-authority/NsExpose.vue",
            verter_session::PublicApiMode::Public,
            None,
        )
        .expect("the expose projection must not fault")
        .expect("a loaded Vue carrier must project");
    let expose_code = exposed.ts_labeled_code();
    // Structural: the exposed member's GENERATED type is the `ShallowUnwrapRef`
    // member — a binding-anchored `typeof` reference, never the widened
    // `symbol`. The authored annotation spelling appears only in the echoed
    // script, which must not satisfy this pin.
    assert!(
        expose_code
            .lines()
            .any(|line| line.contains("ShallowUnwrapRef<{ kind: typeof kind }>")),
        "the generated exposed member keeps a nominal `typeof` reference in type \
         position — widening it to `symbol` fails here:\n{expose_code}"
    );
    assert!(
        expose_code
            .lines()
            .any(|line| line.starts_with("import ") && line.contains("* as Ns")),
        "the exposed `typeof Ns.A_KIND` names the Ns binding — recording the joined \
         spelling would reference an undeclared identifier:\n{expose_code}"
    );

    // THE discriminating leg: the Svelte declaration prelude resolves every
    // retained reference name through the shallow binding table
    // (`import_target_in`, an EXACT local-name lookup). The export's
    // reference names come from the resolved nominal carrier alone — there
    // is no authored macro surface — so the import line exists ONLY through
    // the lexical-root recording.
    let svelte = host
        .get_public_api_with_mode(
            "/relation-authority/NsSurface.svelte",
            verter_session::PublicApiMode::Declaration,
            None,
        )
        .expect("the Svelte declaration projection must not fault")
        .expect("the component projects a public declaration");
    let svelte_code = svelte.ts_labeled_code();
    // Structural: the declaration prelude echoes no authored script, so the
    // export line is pinned EXACTLY — member name, authored nominal spelling,
    // and type position together.
    assert!(
        svelte_code
            .lines()
            .any(|line| line.trim() == "export declare const holder: { t: typeof Ns.A_KIND };"),
        "the declaration renders the export with its authored nominal member type:\n{svelte_code}"
    );
    assert!(
        svelte_code
            .lines()
            .any(|line| line.starts_with("import type ") && line.contains("{ Ns }")),
        "the rendered `typeof Ns.A_KIND` needs the NAMESPACE binding imported — the \
         prelude resolves references by exact local name, and recording the joined \
         `Ns.A_KIND` spelling leaves the surface naming an undeclared identifier:\n{svelte_code}"
    );
}

/// A warm nominal judgement does not outlive the declaration it read: an
/// edit to the declaring `unique symbol` (or to the re-export route a
/// reference travelled) must MISS the warm entry and recompute to exactly
/// what a fresh host answers for the edited program.
///
/// Two legs, one per invalidation rail: the DECLARING file changing which
/// symbol its alias names, and the ROUTE (a barrel re-export) changing
/// which declaration it forwards. Both edits flip the verdict for a
/// reference the consumer did not touch — a stale warm `Holds` served
/// after the edit is exactly the regression this pins.
#[test]
fn warm_nominal_relation_misses_and_recomputes_after_declaration_edit() {
    // Leg 1 — the declaring file changes which symbol the alias names.
    {
        let host = make_audit_host();
        upsert(
            &host,
            "/relation-authority/decl.ts",
            "export declare const A_KIND: unique symbol;\n\
             export declare const A2_KIND: unique symbol;\n\
             export type KindA = typeof A_KIND;",
        );
        upsert(
            &host,
            "/relation-authority/consumer.ts",
            "import { A_KIND } from \"./decl\";\n\
             export type CKind = typeof A_KIND;",
        );
        let decl =
            dispatch_resolve_type_decl_for_tests(&host, "/relation-authority/decl.ts", "KindA");
        let consumer =
            dispatch_resolve_type_decl_for_tests(&host, "/relation-authority/consumer.ts", "CKind");
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                decl,
                consumer,
                RelationKind::Identity
            ),
            RelateVerdictForTests::Holds,
            "fixture: both references name A_KIND before the edit"
        );

        // The edit: `KindA` now names a DIFFERENT unique symbol.
        upsert(
            &host,
            "/relation-authority/decl.ts",
            "export declare const A_KIND: unique symbol;\n\
             export declare const A2_KIND: unique symbol;\n\
             export type KindA = typeof A2_KIND;",
        );
        let verdict_after_edit = dispatch_execute_relate_verdict_for_tests(
            &host,
            decl,
            consumer,
            RelationKind::Identity,
        );
        assert_eq!(
            verdict_after_edit,
            RelateVerdictForTests::DoesNotHold,
            "the declaration edit must invalidate the warm Holds judgement"
        );

        // Equivalence: a fresh host given the same (edited) files answers
        // the same question the same way.
        let fresh = make_audit_host();
        upsert(
            &fresh,
            "/relation-authority/decl.ts",
            "export declare const A_KIND: unique symbol;\n\
             export declare const A2_KIND: unique symbol;\n\
             export type KindA = typeof A2_KIND;",
        );
        upsert(
            &fresh,
            "/relation-authority/consumer.ts",
            "import { A_KIND } from \"./decl\";\n\
             export type CKind = typeof A_KIND;",
        );
        let fresh_decl =
            dispatch_resolve_type_decl_for_tests(&fresh, "/relation-authority/decl.ts", "KindA");
        let fresh_consumer = dispatch_resolve_type_decl_for_tests(
            &fresh,
            "/relation-authority/consumer.ts",
            "CKind",
        );
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &fresh,
                fresh_decl,
                fresh_consumer,
                RelationKind::Identity
            ),
            verdict_after_edit,
            "the edited host and a fresh host agree after the declaration edit"
        );
    }

    // Leg 2 — the re-export ROUTE changes which declaration is forwarded.
    {
        let host = make_audit_host();
        upsert(
            &host,
            "/relation-authority/decl.ts",
            "export declare const A_KIND: unique symbol;\n\
             export type DeclaredKindA = typeof A_KIND;",
        );
        upsert(
            &host,
            "/relation-authority/other.ts",
            "export declare const A_KIND: unique symbol;\n\
             export type DeclaredKindA = typeof A_KIND;",
        );
        upsert(
            &host,
            "/relation-authority/barrel.ts",
            "export { A_KIND } from \"./decl\";",
        );
        upsert(
            &host,
            "/relation-authority/consumer.ts",
            "import { A_KIND } from \"./barrel\";\n\
             export type CKind = typeof A_KIND;",
        );
        let declared = dispatch_resolve_type_decl_for_tests(
            &host,
            "/relation-authority/decl.ts",
            "DeclaredKindA",
        );
        let consumer =
            dispatch_resolve_type_decl_for_tests(&host, "/relation-authority/consumer.ts", "CKind");
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                declared,
                consumer,
                RelationKind::Identity
            ),
            RelateVerdictForTests::Holds,
            "fixture: the barrel forwards decl's A_KIND before the edit"
        );

        // The edit: the barrel now forwards OTHER's distinct A_KIND.
        upsert(
            &host,
            "/relation-authority/barrel.ts",
            "export { A_KIND } from \"./other\";",
        );
        assert_eq!(
            dispatch_execute_relate_verdict_for_tests(
                &host,
                declared,
                consumer,
                RelationKind::Identity
            ),
            RelateVerdictForTests::DoesNotHold,
            "the route edit must invalidate the warm Holds judgement"
        );
    }
}

/// A content-free `TypeOf` key must still validate the declaration version
/// carried by its value: changing `unique symbol` to `symbol` removes the
/// nominal carrier on the next read and agrees with a fresh host.
#[test]
fn warm_typeof_carrier_revalidates_after_unique_symbol_is_widened() {
    const CANONICAL: &str = "/relation-authority/typeof-version.ts";
    let host = make_audit_host();
    let typeof_token = TypeExpr::TypeOf(ValueRef {
        path: vec!["TOKEN".into()],
        type_args: Vec::new(),
    });
    upsert(
        &host,
        CANONICAL,
        "export declare const TOKEN: unique symbol;",
    );
    let before = dispatch_lower_type_expr_in_scope_with_context_for_tests(
        &host,
        CANONICAL,
        &typeof_token,
        ProjectionReductionContext::structural_transit(),
    )
    .expect("the initial typeof query must lower");
    assert!(
        dispatch_relation_nominal_identity_for_tests(&host, before).is_some(),
        "the unique-symbol declaration must initially produce a nominal carrier"
    );

    upsert(&host, CANONICAL, "export declare const TOKEN: symbol;");
    let after = dispatch_lower_type_expr_in_scope_with_context_for_tests(
        &host,
        CANONICAL,
        &typeof_token,
        ProjectionReductionContext::structural_transit(),
    )
    .expect("the edited typeof query must lower");

    let fresh = make_audit_host();
    upsert(&fresh, CANONICAL, "export declare const TOKEN: symbol;");
    let fresh_node = dispatch_lower_type_expr_in_scope_with_context_for_tests(
        &fresh,
        CANONICAL,
        &typeof_token,
        ProjectionReductionContext::structural_transit(),
    )
    .expect("the fresh typeof query must lower");
    let graph = host.project_type_store().semantic_graph();
    let fresh_graph = fresh.project_type_store().semantic_graph();
    assert_eq!(
        graph.node_data(after).as_deref(),
        Some(&SemanticNodeData::Primitive(PrimitiveKind::Symbol)),
        "the edited declaration must not reuse the old nominal carrier"
    );
    assert_eq!(
        graph.node_data(after).as_deref(),
        fresh_graph.node_data(fresh_node).as_deref(),
        "incremental and fresh resolution must agree after the edit"
    );
}

/// A `unique symbol` exposed through `defineExpose` keeps its reference IN
/// SCOPE in the generated TypeScript surface.
///
/// The nominal carrier renders as the text `typeof EXPOSED_TOKEN` rather
/// than the widened `symbol`, and that text is spliced into the generated
/// public-API surface. A rendered reference whose VALUE binding the splice
/// scope does not declare is a spurious error in the user's editor — a
/// wrong-OPEN failure, not a fail-closed one — so the emitted surface must
/// carry a value-capable (non-`import type`) import for the binding it
/// names.
#[test]
fn exposed_unique_symbol_reference_stays_in_scope_in_the_generated_surface() {
    let host = make_audit_host();
    for (canonical, source, language) in [
        (
            "/relation-authority/expose-token.ts",
            "export declare const EXPOSED_TOKEN: unique symbol;",
            FileLanguage::script_ts(),
        ),
        (
            "/relation-authority/ExposeToken.vue",
            "<script setup lang=\"ts\">\nimport { EXPOSED_TOKEN } from \"./expose-token\";\ndefineExpose({ token: EXPOSED_TOKEN });\n</script>\n<template><div /></template>",
            FileLanguage::vue(),
        ),
    ] {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source),
                file_language: language,
                aliases: Vec::new(),
            })
            .expect("expose fixture must upsert");
    }
    host.set_import_dependencies(
        "/relation-authority/ExposeToken.vue",
        vec![verter_session::DependencyResolution {
            specifier: "./expose-token".to_owned(),
            resolved_canonical_id: Some("/relation-authority/expose-token.ts".to_owned()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let response = host
        .get_public_api_with_mode(
            "/relation-authority/ExposeToken.vue",
            verter_session::PublicApiMode::Public,
            None,
        )
        .expect("the public-API projection must not fault")
        .expect("a loaded Vue carrier must project");
    let code = response.ts_labeled_code();

    // Structural: the member's GENERATED type position — the `ShallowUnwrapRef`
    // member renders the nominal reference, never the widened `symbol`; the
    // authored script echo carries only the VALUE spelling of the binding, so
    // it cannot satisfy this pin.
    assert!(
        code.lines()
            .any(|line| line.contains("ShallowUnwrapRef<{ token: typeof EXPOSED_TOKEN }>")),
        "the generated exposed member keeps its nominal reference in type position:\n{code}"
    );
    assert!(
        code.lines().any(|line| {
            line.starts_with("import ")
                && !line.starts_with("import type ")
                && line.contains("EXPOSED_TOKEN")
        }),
        "a rendered `typeof EXPOSED_TOKEN` needs a VALUE-capable import for the binding it \
         names, or the generated surface references an undeclared identifier:\n{code}"
    );
}

/// A nominal (`unique symbol`) `typeof` carrier is the TERMINAL answer, so
/// the Vue runtime publication boundary must classify it CLEAN.
///
/// The carrier-normalization prelude turns a typed partial reason into
/// `ReturnOnly` admission. A `keyof <carrier>` demand reaches that prelude
/// with a subject the carrier normalizer deliberately returns unchanged —
/// resolving a nominal head would widen it to the shared `symbol` primitive
/// and erase the declaring identity that IS the type. Classifying the
/// retained nominal carrier as a query fault would mark a COMPLETE result
/// partial and stop every such publication from ever warming.
///
/// The control leg is the discriminator: an authored `typeof` whose value
/// name does not resolve is still retained as an unresolved carrier, which
/// IS a failed normalization and must still fault. So the exclusion is
/// scoped to the nominal terminal and does not blanket-silence the
/// classifier.
#[test]
fn vue_publication_keeps_a_nominal_typeof_carrier_complete() {
    use verter_session::for_tests::dispatch_vue_publication_keyof_partial_reasons_for_tests;
    use verter_session::semantic_query::PartialReasonSet;

    let host = make_audit_host();
    upsert(
        &host,
        "/relation-authority/nominal-publish.ts",
        "export declare const A_KIND: unique symbol;",
    );
    let typeof_node = |name: &str| {
        dispatch_lower_type_expr_in_scope_with_context_for_tests(
            &host,
            "/relation-authority/nominal-publish.ts",
            &TypeExpr::TypeOf(ValueRef {
                path: vec![name.into()],
                type_args: Vec::new(),
            }),
            ProjectionReductionContext::structural_transit(),
        )
        .expect("a `typeof` annotation must lower")
    };

    let nominal = typeof_node("A_KIND");
    assert!(
        dispatch_relation_nominal_identity_for_tests(&host, nominal).is_some(),
        "fixture guard: `typeof A_KIND` must reach a nominal carrier, otherwise \
         the assertion below proves nothing"
    );
    assert_eq!(
        dispatch_vue_publication_keyof_partial_reasons_for_tests(&host, nominal),
        PartialReasonSet::empty(),
        "a nominal `typeof` carrier is the terminal answer — publishing it must \
         not be forced to ReturnOnly by a fabricated carrier fault"
    );

    // Control: `typeof NOT_DECLARED` retains an UNRESOLVED authored carrier
    // at the same boundary, which is a genuine failed normalization.
    let unresolved = typeof_node("NOT_DECLARED");
    assert!(
        dispatch_relation_nominal_identity_for_tests(&host, unresolved).is_none(),
        "fixture guard: the control subject must carry no nominal identity"
    );
    assert_eq!(
        dispatch_vue_publication_keyof_partial_reasons_for_tests(&host, unresolved),
        PartialReasonSet::SEMANTIC_QUERY_FAULT,
        "an unresolved authored `typeof` carrier is still a publication fault"
    );
}
