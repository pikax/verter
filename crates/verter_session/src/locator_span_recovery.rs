//! Snapshot-backed span recovery: recover the authored spans an IR struct puts
//! in its `Eq`/`Hash` identity, from a retained parse, via a producer-emitted
//! origin locator — BEFORE any identity / interning.
//!
//! Member spans participate in node identity (`ObjectProperty` /
//! `MethodSignature` / `FunctionExpr` / `IndexSignature` derive `Eq`/`Hash` over
//! their span field; `FunctionParam` includes `.span` in its hand-written
//! identity). A closed fact stores no `Span` (the `NoStoredSpan` contract), so a
//! fact that reconstructs such a node carries a [`MemberSpansOrigin`] /
//! [`IndexSignatureSpansOrigin`] / [`FunctionSpansOrigin`] /
//! [`FunctionParamSpanOrigin`] and recovers the exact spans here.
//!
//! # Fail-closed on a stale authored origin
//!
//! Recovery returns a [`Result`]: a bad / stale / out-of-range / unhandled
//! AUTHORED origin is a distinct [`SpanRecoveryError`], NEVER a silent
//! `MemberSpans::default()`. Default (all-absent) spans are the honest result of
//! EXACTLY ONE input — an explicit [`SourceSynthetic`] origin. A caller that
//! could not recover the authored spans must treat that as a partial (never warm
//! an entry with fabricated absence).
//!
//! # No re-parse
//!
//! The outer helpers route through [`DeclLoweringService::run_leased`], which
//! takes NO `source` and NEVER parses: on a lease miss it returns
//! [`SpanRecoveryError::LeaseMiss`]. Span recovery is therefore structurally
//! incapable of triggering a transient re-parse — it can only reuse a live
//! retained snapshot.
//!
//! # Worker-purity split
//!
//! The PURE core ([`recover_member_spans_from_program`] & siblings) walks a
//! re-borrowed `&Program` sub-position named by the origin path and reads the
//! authored spans — no host / dispatch / service re-entry. The outer helpers
//! compose that pure core with the lease-pinned no-parse `run_leased` path.
//!
//! HARD BOUNDARY: these helpers recover ONLY spans. They never lower to a
//! `SemanticNodeId`, touch the `SemanticGraphStore`, memoize under
//! `LocatorLoweringKey`, build a locator lowerer, or reroute
//! `lower_decl_body_with_provenance`.
//!
//! [`SourceSynthetic`]: verter_type_expr::span_origins::SourceSynthetic

#![allow(dead_code)]

use std::sync::Arc;

use oxc_ast::ast::{
    ClassElement, Declaration, ExportDefaultDeclarationKind, Program, Statement, TSFunctionType,
    TSSignature, TSType,
};
use oxc_span::GetSpan;
use verter_span::Span;
use verter_type_expr::span_origins::{
    FunctionParamSelector, FunctionParamSpanOrigin, FunctionSpansOrigin, IndexSignatureSpansOrigin,
    MemberSpansOrigin,
};
use verter_type_expr::{FunctionSpans, IndexSignatureSpans, MemberSpans};

use crate::decl_lowering::{DeclLoweringService, SnapshotKey};

/// Why authored span recovery could not produce the authored spans. A default
/// (all-absent) `MemberSpans` is NEVER returned for any of these — that is
/// reserved for an explicit [`MemberSpansOrigin::Synthetic`] input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpanRecoveryError {
    /// No live lease pinned the origin's snapshot key (the retained parse was
    /// not available). Recovery never parses, so a missing lease is a hard
    /// failure, not a silent default.
    LeaseMiss,
    /// The producing snapshot's parse was fatal (the retained program is
    /// `None`).
    FatalParse,
    /// An AUTHORED origin whose producer-emitted path does not resolve against
    /// the retained parse — a stale / out-of-range ordinal, or a decl form this
    /// navigation does not (yet) handle. Fail-closed: never a fabricated default
    /// span for an authored node.
    AuthoredOriginUnresolved,
}

// ---------------------------------------------------------------------------
// Pure cores — `&Program` + origin → spans. Independently callable/testable.
// ---------------------------------------------------------------------------

/// Recover the `MemberSpans` (declaration / name / type-annotation spans) of the
/// authored member named by `origin`. A synthetic origin recovers the default
/// (all-absent) spans; an authored origin whose path does not resolve (stale /
/// out-of-range / unhandled form) is [`SpanRecoveryError::AuthoredOriginUnresolved`],
/// NEVER a silent default.
pub(crate) fn recover_member_spans_from_program(
    program: &Program<'_>,
    origin: &MemberSpansOrigin,
) -> Result<MemberSpans, SpanRecoveryError> {
    let (anchor, member_path) = match origin {
        MemberSpansOrigin::Authored {
            anchor,
            member_path,
        } => (anchor, member_path),
        MemberSpansOrigin::Synthetic(_) => return Ok(MemberSpans::default()),
    };
    let member = resolve_member(program, anchor.contributor_index, member_path)
        .ok_or(SpanRecoveryError::AuthoredOriginUnresolved)?;
    match member {
        LocatedMember::Signature(TSSignature::TSPropertySignature(prop)) => Ok(MemberSpans {
            declaration: Some(prop.span.into()),
            name: Some(prop.key.span().into()),
            type_annotation: prop
                .type_annotation
                .as_ref()
                .map(|ta| ta.type_annotation.span().into()),
        }),
        LocatedMember::Signature(TSSignature::TSMethodSignature(method)) => Ok(MemberSpans {
            declaration: Some(method.span.into()),
            name: Some(method.key.span().into()),
            type_annotation: None,
        }),
        LocatedMember::ClassElement(ClassElement::PropertyDefinition(prop)) => Ok(MemberSpans {
            declaration: Some(prop.span.into()),
            name: Some(prop.key.span().into()),
            type_annotation: prop
                .type_annotation
                .as_ref()
                .map(|ta| ta.type_annotation.span().into()),
        }),
        LocatedMember::ClassElement(ClassElement::MethodDefinition(method)) => Ok(MemberSpans {
            declaration: Some(method.span.into()),
            name: Some(method.key.span().into()),
            type_annotation: None,
        }),
        // Any other located form is an unhandled authored origin — fail closed.
        _ => Err(SpanRecoveryError::AuthoredOriginUnresolved),
    }
}

/// Recover the `IndexSignatureSpans` of the authored index-signature member named
/// by `origin`. Fail-closed on a stale/unhandled authored origin.
pub(crate) fn recover_index_signature_spans_from_program(
    program: &Program<'_>,
    origin: &IndexSignatureSpansOrigin,
) -> Result<IndexSignatureSpans, SpanRecoveryError> {
    let (anchor, member_path) = match origin {
        IndexSignatureSpansOrigin::Authored {
            anchor,
            member_path,
        } => (anchor, member_path),
        IndexSignatureSpansOrigin::Synthetic(_) => return Ok(IndexSignatureSpans::default()),
    };
    let member = resolve_member(program, anchor.contributor_index, member_path)
        .ok_or(SpanRecoveryError::AuthoredOriginUnresolved)?;
    match member {
        LocatedMember::Signature(TSSignature::TSIndexSignature(idx)) => Ok(IndexSignatureSpans {
            declaration: Some(idx.span.into()),
            key: idx.parameters.first().map(|param| param.span.into()),
            value: Some(idx.type_annotation.type_annotation.span().into()),
        }),
        _ => Err(SpanRecoveryError::AuthoredOriginUnresolved),
    }
}

/// Recover the `FunctionSpans` (signature / return-type spans) of the authored
/// function named by `origin`. Fail-closed on a stale/unhandled authored origin.
pub(crate) fn recover_function_spans_from_program(
    program: &Program<'_>,
    origin: &FunctionSpansOrigin,
) -> Result<FunctionSpans, SpanRecoveryError> {
    if matches!(origin, FunctionSpansOrigin::Synthetic(_)) {
        return Ok(FunctionSpans::default());
    }
    let func =
        function_type_for(program, origin).ok_or(SpanRecoveryError::AuthoredOriginUnresolved)?;
    Ok(FunctionSpans {
        signature: Some(func.signature_span),
        return_type: func.return_type_span,
    })
}

/// Recover a `FunctionParam.span` for the parameter selected by `origin`. A
/// synthetic enclosing function recovers `None` (honest absence); a stale /
/// out-of-range authored selector is [`SpanRecoveryError::AuthoredOriginUnresolved`].
pub(crate) fn recover_function_param_span_from_program(
    program: &Program<'_>,
    origin: &FunctionParamSpanOrigin,
) -> Result<Option<Span>, SpanRecoveryError> {
    if matches!(origin.function, FunctionSpansOrigin::Synthetic(_)) {
        return Ok(None);
    }
    let func = function_type_for(program, &origin.function)
        .ok_or(SpanRecoveryError::AuthoredOriginUnresolved)?;
    match origin.param {
        FunctionParamSelector::Positional { ordinal } => func
            .param_spans
            .get(ordinal as usize)
            .copied()
            .map(Some)
            .ok_or(SpanRecoveryError::AuthoredOriginUnresolved),
        FunctionParamSelector::Rest => func
            .rest_param_span
            .map(Some)
            .ok_or(SpanRecoveryError::AuthoredOriginUnresolved),
    }
}

// ---------------------------------------------------------------------------
// Outer helpers — compose the pure core with the lease-pinned NO-PARSE path.
// ---------------------------------------------------------------------------

/// Recover `MemberSpans` for `origin` against the RETAINED parse for `key`. Takes
/// no `source`: on a lease miss it returns [`SpanRecoveryError::LeaseMiss`] and
/// never re-parses.
pub(crate) fn recover_member_spans(
    service: &Arc<DeclLoweringService>,
    key: &SnapshotKey,
    origin: &MemberSpansOrigin,
) -> Result<MemberSpans, SpanRecoveryError> {
    let origin = origin.clone();
    match service.run_leased(key, move |program| match program {
        Some(program) => recover_member_spans_from_program(program.borrow_dependent(), &origin),
        None => Err(SpanRecoveryError::FatalParse),
    }) {
        Some(inner) => inner,
        None => Err(SpanRecoveryError::LeaseMiss),
    }
}

/// Recover `IndexSignatureSpans` for `origin` against the RETAINED parse for
/// `key`. No re-parse; lease miss ⇒ error.
pub(crate) fn recover_index_signature_spans(
    service: &Arc<DeclLoweringService>,
    key: &SnapshotKey,
    origin: &IndexSignatureSpansOrigin,
) -> Result<IndexSignatureSpans, SpanRecoveryError> {
    let origin = origin.clone();
    match service.run_leased(key, move |program| match program {
        Some(program) => {
            recover_index_signature_spans_from_program(program.borrow_dependent(), &origin)
        }
        None => Err(SpanRecoveryError::FatalParse),
    }) {
        Some(inner) => inner,
        None => Err(SpanRecoveryError::LeaseMiss),
    }
}

/// Recover `FunctionSpans` for `origin` against the RETAINED parse for `key`. No
/// re-parse; lease miss ⇒ error.
pub(crate) fn recover_function_spans(
    service: &Arc<DeclLoweringService>,
    key: &SnapshotKey,
    origin: &FunctionSpansOrigin,
) -> Result<FunctionSpans, SpanRecoveryError> {
    let origin = origin.clone();
    match service.run_leased(key, move |program| match program {
        Some(program) => recover_function_spans_from_program(program.borrow_dependent(), &origin),
        None => Err(SpanRecoveryError::FatalParse),
    }) {
        Some(inner) => inner,
        None => Err(SpanRecoveryError::LeaseMiss),
    }
}

/// Recover a `FunctionParam.span` for `origin` against the RETAINED parse for
/// `key`. No re-parse; lease miss ⇒ error.
pub(crate) fn recover_function_param_span(
    service: &Arc<DeclLoweringService>,
    key: &SnapshotKey,
    origin: &FunctionParamSpanOrigin,
) -> Result<Option<Span>, SpanRecoveryError> {
    let origin = origin.clone();
    match service.run_leased(key, move |program| match program {
        Some(program) => {
            recover_function_param_span_from_program(program.borrow_dependent(), &origin)
        }
        None => Err(SpanRecoveryError::FatalParse),
    }) {
        Some(inner) => inner,
        None => Err(SpanRecoveryError::LeaseMiss),
    }
}

// ---------------------------------------------------------------------------
// Navigation internals (pure; borrow the retained `&Program`).
// ---------------------------------------------------------------------------

/// The span facts of a located function-like node, converted to `verter_span`.
struct FunctionSpanFacts {
    signature_span: Span,
    return_type_span: Option<Span>,
    param_spans: Vec<Span>,
    rest_param_span: Option<Span>,
}

/// A member surface reached from a decl body: interface / type-literal
/// signatures, or class elements. Copy (holds only borrowed slices) so `get`
/// preserves the retained-parse lifetime.
#[derive(Clone, Copy)]
enum MemberSurface<'a> {
    Signatures(&'a [TSSignature<'a>]),
    ClassElements(&'a [ClassElement<'a>]),
}

/// One located member — the value at an ordinal within a [`MemberSurface`].
#[derive(Clone, Copy)]
enum LocatedMember<'a> {
    Signature(&'a TSSignature<'a>),
    ClassElement(&'a ClassElement<'a>),
}

impl<'a> MemberSurface<'a> {
    fn get(self, ordinal: u32) -> Option<LocatedMember<'a>> {
        match self {
            MemberSurface::Signatures(sigs) => {
                sigs.get(ordinal as usize).map(LocatedMember::Signature)
            }
            MemberSurface::ClassElements(elems) => {
                elems.get(ordinal as usize).map(LocatedMember::ClassElement)
            }
        }
    }
}

/// Resolve a member path from a decl body to the target [`LocatedMember`]. Each
/// path ordinal selects a member of the current member surface; a non-final
/// ordinal descends into that member's value-type surface (a nested type
/// literal). The final ordinal names the target member.
fn resolve_member<'a>(
    program: &'a Program<'a>,
    contributor_index: u32,
    member_path: &[u32],
) -> Option<LocatedMember<'a>> {
    let stmt = program.body.get(contributor_index as usize)?;
    let mut surface = statement_member_surface(stmt)?;
    let (last, prefix) = member_path.split_last()?;
    for &ordinal in prefix {
        let member = surface.get(ordinal)?;
        surface = member_value_surface(member)?;
    }
    surface.get(*last)
}

/// The member surface of a top-level statement — a bare or EXPORTED type alias
/// (type literal), interface, or class declaration. Mirrors the export
/// unwrapping in `type_eval_build::lower_top_level_statement`.
fn statement_member_surface<'a>(stmt: &'a Statement<'a>) -> Option<MemberSurface<'a>> {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => tstype_member_surface(&alias.type_annotation),
        Statement::TSInterfaceDeclaration(iface) => {
            Some(MemberSurface::Signatures(&iface.body.body))
        }
        Statement::ClassDeclaration(class) => Some(MemberSurface::ClassElements(&class.body.body)),
        Statement::ExportNamedDeclaration(export) => export
            .declaration
            .as_ref()
            .and_then(declaration_member_surface),
        Statement::ExportDefaultDeclaration(export) => {
            export_default_member_surface(&export.declaration)
        }
        _ => None,
    }
}

/// The member surface of an `export`ed [`Declaration`].
fn declaration_member_surface<'a>(decl: &'a Declaration<'a>) -> Option<MemberSurface<'a>> {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => tstype_member_surface(&alias.type_annotation),
        Declaration::TSInterfaceDeclaration(iface) => {
            Some(MemberSurface::Signatures(&iface.body.body))
        }
        Declaration::ClassDeclaration(class) => {
            Some(MemberSurface::ClassElements(&class.body.body))
        }
        _ => None,
    }
}

/// The member surface of an `export default` declaration (interface / class).
fn export_default_member_surface<'a>(
    kind: &'a ExportDefaultDeclarationKind<'a>,
) -> Option<MemberSurface<'a>> {
    match kind {
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(iface) => {
            Some(MemberSurface::Signatures(&iface.body.body))
        }
        ExportDefaultDeclarationKind::ClassDeclaration(class) => {
            Some(MemberSurface::ClassElements(&class.body.body))
        }
        _ => None,
    }
}

/// The member surface of a `TSType`, if it is a type literal.
fn tstype_member_surface<'a>(ty: &'a TSType<'a>) -> Option<MemberSurface<'a>> {
    match ty {
        TSType::TSTypeLiteral(literal) => Some(MemberSurface::Signatures(&literal.members)),
        _ => None,
    }
}

/// The member surface reached by descending into a member's value type (a nested
/// type literal), if applicable — for a property signature or a class property.
fn member_value_surface<'a>(member: LocatedMember<'a>) -> Option<MemberSurface<'a>> {
    match member {
        LocatedMember::Signature(TSSignature::TSPropertySignature(prop)) => {
            let ta = prop.type_annotation.as_ref()?;
            tstype_member_surface(&ta.type_annotation)
        }
        LocatedMember::ClassElement(ClassElement::PropertyDefinition(prop)) => {
            let ta = prop.type_annotation.as_ref()?;
            tstype_member_surface(&ta.type_annotation)
        }
        _ => None,
    }
}

/// Extract the span facts of the function-like node an origin names.
fn function_type_for(
    program: &Program<'_>,
    origin: &FunctionSpansOrigin,
) -> Option<FunctionSpanFacts> {
    match origin {
        FunctionSpansOrigin::AliasBody { anchor } => {
            let stmt = program.body.get(anchor.contributor_index as usize)?;
            let func = statement_alias_function_type(stmt)?;
            Some(ts_function_type_facts(func))
        }
        FunctionSpansOrigin::Member {
            anchor,
            member_path,
        } => {
            let member = resolve_member(program, anchor.contributor_index, member_path)?;
            located_member_function_facts(member)
        }
        FunctionSpansOrigin::Synthetic(_) => None,
    }
}

/// The `TSFunctionType` of a bare or EXPORTED `type F = (..) => ..` statement.
fn statement_alias_function_type<'a>(stmt: &'a Statement<'a>) -> Option<&'a TSFunctionType<'a>> {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => tstype_function_type(&alias.type_annotation),
        Statement::ExportNamedDeclaration(export) => match export.declaration.as_ref()? {
            Declaration::TSTypeAliasDeclaration(alias) => {
                tstype_function_type(&alias.type_annotation)
            }
            _ => None,
        },
        _ => None,
    }
}

/// The `TSFunctionType` of a `TSType`, if it is a bare function type.
fn tstype_function_type<'a>(ty: &'a TSType<'a>) -> Option<&'a TSFunctionType<'a>> {
    match ty {
        TSType::TSFunctionType(func) => Some(func),
        _ => None,
    }
}

/// The span facts of a standalone `TSFunctionType`.
fn ts_function_type_facts(func: &TSFunctionType<'_>) -> FunctionSpanFacts {
    FunctionSpanFacts {
        signature_span: func.span.into(),
        return_type_span: Some(func.return_type.type_annotation.span().into()),
        param_spans: func.params.items.iter().map(|p| p.span.into()).collect(),
        rest_param_span: func.params.rest.as_ref().map(|r| r.span.into()),
    }
}

/// The function span facts of a located member — an interface method / call /
/// construct signature, or a class method definition.
fn located_member_function_facts(member: LocatedMember<'_>) -> Option<FunctionSpanFacts> {
    match member {
        LocatedMember::Signature(sig) => signature_function_facts(sig),
        LocatedMember::ClassElement(ClassElement::MethodDefinition(method)) => {
            Some(FunctionSpanFacts {
                signature_span: method.value.span.into(),
                return_type_span: method
                    .value
                    .return_type
                    .as_ref()
                    .map(|rt| rt.type_annotation.span().into()),
                param_spans: method
                    .value
                    .params
                    .items
                    .iter()
                    .map(|p| p.span.into())
                    .collect(),
                rest_param_span: method.value.params.rest.as_ref().map(|r| r.span.into()),
            })
        }
        _ => None,
    }
}

/// The function span facts of a method / call / construct member signature.
fn signature_function_facts(sig: &TSSignature<'_>) -> Option<FunctionSpanFacts> {
    match sig {
        TSSignature::TSMethodSignature(method) => Some(FunctionSpanFacts {
            signature_span: method.span.into(),
            return_type_span: method
                .return_type
                .as_ref()
                .map(|rt| rt.type_annotation.span().into()),
            param_spans: method.params.items.iter().map(|p| p.span.into()).collect(),
            rest_param_span: method.params.rest.as_ref().map(|r| r.span.into()),
        }),
        TSSignature::TSCallSignatureDeclaration(call) => Some(FunctionSpanFacts {
            signature_span: call.span.into(),
            return_type_span: call
                .return_type
                .as_ref()
                .map(|rt| rt.type_annotation.span().into()),
            param_spans: call.params.items.iter().map(|p| p.span.into()).collect(),
            rest_param_span: call.params.rest.as_ref().map(|r| r.span.into()),
        }),
        TSSignature::TSConstructSignatureDeclaration(ctor) => Some(FunctionSpanFacts {
            signature_span: ctor.span.into(),
            return_type_span: ctor
                .return_type
                .as_ref()
                .map(|rt| rt.type_annotation.span().into()),
            param_spans: ctor.params.items.iter().map(|p| p.span.into()).collect(),
            rest_param_span: ctor.params.rest.as_ref().map(|r| r.span.into()),
        }),
        _ => None,
    }
}
