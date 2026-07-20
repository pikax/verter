//! [P1] discriminating span-recovery fixtures — one per identity-participating
//! span class, plus fail-closed / no-reparse / decl-form coverage. Each drives
//! the REAL recovery path (the snapshot-backed helper over a genuine retained
//! parse, NOT a hand-built struct) and asserts BOTH:
//!
//! - positive: the node reconstructed with the RECOVERED spans is `Eq`-EQUAL to
//!   the authored node (spans byte-identical to the production lowerer output);
//! - negative: the SAME node reconstructed with `Span::default()` is `Eq`-UNEQUAL
//!   to the authored node — proving member spans participate in identity, so the
//!   recovery is load-bearing.
//!
//! Fail-closed: a STALE/out-of-range AUTHORED origin returns a
//! [`SpanRecoveryError`], never a silent default (default spans are reserved for
//! an explicit `Synthetic` origin). No-reparse: recovery routes through the
//! no-parse `run_leased` path, so a lease miss is an error — never a fresh parse.
//!
//! Ground truth is produced by the PRODUCTION lowerer (`lower_ts_type`) — an
//! independent path from recovery — so agreement is meaningful.

use std::sync::Arc;

use oxc_ast::ast::{ClassElement, Statement};
use oxc_span::GetSpan;
use verter_span::Span;
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
    recover_member_spans, SpanRecoveryError,
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

fn member_origin(ordinals: &[u32]) -> MemberSpansOrigin {
    MemberSpansOrigin::Authored {
        anchor: DeclContributorAnchor {
            contributor_index: 0,
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            owner_local_ordinal: 0,
        },
        member_path: path(ordinals),
    }
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

    let recovered = recover_member_spans(&service, &key, &member_origin(&[0]))
        .expect("authored property spans recover");

    assert_eq!(
        recovered, authored.spans,
        "recovered == authored member spans"
    );
    assert_ne!(
        MemberSpans::default(),
        authored.spans,
        "authored member spans are non-trivial"
    );

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
        &MemberSpansOrigin::Synthetic(SourceSynthetic),
    )
    .expect("synthetic recovers Ok(default)");
    assert_eq!(synthetic, MemberSpans::default());
}

#[test]
fn stale_authored_origin_fails_closed_never_default() {
    // [P1] fail-closed: an out-of-range AUTHORED member ordinal is a distinct
    // error, NOT a silent default. Discriminating: the pre-fix code returned
    // `MemberSpans::default()` here (indistinguishable from a synthetic origin).
    let service = Arc::new(DeclLoweringService::new());
    let source: Arc<str> = Arc::from("type A = { count?: number };\n");
    let key = key();
    let st = oxc_span::SourceType::ts();
    let _lease = service.acquire_lease(&key, &source, st);

    // Member ordinal 9 is out of range (only member 0 exists).
    let stale = recover_member_spans(&service, &key, &member_origin(&[9]));
    assert_eq!(
        stale,
        Err(SpanRecoveryError::AuthoredOriginUnresolved),
        "a stale authored origin must fail closed, never default"
    );

    // A contributor index out of range also fails closed.
    let bad_contributor = recover_member_spans(
        &service,
        &key,
        &MemberSpansOrigin::Authored {
            anchor: DeclContributorAnchor {
                contributor_index: 42,
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 42,
            },
            member_path: path(&[0]),
        },
    );
    assert_eq!(
        bad_contributor,
        Err(SpanRecoveryError::AuthoredOriginUnresolved)
    );
}

#[test]
fn lease_miss_fails_closed_and_never_reparses() {
    // [P1] no-reparse: recovery routes through `run_leased`, which NEVER parses.
    // With NO live lease, recovery is a `LeaseMiss` error — it does not silently
    // re-parse to "succeed". Discriminating: a re-parsing implementation would
    // return `Ok(spans)` here.
    let service = Arc::new(DeclLoweringService::new());
    let key = key();
    // Deliberately DO NOT acquire a lease.
    let result = recover_member_spans(&service, &key, &member_origin(&[0]));
    assert_eq!(
        result,
        Err(SpanRecoveryError::LeaseMiss),
        "with no live lease, recovery must miss — never re-parse"
    );
}

#[test]
fn recovers_nested_member_path_multi_hop() {
    // Multi-hop: `member_path = [0, 1]` descends into member 0's value-type
    // surface (a nested type literal) and selects member 1 there. Exercises the
    // non-terminal-descent branch that the single-ordinal fixtures do not.
    let service = Arc::new(DeclLoweringService::new());
    let source: Arc<str> = Arc::from("type A = { outer: { inner: number; second: string } };\n");
    let key = key();
    let st = oxc_span::SourceType::ts();
    let _lease = service.acquire_lease(&key, &source, st);

    let lowered = lower_single_alias_body(&service, &key, &source, st);
    let TypeExpr::Object(obj) = &lowered else {
        panic!("alias body must be an object literal");
    };
    let ObjectMember::Property(outer) = obj.properties[0].clone() else {
        panic!("member 0 must be a property");
    };
    let TypeExpr::Object(inner_obj) = &outer.ty else {
        panic!("member 0's value must be a nested object literal");
    };
    let ObjectMember::Property(second) = inner_obj.properties[1].clone() else {
        panic!("nested member 1 must be a property");
    };

    let recovered = recover_member_spans(&service, &key, &member_origin(&[0, 1]))
        .expect("nested member spans recover");
    assert_eq!(
        recovered, second.spans,
        "multi-hop recovery reaches the nested member's spans"
    );

    // Discriminating: the nested member [0,1] differs from [0,0].
    let ObjectMember::Property(inner) = inner_obj.properties[0].clone() else {
        panic!("nested member 0 must be a property");
    };
    assert_ne!(second.spans, inner.spans, "nested ordinals discriminate");
    let recovered_00 = recover_member_spans(&service, &key, &member_origin(&[0, 0]))
        .expect("nested member 0 recovers");
    assert_eq!(recovered_00, inner.spans);
    assert_ne!(recovered, recovered_00);
}

#[test]
fn recovers_exported_interface_member_spans() {
    // [P1] decl-form coverage: an `export interface` must be unwrapped, not
    // fail-closed. Discriminating: the pre-fix navigation handled only BARE type
    // aliases/interfaces, so this member would fail to resolve.
    let service = Arc::new(DeclLoweringService::new());
    let source: Arc<str> = Arc::from("export interface I { x: number; y: string }\n");
    let key = key();
    let st = oxc_span::SourceType::ts();
    let _lease = service.acquire_lease(&key, &source, st);

    let x = recover_member_spans(&service, &key, &member_origin(&[0]))
        .expect("exported interface member 0 recovers");
    let y = recover_member_spans(&service, &key, &member_origin(&[1]))
        .expect("exported interface member 1 recovers");
    assert_ne!(x, MemberSpans::default(), "exported member has real spans");
    assert!(x.declaration.is_some() && x.name.is_some());
    assert_ne!(x, y, "distinct members recover distinct spans");
}

#[test]
fn recovers_class_member_spans_across_visibilities() {
    // [P1] decl-form coverage: public / protected / private class-element
    // property + method spans are reachable (visibility lives on the fact, not
    // the span). Ground truth is a DIRECT walk of the class body — independent
    // of the recovery navigation.
    let service = Arc::new(DeclLoweringService::new());
    let source: Arc<str> = Arc::from(
        "class C { a: number; protected b: string; private c: boolean; run(): void {} }\n",
    );
    let key = key();
    let st = oxc_span::SourceType::ts();
    let _lease = service.acquire_lease(&key, &source, st);

    // Direct ground-truth extraction (raw iteration over class elements).
    let direct: Vec<(Span, Span)> = service
        .run(&key, &source, st, |program| {
            let program = program.expect("parse").borrow_dependent();
            let Statement::ClassDeclaration(class) = &program.body[0] else {
                panic!("fixture must begin with a class declaration");
            };
            class
                .body
                .body
                .iter()
                .filter_map(|el| match el {
                    ClassElement::PropertyDefinition(p) => {
                        Some((p.span.into(), p.key.span().into()))
                    }
                    ClassElement::MethodDefinition(m) => Some((m.span.into(), m.key.span().into())),
                    _ => None,
                })
                .collect()
        })
        .value;
    assert_eq!(direct.len(), 4, "a, b, c, run");

    let mut names = Vec::new();
    for (ordinal, (decl_span, name_span)) in direct.iter().enumerate() {
        let recovered = recover_member_spans(&service, &key, &member_origin(&[ordinal as u32]))
            .unwrap_or_else(|e| panic!("class member {ordinal} must recover, got {e:?}"));
        assert_eq!(
            recovered.declaration,
            Some(*decl_span),
            "class member {ordinal} declaration span"
        );
        assert_eq!(
            recovered.name,
            Some(*name_span),
            "class member {ordinal} name span"
        );
        names.push(recovered.name);
    }
    // Discriminating: the public/protected/private member name spans are all
    // distinct — visibility does not collapse recovery.
    assert_ne!(names[0], names[1]);
    assert_ne!(names[1], names[2]);
    assert_ne!(names[0], names[2]);
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

    let fn_origin = FunctionSpansOrigin::Member {
        anchor: DeclContributorAnchor {
            contributor_index: 0,
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            owner_local_ordinal: 0,
        },
        member_path: path(&[0]),
    };
    let recovered_member = recover_member_spans(&service, &key, &member_origin(&[0]))
        .expect("method member spans recover");
    let recovered_fn =
        recover_function_spans(&service, &key, &fn_origin).expect("method fn spans recover");

    assert_eq!(
        recovered_member, authored.spans,
        "recovered method member spans"
    );
    assert_eq!(
        recovered_fn, authored.function.spans,
        "recovered method function spans"
    );
    assert_ne!(FunctionSpans::default(), authored.function.spans);

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
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 0,
            },
            member_path: path(&[0]),
        };
        let recovered = recover_index_signature_spans(&service, &key, &origin)
            .expect("index signature spans recover");

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
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 0,
            },
        },
        param: FunctionParamSelector::Positional { ordinal: 0 },
    };
    let rest = FunctionParamSpanOrigin {
        function: FunctionSpansOrigin::AliasBody {
            anchor: DeclContributorAnchor {
                contributor_index: 0,
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 0,
            },
        },
        param: FunctionParamSelector::Rest,
    };

    let authored_positional = authored_fn.parameters[0].clone();
    let authored_rest = authored_fn
        .parameters
        .iter()
        .find(|p| p.rest)
        .expect("a rest parameter")
        .clone();

    let recovered_positional = recover_function_param_span(&service, &key, &positional)
        .expect("positional param recovers");
    let recovered_rest =
        recover_function_param_span(&service, &key, &rest).expect("rest param recovers");

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

    // Fail-closed: an out-of-range positional ordinal is an error, not a default.
    let stale = recover_function_param_span(
        &service,
        &key,
        &FunctionParamSpanOrigin {
            function: FunctionSpansOrigin::AliasBody {
                anchor: DeclContributorAnchor {
                    contributor_index: 0,
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    owner_local_ordinal: 0,
                },
            },
            param: FunctionParamSelector::Positional { ordinal: 99 },
        },
    );
    assert_eq!(stale, Err(SpanRecoveryError::AuthoredOriginUnresolved));
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
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            owner_local_ordinal: 0,
        },
    };
    let recovered =
        recover_function_spans(&service, &key, &origin).expect("standalone fn spans recover");

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
