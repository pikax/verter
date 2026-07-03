//! [P1] discriminating span-recovery fixtures — one per identity-participating
//! span class. Each drives the REAL recovery path (the snapshot-backed helper
//! over a genuine retained parse, NOT a hand-built struct) and asserts BOTH:
//!
//! - positive: the node reconstructed with the RECOVERED spans is `Eq`-EQUAL to
//!   the authored node (spans byte-identical to the production lowerer output);
//! - negative: the SAME node reconstructed with `Span::default()` is `Eq`-UNEQUAL
//!   to the authored node — proving member spans participate in identity, so the
//!   recovery is load-bearing.
//!
//! Revert-probe: making any `recover_*` return default spans turns each positive
//! assertion RED (recovered would equal the default node, not the authored one).
//!
//! Ground truth is produced by the PRODUCTION lowerer (`lower_ts_type`) — an
//! independent path from recovery — so agreement is meaningful.

use std::sync::Arc;

use oxc_ast::ast::Statement;
use verter_type_expr::span_origins::{
    DeclContributorAnchor, FunctionParamSelector, FunctionParamSpanOrigin, FunctionSpansOrigin,
    IndexSignatureSpansOrigin, MemberSpansOrigin, SourceSynthetic,
};
use verter_type_expr::{
    FunctionExpr, FunctionParam, FunctionSpans, IndexSignature, IndexSignatureSpans, MemberSpans,
    MethodSignature, ObjectMember, ObjectProperty, TypeExpr,
};

use crate::decl_lowering::{DeclLoweringService, SnapshotKey};
use crate::locator_span_recovery::{
    recover_function_param_span, recover_function_spans, recover_index_signature_spans,
    recover_member_spans,
};
use crate::ParsedEvalProgram;

fn key() -> SnapshotKey {
    SnapshotKey {
        canonical: Arc::from("/ws/a.ts"),
        whole_hash: [7u8; 16],
        parse_env_hash: [0u8; 16],
    }
}

fn path(ordinals: &[u32]) -> Arc<[u32]> {
    Arc::from(ordinals.to_vec().into_boxed_slice())
}

/// Lower the single top-level type-alias body of `source` through the PRODUCTION
/// lowerer and hand the result back (owned). Runs on the retained-parse worker,
/// a pure job (no service re-entry).
fn lower_single_alias_body(
    service: &Arc<DeclLoweringService>,
    key: &SnapshotKey,
    source: &Arc<str>,
    st: oxc_span::SourceType,
) -> TypeExpr {
    service
        .run(key, source, st, |program: Option<&ParsedEvalProgram>| {
            let program = program.expect("parse must succeed");
            let src = program.source_str();
            let program = program.borrow_dependent();
            let Statement::TSTypeAliasDeclaration(alias) = &program.body[0] else {
                panic!("fixture source must begin with a bare type alias");
            };
            verter_type_expr_oxc::lower_ts_type(&alias.type_annotation, src)
        })
        .value
}

#[test]
fn recovers_property_member_spans_and_discriminates_default() {
    let service = Arc::new(DeclLoweringService::new());
    let source: Arc<str> = Arc::from("type A = { count?: number };\n");
    let key = key();
    let st = oxc_span::SourceType::ts();
    let _lease = service.acquire_lease(&key, &source, st);

    let lowered = lower_single_alias_body(&service, &key, &source, st);
    let TypeExpr::Object(obj) = &lowered else {
        panic!("alias body must be an object literal");
    };
    let ObjectMember::Property(authored) = obj.properties[0].clone() else {
        panic!("member 0 must be a property");
    };

    let origin = MemberSpansOrigin::Authored {
        anchor: DeclContributorAnchor {
            contributor_index: 0,
        },
        member_path: path(&[0]),
    };
    let recovered = recover_member_spans(&service, &key, &source, st, &origin);

    // Recovery matches the production lowerer, and the authored spans are
    // non-trivial (so the default would diverge).
    assert_eq!(
        recovered, authored.spans,
        "recovered == authored member spans"
    );
    assert_ne!(
        MemberSpans::default(),
        authored.spans,
        "authored member spans are non-trivial"
    );

    // Node identity: recovered spans reproduce the authored node; default spans
    // do not (member spans participate in Eq/Hash).
    let reconstructed = ObjectProperty::with_spans_public(
        authored.name.clone(),
        authored.ty.clone(),
        authored.optional,
        authored.readonly,
        recovered,
    );
    assert_eq!(
        reconstructed, authored,
        "recovered spans -> equal node identity"
    );
    let with_default = ObjectProperty::with_spans_public(
        authored.name.clone(),
        authored.ty.clone(),
        authored.optional,
        authored.readonly,
        MemberSpans::default(),
    );
    assert_ne!(
        with_default, authored,
        "default spans -> unequal node identity (discriminates)"
    );

    // A synthetic origin honestly recovers absence, never a fabricated span.
    let synthetic = recover_member_spans(
        &service,
        &key,
        &source,
        st,
        &MemberSpansOrigin::Synthetic(SourceSynthetic),
    );
    assert_eq!(synthetic, MemberSpans::default());
}

#[test]
fn recovers_method_member_and_function_spans_and_exercises_optional() {
    let service = Arc::new(DeclLoweringService::new());
    let source: Arc<str> = Arc::from("type A = { run?(x: number): void };\n");
    let key = key();
    let st = oxc_span::SourceType::ts();
    let _lease = service.acquire_lease(&key, &source, st);

    let lowered = lower_single_alias_body(&service, &key, &source, st);
    let TypeExpr::Object(obj) = &lowered else {
        panic!("alias body must be an object literal");
    };
    let ObjectMember::Method(authored) = obj.properties[0].clone() else {
        panic!("member 0 must be a method");
    };
    assert!(authored.optional, "the fixture method is optional");

    let member_origin = MemberSpansOrigin::Authored {
        anchor: DeclContributorAnchor {
            contributor_index: 0,
        },
        member_path: path(&[0]),
    };
    let fn_origin = FunctionSpansOrigin::Member {
        anchor: DeclContributorAnchor {
            contributor_index: 0,
        },
        member_path: path(&[0]),
    };
    let recovered_member = recover_member_spans(&service, &key, &source, st, &member_origin);
    let recovered_fn = recover_function_spans(&service, &key, &source, st, &fn_origin);

    assert_eq!(
        recovered_member, authored.spans,
        "recovered method member spans"
    );
    assert_eq!(
        recovered_fn, authored.function.spans,
        "recovered method function spans"
    );
    assert_ne!(FunctionSpans::default(), authored.function.spans);

    // Reconstruct the method with recovered spans -> equal identity.
    let recon_fn = FunctionExpr::with_spans(
        authored.function.parameters.clone(),
        authored.function.return_type.clone(),
        authored.function.type_parameters.clone(),
        recovered_fn,
    );
    let reconstructed = MethodSignature::with_spans_public(
        authored.name.clone(),
        recon_fn,
        authored.optional,
        recovered_member,
    );
    assert_eq!(
        reconstructed, authored,
        "recovered spans -> equal method identity"
    );

    // Default function spans -> unequal identity (function spans are in Eq/Hash).
    let recon_fn_default = FunctionExpr::with_spans(
        authored.function.parameters.clone(),
        authored.function.return_type.clone(),
        authored.function.type_parameters.clone(),
        FunctionSpans::default(),
    );
    let with_default = MethodSignature::with_spans_public(
        authored.name.clone(),
        recon_fn_default,
        authored.optional,
        MemberSpans::default(),
    );
    assert_ne!(
        with_default, authored,
        "default spans -> unequal method identity"
    );
}

#[test]
fn recovers_index_signature_spans_for_string_and_number_keys() {
    let service = Arc::new(DeclLoweringService::new());
    // Two index-signature key shapes; span recovery must succeed for both.
    for source_text in [
        "type A = { [k: string]: number };\n",
        "type A = { [k: number]: string };\n",
    ] {
        let source: Arc<str> = Arc::from(source_text);
        let key = key();
        let st = oxc_span::SourceType::ts();
        let _lease = service.acquire_lease(&key, &source, st);

        let lowered = lower_single_alias_body(&service, &key, &source, st);
        let TypeExpr::Object(obj) = &lowered else {
            panic!("alias body must be an object literal");
        };
        let ObjectMember::IndexSignature(authored) = obj.properties[0].clone() else {
            panic!("member 0 must be an index signature");
        };

        let origin = IndexSignatureSpansOrigin::Authored {
            anchor: DeclContributorAnchor {
                contributor_index: 0,
            },
            member_path: path(&[0]),
        };
        let recovered = recover_index_signature_spans(&service, &key, &source, st, &origin);

        assert_eq!(
            recovered, authored.spans,
            "recovered index-signature spans ({source_text})"
        );
        assert_ne!(IndexSignatureSpans::default(), authored.spans);

        let reconstructed = IndexSignature::with_spans(
            authored.key_name.clone(),
            authored.key_type.clone(),
            authored.value_type.clone(),
            authored.readonly,
            recovered,
        );
        assert_eq!(
            reconstructed, authored,
            "recovered spans -> equal index-sig identity"
        );
        let with_default = IndexSignature::with_spans(
            authored.key_name.clone(),
            authored.key_type.clone(),
            authored.value_type.clone(),
            authored.readonly,
            IndexSignatureSpans::default(),
        );
        assert_ne!(
            with_default, authored,
            "default spans -> unequal index-sig identity"
        );
    }
}

#[test]
fn recovers_function_param_span_positional_and_rest() {
    let service = Arc::new(DeclLoweringService::new());
    let source: Arc<str> = Arc::from("type F = (a: number, ...rest: string[]) => void;\n");
    let key = key();
    let st = oxc_span::SourceType::ts();
    let _lease = service.acquire_lease(&key, &source, st);

    let lowered = lower_single_alias_body(&service, &key, &source, st);
    let TypeExpr::Function(authored_fn) = &lowered else {
        panic!("alias body must be a function type");
    };
    let authored_fn = authored_fn.clone();

    let positional = FunctionParamSpanOrigin {
        function: FunctionSpansOrigin::AliasBody {
            anchor: DeclContributorAnchor {
                contributor_index: 0,
            },
        },
        param: FunctionParamSelector::Positional { ordinal: 0 },
    };
    let rest = FunctionParamSpanOrigin {
        function: FunctionSpansOrigin::AliasBody {
            anchor: DeclContributorAnchor {
                contributor_index: 0,
            },
        },
        param: FunctionParamSelector::Rest,
    };

    let authored_positional = authored_fn.parameters[0].clone();
    // The rest parameter is the last lowered parameter (`rest == true`).
    let authored_rest = authored_fn
        .parameters
        .iter()
        .find(|p| p.rest)
        .expect("a rest parameter")
        .clone();

    let recovered_positional =
        recover_function_param_span(&service, &key, &source, st, &positional);
    let recovered_rest = recover_function_param_span(&service, &key, &source, st, &rest);

    assert_eq!(
        recovered_positional, authored_positional.span,
        "recovered positional param span"
    );
    assert_eq!(
        recovered_rest, authored_rest.span,
        "recovered rest param span"
    );
    assert!(
        authored_positional.span.is_some(),
        "authored param span present"
    );

    // Node identity: FunctionParam includes `.span` in its hand-written Eq/Hash.
    let reconstructed = FunctionParam::with_span(
        authored_positional.name.clone(),
        authored_positional.ty.clone(),
        authored_positional.optional,
        authored_positional.rest,
        recovered_positional,
        authored_positional.has_ts_annotation,
    );
    assert_eq!(
        reconstructed, authored_positional,
        "recovered span -> equal param identity"
    );
    let with_default = FunctionParam::with_span(
        authored_positional.name.clone(),
        authored_positional.ty.clone(),
        authored_positional.optional,
        authored_positional.rest,
        None,
        authored_positional.has_ts_annotation,
    );
    assert_ne!(
        with_default, authored_positional,
        "absent span -> unequal param identity"
    );
}

#[test]
fn recovers_standalone_function_type_spans() {
    let service = Arc::new(DeclLoweringService::new());
    let source: Arc<str> = Arc::from("type F = (a: number) => void;\n");
    let key = key();
    let st = oxc_span::SourceType::ts();
    let _lease = service.acquire_lease(&key, &source, st);

    let lowered = lower_single_alias_body(&service, &key, &source, st);
    let TypeExpr::Function(authored_fn) = &lowered else {
        panic!("alias body must be a function type");
    };
    let authored_fn = authored_fn.clone();

    let origin = FunctionSpansOrigin::AliasBody {
        anchor: DeclContributorAnchor {
            contributor_index: 0,
        },
    };
    let recovered = recover_function_spans(&service, &key, &source, st, &origin);

    assert_eq!(
        recovered, authored_fn.spans,
        "recovered standalone function spans"
    );
    assert_ne!(FunctionSpans::default(), authored_fn.spans);

    let reconstructed = FunctionExpr::with_spans(
        authored_fn.parameters.clone(),
        authored_fn.return_type.clone(),
        authored_fn.type_parameters.clone(),
        recovered,
    );
    assert_eq!(reconstructed.spans, authored_fn.spans);
    assert_eq!(
        reconstructed, *authored_fn,
        "recovered spans -> equal function identity"
    );
    let with_default = FunctionExpr::with_spans(
        authored_fn.parameters.clone(),
        authored_fn.return_type.clone(),
        authored_fn.type_parameters.clone(),
        FunctionSpans::default(),
    );
    assert_ne!(
        with_default, *authored_fn,
        "default spans -> unequal function identity"
    );
}
