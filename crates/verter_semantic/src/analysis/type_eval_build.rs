//! Build an [`EvalEnv`] from an OXC program AST.
//!
//! Walks top-level declarations, lowers each to its TRANSIENT typed-IR parts,
//! and populates the type and value symbol tables with the content-free
//! facts + locators minted from those parts (the transient typed IR is
//! discarded; bodies are lowered again on demand through the shared
//! resolver's body service).

use std::io::Write;
use std::sync::{Arc, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::analysis::fact_projection::value_type_annotation_fact;
use crate::analysis::top_level_owners::{TopLevelOwnerTable, TopLevelStatementOwner};
use crate::analysis::type_eval::*;
use oxc_ast::ast::{
    ArrowFunctionExpression, BinaryOperator, BindingPattern, Class, ClassElement, Declaration,
    ExportDefaultDeclarationKind, Expression, FormalParameters, Function, MethodDefinitionKind,
    ObjectExpression, ObjectPropertyKind, Program, Statement, TSAccessibility, TSEnumDeclaration,
    TSInterfaceDeclaration, TSModuleBlock, TSModuleDeclaration, TSModuleDeclarationBody,
    TSModuleDeclarationName, TSSignature, TSTypeAliasDeclaration, TSTypeParameterDeclaration,
    UnaryOperator, VariableDeclarationKind, VariableDeclarator,
};
use oxc_span::GetSpan;
use verter_type_expr::facts::{
    ClosedTypeFact, EnumMemberEntry, EnumMemberFact, EnumMemberNamesFact, EnumPrimitiveDomain,
    EnumScalar, FunctionParamFact, FunctionSignatureFact, IndexSignatureFact,
    InferenceUnavailableReason, KeyTypeShape, LeafTypeFact, MemberHeaderFact,
    MemberReturnInferenceFact, NarrowTypeParam, ObjectMemberFact, ObjectMethodFact,
    ObjectPropertyFact, ObjectShapeFact, ReturnInferenceCompleteness, ReturnInferenceUnsupported,
    SemanticTypeSource, TypeParamDeclFact,
};
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodyPathStep, TypeBodySlot,
    TypeParamBoundPosition,
};
use verter_type_expr::span_origins::{
    DeclContributorAnchor, FunctionParamSelector, FunctionParamSpanOrigin, FunctionSpansOrigin,
    IndexSignatureSpansOrigin, MemberSpansOrigin, SourceSynthetic,
};
use verter_type_expr::{
    FunctionExpr, FunctionParam, FunctionSpans, IndexSignature, IndexSignatureSpans, LiteralValue,
    MemberSpans, MemberVisibility, MethodSignature, ObjectExpr, ObjectMember, PrimitiveName,
    TopLevelOwnerId, TypeExpr, TypeParam, ValueRef,
};
use verter_type_expr_oxc::{lower_ts_type, property_key_name};

fn type_expand_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
            || std::env::var_os("VERTER_META_DEBUG").is_some()
    })
}

fn type_expand_debug(message: impl FnOnce() -> String) {
    if type_expand_debug_enabled() {
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "[verter-type-expand] {}", message());
        let _ = stderr.flush();
    }
}

fn expansion_metadata_hit_budget(
    exactness: crate::analysis::type_expand::ExpansionExactness,
    diagnostics: &[crate::analysis::type_expand::ExpansionDiagnostic],
) -> bool {
    exactness == crate::analysis::type_expand::ExpansionExactness::Incomplete
        && diagnostics.iter().any(|diagnostic| {
            diagnostic.reason == crate::analysis::type_expand::ExpansionStopReason::BudgetExceeded
        })
}

struct ExpandStageLog<'a> {
    macro_index: usize,
    macro_kind: crate::analysis::types::AnalyzedMacroKind,
    stage: &'a str,
    target: &'a str,
    started: Instant,
    start_steps: usize,
}

fn log_expand_stage(
    log: ExpandStageLog<'_>,
    exactness: crate::analysis::type_expand::ExpansionExactness,
    execution_status: crate::analysis::type_expand::ExpansionExecutionStatus,
    diagnostics: &[crate::analysis::type_expand::ExpansionDiagnostic],
    env: Option<&EvalEnv>,
) {
    type_expand_debug(|| {
        format!(
            "expand_macro_types:item macro_index={} macro_kind={:?} stage={} target={} took {:?} steps_delta={} exactness={:?} execution_status={:?} diagnostics={} budget_hit={}",
            log.macro_index,
            log.macro_kind,
            log.stage,
            log.target,
            log.started.elapsed(),
            env.map(|env| env.steps().saturating_sub(log.start_steps))
                .unwrap_or(0),
            exactness,
            execution_status,
            diagnostics.len(),
            expansion_metadata_hit_budget(exactness, diagnostics),
        )
    });
}

fn log_expand_stage_start(log: &ExpandStageLog<'_>) {
    type_expand_debug(|| {
        format!(
            "expand_macro_types:item_start macro_index={} macro_kind={:?} stage={} target={} steps={}",
            log.macro_index,
            log.macro_kind,
            log.stage,
            log.target,
            log.start_steps,
        )
    });
}

/// Producer context for one whole-file eval-env lowering walk.
///
/// Carries the PRODUCING canonical id — the anchor canonical every
/// producer-emitted authored locator / span-origin fact names
/// (`AuthoredAnchor.canonical_id`). Locators/origins are minted ONLY where the
/// OXC nodes are in scope (this walk); a pre-lowered consumer cannot recover
/// them. Callers building a whole-file environment supply the file's canonical
/// id; test fixtures supply a deterministic fixture canonical.
#[derive(Debug, Clone)]
pub struct BuildEvalEnvContext {
    /// Canonical id of the file whose parse this walk lowers.
    pub canonical_id: Arc<str>,
}

impl BuildEvalEnvContext {
    /// Context anchored at `canonical_id`.
    pub fn new(canonical_id: impl Into<Arc<str>>) -> Self {
        Self {
            canonical_id: canonical_id.into(),
        }
    }
}

/// Per-statement producer context: the whole-walk build context plus this
/// statement's PRODUCER-EMITTED contributor index (the `program.body` ordinal —
/// the `DeclContributorAnchor` ordinal authored span-origin minting anchors
/// to). Selective per-statement lowering passes the statement's ORIGINAL
/// top-level index (recorded by the header index's `contributors` locators),
/// never a renumbered position.
#[derive(Debug, Clone, Copy)]
pub struct StatementLowerCtx<'a> {
    /// The whole-walk build context (the anchor canonical).
    pub build: &'a BuildEvalEnvContext,
    /// This statement's `program.body` ordinal.
    pub contributor_index: u32,
    /// Validated lexical owner coordinates of this statement.
    pub statement_owner: TopLevelStatementOwner,
}

impl<'a> StatementLowerCtx<'a> {
    #[must_use]
    pub fn ordinary_file(build: &'a BuildEvalEnvContext, contributor_index: u32) -> Self {
        Self {
            build,
            contributor_index,
            statement_owner: TopLevelStatementOwner {
                owner: TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: contributor_index,
            },
        }
    }

    #[must_use]
    pub fn from_contributor_anchor(
        build: &'a BuildEvalEnvContext,
        anchor: DeclContributorAnchor,
    ) -> Self {
        Self {
            build,
            contributor_index: anchor.contributor_index,
            statement_owner: TopLevelStatementOwner {
                owner: anchor.owner,
                owner_local_ordinal: anchor.owner_local_ordinal,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Transient lowered declaration parts
// ---------------------------------------------------------------------------

/// TRANSIENT lowered parts of one TYPE declaration: the fully-lowered typed-IR
/// view the producer builds, derives facts and locators from, and DISCARDS.
/// Returned by the shared statement lowering so in-crate lowering tests can
/// characterize the lowering semantics the fact minting consumes. Never stored
/// on [`EvalEnv`] or any cache.
#[derive(Debug, Clone)]
pub struct LoweredTypeDeclParts {
    pub name: String,
    pub kind: TypeDeclKind,
    /// Lowered type-parameter headers (constraint/default typed IR included).
    pub type_parameters: Vec<TypeParam>,
    /// The fully-lowered declaration body.
    pub body: TypeExpr,
    /// Exact produced-shape paths and verdicts for authored methods. The
    /// declaration contributor is attached when the stored fact is minted.
    pub direct_member_return_inference: Vec<LoweredMemberReturnInference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredMemberReturnInference {
    pub member_path: Arc<[u32]>,
    pub return_inference: ReturnInferenceCompleteness,
}

/// Where a transient signature's authored function node lives, relative to its
/// owning declaration statement — drives the minted [`FunctionSpansOrigin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoweredSignatureOrigin {
    /// The declaration statement's body IS the function (a `function` decl, an
    /// arrow / function-expression initializer).
    DeclBody,
    /// A member of the produced object shape at this ordinal (a class
    /// constructor / static method in the `typeof C` constructor shape).
    ShapeMember { ordinal: u32 },
    /// Genuinely synthesized — no authored function node (a class with no
    /// declared constructor).
    Synthetic,
}

/// TRANSIENT lowered parts of one function/method signature: the typed-IR
/// parameter / return / type-parameter forms JSDoc enrichment and inference
/// operate on. The stored form is the minted [`FunctionSignatureFact`].
#[derive(Debug, Clone)]
pub struct LoweredSignatureParts {
    pub parameters: Vec<FunctionParam>,
    pub return_type: Option<TypeExpr>,
    /// Completeness of body-derived return inference. Authored/JSDoc returns,
    /// bodiless declarations, and synthesized signatures are `NotInferred`.
    pub return_inference: ReturnInferenceCompleteness,
    pub type_parameters: Vec<TypeParam>,
    /// Whether this signature is backed by an implementation body (vs. a
    /// bodiless overload / ambient declaration). Projection-time overload
    /// visibility reads the stored fact's copy of this flag.
    pub has_implementation_body: bool,
    /// Whether the function carried an explicit AUTHORED TS return annotation
    /// (`(): T`). Only an authored return position mints a `FunctionReturn`
    /// body locator — an inferred / JSDoc-filled return has no authored
    /// `TSType` node to address and is recovered whole-signature on demand.
    pub has_authored_return: bool,
    /// Span-recovery origin of the authored function node.
    pub origin: LoweredSignatureOrigin,
}

/// TRANSIENT lowered parts of one VALUE declaration.
#[derive(Debug, Clone)]
pub struct LoweredValueDeclParts {
    pub name: String,
    pub kind: ValueDeclKind,
    /// The lowered annotation typed IR: the authored TS annotation, the JSDoc
    /// `@type` payload, or the initializer-inferred type (in that precedence).
    pub type_annotation: Option<TypeExpr>,
    /// Whether [`type_annotation`](Self::type_annotation) is an AUTHORED
    /// annotation (TS annotation or JSDoc `@type`) vs initializer-inferred.
    /// Drives the minted annotation SOURCE: an authored annotation is its
    /// decl-body locator; an inferred one is carried as a closed leaf fact
    /// when trivially closed, else recovered by demand.
    pub annotation_is_authored: bool,
    /// Typed refusal when initializer inference could not safely produce an
    /// exact annotation. Mutually exclusive with `type_annotation`.
    pub inference_unavailable: Option<InferenceUnavailableReason>,
    /// Lowered function signatures (source order within this declaration).
    pub signatures: Vec<LoweredSignatureParts>,
    /// Lowered object shape (const object initializer / class constructor
    /// shape).
    pub object_shape: Option<ObjectExpr>,
    /// Exact produced-shape paths and return-inference verdicts for authored
    /// value-side methods (currently class statics). The declaration
    /// contributor is attached when the stored method fact is minted.
    pub object_member_return_inference: Vec<LoweredMemberReturnInference>,
    /// Ordered enum member inventory (`Some` exactly for an enum decl).
    pub enum_members: Option<Vec<(String, EnumMemberValue)>>,
    /// Enum member-NAME inventory fact (`Some` exactly for an enum decl).
    pub enum_member_names: Option<EnumMemberNamesFact>,
}

/// The TRANSIENT lowered parts one top-level statement contributes, routed to
/// their target inventory scope. Registration order within each vector is the
/// statement's own declaration order.
#[derive(Debug, Clone, Default)]
pub struct LoweredStatementParts {
    pub type_decls: Vec<LoweredTypeDeclParts>,
    pub value_decls: Vec<LoweredValueDeclParts>,
    pub aug_type_decls: Vec<(AugmentationScopeKind, LoweredTypeDeclParts)>,
    pub aug_value_decls: Vec<(AugmentationScopeKind, LoweredValueDeclParts)>,
    /// `export default class C` / `export default interface I` — after
    /// registration, mirror the declared-name type symbol under the `default`
    /// export name (see [`alias_default_export_type_symbol`]).
    pub alias_default_type_to: Option<String>,
}

/// Build an inventory environment from an OXC program AST.
///
/// Lowers each statement's declarations to TRANSIENT typed-IR parts, mints the
/// content-free facts + locators the inventory stores, and discards the
/// transient forms:
/// - Type aliases / interfaces / classes → [`TypeDeclInfo`] (body slot +
///   header facts)
/// - Functions / variables / enums → [`ValueDeclInfo`] (annotation /
///   signature / shape / enum facts)
pub fn build_eval_env(program: &Program<'_>, source: &str, ctx: &BuildEvalEnvContext) -> EvalEnv {
    let owners = TopLevelOwnerTable::ordinary_file(program.body.len());
    build_eval_env_with_owners(program, source, ctx, &owners)
}

/// Build an eval environment under an explicit validated lexical-owner table.
pub fn build_eval_env_with_owners(
    program: &Program<'_>,
    source: &str,
    ctx: &BuildEvalEnvContext,
    owners: &TopLevelOwnerTable,
) -> EvalEnv {
    assert_eq!(
        owners.len(),
        program.body.len(),
        "validated owner table must cover the lowered program exactly"
    );
    let mut env = EvalEnv::new();

    for (contributor_index, stmt) in program.body.iter().enumerate() {
        let statement_owner = owners.statement(contributor_index);
        let Some(contributor_index) = u32::try_from(contributor_index).ok() else {
            break;
        };
        lower_top_level_statement(
            stmt,
            StatementLowerCtx {
                build: ctx,
                contributor_index,
                statement_owner,
            },
            source,
            &mut env,
        );
    }

    // JSDoc `@typedef {T} Name` declarations are first-class REGULAR types: a
    // `/** @typedef {{a: number}} Alias */` block declares `Alias` exactly like
    // a TS `type Alias = { a: number }`. Register them on the SAME type-symbol
    // registry the TS declarations above populated, so a later `@type {Alias}`
    // or bare `Alias` reference resolves through the shared dispatch with no
    // JSDoc-specific path. This runs AFTER the statement walk so a real TS
    // declaration of the same name always wins (TS-decl precedence).
    register_jsdoc_typedefs(program, source, ctx, owners, &mut env);

    env
}

/// Lower ONE top-level statement's declarations into `env`.
///
/// The statement-granular lowering entry: [`build_eval_env`] folds every
/// statement through it, and the lazy declaration-body service lowers only
/// a demanded symbol's contributing statements through the same arms — one
/// shared lowering path, no per-consumer fork. JSDoc `@typedef`
/// registration is NOT part of the statement walk (it reads the program's
/// comments); whole-env builds run [`build_eval_env`], selective demands
/// register a demanded typedef through
/// [`lower_jsdoc_typedef_named`].
///
/// `ctx` is the producer-emitted anchor context (producing canonical +
/// contributor index) the minted authored locators / span origins anchor to.
pub fn lower_top_level_statement(
    stmt: &Statement<'_>,
    ctx: StatementLowerCtx<'_>,
    source: &str,
    env: &mut EvalEnv,
) {
    let parts = lower_statement_parts(stmt, source);
    register_statement_parts(parts, ctx, env);
}

/// Lower ONE top-level statement to its TRANSIENT declaration parts, without
/// registering anything. The single dispatch both the production registration
/// walk and the in-crate lowering tests consume — one lowering path, no fork.
pub fn lower_statement_parts(stmt: &Statement<'_>, source: &str) -> LoweredStatementParts {
    let mut out = LoweredStatementParts::default();
    collect_statement_parts(stmt, source, &mut out);
    out
}

/// Lower one top-level statement under Svelte 5 runes semantics.
///
/// This is deliberately a narrow extension of [`lower_statement_parts`], not a
/// second declaration-lowering engine: the ordinary statement walk produces
/// the complete transient declaration inventory first, then this function
/// replaces only an inferred top-level variable annotation whose initializer
/// is a supported identity-like rune call (`$state`, `$state.raw`,
/// `$state.snapshot`, `$state.eager`, `$derived`, or `$derived.by`). Explicit
/// TypeScript/JSDoc annotations continue to win, and callers must select this
/// entry only after the component's scope-aware mode classifier has proved
/// runes mode. Consequently a legacy store accessor named `$state` is never
/// reinterpreted as a rune.
pub fn lower_svelte_runes_statement_parts(
    stmt: &Statement<'_>,
    source: &str,
) -> LoweredStatementParts {
    let mut out = lower_statement_parts(stmt, source);
    apply_svelte_rune_initializer_inference(stmt, source, &mut out.value_decls);
    out
}

/// Mint the stored facts/locators from TRANSIENT statement parts and register
/// them on `env`, discarding the transient typed IR.
///
/// Public alongside [`lower_statement_parts`] so a demand-time producer can
/// SPLIT the two steps — retain the transient lowered bodies it needs for
/// fact-production (the decl-body content fingerprint) between lowering and
/// registration — without forking the lowering path. The retained transients
/// remain fact-production intermediates; only the minted facts/locators are
/// registered.
pub fn register_statement_parts(
    parts: LoweredStatementParts,
    ctx: StatementLowerCtx<'_>,
    env: &mut EvalEnv,
) {
    let LoweredStatementParts {
        type_decls,
        value_decls,
        aug_type_decls,
        aug_value_decls,
        alias_default_type_to,
    } = parts;
    for parts in type_decls {
        env.add_type(mint_type_decl(
            &parts,
            &ctx.build.canonical_id,
            ctx.statement_owner.owner,
            Some(DeclContributorAnchor {
                contributor_index: ctx.contributor_index,
                owner: ctx.statement_owner.owner,
                owner_local_ordinal: ctx.statement_owner.owner_local_ordinal,
            }),
        ));
    }
    for parts in value_decls {
        env.add_value(mint_value_decl(&parts, &ctx.build.canonical_id, ctx));
    }
    for (scope, parts) in aug_type_decls {
        env.add_augmentation_type(
            scope,
            mint_type_decl(
                &parts,
                &ctx.build.canonical_id,
                ctx.statement_owner.owner,
                Some(DeclContributorAnchor {
                    contributor_index: ctx.contributor_index,
                    owner: ctx.statement_owner.owner,
                    owner_local_ordinal: ctx.statement_owner.owner_local_ordinal,
                }),
            ),
        );
    }
    for (scope, parts) in aug_value_decls {
        env.add_augmentation_value(scope, mint_value_decl(&parts, &ctx.build.canonical_id, ctx));
    }
    if let Some(name) = alias_default_type_to {
        alias_default_export_type_symbol(env, ctx.statement_owner.owner, &name);
    }
}

fn collect_statement_parts(stmt: &Statement<'_>, source: &str, out: &mut LoweredStatementParts) {
    match stmt {
        Statement::TSTypeAliasDeclaration(decl) => {
            out.type_decls.push(lower_named_type_alias_parts(
                decl,
                source,
                decl.id.name.to_string(),
            ));
        }
        Statement::TSInterfaceDeclaration(decl) => {
            out.type_decls.push(lower_named_interface_parts(
                decl,
                source,
                decl.id.name.to_string(),
            ));
        }
        Statement::TSModuleDeclaration(module) => {
            collect_module_declaration(module, source, out, None);
        }
        Statement::TSGlobalDeclaration(global) => {
            collect_augmentation_block(&global.body, source, out, AugmentationScopeKind::Global);
        }
        Statement::ClassDeclaration(decl) => {
            collect_class(decl, source, out);
        }
        Statement::TSEnumDeclaration(decl) => {
            collect_enum(decl, out);
        }
        Statement::FunctionDeclaration(func) => {
            if let Some(parts) = lower_function_parts(func, source) {
                out.value_decls.push(parts);
            }
        }
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                if let Some(parts) = lower_variable_parts(decl, var_decl.kind, source, None) {
                    out.value_decls.push(parts);
                }
            }
        }
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                collect_from_declaration(decl, source, out);
            }
        }
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                if let Some(parts) = lower_function_parts(func, source) {
                    out.value_decls.push(parts);
                }
            }
            ExportDefaultDeclarationKind::ClassDeclaration(cls) => {
                collect_class(cls, source, out);
                // `export default class Props { … }` exports the class under
                // the `default` export name (the named identifier is NOT a
                // separate export — see ShallowFileState's default-export
                // contract), but the class lowering keys the instance shape
                // under the declared name `Props`. A barrel that reaches this
                // file resolves the `(canonical, "default")` route, so the
                // class body must also be reachable under `default`. Alias the
                // declared-name type symbol into a `default` entry (same body
                // slot, same params) so the prepared-decl lookup at the
                // resolved default route hydrates the class.
                out.alias_default_type_to = class_or_function_default_name(&cls.id);
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(iface) => {
                out.type_decls.push(lower_named_interface_parts(
                    iface,
                    source,
                    iface.id.name.to_string(),
                ));
                out.alias_default_type_to = Some(iface.id.name.to_string());
            }
            other => {
                if let Some(expr) = other.as_expression() {
                    out.value_decls
                        .push(lower_default_expression_parts(expr, source));
                }
            }
        },
        _ => {}
    }
}

/// Register the JSDoc `@typedef {T} Name` declaration named `name` into
/// `env`, applying the same TS-decl precedence as the whole-env walk: a
/// name a TS declaration already claimed in `env` is skipped. Returns
/// the registered typedef's TRANSIENT lowered body (`None` when nothing
/// registered) so the demanding producer can derive body-sensitive facts
/// (dependency roots, the decl-body content fingerprint) from the same
/// lowering that registered the facts — a fact-production intermediate,
/// never a persisted body.
///
/// The selective counterpart to the whole-env typedef registration inside
/// [`build_eval_env`] — a demanded symbol that exists only as a `@typedef`
/// lowers exactly its own `{T}` payload.
pub fn lower_jsdoc_typedef_named(
    comments: &[oxc_ast::Comment],
    source: &str,
    name: &str,
    ctx: &BuildEvalEnvContext,
    env: &mut EvalEnv,
) -> Option<crate::analysis::jsdoc::JsdocTypedef> {
    lower_jsdoc_typedef_named_matching(
        comments,
        source,
        name,
        TopLevelOwnerId::ordinary_file(),
        None,
        ctx,
        env,
    )
}

/// Lower one exact owner-qualified JSDoc typedef comment.
pub fn lower_jsdoc_typedef_at_comment(
    comments: &[oxc_ast::Comment],
    source: &str,
    name: &str,
    owner: TopLevelOwnerId,
    comment_start: u32,
    ctx: &BuildEvalEnvContext,
    env: &mut EvalEnv,
) -> Option<crate::analysis::jsdoc::JsdocTypedef> {
    lower_jsdoc_typedef_named_matching(comments, source, name, owner, Some(comment_start), ctx, env)
}

fn lower_jsdoc_typedef_named_matching(
    comments: &[oxc_ast::Comment],
    source: &str,
    name: &str,
    owner: TopLevelOwnerId,
    comment_start: Option<u32>,
    ctx: &BuildEvalEnvContext,
    env: &mut EvalEnv,
) -> Option<crate::analysis::jsdoc::JsdocTypedef> {
    if env.type_group_in(owner, name).is_some() {
        return None;
    }
    for typedef in crate::analysis::jsdoc::collect_jsdoc_typedefs(comments, source) {
        if typedef.name != name
            || comment_start.is_some_and(|start| typedef.comment_span.start != start)
        {
            continue;
        }
        let parts = LoweredTypeDeclParts {
            name: typedef.name.clone(),
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: typedef.body.clone(),
            direct_member_return_inference: Vec::new(),
        };
        env.add_type(mint_type_decl(&parts, &ctx.canonical_id, owner, None));
        return Some(typedef);
    }
    None
}

/// Register each JSDoc `@typedef {T} Name` from the program's comments as a
/// `TypeDeclInfo` alias, skipping any name a TS declaration already claimed
/// (TS-decl precedence).
fn register_jsdoc_typedefs(
    program: &Program<'_>,
    source: &str,
    ctx: &BuildEvalEnvContext,
    owners: &TopLevelOwnerTable,
    env: &mut EvalEnv,
) {
    for typedef in crate::analysis::jsdoc::collect_jsdoc_typedefs(&program.comments, source) {
        let Some(attached_owner) = owners.resolve_comment_owner(
            typedef.attached_to,
            typedef.comment_span,
            program.body.iter().map(|statement| statement.span().start),
        ) else {
            continue;
        };
        if env
            .type_group_in(attached_owner.owner, &typedef.name)
            .is_some()
        {
            // A real TS `type`/`interface`/`class` of this name was registered
            // during the statement walk; it is authoritative.
            continue;
        }
        let parts = LoweredTypeDeclParts {
            name: typedef.name,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: typedef.body,
            direct_member_return_inference: Vec::new(),
        };
        env.add_type(mint_type_decl(
            &parts,
            &ctx.canonical_id,
            attached_owner.owner,
            None,
        ));
    }
}

fn collect_from_declaration(decl: &Declaration<'_>, source: &str, out: &mut LoweredStatementParts) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            out.type_decls.push(lower_named_type_alias_parts(
                alias,
                source,
                alias.id.name.to_string(),
            ));
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            out.type_decls.push(lower_named_interface_parts(
                iface,
                source,
                iface.id.name.to_string(),
            ));
        }
        Declaration::TSModuleDeclaration(module) => {
            collect_module_declaration(module, source, out, None);
        }
        Declaration::TSGlobalDeclaration(global) => {
            collect_augmentation_block(&global.body, source, out, AugmentationScopeKind::Global);
        }
        Declaration::ClassDeclaration(cls) => {
            collect_class(cls, source, out);
        }
        Declaration::TSEnumDeclaration(decl) => {
            collect_enum(decl, out);
        }
        Declaration::FunctionDeclaration(func) => {
            if let Some(parts) = lower_function_parts(func, source) {
                out.value_decls.push(parts);
            }
        }
        Declaration::VariableDeclaration(var_decl) => {
            for d in &var_decl.declarations {
                if let Some(parts) = lower_variable_parts(d, var_decl.kind, source, None) {
                    out.value_decls.push(parts);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Fact / locator minting (transient parts → stored inventory)
// ---------------------------------------------------------------------------

/// The content-free anchor of a declared symbol's authored positions.
fn decl_anchor(
    canonical_id: &Arc<str>,
    owner: TopLevelOwnerId,
    name: &str,
    space: LocatorSymbolSpace,
) -> AuthoredAnchor {
    AuthoredAnchor {
        canonical_id: canonical_id.clone(),
        owner,
        symbol: Arc::from(name),
        space,
    }
}

/// A body slot at `anchor` with the given path.
fn anchored_slot(anchor: &AuthoredAnchor, path: Vec<TypeBodyPathStep>) -> TypeBodySlot {
    TypeBodySlot {
        anchor: anchor.clone(),
        path: path.into(),
    }
}

/// Narrow a lowered decl-header type-parameter list to its header facts: each
/// parameter's name + ordinal, plus the content-free locators of its AUTHORED
/// constraint / default bound positions (`[TypeParamBound { ordinal, position }]`
/// rooted at the declaration header — the one placement the closed path
/// vocabulary defines for type-parameter bounds).
fn narrow_decl_header_type_params(
    params: &[TypeParam],
    anchor: &AuthoredAnchor,
) -> TypeParamDeclFact {
    TypeParamDeclFact {
        params: params
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                let ordinal = u32::try_from(index).ok()?;
                let bound_slot = |position: TypeParamBoundPosition| {
                    anchored_slot(
                        anchor,
                        vec![TypeBodyPathStep::TypeParamBound { ordinal, position }],
                    )
                };
                Some(NarrowTypeParam {
                    name: param.name.clone(),
                    ordinal,
                    constraint: param
                        .constraint
                        .is_some()
                        .then(|| bound_slot(TypeParamBoundPosition::Constraint)),
                    default: param
                        .default
                        .is_some()
                        .then(|| bound_slot(TypeParamBoundPosition::Default)),
                })
            })
            .collect(),
    }
}

/// Narrow a SIGNATURE-scoped type-parameter list (a function declaration's /
/// method's own `<T extends C>` list) to name + ordinal facts. Signature-scoped
/// bounds live ON the signature's authored position: the closed path vocabulary
/// addresses type-parameter bounds only on TYPE-space declaration headers
/// (a value / method signature's bound is recovered whole-signature when the
/// signature position is demanded), so no independent bound slot exists to
/// mint — deliberately NOT a fabricated locator.
pub(crate) fn narrow_signature_type_params(params: &[TypeParam]) -> Arc<[NarrowTypeParam]> {
    params
        .iter()
        .enumerate()
        .filter_map(|(index, param)| {
            let ordinal = u32::try_from(index).ok()?;
            Some(NarrowTypeParam {
                name: param.name.clone(),
                ordinal,
                // `TypeParamBound` is a type-space DECL-HEADER first-step-only
                // position — not addressable for a signature-scoped parameter.
                // Honest typed miss: an authored `extends` / `=` bound here is
                // recovered whole-signature on demand, never through a fabricated
                // slot.
                constraint: None,
                default: None,
            })
        })
        .collect()
}

/// Mint the stored [`TypeDeclInfo`] from transient type-decl parts: the
/// whole-body slot locator, the type-parameter header facts, and the direct
/// member-header facts — the body typed IR is derived from and discarded.
fn mint_type_decl(
    parts: &LoweredTypeDeclParts,
    canonical_id: &Arc<str>,
    owner: TopLevelOwnerId,
    contributor: Option<DeclContributorAnchor>,
) -> TypeDeclInfo {
    let anchor = decl_anchor(canonical_id, owner, &parts.name, LocatorSymbolSpace::Type);
    let direct_member_return_inference = match contributor {
        Some(contributor) => parts
            .direct_member_return_inference
            .iter()
            .map(|fact| MemberReturnInferenceFact {
                origin: FunctionSpansOrigin::Member {
                    anchor: contributor,
                    member_path: Arc::clone(&fact.member_path),
                },
                return_inference: fact.return_inference,
            })
            .collect(),
        None => {
            debug_assert!(parts.direct_member_return_inference.is_empty());
            Arc::from(Vec::<MemberReturnInferenceFact>::new().into_boxed_slice())
        }
    };
    TypeDeclInfo {
        name: parts.name.clone(),
        owner,
        declaration_id: 0,
        kind: parts.kind,
        type_parameters: narrow_decl_header_type_params(&parts.type_parameters, &anchor),
        direct_member_headers: member_header_facts_from_body(&parts.body),
        direct_member_return_inference,
        body: anchored_slot(&anchor, Vec::new()),
    }
}

/// The annotation SOURCE for a value declaration's annotation fact:
///
/// - an AUTHORED annotation (TS annotation / JSDoc `@type`) → its decl-body
///   locator (the value-space whole-decl slot addresses exactly the annotation
///   position);
/// - an INFERRED annotation that is a trivially-closed leaf (primitive /
///   literal) → the closed leaf fact;
/// - any other INFERRED annotation → `None`: no authored `TSType` node exists
///   to address and the shape is not closed-representable, so the type is
///   recovered by demanding the declaration (never a fabricated locator).
fn annotation_source(
    annotation: Option<&TypeExpr>,
    annotation_is_authored: bool,
    anchor: &AuthoredAnchor,
) -> Option<SemanticTypeSource> {
    let annotation = annotation?;
    if annotation_is_authored {
        return Some(SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(
            anchored_slot(anchor, Vec::new()),
        )));
    }
    let leaf = match annotation {
        TypeExpr::Primitive(name) => LeafTypeFact::Primitive(*name),
        TypeExpr::Literal(LiteralValue::String(value)) => {
            LeafTypeFact::StringLiteral(value.clone())
        }
        TypeExpr::Literal(LiteralValue::Number(value)) => {
            LeafTypeFact::NumberLiteral(format_enum_number(*value))
        }
        TypeExpr::Literal(LiteralValue::Boolean(value)) => LeafTypeFact::BooleanLiteral(*value),
        _ => return None,
    };
    Some(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(leaf)))
}

/// The span-recovery origin of one shape member at `ordinal`, under the owning
/// declaration's authored contributor statement.
fn shape_member_span_origin(contributor: DeclContributorAnchor, ordinal: u32) -> MemberSpansOrigin {
    MemberSpansOrigin::Authored {
        anchor: contributor,
        member_path: Arc::from(vec![ordinal]),
    }
}

/// The [`FunctionSpansOrigin`] for a transient signature, under the owning
/// declaration's contributor statement.
fn signature_spans_origin(
    origin: LoweredSignatureOrigin,
    contributor: DeclContributorAnchor,
) -> FunctionSpansOrigin {
    match origin {
        LoweredSignatureOrigin::DeclBody => FunctionSpansOrigin::AliasBody {
            anchor: contributor,
        },
        LoweredSignatureOrigin::ShapeMember { ordinal } => FunctionSpansOrigin::Member {
            anchor: contributor,
            member_path: Arc::from(vec![ordinal]),
        },
        LoweredSignatureOrigin::Synthetic => FunctionSpansOrigin::Synthetic(SourceSynthetic),
    }
}

/// Mint one [`FunctionSignatureFact`] from transient signature parts.
///
/// `first_step` roots every parameter / return locator at the signature's
/// authored position (`ValueSignature { ordinal }` for a value declaration's
/// own overload-group member; `Member { ordinal }` for an object-shape member
/// signature). A parameter slot mints ONLY for an authored positional TS
/// annotation — the one position the `FunctionParam` step derefs
/// (`params.items[ordinal].type_annotation`). An UNANNOTATED parameter has no
/// authored `TSType` to address, and a REST parameter lives past `params.items`
/// (its annotation, if any, is recovered whole-signature); both store
/// `ty: None` — the typed miss, never a fabricated slot. The rest span
/// selector still carries the honest `Rest` marker.
fn signature_fact(
    sig: &LoweredSignatureParts,
    anchor: &AuthoredAnchor,
    first_step: TypeBodyPathStep,
    contributor: DeclContributorAnchor,
) -> FunctionSignatureFact {
    let spans_origin = signature_spans_origin(sig.origin, contributor);
    FunctionSignatureFact {
        type_parameters: narrow_signature_type_params(&sig.type_parameters),
        parameters: sig
            .parameters
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                let ordinal = u32::try_from(index).ok()?;
                Some(FunctionParamFact {
                    name: param.name.clone(),
                    optional: param.optional,
                    rest: param.rest,
                    has_ts_annotation: param.has_ts_annotation,
                    ty: (!param.rest && param.has_ts_annotation).then(|| {
                        anchored_slot(
                            anchor,
                            vec![first_step, TypeBodyPathStep::FunctionParam { ordinal }],
                        )
                    }),
                    span_origin: FunctionParamSpanOrigin {
                        function: spans_origin.clone(),
                        param: if param.rest {
                            FunctionParamSelector::Rest
                        } else {
                            FunctionParamSelector::Positional { ordinal }
                        },
                    },
                })
            })
            .collect(),
        // Return inference is a semantic body product even when no authored
        // `TSType` node exists. Keep the content-free replay path whenever the
        // lowering produced a return carrier; `spans_origin` remains the
        // independent source-span authority and therefore never fabricates an
        // authored return span for this inferred slot.
        return_ty: sig
            .return_type
            .as_ref()
            .map(|_| anchored_slot(anchor, vec![first_step, TypeBodyPathStep::FunctionReturn])),
        return_inference: sig.return_inference,
        has_implementation_body: sig.has_implementation_body,
        spans_origin,
    }
}

/// Mint one member-position [`FunctionSignatureFact`] from a transient
/// [`FunctionExpr`] (an object-shape method / call / construct signature).
/// `has_implementation_body` is inert at member positions (overload-group
/// visibility is a value-space concern), so it carries the caller-known flag —
/// `false` where the transient IR does not record one.
fn member_signature_fact(
    function: &FunctionExpr,
    anchor: &AuthoredAnchor,
    member_ordinal: u32,
    contributor: DeclContributorAnchor,
    has_implementation_body: bool,
    return_inference: ReturnInferenceCompleteness,
) -> FunctionSignatureFact {
    let sig = LoweredSignatureParts {
        parameters: function.parameters.clone(),
        return_type: function.return_type.as_deref().cloned(),
        return_inference,
        type_parameters: function.type_parameters.clone(),
        has_implementation_body,
        // A member signature's authored return position is part of the member
        // body; the transient `FunctionExpr` does not record whether it was
        // authored, so no independent return locator is minted (recovered
        // whole-member on demand).
        has_authored_return: false,
        origin: LoweredSignatureOrigin::ShapeMember {
            ordinal: member_ordinal,
        },
    };
    signature_fact(
        &sig,
        anchor,
        TypeBodyPathStep::Member {
            ordinal: member_ordinal,
        },
        contributor,
    )
}

/// Mint the [`ObjectShapeFact`] from a transient object shape. `Member`
/// ordinals index THIS produced shape surface in source order (raw member
/// index — the shape is recovered by re-lowering the declaration on demand).
fn object_shape_fact(
    shape: &ObjectExpr,
    anchor: &AuthoredAnchor,
    contributor: DeclContributorAnchor,
    declared_ctor: Option<bool>,
    member_return_inference: &[LoweredMemberReturnInference],
) -> ObjectShapeFact {
    let members = shape
        .properties
        .iter()
        .enumerate()
        .filter_map(|(index, member)| {
            let ordinal = u32::try_from(index).ok()?;
            Some(match member {
                ObjectMember::Property(prop) => ObjectMemberFact::Property(ObjectPropertyFact {
                    name: prop.name.clone(),
                    optional: prop.optional,
                    readonly: prop.readonly,
                    visibility: prop.visibility,
                    ty: anchored_slot(
                        anchor,
                        vec![
                            TypeBodyPathStep::Member { ordinal },
                            TypeBodyPathStep::MemberValue,
                        ],
                    ),
                    span_origin: shape_member_span_origin(contributor, ordinal),
                }),
                ObjectMember::Method(method) => ObjectMemberFact::Method(ObjectMethodFact {
                    name: method.name.clone(),
                    optional: method.optional,
                    visibility: method.visibility,
                    function: member_signature_fact(
                        &method.function,
                        anchor,
                        ordinal,
                        contributor,
                        false,
                        if declared_ctor.is_some() {
                            member_return_inference
                                .iter()
                                .find(|fact| fact.member_path.as_ref() == [ordinal])
                                .map(|fact| fact.return_inference)
                                .expect("class static method has an exact producer verdict")
                        } else {
                            member_return_inference
                                .iter()
                                .find(|fact| fact.member_path.as_ref() == [ordinal])
                                .map(|fact| fact.return_inference)
                                .unwrap_or(ReturnInferenceCompleteness::NotInferred)
                        },
                    ),
                    span_origin: shape_member_span_origin(contributor, ordinal),
                }),
                ObjectMember::CallSignature(function) => {
                    ObjectMemberFact::CallSignature(member_signature_fact(
                        function,
                        anchor,
                        ordinal,
                        contributor,
                        false,
                        ReturnInferenceCompleteness::NotInferred,
                    ))
                }
                ObjectMember::ConstructSignature(function) => {
                    // A class's construct signature is authored exactly when a
                    // constructor was declared; a synthesized default carries
                    // the honest synthetic origin instead of a fabricated
                    // member position.
                    let fact = if declared_ctor == Some(false) {
                        let sig = LoweredSignatureParts {
                            parameters: function.parameters.clone(),
                            return_type: function.return_type.as_deref().cloned(),
                            return_inference: ReturnInferenceCompleteness::NotInferred,
                            type_parameters: function.type_parameters.clone(),
                            has_implementation_body: true,
                            has_authored_return: false,
                            origin: LoweredSignatureOrigin::Synthetic,
                        };
                        signature_fact(
                            &sig,
                            anchor,
                            TypeBodyPathStep::Member { ordinal },
                            contributor,
                        )
                    } else {
                        member_signature_fact(
                            function,
                            anchor,
                            ordinal,
                            contributor,
                            true,
                            ReturnInferenceCompleteness::NotInferred,
                        )
                    };
                    ObjectMemberFact::ConstructSignature(fact)
                }
                ObjectMember::IndexSignature(index_sig) => {
                    ObjectMemberFact::IndexSignature(IndexSignatureFact {
                        key_name: index_sig.key_name.clone(),
                        key_type: match &index_sig.key_type {
                            TypeExpr::Primitive(PrimitiveName::String) => KeyTypeShape::String,
                            TypeExpr::Primitive(PrimitiveName::Number) => KeyTypeShape::Number,
                            TypeExpr::Primitive(PrimitiveName::Symbol) => KeyTypeShape::Symbol,
                            _ => KeyTypeShape::Other(anchored_slot(
                                anchor,
                                vec![
                                    TypeBodyPathStep::Member { ordinal },
                                    TypeBodyPathStep::IndexSignatureKey,
                                ],
                            )),
                        },
                        value_type: anchored_slot(
                            anchor,
                            vec![
                                TypeBodyPathStep::Member { ordinal },
                                TypeBodyPathStep::IndexSignatureValue,
                            ],
                        ),
                        readonly: index_sig.readonly,
                        span_origin: IndexSignatureSpansOrigin::Authored {
                            anchor: contributor,
                            member_path: Arc::from(vec![ordinal]),
                        },
                    })
                }
            })
        })
        .collect();
    ObjectShapeFact { members }
}

/// Mint the stored [`ValueDeclInfo`] from transient value-decl parts. Signature
/// locators are rooted at their LOCAL overload ordinal here; group-level
/// rebasing happens at registration ([`EvalEnv::add_value`]).
fn mint_value_decl(
    parts: &LoweredValueDeclParts,
    canonical_id: &Arc<str>,
    ctx: StatementLowerCtx<'_>,
) -> ValueDeclInfo {
    let owner = ctx.statement_owner.owner;
    let anchor = decl_anchor(canonical_id, owner, &parts.name, LocatorSymbolSpace::Value);
    let contributor = DeclContributorAnchor {
        contributor_index: ctx.contributor_index,
        owner,
        owner_local_ordinal: ctx.statement_owner.owner_local_ordinal,
    };
    let type_annotation = value_type_annotation_fact(
        parts.type_annotation.as_ref(),
        &parts.name,
        canonical_id,
        owner,
        annotation_source(
            parts.type_annotation.as_ref(),
            parts.annotation_is_authored,
            &anchor,
        ),
        parts.inference_unavailable,
    );
    let signatures = parts
        .signatures
        .iter()
        .enumerate()
        .filter_map(|(index, sig)| {
            let ordinal = u32::try_from(index).ok()?;
            Some(signature_fact(
                sig,
                &anchor,
                TypeBodyPathStep::ValueSignature { ordinal },
                contributor,
            ))
        })
        .collect();
    // A class value decl's constructor-shape ConstructSignature member is
    // authored exactly when the class declared a constructor; the flag is
    // derived from the transient signature's origin.
    let declared_ctor = (parts.kind == ValueDeclKind::Class).then(|| {
        parts
            .signatures
            .first()
            .is_some_and(|sig| sig.origin != LoweredSignatureOrigin::Synthetic)
    });
    let object_shape = parts.object_shape.as_ref().map(|shape| {
        object_shape_fact(
            shape,
            &anchor,
            contributor,
            declared_ctor,
            &parts.object_member_return_inference,
        )
    });
    let enum_members = parts.enum_members.as_ref().map(|members| EnumMemberFact {
        members: members
            .iter()
            .map(|(name, value)| EnumMemberEntry {
                name: name.clone(),
                value: value.projected_scalar(),
            })
            .collect(),
    });
    ValueDeclInfo {
        name: parts.name.clone(),
        owner,
        declaration_id: 0,
        kind: parts.kind,
        type_annotation,
        signatures,
        object_shape,
        enum_members,
        enum_member_names: parts.enum_member_names.clone(),
    }
}

/// Exact-repr formatting of a folded numeric enum scalar / literal (Rust's
/// minimal `f64` display: `1.0` → `"1"`, `-1.0` → `"-1"`, `0.5` → `"0.5"`).
fn format_enum_number(value: f64) -> String {
    format!("{value}")
}

// ---------------------------------------------------------------------------
// Type declarations
// ---------------------------------------------------------------------------

/// Mint the DIRECT member-header FACT inventory from a freshly-lowered decl
/// body: its own object members, descending intersection / parenthesized arms
/// (a heritage `Ref` arm carries no direct member and contributes nothing).
/// First-seen dedup by name, matching the production header index's member
/// union. This is a PRODUCER-time transform over the producing value (the body
/// this same lowering just built) — consumers read the stored facts, never
/// re-walk a body.
fn member_header_facts_from_body(body: &TypeExpr) -> Arc<[MemberHeaderFact]> {
    fn collect(body: &TypeExpr, out: &mut Vec<MemberHeaderFact>) {
        match body {
            TypeExpr::Object(object) => {
                for member in &object.properties {
                    let fact = match member {
                        ObjectMember::Property(prop) => MemberHeaderFact {
                            name: prop.name.clone(),
                            is_method: false,
                            optional: prop.optional,
                            readonly: prop.readonly,
                            visibility: prop.visibility,
                        },
                        ObjectMember::Method(method) => MemberHeaderFact {
                            name: method.name.clone(),
                            is_method: true,
                            optional: method.optional,
                            readonly: false,
                            visibility: method.visibility,
                        },
                        // Call / construct / index signatures are nameless —
                        // they are not member HEADERS.
                        _ => continue,
                    };
                    if !out.iter().any(|existing| existing.name == fact.name) {
                        out.push(fact);
                    }
                }
            }
            TypeExpr::Intersection(parts) => {
                for part in parts.iter() {
                    collect(part, out);
                }
            }
            TypeExpr::Parenthesized(inner) => collect(inner, out),
            _ => {}
        }
    }

    let mut out = Vec::new();
    collect(body, &mut out);
    out.into()
}

fn lower_named_type_alias_parts(
    decl: &TSTypeAliasDeclaration<'_>,
    source: &str,
    name: String,
) -> LoweredTypeDeclParts {
    let type_parameters = decl
        .type_parameters
        .as_ref()
        .map(|tp| lower_type_param_decls(tp, source))
        .unwrap_or_default();
    let body = lower_ts_type(&decl.type_annotation, source);

    LoweredTypeDeclParts {
        name,
        kind: TypeDeclKind::Alias,
        type_parameters,
        body,
        direct_member_return_inference: Vec::new(),
    }
}

fn heritage_expression_name(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::Identifier(identifier) => Some(identifier.name.to_string()),
        Expression::StaticMemberExpression(member) => {
            let mut path = Vec::new();
            collect_static_member_path(member, &mut path);
            (!path.is_empty()).then(|| path.join("."))
        }
        _ => None,
    }
}

fn lower_named_interface_parts(
    decl: &TSInterfaceDeclaration<'_>,
    source: &str,
    name: String,
) -> LoweredTypeDeclParts {
    let type_parameters = decl
        .type_parameters
        .as_ref()
        .map(|tp| lower_type_param_decls(tp, source))
        .unwrap_or_default();

    // Build the body from the interface members. Pre-sized to the AST
    // member count — a tight upper bound (only unloweable members drop).
    let mut members = Vec::with_capacity(decl.body.body.len());
    for sig in &decl.body.body {
        if let Some(m) = lower_interface_member(sig, source) {
            members.push(m);
        }
    }

    // Handle extends clauses — merge inherited properties
    let mut body = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }));

    if !decl.extends.is_empty() {
        // One part per heritage clause (non-identifier clauses drop) plus
        // the own-member object pushed after the loop.
        let mut parts = Vec::with_capacity(decl.extends.len() + 1);
        for heritage in &decl.extends {
            let Some(base_name) = heritage_expression_name(&heritage.expression) else {
                continue;
            };
            let base_args: Vec<TypeExpr> = heritage
                .type_arguments
                .as_ref()
                .map(|tp| tp.params.iter().map(|p| lower_ts_type(p, source)).collect())
                .unwrap_or_default();
            parts.push(if base_args.is_empty() {
                TypeExpr::named(base_name)
            } else {
                TypeExpr::named_with_args(base_name, base_args)
            });
        }
        parts.push(body);
        body = TypeExpr::intersection(parts);
    }

    LoweredTypeDeclParts {
        name,
        kind: TypeDeclKind::Interface,
        type_parameters,
        body,
        direct_member_return_inference: Vec::new(),
    }
}

fn collect_module_declaration(
    decl: &TSModuleDeclaration<'_>,
    source: &str,
    out: &mut LoweredStatementParts,
    prefix: Option<&str>,
) {
    // `declare module "<specifier>" { ... }` — an AMBIENT MODULE AUGMENTATION,
    // NOT a file-scope namespace. Its inner declarations augment the surface of
    // the module reached by `<specifier>` (the canonical Vue/Vite `declare
    // module "vue"` pattern, or a relative `declare module "./base"`), so they
    // are retained in the augmentation-scope inventory keyed by the raw
    // specifier — never the file's top-level `type_symbols`. (A string-literal
    // name only ever wraps a single `TSModuleBlock`, never a nested module.)
    if let TSModuleDeclarationName::StringLiteral(spec) = &decl.id {
        if let Some(TSModuleDeclarationBody::TSModuleBlock(block)) = decl.body.as_ref() {
            collect_augmentation_block(
                block,
                source,
                out,
                AugmentationScopeKind::Module(spec.value.to_string()),
            );
        }
        return;
    }

    let Some(module_name) = qualified_module_name(prefix, &decl.id) else {
        return;
    };
    let Some(body) = decl.body.as_ref() else {
        return;
    };

    match body {
        TSModuleDeclarationBody::TSModuleDeclaration(inner) => {
            collect_module_declaration(inner, source, out, Some(module_name.as_str()));
        }
        TSModuleDeclarationBody::TSModuleBlock(block) => {
            for stmt in &block.body {
                collect_namespaced_statement(stmt, source, out, module_name.as_str());
            }
        }
    }
}

/// Retain the inner declarations of an ambient augmentation block
/// (`declare module "X" { ... }` or `declare global { ... }`) into the scoped
/// augmentation parts under `scope`. Inner interfaces/type-aliases keep
/// their UNQUALIFIED names (an augmenter contributes `interface Config`, not
/// `external-spec.Config`) and never enter file-scope `type_symbols`.
fn collect_augmentation_block(
    block: &TSModuleBlock<'_>,
    source: &str,
    out: &mut LoweredStatementParts,
    scope: AugmentationScopeKind,
) {
    for stmt in &block.body {
        match stmt {
            Statement::TSInterfaceDeclaration(iface) => {
                let name = iface.id.name.to_string();
                out.aug_type_decls.push((
                    scope.clone(),
                    lower_named_interface_parts(iface, source, name),
                ));
            }
            Statement::TSTypeAliasDeclaration(alias) => {
                let name = alias.id.name.to_string();
                out.aug_type_decls.push((
                    scope.clone(),
                    lower_named_type_alias_parts(alias, source, name),
                ));
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = export.declaration.as_ref() {
                    collect_augmentation_declaration(decl, source, out, &scope);
                }
            }
            // Value-space declarations (`const`/`let`/`var`, `function`,
            // `class`) augment the target module's VALUE surface. Reuse the
            // file-scope lowering so the full retained parts are built exactly
            // as for a top-level declaration, routed into the augmentation
            // value scope (never file-scope `value_symbols`).
            Statement::VariableDeclaration(_)
            | Statement::FunctionDeclaration(_)
            | Statement::ClassDeclaration(_) => {
                collect_value_statement_into_augmentation(stmt, source, out, &scope);
            }
            // A namespace nested inside an ambient augmentation block
            // (`declare global { namespace JSX { ... } }` /
            // `declare module "X" { namespace N { ... } }`) contributes its
            // inner type/value members under their QUALIFIED `Ns.Member` names
            // into the SAME augmentation scope. Because
            // [`EvalEnv::add_augmentation_type`] keys on `(scope, qualified
            // name)` and APPENDS, a repeated `declare global { namespace JSX {
            // ... } }` block folds into the same ordered group and the existing
            // `MergedDecl` peer-merge stitch unions the surfaces.
            Statement::TSModuleDeclaration(module) => {
                collect_augmentation_module_declaration(module, source, out, &scope, None);
            }
            _ => {}
        }
    }
}

/// Route a `Declaration` inside an ambient augmentation block to the correct
/// augmentation parts: interfaces / type-aliases to the type scope, value
/// declarations to the value scope.
fn collect_augmentation_declaration(
    decl: &Declaration<'_>,
    source: &str,
    out: &mut LoweredStatementParts,
    scope: &AugmentationScopeKind,
) {
    match decl {
        Declaration::TSInterfaceDeclaration(iface) => {
            let name = iface.id.name.to_string();
            out.aug_type_decls.push((
                scope.clone(),
                lower_named_interface_parts(iface, source, name),
            ));
        }
        Declaration::TSTypeAliasDeclaration(alias) => {
            let name = alias.id.name.to_string();
            out.aug_type_decls.push((
                scope.clone(),
                lower_named_type_alias_parts(alias, source, name),
            ));
        }
        Declaration::VariableDeclaration(_)
        | Declaration::FunctionDeclaration(_)
        | Declaration::ClassDeclaration(_) => {
            let mut inner = LoweredStatementParts::default();
            collect_from_declaration(decl, source, &mut inner);
            move_value_parts_into_augmentation(inner, out, scope);
        }
        Declaration::TSModuleDeclaration(module) => {
            collect_augmentation_module_declaration(module, source, out, scope, None);
        }
        _ => {}
    }
}

/// Retain a `namespace N { ... }` nested inside an ambient augmentation block
/// (`declare global { namespace JSX { ... } }` /
/// `declare module "X" { namespace N { ... } }`) into the scoped augmentation
/// parts. Inner interfaces / type-aliases register under their QUALIFIED
/// `Ns.Member` name (`JSX.IntrinsicElements`) — a consumer references the member
/// as `JSX.IntrinsicElements`, never a bare `IntrinsicElements` — and never
/// enter file-scope `type_symbols`. Because [`EvalEnv::add_augmentation_type`]
/// keys on `(scope, qualified name)` and APPENDS, a repeated `declare global {
/// namespace JSX { ... } }` block folds its members into the same ordered
/// `TypeDeclGroup`, so the existing `MergedDecl` peer-merge stitch unions the
/// surfaces.
///
/// This is the augmentation-scope mirror of [`collect_module_declaration`]'s
/// identifier-name branch (which routes a file-scope namespace's members to
/// the file-scope parts under the same qualified names).
fn collect_augmentation_module_declaration(
    decl: &TSModuleDeclaration<'_>,
    source: &str,
    out: &mut LoweredStatementParts,
    scope: &AugmentationScopeKind,
    prefix: Option<&str>,
) {
    // A string-literal module name (`declare module "X"`) nested inside another
    // augmentation block is not a namespace-member contributor; only
    // identifier-named namespaces (`namespace JSX`) qualify members here.
    let Some(namespace) = qualified_module_name(prefix, &decl.id) else {
        return;
    };
    let Some(body) = decl.body.as_ref() else {
        return;
    };
    match body {
        TSModuleDeclarationBody::TSModuleDeclaration(inner) => {
            collect_augmentation_module_declaration(
                inner,
                source,
                out,
                scope,
                Some(namespace.as_str()),
            );
        }
        TSModuleDeclarationBody::TSModuleBlock(block) => {
            for stmt in &block.body {
                collect_namespaced_statement_into_augmentation(
                    stmt,
                    source,
                    out,
                    namespace.as_str(),
                    scope,
                );
            }
        }
    }
}

/// Augmentation-scope mirror of [`collect_namespaced_statement`]: register a
/// namespace member nested inside an ambient augmentation block under its
/// qualified `Ns.Member` name in the augmentation parts (never file scope).
fn collect_namespaced_statement_into_augmentation(
    stmt: &Statement<'_>,
    source: &str,
    out: &mut LoweredStatementParts,
    namespace: &str,
    scope: &AugmentationScopeKind,
) {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => {
            out.aug_type_decls.push((
                scope.clone(),
                lower_named_type_alias_parts(
                    alias,
                    source,
                    qualified_name(namespace, &alias.id.name),
                ),
            ));
        }
        Statement::TSInterfaceDeclaration(iface) => {
            out.aug_type_decls.push((
                scope.clone(),
                lower_named_interface_parts(
                    iface,
                    source,
                    qualified_name(namespace, &iface.id.name),
                ),
            ));
        }
        Statement::TSModuleDeclaration(module) => {
            collect_augmentation_module_declaration(module, source, out, scope, Some(namespace));
        }
        // Namespace VALUE indexing is EXPORT-ONLY (mirrors
        // `collect_namespaced_statement`): a non-exported `const hidden = …` is
        // private to the namespace body, so a DIRECT `VariableDeclaration` is
        // intentionally not indexed. Only the exported path registers a
        // qualified value member such as `JSX.VERSION`.
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                collect_namespaced_declaration_into_augmentation(
                    decl, source, out, namespace, scope,
                );
            }
        }
        _ => {}
    }
}

/// Augmentation-scope mirror of [`collect_namespaced_declaration`]: an exported
/// namespace member nested in an ambient augmentation block registers under its
/// qualified `Ns.Member` name (types into the type scope, values into the value
/// scope).
fn collect_namespaced_declaration_into_augmentation(
    decl: &Declaration<'_>,
    source: &str,
    out: &mut LoweredStatementParts,
    namespace: &str,
    scope: &AugmentationScopeKind,
) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            out.aug_type_decls.push((
                scope.clone(),
                lower_named_type_alias_parts(
                    alias,
                    source,
                    qualified_name(namespace, &alias.id.name),
                ),
            ));
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            out.aug_type_decls.push((
                scope.clone(),
                lower_named_interface_parts(
                    iface,
                    source,
                    qualified_name(namespace, &iface.id.name),
                ),
            ));
        }
        Declaration::TSModuleDeclaration(module) => {
            collect_augmentation_module_declaration(module, source, out, scope, Some(namespace));
        }
        Declaration::VariableDeclaration(var_decl) => {
            // A namespaced value member registers under its qualified `NS.M`
            // name into the augmentation VALUE scope (lowered exactly as the
            // file-scope namespaced-value path does).
            for declarator in &var_decl.declarations {
                if let Some(parts) =
                    lower_variable_parts(declarator, var_decl.kind, source, Some(namespace))
                {
                    out.aug_value_decls.push((scope.clone(), parts));
                }
            }
        }
        _ => {}
    }
}

/// Reuse the file-scope lowering to build the full retained value parts for a
/// value-space statement, then route them into the augmentation value scope.
fn collect_value_statement_into_augmentation(
    stmt: &Statement<'_>,
    source: &str,
    out: &mut LoweredStatementParts,
    scope: &AugmentationScopeKind,
) {
    let mut inner = LoweredStatementParts::default();
    match stmt {
        Statement::ClassDeclaration(decl) => collect_class(decl, source, &mut inner),
        Statement::FunctionDeclaration(func) => {
            if let Some(parts) = lower_function_parts(func, source) {
                inner.value_decls.push(parts);
            }
        }
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                if let Some(parts) = lower_variable_parts(decl, var_decl.kind, source, None) {
                    inner.value_decls.push(parts);
                }
            }
        }
        _ => {}
    }
    move_value_parts_into_augmentation(inner, out, scope);
}

/// Route the VALUE parts an inner collection produced into the augmentation
/// value scope (the type side a `class` also produces is intentionally
/// dropped — an ambient `declare module` class augments the value surface; its
/// instance type is not stitched cross-file today).
fn move_value_parts_into_augmentation(
    inner: LoweredStatementParts,
    out: &mut LoweredStatementParts,
    scope: &AugmentationScopeKind,
) {
    for parts in inner.value_decls {
        out.aug_value_decls.push((scope.clone(), parts));
    }
}

fn collect_namespaced_statement(
    stmt: &Statement<'_>,
    source: &str,
    out: &mut LoweredStatementParts,
    namespace: &str,
) {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => {
            out.type_decls.push(lower_named_type_alias_parts(
                alias,
                source,
                qualified_name(namespace, &alias.id.name),
            ));
        }
        Statement::TSInterfaceDeclaration(iface) => {
            out.type_decls.push(lower_named_interface_parts(
                iface,
                source,
                qualified_name(namespace, &iface.id.name),
            ));
        }
        Statement::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                collect_named_class(
                    class,
                    source,
                    out,
                    qualified_name(namespace, &identifier.name),
                );
            }
        }
        Statement::TSModuleDeclaration(module) => {
            collect_module_declaration(module, source, out, Some(namespace));
        }
        // Namespace value indexing is EXPORT-ONLY: a non-exported
        // `namespace N { const hidden = … }` is private to the namespace body
        // (TS: `N.hidden` does not exist on `typeof N`), so a DIRECT
        // `Statement::VariableDeclaration` is intentionally NOT indexed under
        // its qualified name. Only the exported path below
        // (`export const VERSION = …` → `collect_namespaced_declaration`)
        // registers a qualified value member such as `N.VERSION`.
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                collect_namespaced_declaration(decl, source, out, namespace);
            }
        }
        _ => {}
    }
}

fn collect_namespaced_declaration(
    decl: &Declaration<'_>,
    source: &str,
    out: &mut LoweredStatementParts,
    namespace: &str,
) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            out.type_decls.push(lower_named_type_alias_parts(
                alias,
                source,
                qualified_name(namespace, &alias.id.name),
            ));
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            out.type_decls.push(lower_named_interface_parts(
                iface,
                source,
                qualified_name(namespace, &iface.id.name),
            ));
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                collect_named_class(
                    class,
                    source,
                    out,
                    qualified_name(namespace, &identifier.name),
                );
            }
        }
        Declaration::TSModuleDeclaration(module) => {
            collect_module_declaration(module, source, out, Some(namespace));
        }
        // A namespaced value member (`namespace NS { export const M = … }`)
        // registers under its QUALIFIED name `NS.M` so `typeof NS.M` binds.
        Declaration::VariableDeclaration(var_decl) => {
            for declarator in &var_decl.declarations {
                if let Some(parts) =
                    lower_variable_parts(declarator, var_decl.kind, source, Some(namespace))
                {
                    out.value_decls.push(parts);
                }
            }
        }
        _ => {}
    }
}

fn qualified_module_name(prefix: Option<&str>, id: &TSModuleDeclarationName<'_>) -> Option<String> {
    match id {
        TSModuleDeclarationName::Identifier(id) => Some(match prefix {
            Some(prefix) => qualified_name(prefix, &id.name),
            None => id.name.to_string(),
        }),
        TSModuleDeclarationName::StringLiteral(_) => None,
    }
}

fn qualified_name(prefix: &str, name: &str) -> String {
    format!("{prefix}.{name}")
}

/// The declared name of a default-exported class/function declaration,
/// when it carries one (`export default class Props` → `Some("Props")`;
/// an anonymous `export default class {}` → `None`).
fn class_or_function_default_name(
    id: &Option<oxc_ast::ast::BindingIdentifier<'_>>,
) -> Option<String> {
    id.as_ref().map(|id| id.name.to_string())
}

/// Mirror a default-exported named type symbol (`export default class Props` /
/// `export default interface Foo`) under the `default` export name. The default
/// export route resolves to `(canonical, "default")`, so the prepared-decl
/// lookup must find the declaration body there as well as under its declared
/// name. The cloned [`TypeDeclInfo`] carries the SAME body slot / params (only
/// the `name` key changes to `default` — the body slot keeps the DECLARED
/// symbol anchor, which is where the authored body genuinely lives); it is a
/// no-op when the declared symbol was not registered (e.g. an empty class body
/// produced no type symbol).
fn alias_default_export_type_symbol(
    env: &mut EvalEnv,
    owner: TopLevelOwnerId,
    declared_name: &str,
) {
    if env.type_group_in(owner, "default").is_some() {
        return;
    }
    let Some(group) = env.type_group_in(owner, declared_name) else {
        return;
    };
    let decl = group.primary();
    let aliased = TypeDeclInfo {
        name: "default".to_string(),
        owner,
        declaration_id: 0,
        kind: decl.kind,
        type_parameters: decl.type_parameters.clone(),
        body: decl.body.clone(),
        direct_member_headers: decl.direct_member_headers.clone(),
        direct_member_return_inference: decl.direct_member_return_inference.clone(),
    };
    env.add_type(aliased);
}

/// Lower an OXC `TSAccessibility` token to the shared-IR [`MemberVisibility`].
/// `None` (no modifier) and `Some(Public)` map to [`MemberVisibility::Public`];
/// `Some(Protected)` / `Some(Private)` carry the declared accessibility. This
/// lowers the OXC token directly — it does NOT text-scan the source
/// (Typed-IR-Only).
fn visibility_from_ts_accessibility(acc: Option<TSAccessibility>) -> MemberVisibility {
    match acc {
        None | Some(TSAccessibility::Public) => MemberVisibility::Public,
        Some(TSAccessibility::Protected) => MemberVisibility::Protected,
        Some(TSAccessibility::Private) => MemberVisibility::Private,
    }
}

fn object_method_kind(kind: MethodDefinitionKind) -> verter_type_expr::ObjectMethodKind {
    match kind {
        MethodDefinitionKind::Get => verter_type_expr::ObjectMethodKind::Get,
        MethodDefinitionKind::Set => verter_type_expr::ObjectMethodKind::Set,
        MethodDefinitionKind::Method | MethodDefinitionKind::Constructor => {
            verter_type_expr::ObjectMethodKind::Method
        }
    }
}

fn collect_class(decl: &Class<'_>, source: &str, out: &mut LoweredStatementParts) {
    let name = match &decl.id {
        Some(id) => id.name.to_string(),
        None => return,
    };
    collect_named_class(decl, source, out, name);
}

fn collect_named_class(
    decl: &Class<'_>,
    source: &str,
    out: &mut LoweredStatementParts,
    name: String,
) {
    // Extract the public instance shape AND the value-side static surface
    // from the class body. Instance members go to the TYPE-space body;
    // static members ride INSIDE the value-side constructor-shape
    // `ObjectExpr` (the `typeof C` constructor-object model) next to the
    // `ConstructSignature` — never a separate field.
    let mut members = Vec::new();
    let mut static_members = Vec::new();
    let mut direct_member_return_inference = Vec::new();
    let mut static_member_return_inference = Vec::new();
    let mut ctor_sig = None;
    let mut ctor_fn_spans = FunctionSpans::default();
    let mut inference_unavailable = None;

    for element in &decl.body.body {
        match element {
            ClassElement::PropertyDefinition(prop) => {
                // Record every class field WITH its declared accessibility
                // (a `private` / `protected` member is RECORDED; the
                // published-prop projection re-applies a Public-only filter
                // at the publication boundary). `static` selects the surface:
                // instance body vs constructor shape. A `#private` key has no
                // public name (`property_key_name` → `None`) and never lands
                // on either surface.
                if let Some(prop_name) = property_key_name(&prop.key) {
                    let ty = prop
                        .type_annotation
                        .as_ref()
                        .map(|ta| lower_ts_type(&ta.type_annotation, source))
                        .or_else(|| {
                            prop.value.as_ref().map(|value| {
                                infer_declaration_or_unknown(
                                    value,
                                    source,
                                    prop.readonly,
                                    &mut inference_unavailable,
                                )
                            })
                        })
                        .unwrap_or(TypeExpr::Primitive(PrimitiveName::Unknown));
                    let spans = MemberSpans {
                        declaration: Some(prop.span.into()),
                        name: Some(prop.key.span().into()),
                        type_annotation: prop
                            .type_annotation
                            .as_ref()
                            .map(|ta| ta.type_annotation.span().into()),
                    };
                    let member =
                        ObjectMember::Property(verter_type_expr::ObjectProperty::with_visibility(
                            prop_name,
                            ty,
                            prop.optional,
                            prop.readonly,
                            visibility_from_ts_accessibility(prop.accessibility),
                            spans,
                        ));
                    if prop.r#static {
                        static_members.push(member);
                    } else {
                        members.push(member);
                    }
                }
            }
            ClassElement::MethodDefinition(method) => {
                if method.r#static {
                    // Static method → constructor-shape member with its
                    // declared accessibility (a static can never be the
                    // constructor — `static constructor` is invalid TS).
                    if let Some(method_name) = property_key_name(&method.key) {
                        let func = extract_function_signature(&method.value, source);
                        let return_inference = func.return_inference;
                        let Some(member_ordinal) = u32::try_from(static_members.len())
                            .ok()
                            .and_then(|ordinal| ordinal.checked_add(1))
                        else {
                            continue;
                        };
                        let fn_spans = FunctionSpans {
                            signature: Some(method.value.span.into()),
                            return_type: method
                                .value
                                .return_type
                                .as_ref()
                                .map(|rt| rt.type_annotation.span().into()),
                        };
                        let member_spans = MemberSpans {
                            declaration: Some(method.span.into()),
                            name: Some(method.key.span().into()),
                            type_annotation: None,
                        };
                        let mut signature = MethodSignature::with_visibility(
                            method_name,
                            FunctionExpr::with_spans(
                                func.parameters,
                                func.return_type.map(Arc::new),
                                func.type_parameters,
                                fn_spans,
                            ),
                            method.optional,
                            visibility_from_ts_accessibility(method.accessibility),
                            member_spans,
                        );
                        signature.method_kind = object_method_kind(method.kind);
                        signature.has_implementation_body = method.value.body.is_some();
                        static_member_return_inference.push(LoweredMemberReturnInference {
                            member_path: Arc::from(vec![member_ordinal]),
                            return_inference,
                        });
                        static_members.push(ObjectMember::Method(signature));
                    }
                } else if method.kind == MethodDefinitionKind::Constructor {
                    for parameter in &method.value.params.items {
                        if parameter.accessibility.is_none()
                            && !parameter.readonly
                            && !parameter.r#override
                        {
                            continue;
                        }
                        let BindingPattern::BindingIdentifier(identifier) = &parameter.pattern
                        else {
                            continue;
                        };
                        let ty = parameter
                            .type_annotation
                            .as_ref()
                            .map(|annotation| lower_ts_type(&annotation.type_annotation, source))
                            .or_else(|| {
                                parameter.initializer.as_ref().map(|initializer| {
                                    infer_declaration_or_unknown(
                                        initializer,
                                        source,
                                        false,
                                        &mut inference_unavailable,
                                    )
                                })
                            })
                            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Unknown));
                        members.push(ObjectMember::Property(
                            verter_type_expr::ObjectProperty::with_visibility(
                                identifier.name.to_string(),
                                ty,
                                parameter.optional,
                                parameter.readonly,
                                visibility_from_ts_accessibility(parameter.accessibility),
                                MemberSpans {
                                    declaration: Some(parameter.span.into()),
                                    name: Some(identifier.span.into()),
                                    type_annotation: parameter
                                        .type_annotation
                                        .as_ref()
                                        .map(|annotation| annotation.type_annotation.span().into()),
                                },
                            ),
                        ));
                    }
                    // The constructor is NOT an instance surface member; it
                    // feeds the VALUE-side `ConstructSignature` (for
                    // `typeof ClassName` / `InstanceType`). Its value-side
                    // extraction is unchanged by the visibility flip — a
                    // non-public constructor still does not contribute a
                    // call signature to the consuming surface.
                    if matches!(method.accessibility, None | Some(TSAccessibility::Public)) {
                        ctor_sig = Some(extract_function_signature(&method.value, source));
                        ctor_fn_spans = FunctionSpans {
                            signature: Some(method.span.into()),
                            return_type: method
                                .value
                                .return_type
                                .as_ref()
                                .map(|rt| rt.type_annotation.span().into()),
                        };
                    }
                } else if let Some(method_name) = property_key_name(&method.key) {
                    // Record every NON-static instance method with its
                    // declared accessibility (no longer an exclusion).
                    let func = extract_function_signature(&method.value, source);
                    let return_inference = func.return_inference;
                    let Some(member_ordinal) = u32::try_from(members.len()).ok() else {
                        continue;
                    };
                    let fn_spans = FunctionSpans {
                        signature: Some(method.value.span.into()),
                        return_type: method
                            .value
                            .return_type
                            .as_ref()
                            .map(|rt| rt.type_annotation.span().into()),
                    };
                    let member_spans = MemberSpans {
                        declaration: Some(method.span.into()),
                        name: Some(method.key.span().into()),
                        type_annotation: None,
                    };
                    let mut signature = MethodSignature::with_visibility(
                        method_name,
                        FunctionExpr::with_spans(
                            func.parameters,
                            func.return_type.map(Arc::new),
                            func.type_parameters,
                            fn_spans,
                        ),
                        method.optional,
                        visibility_from_ts_accessibility(method.accessibility),
                        member_spans,
                    );
                    signature.method_kind = object_method_kind(method.kind);
                    signature.has_implementation_body = method.value.body.is_some();
                    direct_member_return_inference.push(LoweredMemberReturnInference {
                        member_path: Arc::from(vec![member_ordinal]),
                        return_inference,
                    });
                    members.push(ObjectMember::Method(signature));
                }
            }
            // `ClassElement::AccessorProperty` (the `accessor` keyword)
            // deliberately does NOT fold — accessor lowering is out of the
            // producer's scope on both the constructor (static) and
            // instance surfaces. The future accessor-projection contract is
            // tracked by the ignored
            // `decorators_identity_accessor_decorator_publishes_public_property`
            // typeinfo test; landing it means projecting the accessor's
            // PUBLIC get/set surface, not folding the raw property shape.
            _ => {}
        }
    }

    let type_parameters = decl
        .type_parameters
        .as_ref()
        .map(|tp| lower_type_param_decls(tp, source))
        .unwrap_or_default();

    // Fold `extends BaseClass` heritage into the body as an
    // `Intersection`, mirroring `extract_named_interface`. A subclass
    // inherits the public instance shape of its base: `class Props extends
    // BaseProps { own }` exposes both `BaseProps`'s members and `own`. The
    // base is lowered as a `Ref` (resolved later through the shared
    // resolver), with its `super_type_arguments` lowered as generic args
    // (`class C extends Base<string>`). Without this fold the class body
    // carried only its own members and every body-driven surface reader
    // dropped the cross-file heritage. Folding it here gives every terminal
    // projection the same typed-IR declaration shape.
    let own_body = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }));
    let body = match decl.super_class.as_ref().and_then(heritage_expression_name) {
        Some(base_name) => {
            let base_args: Vec<TypeExpr> = decl
                .super_type_arguments
                .as_ref()
                .map(|tp| tp.params.iter().map(|p| lower_ts_type(p, source)).collect())
                .unwrap_or_default();
            let base_ref = if base_args.is_empty() {
                TypeExpr::named(base_name)
            } else {
                TypeExpr::named_with_args(base_name, base_args)
            };
            // Heritage base first, own body last — matches the interface
            // fold order (`parts.push(base); parts.push(body)`), so the
            // first-writer-wins member precedence in downstream surface
            // readers keeps own-body members shadowing inherited ones.
            TypeExpr::intersection(vec![base_ref, own_body])
        }
        _ => own_body,
    };

    out.type_decls.push(LoweredTypeDeclParts {
        name: name.clone(),
        kind: TypeDeclKind::Class,
        type_parameters,
        body,
        direct_member_return_inference,
    });

    // Also register as a value (for typeof ClassName / InstanceType)
    let ctor_declared = ctor_sig.is_some();
    let mut constructor_signature = ctor_sig.unwrap_or_else(|| LoweredSignatureParts {
        parameters: Vec::new(),
        return_type: Some(TypeExpr::named(name.clone())),
        return_inference: ReturnInferenceCompleteness::NotInferred,
        type_parameters: Vec::new(),
        has_implementation_body: true,
        has_authored_return: false,
        origin: LoweredSignatureOrigin::Synthetic,
    });
    // A DECLARED constructor carries no return annotation — its construct
    // "return" IS the class instance. Backfill the instance reference so
    // `InstanceType<typeof C>` reads the instance type from the construct
    // signature exactly as it does from the synthesized default. (The
    // backfilled reference is transient inference, never an authored return
    // position — `has_authored_return` stays false for constructors.)
    if constructor_signature.return_type.is_none() {
        constructor_signature.return_type = Some(TypeExpr::named(name.clone()));
    }
    // The declared constructor's authored function node is the construct
    // signature at shape ordinal 0 of the produced `typeof C` constructor
    // shape (a class with no declared constructor keeps the honest Synthetic
    // origin instead).
    if ctor_declared {
        constructor_signature.origin = LoweredSignatureOrigin::ShapeMember { ordinal: 0 };
    }
    // The constructor shape is the `typeof C` constructor-object model: the
    // construct signature first, then the class's OWN static members (with
    // their declared visibility). Base statics are NOT folded here — static
    // heritage composes at query time through the shared class-surface
    // reducer, never eagerly at the producer.
    let mut constructor_properties =
        vec![ObjectMember::ConstructSignature(FunctionExpr::with_spans(
            constructor_signature.parameters.clone(),
            constructor_signature.return_type.clone().map(Arc::new),
            constructor_signature.type_parameters.clone(),
            ctor_fn_spans,
        ))];
    constructor_properties.extend(static_members);
    let constructor_shape = ObjectExpr {
        properties: constructor_properties,
    };

    out.value_decls.push(LoweredValueDeclParts {
        name,
        kind: ValueDeclKind::Class,
        type_annotation: None,
        annotation_is_authored: false,
        inference_unavailable,
        signatures: vec![constructor_signature],
        object_shape: Some(constructor_shape),
        object_member_return_inference: static_member_return_inference,
        enum_members: None,
        enum_member_names: None,
    });
}

fn infer_declaration_or_unknown(
    expression: &Expression<'_>,
    source: &str,
    preserve_literal: bool,
    unavailable: &mut Option<InferenceUnavailableReason>,
) -> TypeExpr {
    match infer_declaration_expression_type(expression, source, preserve_literal) {
        Ok(inferred) => inferred,
        Err(reason) => {
            unavailable.get_or_insert(reason);
            TypeExpr::Primitive(PrimitiveName::Unknown)
        }
    }
}

// ---------------------------------------------------------------------------
// Value declarations
// ---------------------------------------------------------------------------

/// The narrowest SOUND primitive DOMAIN for a DEFERRED enum member, proven from
/// its initializer-expression KIND. This is a typed AST classification at the
/// lowering boundary — NOT a string heuristic and NOT a constant-fold: it never
/// evaluates the expression, only reads its shape to BOUND the runtime value's
/// type. An enum member is `number | string`-valued at runtime; this narrows to
/// the soundest provable arm so a deferred member is honestly typed, never
/// under-approximated to `never` and never widened past what the syntax proves:
/// - a bare member (the auto-increment series — always numeric) ⇒ `number`;
/// - a numeric-guaranteed expression (`1 << 2`, `~A`, `-x`, `a * b`) ⇒ `number`;
/// - a `+` expression (numeric add OR string concat) ⇒ `number | string`;
/// - a PLAIN string / template-literal expression (no tag) ⇒ `string`;
/// - a member-reference (`B = A`), call (`someFn()`), TAGGED template
///   (`` tag`...` `` — a call that can return ANY type, so `string` would
///   under-approximate), comparison/logical operator (boolean-valued), or any
///   other unclassifiable initializer ⇒ `unknown` — no narrower domain is
///   provable without constant-folding, which the literal-enum reducer
///   deliberately does not do.
fn degraded_member_domain(initializer: Option<&Expression<'_>>) -> EnumPrimitiveDomain {
    let Some(expr) = initializer else {
        // A bare member is only deferred when the running auto-increment value
        // is unknown; the auto-increment series is always NUMERIC.
        return EnumPrimitiveDomain::Number;
    };
    match expr {
        // A plain string or template literal (NO tag) is a string-valued
        // expression. A TAGGED template (`tag`...``) is deliberately EXCLUDED:
        // it is a call to `tag`, which can return any type, so `string` is not a
        // sound bound — it falls to the `_ => Unknown` arm below.
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => {
            EnumPrimitiveDomain::String
        }
        Expression::NumericLiteral(_) => EnumPrimitiveDomain::Number,
        Expression::UnaryExpression(unary) => match unary.operator {
            UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus | UnaryOperator::BitwiseNot => {
                EnumPrimitiveDomain::Number
            }
            // `!x` (boolean), `typeof`/`void`/`delete` — not a sound numeric or
            // string enum value; no narrower domain than `unknown` is provable.
            _ => EnumPrimitiveDomain::Unknown,
        },
        Expression::BinaryExpression(binary) => match binary.operator {
            BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight
            | BinaryOperator::ShiftRightZeroFill
            | BinaryOperator::BitwiseOR
            | BinaryOperator::BitwiseXOR
            | BinaryOperator::BitwiseAnd
            | BinaryOperator::Subtraction
            | BinaryOperator::Multiplication
            | BinaryOperator::Division
            | BinaryOperator::Remainder
            | BinaryOperator::Exponential => EnumPrimitiveDomain::Number,
            // `+` is numeric add OR string concat — the soundest bound is the
            // union of both.
            BinaryOperator::Addition => EnumPrimitiveDomain::NumberOrString,
            // Comparison / logical / `in` / `instanceof` produce booleans —
            // never a sound enum value.
            _ => EnumPrimitiveDomain::Unknown,
        },
        // A parenthesized wrapper carries no domain of its own — classify the
        // inner expression (`A = (1 << 2)` is still `number`).
        Expression::ParenthesizedExpression(paren) => {
            degraded_member_domain(Some(&paren.expression))
        }
        // Member-reference, call, identifier, anything else — unprovable here.
        _ => EnumPrimitiveDomain::Unknown,
    }
}

/// Register a TypeScript `enum` as the dual-space symbol it is: a VALUE
/// binding carrying the ordered member inventory (NAME → [`EnumMemberValue`];
/// drives `typeof Enum` — an object keyed by the member NAMES — and the
/// `Enum.Member` member projection) AND a TYPE binding for the enum used as
/// a type (e.g. a `${Enum}` template-literal expansion or an enum-member
/// discriminant). The type body — the projected-type union (folded literals
/// plus degraded primitive arms for deferred members) — is NOT computed here:
/// a per-declaration walk cannot see same-name merged
/// contributors, so the type binding gets a non-served placeholder body and
/// the single source of truth is [`ValueDeclGroup::enum_type_union`], which
/// derives the union from the MERGED value members on demand.
///
/// Member NAMES are resolved for EVERY member via the SAME `static_name` helper
/// the production `index_enum` header walk uses (all four `TSEnumMemberName`
/// variants — `Identifier`, `String`, `ComputedString`, `ComputedTemplateString`
/// — carry a static identity), so the eval-env member-NAME set always matches
/// the header walk. A computed string/template member name (`["A"]`, `` [`A`] ``)
/// is recorded, NOT dropped.
///
/// Member VALUES follow TypeScript's literal-enum rules: a string-literal
/// initializer is the member's value; a numeric-literal initializer (including
/// a leading unary `-` / `+` over one, e.g. `A = -1`) both IS the value and
/// reseeds the auto-increment counter; a bare member takes the next
/// auto-increment numeric (start 0, previous numeric + 1 — so `A = -1, B` ⇒
/// `B = 0`). The `const` modifier does not change the type-level value
/// (const-enum inlining is a runtime concern; the type-level projection equals
/// the assigned literal).
///
/// VALUE-DEFERRED (the member NAME is recorded with an
/// [`EnumMemberValue::Deferred`] value — never crashed, never given a wrong
/// literal): a member-REFERENCE initializer (`B = A`), a computed / expression
/// initializer (`B = 1 << 2`, `B = someFn()`, `~A`). Resolving those would
/// require constant-folding a member-reference graph, which the literal-enum
/// reducer deliberately does not model. A deferred member is NOT dropped — it
/// carries the narrowest SOUND primitive DOMAIN proven from its
/// initializer-expression kind (`degraded_member_domain`), so it stays honestly
/// typed on every projection surface (`typeof Enum`, `Enum.Member`, the enum
/// type union) while its DEGRADED value is projected out of the foldable rail
/// ([`ValueDeclGroup::merged_enum_members`]) that only the value-body
/// fingerprint observes.
///
/// A deferred member ALSO makes the running auto-increment value UNKNOWN:
/// because a bare member's value is `previous + 1`, once the previous value is
/// unknowable a following BARE member's value is DEFERRED too rather than
/// fabricated off a stale counter (its degraded domain is still `number` — the
/// auto-increment series is numeric). The next explicit foldable literal
/// RESEEDS the counter to KNOWN. (A string value likewise cannot seed a numeric
/// `+ 1`, so a bare member following a string member has a deferred value.)
/// Example: `enum E { A = 1 << 2, B, C = 5, D }` ⇒ NAMES `A`/`B`/`C`/`D` all
/// recorded; `A` (`1 << 2`) and `B` (bare after a deferred value) degrade to
/// `number`; `C = 5`, `D = 6` fold. Members are folded in SOURCE order; the
/// enum's full member set across same-name merged declarations is unioned by
/// the `merged_enum_*` accessors.
fn collect_enum(decl: &TSEnumDeclaration<'_>, out: &mut LoweredStatementParts) {
    let name = decl.id.name.to_string();

    // The ordered member inventory: the NAME of EVERY statically-named member
    // plus its [`EnumMemberValue`] — `Folded` (a literal [`EnumScalar`]) when
    // statically foldable, `Deferred` (carrying the degraded sound domain)
    // otherwise. See the `ValueDeclInfo::enum_members` field doc for the rail
    // contract (the NAME set is the presence-rail authority; the `Folded`
    // subset is the foldable rail; every member's projected scalar drives the
    // type surfaces). The NAME set must equal what `index_enum` records.
    let mut members: Vec<(String, EnumMemberValue)> = Vec::new();
    // The running auto-increment value, tracked as KNOWN (`Some`) / UNKNOWN
    // (`None`). A bare member's value is `previous + 1`, so the moment a
    // member's value cannot be statically folded (an unsupported initializer,
    // or a string value a numeric `+ 1` cannot follow) the running value
    // becomes UNKNOWN — and a subsequent BARE member with an unknown running
    // value has its VALUE DEFERRED, never fabricated. The next explicit foldable
    // numeric literal RESEEDS it to KNOWN.
    let mut next_auto: Option<f64> = Some(0.0);
    for member in &decl.body.members {
        // Member NAME resolution is SHARED with `index_enum`'s header walk
        // (`static_name` over all four `TSEnumMemberName` variants:
        // `Identifier`, `String`, `ComputedString`, `ComputedTemplateString`).
        // A computed string / template member name (`["A"]`, `` [`A`] ``)
        // carries a STATIC identity — it is recorded, NOT dropped — so the
        // eval-env member-NAME set matches the production header walk exactly
        // (name logic is shared, never forked, so the two paths cannot diverge).
        let member_name = member.id.static_name().to_string();
        // The VALUE is `Folded` when statically foldable, `Deferred` (degraded)
        // otherwise; the NAME above is recorded either way.
        let value: EnumMemberValue = match &member.initializer {
            // A string value cannot seed a numeric `+ 1`, so a bare member that
            // follows has a deferred value: record this value, mark UNKNOWN.
            Some(Expression::StringLiteral(s)) => {
                next_auto = None;
                EnumMemberValue::Folded(EnumScalar::String(s.value.to_string()))
            }
            Some(Expression::NumericLiteral(n)) => {
                next_auto = Some(n.value + 1.0);
                EnumMemberValue::Folded(EnumScalar::Number(format_enum_number(n.value)))
            }
            // TS represents a signed numeric initializer (`A = -1`, `A = +2`)
            // as a unary expression over a numeric literal. Fold it to the
            // signed literal and reseed the auto-increment counter from it.
            Some(Expression::UnaryExpression(unary)) => {
                match (unary.operator, &unary.argument) {
                    (UnaryOperator::UnaryNegation, Expression::NumericLiteral(n)) => {
                        next_auto = Some(-n.value + 1.0);
                        EnumMemberValue::Folded(EnumScalar::Number(format_enum_number(-n.value)))
                    }
                    (UnaryOperator::UnaryPlus, Expression::NumericLiteral(n)) => {
                        next_auto = Some(n.value + 1.0);
                        EnumMemberValue::Folded(EnumScalar::Number(format_enum_number(n.value)))
                    }
                    // A non-`+`/`-` unary (`~A`, `!x`) or a unary over a
                    // non-literal argument is a computed enum expression — out
                    // of the literal-enum scope. The member NAME stays recorded;
                    // its VALUE is DEFERRED (degraded from the initializer kind)
                    // and the running value becomes UNKNOWN so a following bare
                    // member is not fabricated off it.
                    _ => {
                        next_auto = None;
                        EnumMemberValue::Deferred(degraded_member_domain(
                            member.initializer.as_ref(),
                        ))
                    }
                }
            }
            None => match next_auto {
                // KNOWN running value: this bare member is `previous + 1`.
                Some(assigned) => {
                    next_auto = Some(assigned + 1.0);
                    EnumMemberValue::Folded(EnumScalar::Number(format_enum_number(assigned)))
                }
                // UNKNOWN running value (a preceding member was unfoldable): a
                // bare member's value depends on the previous member, which is
                // unknown — DEFER its VALUE, never fabricate. The NAME is still
                // recorded; its degraded domain is `number` (the auto-increment
                // series is numeric). It stays UNKNOWN until the next explicit
                // foldable literal reseeds the counter.
                None => EnumMemberValue::Deferred(degraded_member_domain(None)),
            },
            // A member-REFERENCE (`B = A`) or other computed / expression
            // initializer has no statically known literal value here — out of
            // the literal-enum scope. The member NAME stays recorded; its VALUE
            // is DEFERRED (degraded from the initializer kind) and the running
            // value becomes UNKNOWN so a following bare member is not fabricated
            // off it.
            Some(_) => {
                next_auto = None;
                EnumMemberValue::Deferred(degraded_member_domain(member.initializer.as_ref()))
            }
        };
        // Members are unique within a single enum body (TS forbids a repeated
        // member name); dedup defensively so a malformed repeat does not
        // double-count, keeping the first occurrence's entry.
        if !members.iter().any(|(existing, _)| existing == &member_name) {
            members.push((member_name, value));
        }
    }

    // Value-space: the enum binding carries the ordered member inventory —
    // each member NAME with an `EnumMemberValue` (a folded value literal, or a
    // degraded sound primitive for a value that is not statically foldable).
    // The member-NAME fact is minted from the SAME walk (one derivation
    // point), so the presence rail and the value inventory cannot diverge.
    let enum_member_names = EnumMemberNamesFact {
        names: members
            .iter()
            .map(|(member_name, _)| member_name.clone())
            .collect(),
    };
    out.value_decls.push(LoweredValueDeclParts {
        name: name.clone(),
        kind: ValueDeclKind::Enum,
        type_annotation: None,
        annotation_is_authored: false,
        inference_unavailable: None,
        signatures: Vec::new(),
        object_shape: None,
        object_member_return_inference: Vec::new(),
        enum_members: Some(members),
        enum_member_names: Some(enum_member_names),
    });

    // Type-space: the enum used AS A TYPE is the union of its members'
    // projected scalars (folded literals plus degraded primitive arms for
    // unfoldable members) — but that union is DERIVED from the MERGED value
    // members by `ValueDeclGroup::enum_type_union` (the single source of
    // truth), because a per-declaration walk here cannot see same-name merged
    // contributors (an eager union would be last-wins and drop earlier
    // declarations' members). So this registers only the dual-space TYPE
    // binding (kind `Alias` — there is no dedicated enum `TypeDeclKind`, and a
    // union carries no nominal identity Verter models) whose body slot
    // addresses the enum declaration itself; the demand-driven body service
    // serves the derived union. The transient `never` body below exists only
    // to derive the (empty) member-header inventory — the enum TYPE is a
    // member union, never an object surface, so it has no direct member
    // headers (member NAMES live on the value decl's `enum_member_names`
    // fact).
    out.type_decls.push(LoweredTypeDeclParts {
        name,
        kind: TypeDeclKind::Alias,
        type_parameters: Vec::new(),
        body: TypeExpr::Primitive(PrimitiveName::Never),
        direct_member_return_inference: Vec::new(),
    });
}

fn lower_function_parts(func: &Function<'_>, source: &str) -> Option<LoweredValueDeclParts> {
    let (name, name_offset) = match &func.id {
        Some(id) => (id.name.to_string(), id.span.start),
        None => return None,
    };

    let mut sig = extract_function_signature(func, source);
    // A JSDoc-documented function's `@param {T} name` / `@returns {T}` tags ARE
    // the parameter / return type annotations when the TS annotation is absent
    // (JSDoc-typed JS is first-class). Backfill them through the SAME lowering a
    // TS annotation uses so the function type resolves through the shared
    // dispatch with no JSDoc-specific path. TS annotations always win (we only
    // touch params/return that lacked one).
    enrich_function_signature_with_jsdoc(&mut sig, source, name_offset, func.return_type.is_some());
    let kind = if func.r#async {
        ValueDeclKind::AsyncFunction
    } else {
        ValueDeclKind::Function
    };

    Some(LoweredValueDeclParts {
        name,
        kind,
        type_annotation: None,
        annotation_is_authored: false,
        inference_unavailable: None,
        signatures: vec![sig],
        object_shape: None,
        object_member_return_inference: Vec::new(),
        enum_members: None,
        enum_member_names: None,
    })
}

/// Backfill a function signature's parameter / return types from a leading
/// JSDoc block, for the parameters / return that carried NO TS annotation.
///
/// `has_ts_return` records whether the function had an explicit TS return
/// annotation; when it did, the JSDoc `@returns` is ignored (the TS annotation
/// is authoritative). Each backfilled type is the lowered `{T}` payload from
/// [`crate::analysis::jsdoc`], stored on the same `FunctionParam.ty` /
/// `FunctionSignature.return_type` carrier a TS annotation would populate.
fn enrich_function_signature_with_jsdoc(
    sig: &mut LoweredSignatureParts,
    source: &str,
    name_offset: u32,
    has_ts_return: bool,
) {
    if enrich_params_and_return_with_jsdoc(
        &mut sig.parameters,
        &mut sig.return_type,
        source,
        name_offset,
        has_ts_return,
    ) {
        // An explicit JSDoc return is authored intent, not body inference.
        sig.return_inference = ReturnInferenceCompleteness::NotInferred;
    }
}

/// Backfill a parameter list + return type from a leading JSDoc block, for the
/// parameters / return that carried NO TS annotation. The shared core both
/// [`LoweredSignatureParts`] (function declarations / initializer signatures)
/// and an inferred [`FunctionExpr`] `type_annotation` (an arrow / function-
/// expression value's inferred type) enrich through.
///
/// `has_ts_return` records whether the function had an explicit TS return
/// annotation; when it did, the JSDoc `@returns` is ignored (the TS annotation
/// is authoritative). Each backfilled type is the lowered `{T}` payload from
/// [`crate::analysis::jsdoc`].
fn enrich_params_and_return_with_jsdoc(
    parameters: &mut [FunctionParam],
    return_type: &mut Option<TypeExpr>,
    source: &str,
    name_offset: u32,
    has_ts_return: bool,
) -> bool {
    let param_types =
        crate::analysis::jsdoc::extract_jsdoc_param_types_at_offset(source, name_offset);
    if !param_types.is_empty() {
        for param in parameters.iter_mut() {
            // Only fill a parameter that carried NO explicit TS annotation at its
            // declaration site. `has_ts_annotation` is the OXC structural fact
            // captured by `lower_function_params`; it is the correct authority
            // here because an explicit `: any` lowers to `Primitive(Any)` exactly
            // like a missing annotation — testing the lowered `ty` would wrongly
            // overwrite an explicit `: any` (TS never lets JSDoc override an
            // explicit annotation).
            if param.has_ts_annotation {
                continue;
            }
            let Some(param_name) = param.name.as_deref() else {
                continue;
            };
            if let Some((_, jsdoc_ty)) = param_types.iter().find(|(n, _)| n == param_name) {
                param.ty = jsdoc_ty.clone();
            }
        }
    }

    // A TS return annotation is authoritative; only consult `@returns` when the
    // function declared no TS return type. The signature may have body-inferred
    // a return type, but an explicit JSDoc `@returns` is a stated annotation and
    // takes priority over body inference.
    if !has_ts_return {
        if let Some(jsdoc_return) =
            crate::analysis::jsdoc::extract_jsdoc_return_type_at_offset(source, name_offset)
        {
            *return_type = Some(jsdoc_return);
            return true;
        }
    }
    false
}

/// Enrich an inferred [`FunctionExpr`] `type_annotation` (built by
/// `infer_expression_type` from a function-expression initializer) with the
/// declaration's JSDoc `@param`/`@returns`, bridging the `Arc<TypeExpr>` return
/// carrier to the shared [`enrich_params_and_return_with_jsdoc`] core.
fn enrich_function_expr_with_jsdoc(
    function: &mut Arc<FunctionExpr>,
    source: &str,
    name_offset: u32,
    has_ts_return: bool,
) {
    let function = Arc::make_mut(function);
    let mut return_type = function.return_type.as_ref().map(|rt| (**rt).clone());
    let _ = enrich_params_and_return_with_jsdoc(
        &mut function.parameters,
        &mut return_type,
        source,
        name_offset,
        has_ts_return,
    );
    function.return_type = return_type.map(Arc::new);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SvelteRuneInitializer {
    State { allows_omitted_initial: bool },
    Derived,
    DerivedBy,
}

fn apply_svelte_rune_initializer_inference(
    stmt: &Statement<'_>,
    source: &str,
    lowered: &mut [LoweredValueDeclParts],
) {
    let variable = match stmt {
        Statement::VariableDeclaration(variable) => Some(variable.as_ref()),
        Statement::ExportNamedDeclaration(export) => {
            export
                .declaration
                .as_ref()
                .and_then(|declaration| match declaration {
                    Declaration::VariableDeclaration(variable) => Some(variable.as_ref()),
                    _ => None,
                })
        }
        _ => None,
    };
    let Some(variable) = variable else {
        return;
    };

    for declarator in &variable.declarations {
        let BindingPattern::BindingIdentifier(identifier) = &declarator.id else {
            continue;
        };
        let Some(initializer) = declarator.init.as_ref() else {
            continue;
        };
        let Some(parts) = lowered
            .iter_mut()
            .find(|parts| parts.name == identifier.name.as_str())
        else {
            continue;
        };
        let inferred = match infer_svelte_rune_initializer(initializer, source) {
            Ok(Some(inferred)) => inferred,
            Ok(None) => continue,
            Err(reason) => {
                if !parts.annotation_is_authored {
                    parts.type_annotation = None;
                    parts.inference_unavailable = Some(reason);
                }
                continue;
            }
        };
        let inferred = if matches!(
            variable.kind,
            VariableDeclarationKind::Let | VariableDeclarationKind::Var
        ) {
            match widen_literal_type(inferred) {
                Ok(inferred) => inferred,
                Err(reason) => {
                    if !parts.annotation_is_authored {
                        parts.type_annotation = None;
                        parts.inference_unavailable = Some(reason);
                    }
                    continue;
                }
            }
        } else {
            inferred
        };
        if !parts.annotation_is_authored {
            parts.type_annotation = Some(inferred);
            parts.inference_unavailable = None;
        }
    }
}

fn infer_svelte_rune_initializer(
    expr: &Expression<'_>,
    source: &str,
) -> InferenceResult<Option<TypeExpr>> {
    let mut budget = InferenceBudget::default();
    infer_svelte_rune_initializer_with_budget(expr, source, &mut budget, 0)
}

fn infer_svelte_rune_initializer_with_budget(
    expr: &Expression<'_>,
    source: &str,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<Option<TypeExpr>> {
    budget.visit(depth)?;
    let expr = unwrap_expression_wrappers_with_budget(expr, budget, depth + 1)?;
    let Expression::CallExpression(call) = expr else {
        return Ok(None);
    };
    let Some(kind) = classify_svelte_rune_initializer(&call.callee, budget, depth + 1)? else {
        return Ok(None);
    };

    let explicit = call
        .type_arguments
        .as_ref()
        .and_then(|arguments| arguments.params.first())
        .map(|argument| lower_ts_type(argument, source));
    if let Some(explicit) = explicit {
        return Ok(match kind {
            SvelteRuneInitializer::State {
                allows_omitted_initial: true,
            } if call.arguments.is_empty() => Some(TypeExpr::union(vec![
                explicit,
                TypeExpr::Primitive(PrimitiveName::Undefined),
            ])),
            _ => Some(explicit),
        });
    }

    let first_argument = call
        .arguments
        .first()
        .and_then(|argument| argument.as_expression());
    match kind {
        SvelteRuneInitializer::State {
            allows_omitted_initial: true,
        } if first_argument.is_none() => Ok(Some(TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::Unknown),
            TypeExpr::Primitive(PrimitiveName::Undefined),
        ]))),
        SvelteRuneInitializer::State { .. } | SvelteRuneInitializer::Derived => first_argument
            .map(|argument| {
                infer_expression_type_ctx(
                    argument,
                    source,
                    MemberLiteralPolicy::Widen,
                    budget,
                    depth + 1,
                )
            })
            .transpose(),
        SvelteRuneInitializer::DerivedBy => {
            let Some(callback) = first_argument else {
                return Ok(None);
            };
            let callback_type = infer_expression_type_ctx(
                callback,
                source,
                MemberLiteralPolicy::Widen,
                budget,
                depth + 1,
            )?;
            if let TypeExpr::Function(function) = &callback_type {
                Ok(function.return_type.as_deref().cloned())
            } else if !matches!(callback_type, TypeExpr::Primitive(PrimitiveName::Any)) {
                Ok(Some(TypeExpr::Ref {
                    name: Arc::from("ReturnType"),
                    type_arguments: Arc::from(vec![callback_type]),
                }))
            } else {
                Ok(None)
            }
        }
    }
}

fn classify_svelte_rune_initializer(
    callee: &Expression<'_>,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<Option<SvelteRuneInitializer>> {
    match unwrap_expression_wrappers_with_budget(callee, budget, depth)? {
        Expression::Identifier(identifier) => Ok(match identifier.name.as_str() {
            "$state" => Some(SvelteRuneInitializer::State {
                allows_omitted_initial: true,
            }),
            "$derived" => Some(SvelteRuneInitializer::Derived),
            _ => None,
        }),
        Expression::StaticMemberExpression(member) => {
            let Expression::Identifier(root) =
                unwrap_expression_wrappers_with_budget(&member.object, budget, depth + 1)?
            else {
                return Ok(None);
            };
            Ok(match (root.name.as_str(), member.property.name.as_str()) {
                ("$state", "raw") => Some(SvelteRuneInitializer::State {
                    allows_omitted_initial: true,
                }),
                ("$state", "snapshot" | "eager") => Some(SvelteRuneInitializer::State {
                    allows_omitted_initial: false,
                }),
                ("$derived", "by") => Some(SvelteRuneInitializer::DerivedBy),
                _ => None,
            })
        }
        _ => Ok(None),
    }
}

fn unwrap_expression_wrappers_with_budget<'a>(
    mut expr: &'a Expression<'a>,
    budget: &mut InferenceBudget,
    mut depth: usize,
) -> InferenceResult<&'a Expression<'a>> {
    loop {
        budget.visit(depth)?;
        expr = match expr {
            Expression::ParenthesizedExpression(parenthesized) => &parenthesized.expression,
            Expression::TSAsExpression(assertion) => &assertion.expression,
            Expression::TSSatisfiesExpression(satisfies) => &satisfies.expression,
            Expression::TSNonNullExpression(non_null) => &non_null.expression,
            _ => return Ok(expr),
        };
        depth += 1;
    }
}

fn unwrap_expression_wrappers<'a>(mut expr: &'a Expression<'a>) -> &'a Expression<'a> {
    loop {
        expr = match expr {
            Expression::ParenthesizedExpression(parenthesized) => &parenthesized.expression,
            Expression::TSAsExpression(assertion) => &assertion.expression,
            Expression::TSSatisfiesExpression(satisfies) => &satisfies.expression,
            Expression::TSNonNullExpression(non_null) => &non_null.expression,
            _ => return expr,
        };
    }
}

fn lower_variable_parts(
    decl: &VariableDeclarator<'_>,
    kind: VariableDeclarationKind,
    source: &str,
    namespace: Option<&str>,
) -> Option<LoweredValueDeclParts> {
    let (name, name_offset) = match &decl.id {
        // A namespaced value member is added under its QUALIFIED name
        // (`NS.M`), mirroring the qualified TYPE member registration, so
        // `typeof NS.M` binds the value root. The JSDoc `@type` offset stays
        // the real declaration-site offset (used for source lookups).
        BindingPattern::BindingIdentifier(id) => {
            let name = match namespace {
                Some(ns) => qualified_name(ns, &id.name),
                None => id.name.to_string(),
            };
            (name, id.span.start)
        }
        _ => return None,
    };

    let var_kind = match kind {
        VariableDeclarationKind::Const
        | VariableDeclarationKind::Using
        | VariableDeclarationKind::AwaitUsing => ValueDeclKind::Const,
        VariableDeclarationKind::Let => ValueDeclKind::Let,
        VariableDeclarationKind::Var => ValueDeclKind::Var,
    };

    // Extract type annotation from the variable declarator
    let mut type_annotation = decl
        .type_annotation
        .as_ref()
        .map(|ta| lower_ts_type(&ta.type_annotation, source));

    // No TS annotation → a leading JSDoc `@type {T}` IS the explicit type
    // annotation (TS treats `/** @type {Foo} */ const x = ...` exactly like
    // `const x: Foo`). Lower it through the JSDoc-private OXC bridge into the
    // SAME `type_annotation` carrier a TS annotation populates, so it resolves
    // through the shared dispatch with no JSDoc-specific resolution path. The
    // JSDoc `@type` takes priority over initializer inference below, matching
    // TS's explicit-annotation precedence.
    if type_annotation.is_none() {
        type_annotation = crate::analysis::jsdoc::extract_jsdoc_type_at_offset(source, name_offset);
    }
    // Both the TS annotation and the JSDoc `@type` payload are AUTHORED
    // annotations; anything filled by initializer inference below is not.
    let annotation_is_authored = type_annotation.is_some();

    // Extract function signature from arrow functions or function expressions
    let mut function_signature = None;
    let mut object_shape = None;
    let mut inference_unavailable = None;

    if let Some(ref init) = decl.init {
        function_signature = extract_initializer_function_signature(init, source);
        match extract_initializer_object_shape(init, source, MemberLiteralPolicy::Widen) {
            Ok(shape) => object_shape = shape,
            Err(reason) => inference_unavailable = Some(reason),
        }

        // An arrow / function-expression VALUE documents its parameter / return
        // types the same way a `function` declaration does: a leading JSDoc
        // `@param {T} name` / `@returns {T}` IS the annotation when no TS
        // annotation is present (JSDoc-typed JS is first-class). Enrich the
        // initializer signature through the SAME lowering a TS annotation uses,
        // preserving TS precedence — a parameter that carried a TS annotation
        // keeps it, and a TS return annotation on the initializer suppresses
        // `@returns`.
        let has_ts_return = initializer_has_ts_return_annotation(init);
        if let Some(sig) = function_signature.as_mut() {
            enrich_function_signature_with_jsdoc(sig, source, name_offset, has_ts_return);
        }

        if type_annotation.is_none() {
            let inferred = infer_expression_type(init, source).and_then(|inferred| {
                if matches!(var_kind, ValueDeclKind::Let | ValueDeclKind::Var) {
                    widen_literal_type(inferred)
                } else {
                    Ok(inferred)
                }
            });
            let mut inferred = match inferred {
                Ok(inferred) => inferred,
                Err(reason) => {
                    inference_unavailable = Some(reason);
                    TypeExpr::Primitive(PrimitiveName::Any)
                }
            };
            // The inferred `type_annotation` is the carrier query-time projection
            // consumes first (it precedes `function_signature`). When inference
            // produced a function type from a function-expression initializer,
            // enrich THAT function's params/return from the same JSDoc so the
            // projected signature is JSDoc-typed (not the un-enriched inference).
            if let TypeExpr::Function(function) = &mut inferred {
                enrich_function_expr_with_jsdoc(function, source, name_offset, has_ts_return);
            }
            if inference_unavailable.is_none()
                && !matches!(inferred, TypeExpr::Primitive(PrimitiveName::Any))
            {
                type_annotation = Some(inferred);
            }
        }
    }

    Some(LoweredValueDeclParts {
        name,
        kind: var_kind,
        type_annotation,
        annotation_is_authored,
        inference_unavailable: if annotation_is_authored {
            None
        } else {
            inference_unavailable
        },
        signatures: function_signature.into_iter().collect(),
        object_shape,
        object_member_return_inference: Vec::new(),
        enum_members: None,
        enum_member_names: None,
    })
}

fn lower_default_expression_parts(expr: &Expression<'_>, source: &str) -> LoweredValueDeclParts {
    let function_signature = extract_initializer_function_signature(expr, source);
    let mut inference_unavailable = None;
    let object_shape =
        match extract_initializer_object_shape(expr, source, MemberLiteralPolicy::Widen) {
            Ok(shape) => shape,
            Err(reason) => {
                inference_unavailable = Some(reason);
                None
            }
        };
    let type_annotation = match lower_value_expression(expr, source) {
        Ok(annotation) => Some(annotation),
        Err(reason) => {
            inference_unavailable = Some(reason);
            None
        }
    };

    LoweredValueDeclParts {
        name: "default".to_string(),
        kind: ValueDeclKind::Const,
        type_annotation,
        // The default-export expression's type is INFERRED from the exported
        // value (there is no authored annotation position on an
        // `export default <expr>`).
        annotation_is_authored: false,
        inference_unavailable,
        signatures: function_signature.into_iter().collect(),
        object_shape,
        object_member_return_inference: Vec::new(),
        enum_members: None,
        enum_member_names: None,
    }
}

fn extract_initializer_function_signature(
    expr: &Expression<'_>,
    source: &str,
) -> Option<LoweredSignatureParts> {
    match unwrap_expression_wrappers(expr) {
        Expression::ArrowFunctionExpression(arrow) => Some(extract_arrow_signature(arrow, source)),
        Expression::FunctionExpression(func) => Some(extract_function_signature(func, source)),
        _ => None,
    }
}

/// Whether an arrow / function-expression initializer carries an explicit TS
/// return annotation (`(x) => T` / `function (): T`). Mirrors the unwrap chain
/// of [`extract_initializer_function_signature`] so a wrapped initializer
/// (`as` / `satisfies` / parenthesized) is seen through. Used to suppress a
/// JSDoc `@returns` when the value already states a TS return type.
fn initializer_has_ts_return_annotation(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::ArrowFunctionExpression(arrow) => arrow.return_type.is_some(),
        Expression::FunctionExpression(func) => func.return_type.is_some(),
        Expression::TSAsExpression(ts_as) => {
            initializer_has_ts_return_annotation(&ts_as.expression)
        }
        Expression::TSSatisfiesExpression(sat) => {
            initializer_has_ts_return_annotation(&sat.expression)
        }
        Expression::ParenthesizedExpression(paren) => {
            initializer_has_ts_return_annotation(&paren.expression)
        }
        _ => false,
    }
}

fn extract_initializer_object_shape(
    expr: &Expression<'_>,
    source: &str,
    policy: MemberLiteralPolicy,
) -> InferenceResult<Option<ObjectExpr>> {
    let mut budget = InferenceBudget::default();
    extract_initializer_object_shape_with_budget(expr, source, policy, &mut budget, 0)
}

fn extract_initializer_object_shape_with_budget(
    expr: &Expression<'_>,
    source: &str,
    policy: MemberLiteralPolicy,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<Option<ObjectExpr>> {
    budget.visit(depth)?;
    match expr {
        Expression::ObjectExpression(obj) => {
            extract_object_literal(obj, source, policy, budget, depth + 1).map(Some)
        }
        Expression::TSAsExpression(ts_as) => {
            // `… as const` establishes a const context for the underlying
            // object shape (properties keep literals + become `readonly`).
            let inner_policy =
                if is_const_assertion_type_expr(&lower_ts_type(&ts_as.type_annotation, source)) {
                    MemberLiteralPolicy::ConstAssert
                } else {
                    policy
                };
            extract_initializer_object_shape_with_budget(
                &ts_as.expression,
                source,
                inner_policy,
                budget,
                depth + 1,
            )
        }
        Expression::TSSatisfiesExpression(sat) => {
            // `satisfies` preserves members without widening, unless an
            // enclosing `as const` already pinned the readonly context.
            let inner_policy = if policy == MemberLiteralPolicy::ConstAssert {
                MemberLiteralPolicy::ConstAssert
            } else {
                MemberLiteralPolicy::Preserve
            };
            extract_initializer_object_shape_with_budget(
                &sat.expression,
                source,
                inner_policy,
                budget,
                depth + 1,
            )
        }
        Expression::ParenthesizedExpression(paren) => extract_initializer_object_shape_with_budget(
            &paren.expression,
            source,
            policy,
            budget,
            depth + 1,
        ),
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn extract_function_signature(func: &Function<'_>, source: &str) -> LoweredSignatureParts {
    let mut budget = InferenceBudget::default();
    match extract_function_signature_with_budget(func, source, &mut budget, 0) {
        Ok(signature) => signature,
        Err(reason) => unavailable_function_signature(
            &func.params,
            source,
            func.return_type.is_some(),
            func.body.is_some(),
            func.type_parameters
                .as_ref()
                .map(|parameters| lower_type_param_decls(parameters, source))
                .unwrap_or_default(),
            reason,
        ),
    }
}

fn extract_function_signature_with_budget(
    func: &Function<'_>,
    source: &str,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<LoweredSignatureParts> {
    budget.visit(depth)?;
    let has_authored_return = func.return_type.is_some();
    let parameters = lower_function_params_with_budget(&func.params, source, budget, depth + 1)?;
    let inferred_return = if let Some(return_type) = &func.return_type {
        InferredReturn {
            return_type: Some(lower_ts_type(&return_type.type_annotation, source)),
            completeness: ReturnInferenceCompleteness::NotInferred,
        }
    } else if let Some(body) = &func.body {
        infer_return_type_with_budget(body, source, &parameters, budget, depth + 1)
    } else {
        InferredReturn {
            return_type: None,
            completeness: ReturnInferenceCompleteness::NotInferred,
        }
    };
    let type_parameters = func
        .type_parameters
        .as_ref()
        .map(|tp| lower_type_param_decls(tp, source))
        .unwrap_or_default();

    Ok(LoweredSignatureParts {
        parameters,
        return_type: inferred_return.return_type,
        return_inference: inferred_return.completeness,
        type_parameters,
        has_implementation_body: func.body.is_some(),
        has_authored_return,
        origin: LoweredSignatureOrigin::DeclBody,
    })
}

fn extract_arrow_signature(
    arrow: &ArrowFunctionExpression<'_>,
    source: &str,
) -> LoweredSignatureParts {
    let mut budget = InferenceBudget::default();
    match extract_arrow_signature_with_budget(arrow, source, &mut budget, 0) {
        Ok(signature) => signature,
        Err(reason) => unavailable_function_signature(
            &arrow.params,
            source,
            arrow.return_type.is_some(),
            true,
            arrow
                .type_parameters
                .as_ref()
                .map(|parameters| lower_type_param_decls(parameters, source))
                .unwrap_or_default(),
            reason,
        ),
    }
}

fn extract_arrow_signature_with_budget(
    arrow: &ArrowFunctionExpression<'_>,
    source: &str,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<LoweredSignatureParts> {
    budget.visit(depth)?;
    let has_authored_return = arrow.return_type.is_some();
    let parameters = lower_function_params_with_budget(&arrow.params, source, budget, depth + 1)?;
    let inferred_return = if let Some(return_type) = &arrow.return_type {
        InferredReturn {
            return_type: Some(lower_ts_type(&return_type.type_annotation, source)),
            completeness: ReturnInferenceCompleteness::NotInferred,
        }
    } else if arrow.expression {
        let return_type = arrow
            .body
            .statements
            .first()
            .and_then(|statement| match statement {
                Statement::ExpressionStatement(expression) => Some(infer_expression_type_ctx(
                    &expression.expression,
                    source,
                    MemberLiteralPolicy::Widen,
                    budget,
                    depth + 1,
                )),
                _ => None,
            })
            .transpose()?;
        InferredReturn {
            return_type,
            completeness: ReturnInferenceCompleteness::Complete {
                can_fall_through: false,
            },
        }
    } else {
        infer_return_type_with_budget(&arrow.body, source, &parameters, budget, depth + 1)
    };
    let type_parameters = arrow
        .type_parameters
        .as_ref()
        .map(|tp| lower_type_param_decls(tp, source))
        .unwrap_or_default();

    // An arrow function always carries an implementation body (expression or
    // block form).
    Ok(LoweredSignatureParts {
        parameters,
        return_type: inferred_return.return_type,
        return_inference: inferred_return.completeness,
        type_parameters,
        has_implementation_body: true,
        has_authored_return,
        origin: LoweredSignatureOrigin::DeclBody,
    })
}

fn unavailable_function_signature(
    params: &FormalParameters<'_>,
    source: &str,
    has_authored_return: bool,
    has_implementation_body: bool,
    type_parameters: Vec<TypeParam>,
    reason: InferenceUnavailableReason,
) -> LoweredSignatureParts {
    LoweredSignatureParts {
        parameters: lower_function_params_without_initializer_inference(params, source),
        return_type: None,
        return_inference: ReturnInferenceCompleteness::Unavailable(reason),
        type_parameters,
        has_implementation_body,
        has_authored_return,
        origin: LoweredSignatureOrigin::DeclBody,
    }
}

fn extract_object_literal(
    obj: &ObjectExpression<'_>,
    source: &str,
    policy: MemberLiteralPolicy,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<ObjectExpr> {
    budget.visit(depth)?;
    let mut members = Vec::new();
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                if let Some(name) = property_key_name(&p.key) {
                    let (ty, readonly) =
                        object_member_value(&p.value, source, policy, budget, depth + 1)?;
                    let spans = MemberSpans {
                        declaration: Some(p.span.into()),
                        name: Some(p.key.span().into()),
                        // Value-inferred property: there is no source type
                        // annotation to anchor.
                        type_annotation: None,
                    };
                    push_object_property_with_override(
                        &mut members,
                        verter_type_expr::ObjectProperty::with_spans_public(
                            name, ty, false, readonly, spans,
                        ),
                    );
                }
            }
            ObjectPropertyKind::SpreadProperty(_) => {
                // This function returns ObjectExpr only — can't represent intersections.
                // Use extract_object_literal_as_type() for spread-aware inference.
            }
        }
    }
    Ok(ObjectExpr {
        properties: members,
    })
}

/// Like `extract_object_literal`, but returns a `TypeExpr` directly so it can
/// represent intersections when the object contains spread of non-literal sources.
///
/// `policy` carries the enclosing object-literal context (see
/// [`MemberLiteralPolicy`]): a property widens / preserves / preserves+readonly
/// per the policy, with a per-property `as const` overriding to `ConstAssert`.
fn extract_object_literal_as_type(
    obj: &ObjectExpression<'_>,
    source: &str,
    policy: MemberLiteralPolicy,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<TypeExpr> {
    budget.visit(depth)?;
    let mut members = Vec::new();
    let mut spread_types: Vec<TypeExpr> = Vec::new();
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                if let Some(name) = property_key_name(&p.key) {
                    let (ty, readonly) =
                        object_member_value(&p.value, source, policy, budget, depth + 1)?;
                    let spans = MemberSpans {
                        declaration: Some(p.span.into()),
                        name: Some(p.key.span().into()),
                        // Value-inferred property: there is no source type
                        // annotation to anchor.
                        type_annotation: None,
                    };
                    push_object_property_with_override(
                        &mut members,
                        verter_type_expr::ObjectProperty::with_spans_public(
                            name, ty, false, readonly, spans,
                        ),
                    );
                }
            }
            ObjectPropertyKind::SpreadProperty(spread) => {
                let spread_ty =
                    infer_expression_type_ctx(&spread.argument, source, policy, budget, depth + 1)?;
                match spread_ty {
                    TypeExpr::Object(ref obj_expr) => {
                        for member in &obj_expr.properties {
                            push_object_member_with_override(&mut members, member.clone());
                        }
                    }
                    ty if !matches!(ty, TypeExpr::Primitive(PrimitiveName::Any)) => {
                        spread_types.push(ty);
                    }
                    _ => {}
                }
            }
        }
    }

    let own_obj = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }));

    if spread_types.is_empty() {
        Ok(own_obj)
    } else if matches!(&own_obj, TypeExpr::Object(obj) if obj.properties.is_empty()) {
        Ok(TypeExpr::intersection(spread_types))
    } else {
        spread_types.push(own_obj);
        Ok(TypeExpr::Intersection(spread_types.into()))
    }
}

fn push_object_property_with_override(
    members: &mut Vec<ObjectMember>,
    property: verter_type_expr::ObjectProperty,
) {
    if let Some(existing_index) = members.iter().position(|member| match member {
        ObjectMember::Property(existing) => existing.name == property.name,
        _ => false,
    }) {
        members.remove(existing_index);
    }
    members.push(ObjectMember::Property(property));
}

fn push_object_member_with_override(members: &mut Vec<ObjectMember>, member: ObjectMember) {
    match member {
        ObjectMember::Property(property) => push_object_property_with_override(members, property),
        other => members.push(other),
    }
}

#[derive(Debug)]
struct InferredReturn {
    return_type: Option<TypeExpr>,
    completeness: ReturnInferenceCompleteness,
}

#[derive(Debug)]
enum ReturnInferenceFailure {
    Unsupported(ReturnInferenceUnsupported),
    Unavailable(InferenceUnavailableReason),
}

impl From<ReturnInferenceUnsupported> for ReturnInferenceFailure {
    fn from(reason: ReturnInferenceUnsupported) -> Self {
        Self::Unsupported(reason)
    }
}

impl From<InferenceUnavailableReason> for ReturnInferenceFailure {
    fn from(reason: InferenceUnavailableReason) -> Self {
        Self::Unavailable(reason)
    }
}

/// Infer the return type of a function body only when its reachable
/// control-flow surface is completely modeled.
fn infer_return_type_with_budget(
    body: &oxc_ast::ast::FunctionBody<'_>,
    source: &str,
    parameters: &[FunctionParam],
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferredReturn {
    if let Err(reason) = budget.visit(depth) {
        return InferredReturn {
            return_type: None,
            completeness: ReturnInferenceCompleteness::Unavailable(reason),
        };
    }
    let mut return_types: Vec<TypeExpr> = Vec::new();
    let can_fall_through = match collect_return_types_in_sequence(
        &body.statements,
        source,
        parameters,
        &mut return_types,
        budget,
        depth + 1,
    ) {
        Ok(can_fall_through) => can_fall_through,
        Err(ReturnInferenceFailure::Unsupported(reason)) => {
            return InferredReturn {
                return_type: None,
                completeness: ReturnInferenceCompleteness::Unsupported(reason),
            };
        }
        Err(ReturnInferenceFailure::Unavailable(reason)) => {
            return InferredReturn {
                return_type: None,
                completeness: ReturnInferenceCompleteness::Unavailable(reason),
            };
        }
    };

    if can_fall_through && !return_types.is_empty() {
        return_types.push(TypeExpr::Primitive(PrimitiveName::Undefined));
    }

    let return_type = match return_types.len() {
        0 if can_fall_through => TypeExpr::Primitive(PrimitiveName::Void),
        0 => TypeExpr::Primitive(PrimitiveName::Never),
        1 => return_types.pop().expect("one inferred return type"),
        _ => TypeExpr::union(return_types),
    };
    InferredReturn {
        return_type: Some(return_type),
        completeness: ReturnInferenceCompleteness::Complete { can_fall_through },
    }
}

/// Analyze a sequential statement list. Statements after a terminal path are
/// unreachable and therefore do not affect either inference or support.
fn collect_return_types_in_sequence(
    statements: &[Statement<'_>],
    source: &str,
    parameters: &[FunctionParam],
    results: &mut Vec<TypeExpr>,
    budget: &mut InferenceBudget,
    depth: usize,
) -> Result<bool, ReturnInferenceFailure> {
    budget.visit(depth)?;
    let mut can_fall_through = true;
    for statement in statements {
        if !can_fall_through {
            break;
        }
        can_fall_through =
            collect_return_types(statement, source, parameters, results, budget, depth + 1)?;
    }
    Ok(can_fall_through)
}

fn collect_return_types(
    stmt: &Statement<'_>,
    source: &str,
    parameters: &[FunctionParam],
    results: &mut Vec<TypeExpr>,
    budget: &mut InferenceBudget,
    depth: usize,
) -> Result<bool, ReturnInferenceFailure> {
    budget.visit(depth)?;
    match stmt {
        Statement::ReturnStatement(ret) => {
            if let Some(ref arg) = ret.argument {
                let inferred = match arg {
                    Expression::Identifier(identifier) => parameters
                        .iter()
                        .find(|parameter| {
                            parameter.name.as_deref() == Some(identifier.name.as_str())
                        })
                        .map(|parameter| Ok(parameter.ty.clone()))
                        .unwrap_or_else(|| {
                            infer_declaration_expression_type_with_budget(
                                arg,
                                source,
                                false,
                                budget,
                                depth + 1,
                            )
                        })?,
                    _ => infer_declaration_expression_type_with_budget(
                        arg,
                        source,
                        false,
                        budget,
                        depth + 1,
                    )?,
                };
                results.push(inferred);
            } else {
                results.push(TypeExpr::Primitive(PrimitiveName::Undefined));
            }
            Ok(false)
        }
        Statement::BlockStatement(block) => collect_return_types_in_sequence(
            &block.body,
            source,
            parameters,
            results,
            budget,
            depth + 1,
        ),
        Statement::IfStatement(if_stmt) => {
            let consequent = collect_return_types(
                &if_stmt.consequent,
                source,
                parameters,
                results,
                budget,
                depth + 1,
            )?;
            let alternate = match &if_stmt.alternate {
                Some(alternate) => {
                    collect_return_types(alternate, source, parameters, results, budget, depth + 1)?
                }
                None => true,
            };
            Ok(consequent || alternate)
        }
        Statement::ThrowStatement(_) => Ok(false),

        Statement::DebuggerStatement(_)
        | Statement::EmptyStatement(_)
        | Statement::ExpressionStatement(_)
        | Statement::VariableDeclaration(_)
        | Statement::FunctionDeclaration(_)
        | Statement::ClassDeclaration(_)
        | Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSEnumDeclaration(_)
        | Statement::TSModuleDeclaration(_)
        | Statement::TSGlobalDeclaration(_)
        | Statement::TSImportEqualsDeclaration(_) => Ok(true),

        // Loop / labeled statements: a subtree with NO `return` anywhere in
        // its STATEMENT tree is RETURN-TRANSPARENT — it contributes no
        // return arm and never prevents fall-through past it (a `break` /
        // `continue` inside only exits the construct), so collection of the
        // CURRENT function-level returns continues exactly (the CF13
        // labeled-break contract: `outer: for … { break outer } return
        // "done" as const` infers `"done"`). A loop/labeled subtree that
        // DOES contain a `return` stays honestly Unsupported: the union's
        // arms depend on loop reachability/endpoint analysis this collector
        // does not model, and narrowing there is the exact hazard
        // `return_inference_flow_rejects_unsupported_instead_of_narrowing`
        // pins.
        Statement::DoWhileStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_)
        | Statement::ForStatement(_)
        | Statement::WhileStatement(_) => {
            if statement_subtree_has_return(stmt) {
                Err(ReturnInferenceUnsupported::Loop.into())
            } else {
                Ok(true)
            }
        }
        Statement::SwitchStatement(_) => Err(ReturnInferenceUnsupported::Switch.into()),
        Statement::TryStatement(_) => Err(ReturnInferenceUnsupported::Try.into()),
        Statement::BreakStatement(_) | Statement::ContinueStatement(_) => {
            Err(ReturnInferenceUnsupported::Jump.into())
        }
        Statement::LabeledStatement(_) => {
            if statement_subtree_has_return(stmt) {
                Err(ReturnInferenceUnsupported::Labeled.into())
            } else {
                Ok(true)
            }
        }
        Statement::WithStatement(_) => Err(ReturnInferenceUnsupported::With.into()),
        Statement::ImportDeclaration(_)
        | Statement::ExportAllDeclaration(_)
        | Statement::ExportDefaultDeclaration(_)
        | Statement::ExportNamedDeclaration(_)
        | Statement::TSExportAssignment(_)
        | Statement::TSNamespaceExportDeclaration(_) => {
            Err(ReturnInferenceUnsupported::ModuleDeclaration.into())
        }
    }
}

/// Whether a STATEMENT subtree contains a `return` statement of the CURRENT
/// function. Walks statement structure only — nested function/arrow/class
/// bodies live in EXPRESSION position and are never entered, so their
/// `return`s (which belong to the nested function) never count. Drives the
/// return-transparency rule for loop / labeled statements in
/// [`collect_return_types`]: a return-free construct is skipped exactly; a
/// return-bearing one stays typed-Unsupported.
fn statement_subtree_has_return(stmt: &Statement<'_>) -> bool {
    use oxc_ast::ast::Statement;
    match stmt {
        Statement::ReturnStatement(_) => true,
        Statement::BlockStatement(block) => block.body.iter().any(statement_subtree_has_return),
        Statement::IfStatement(if_stmt) => {
            statement_subtree_has_return(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(statement_subtree_has_return)
        }
        Statement::LabeledStatement(labeled) => statement_subtree_has_return(&labeled.body),
        Statement::WhileStatement(w) => statement_subtree_has_return(&w.body),
        Statement::DoWhileStatement(d) => statement_subtree_has_return(&d.body),
        Statement::ForStatement(f) => statement_subtree_has_return(&f.body),
        Statement::ForInStatement(f) => statement_subtree_has_return(&f.body),
        Statement::ForOfStatement(f) => statement_subtree_has_return(&f.body),
        Statement::SwitchStatement(s) => s
            .cases
            .iter()
            .flat_map(|case| case.consequent.iter())
            .any(statement_subtree_has_return),
        Statement::TryStatement(t) => {
            t.block.body.iter().any(statement_subtree_has_return)
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.body.iter().any(statement_subtree_has_return))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.body.iter().any(statement_subtree_has_return))
        }
        Statement::WithStatement(w) => statement_subtree_has_return(&w.body),
        _ => false,
    }
}

/// Infer a simple type from an expression literal.
/// How fresh object-literal MEMBER values are treated during value inference.
/// The three states are the only object-literal widening contexts:
///
/// - [`Widen`](MemberLiteralPolicy::Widen): a plain object literal — a fresh
///   literal member widens to its primitive (`{ count: 0 }` → `{ count: number }`).
/// - [`Preserve`](MemberLiteralPolicy::Preserve): a `satisfies`-constrained
///   object — members keep their literal types (the engine performs no
///   contextual typing; the deeper contextual-widening behaviour is a separate
///   deferred contract) and are NOT `readonly`.
/// - [`ConstAssert`](MemberLiteralPolicy::ConstAssert): an `as const` object —
///   members keep their literals AND are `readonly`.
///
/// A per-property `as const` (`{ tag: "x" as const }`) overrides the enclosing
/// policy to `ConstAssert` for that one member.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MemberLiteralPolicy {
    Widen,
    Preserve,
    ConstAssert,
}

pub(crate) const MAX_SEMANTIC_INFERENCE_DEPTH: usize = 64;
pub(crate) const MAX_SEMANTIC_INFERENCE_WORK: usize = 4096;

type InferenceResult<T> = Result<T, InferenceUnavailableReason>;

#[derive(Debug)]
struct InferenceBudget {
    remaining_work: usize,
}

impl Default for InferenceBudget {
    fn default() -> Self {
        Self {
            remaining_work: MAX_SEMANTIC_INFERENCE_WORK,
        }
    }
}

impl InferenceBudget {
    fn visit(&mut self, depth: usize) -> InferenceResult<()> {
        if depth >= MAX_SEMANTIC_INFERENCE_DEPTH {
            return Err(InferenceUnavailableReason::DepthBudgetExceeded);
        }
        let Some(remaining_work) = self.remaining_work.checked_sub(1) else {
            return Err(InferenceUnavailableReason::WorkBudgetExceeded);
        };
        self.remaining_work = remaining_work;
        Ok(())
    }
}

fn infer_expression_type(expr: &Expression<'_>, source: &str) -> InferenceResult<TypeExpr> {
    let mut budget = InferenceBudget::default();
    infer_expression_type_ctx(expr, source, MemberLiteralPolicy::Widen, &mut budget, 0)
}

fn infer_declaration_expression_type(
    expr: &Expression<'_>,
    source: &str,
    preserve_literal: bool,
) -> InferenceResult<TypeExpr> {
    let mut budget = InferenceBudget::default();
    infer_declaration_expression_type_with_budget(expr, source, preserve_literal, &mut budget, 0)
}

fn infer_declaration_expression_type_with_budget(
    expr: &Expression<'_>,
    source: &str,
    preserve_literal: bool,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<TypeExpr> {
    budget.visit(depth)?;
    if preserve_literal || expr_is_const_asserted(expr, source) {
        return infer_expression_type_ctx(
            expr,
            source,
            MemberLiteralPolicy::Widen,
            budget,
            depth + 1,
        );
    }
    match expr {
        Expression::ParenthesizedExpression(parenthesized) => {
            infer_declaration_expression_type_with_budget(
                &parenthesized.expression,
                source,
                false,
                budget,
                depth + 1,
            )
        }
        Expression::ConditionalExpression(conditional) => Ok(TypeExpr::union(vec![
            infer_declaration_expression_type_with_budget(
                &conditional.consequent,
                source,
                false,
                budget,
                depth + 1,
            )?,
            infer_declaration_expression_type_with_budget(
                &conditional.alternate,
                source,
                false,
                budget,
                depth + 1,
            )?,
        ])),
        Expression::ArrayExpression(array) => {
            let mut element_types = Vec::new();
            for element in &array.elements {
                match element {
                    oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) => {
                        let spread_type = infer_declaration_expression_type_with_budget(
                            &spread.argument,
                            source,
                            false,
                            budget,
                            depth + 1,
                        )?;
                        if let Some(elements) = collect_array_element_types_from_type(&spread_type)
                        {
                            element_types.extend(elements);
                        } else {
                            element_types.push(TypeExpr::Primitive(PrimitiveName::Any));
                        }
                    }
                    oxc_ast::ast::ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(expression) = element.as_expression() {
                            append_union_members(
                                &mut element_types,
                                infer_declaration_expression_type_with_budget(
                                    expression,
                                    source,
                                    false,
                                    budget,
                                    depth + 1,
                                )?,
                            );
                        }
                    }
                }
            }
            Ok(TypeExpr::Array {
                element: Arc::new(if element_types.is_empty() {
                    TypeExpr::Primitive(PrimitiveName::Any)
                } else {
                    TypeExpr::union(element_types)
                }),
                readonly: false,
            })
        }
        _ => infer_expression_type_ctx(expr, source, MemberLiteralPolicy::Widen, budget, depth + 1)
            .map(widen_shallow_literal),
    }
}

/// Whether an expression is a `… as const` assertion (seen through a
/// parenthesised wrapper). Drives object-literal property widening: an
/// `as const`-asserted property keeps its literal type and is `readonly`; a
/// bare-literal property widens to its primitive.
fn expr_is_const_asserted(expr: &Expression<'_>, source: &str) -> bool {
    let mut expr = expr;
    loop {
        match expr {
            Expression::TSAsExpression(ts_as) => {
                return is_const_assertion_type_expr(&lower_ts_type(
                    &ts_as.type_annotation,
                    source,
                ));
            }
            Expression::ParenthesizedExpression(paren) => expr = &paren.expression,
            _ => return false,
        }
    }
}

/// Widen a TOP-LEVEL fresh literal (`"x"` / `1` / `true` / `1n`) to its
/// primitive — the TS object-literal property widening rule applied to one
/// member-value position. Objects / arrays / refs pass through unchanged
/// (their own members were already widened recursively at their own
/// inference level), so an `as const` member nested inside a widened object
/// is never re-widened.
fn widen_shallow_literal(ty: TypeExpr) -> TypeExpr {
    match ty {
        TypeExpr::Literal(verter_type_expr::LiteralValue::String(_)) => {
            TypeExpr::Primitive(PrimitiveName::String)
        }
        TypeExpr::Literal(verter_type_expr::LiteralValue::Number(_)) => {
            TypeExpr::Primitive(PrimitiveName::Number)
        }
        TypeExpr::Literal(verter_type_expr::LiteralValue::Boolean(_)) => {
            TypeExpr::Primitive(PrimitiveName::Boolean)
        }
        TypeExpr::Literal(verter_type_expr::LiteralValue::BigInt(_)) => {
            TypeExpr::Primitive(PrimitiveName::BigInt)
        }
        other => other,
    }
}

/// Compute one object-literal member's `(type, readonly)` under `policy`. A
/// per-property `as const` (`{ tag: "x" as const }`) overrides to
/// `ConstAssert` for that member; otherwise `policy` decides: `Widen` widens a
/// fresh top-level literal to its primitive, `Preserve` keeps it, `ConstAssert`
/// keeps it AND marks it `readonly`. The member value is inferred under the
/// effective policy so nested objects inherit it.
fn object_member_value(
    value: &Expression<'_>,
    source: &str,
    policy: MemberLiteralPolicy,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<(TypeExpr, bool)> {
    let per_prop_const = expr_is_const_asserted(value, source);
    // `readonly` comes ONLY from a WHOLE-OBJECT `as const` (the enclosing
    // `policy`). A per-property `as const` (`{ tag: "x" as const }`) narrows the
    // VALUE to a literal but does NOT add the `readonly` modifier — TS leaves
    // `tag` mutable; only `{ … } as const` makes the properties `readonly`.
    let readonly = policy == MemberLiteralPolicy::ConstAssert;
    // The value (and its NESTED members) is inferred under a const context when
    // the whole object is `as const` OR this property carries its own `as const`,
    // so a nested object under a per-property `as const`
    // (`{ tag: { x: 1 } as const }`) still yields readonly + literal members.
    let value_policy = if per_prop_const {
        MemberLiteralPolicy::ConstAssert
    } else {
        policy
    };
    // Widen a fresh TOP-LEVEL literal only under a plain `Widen` context (no
    // per-property `as const`); `Preserve` (satisfies) and `ConstAssert` keep it.
    let ty = if value_policy == MemberLiteralPolicy::Widen {
        infer_declaration_expression_type_with_budget(value, source, false, budget, depth)?
    } else {
        infer_expression_type_ctx(value, source, value_policy, budget, depth)?
    };
    Ok((ty, readonly))
}

/// Infer the type of a value expression. `policy` governs how fresh
/// object-literal MEMBER values are treated (see [`MemberLiteralPolicy`]):
/// a plain object literal widens its members, a `satisfies`-constrained one
/// preserves them, an `as const` one preserves + marks them `readonly`.
/// Standalone literals never widen (a `const x = 0` is `0`); only
/// OBJECT-PROPERTY positions are affected.
fn infer_expression_type_ctx(
    expr: &Expression<'_>,
    source: &str,
    policy: MemberLiteralPolicy,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<TypeExpr> {
    budget.visit(depth)?;
    match expr {
        Expression::Identifier(ident) => Ok(TypeExpr::TypeOf(ValueRef {
            path: vec![ident.name.as_str().to_string()],
            type_args: Vec::new(),
        })),
        Expression::StringLiteral(s) => Ok(TypeExpr::string_literal(s.value.as_str())),
        Expression::NumericLiteral(n) => Ok(TypeExpr::number_literal(n.value)),
        Expression::BooleanLiteral(b) => Ok(TypeExpr::boolean_literal(b.value)),
        Expression::NullLiteral(_) => Ok(TypeExpr::Primitive(PrimitiveName::Null)),
        Expression::ConditionalExpression(cond) => Ok(TypeExpr::union(vec![
            infer_expression_type_ctx(&cond.consequent, source, policy, budget, depth + 1)?,
            infer_expression_type_ctx(&cond.alternate, source, policy, budget, depth + 1)?,
        ])),
        Expression::ParenthesizedExpression(paren) => {
            infer_expression_type_ctx(&paren.expression, source, policy, budget, depth + 1)
        }
        Expression::ArrayExpression(arr) => {
            let mut element_types = Vec::new();
            for element in &arr.elements {
                match element {
                    oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) => {
                        append_spread_array_element_types(
                            &spread.argument,
                            source,
                            &mut element_types,
                            budget,
                            depth + 1,
                        )?;
                    }
                    oxc_ast::ast::ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(expr) = element.as_expression() {
                            append_union_members(
                                &mut element_types,
                                infer_expression_type_ctx(expr, source, policy, budget, depth + 1)?,
                            );
                        }
                    }
                }
            }

            let element = if element_types.is_empty() {
                TypeExpr::Primitive(PrimitiveName::Any)
            } else {
                TypeExpr::union(element_types)
            };
            Ok(TypeExpr::Array {
                element: Arc::new(element),
                readonly: false,
            })
        }
        Expression::ObjectExpression(obj) => {
            extract_object_literal_as_type(obj, source, policy, budget, depth + 1)
        }
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            let mut value = String::new();
            for quasi in &tpl.quasis {
                value.push_str(quasi.value.raw.as_str());
            }
            Ok(TypeExpr::string_literal(value))
        }
        Expression::TemplateLiteral(_) => Ok(TypeExpr::Primitive(PrimitiveName::String)),
        Expression::ArrowFunctionExpression(arrow) => {
            let sig = extract_arrow_signature_with_budget(arrow, source, budget, depth + 1)?;
            let fn_spans = FunctionSpans {
                signature: Some(arrow.span.into()),
                return_type: arrow
                    .return_type
                    .as_ref()
                    .map(|rt| rt.type_annotation.span().into()),
            };
            Ok(TypeExpr::Function(Arc::new(FunctionExpr::with_spans(
                sig.parameters,
                sig.return_type.map(Arc::new),
                sig.type_parameters,
                fn_spans,
            ))))
        }
        Expression::TSAsExpression(ts_as) => {
            // `as const` should preserve the underlying literal/object surface
            // instead of degrading the inferred type to an opaque `const`
            // marker — AND it establishes a const context, so nested object
            // properties keep their literals + become `readonly`.
            let asserted = lower_ts_type(&ts_as.type_annotation, source);
            if is_const_assertion_type_expr(&asserted) {
                infer_expression_type_ctx(
                    &ts_as.expression,
                    source,
                    MemberLiteralPolicy::ConstAssert,
                    budget,
                    depth + 1,
                )
            } else {
                Ok(asserted)
            }
        }
        Expression::TSSatisfiesExpression(sat) => {
            // const x = value satisfies SomeType → infer from the underlying
            // value expression, not the annotation. `satisfies` validates but
            // does NOT widen the value's members (the engine performs no
            // contextual typing) — Preserve, unless an enclosing `as const`
            // already pinned a stronger (readonly) context.
            let inner_policy = if policy == MemberLiteralPolicy::ConstAssert {
                MemberLiteralPolicy::ConstAssert
            } else {
                MemberLiteralPolicy::Preserve
            };
            infer_expression_type_ctx(&sat.expression, source, inner_policy, budget, depth + 1)
        }
        Expression::StaticMemberExpression(member) => {
            // obj.foo → typeof obj.foo (build a dotted path)
            let mut path = Vec::new();
            collect_static_member_path_with_budget(member, &mut path, budget, depth + 1)?;
            if path.is_empty() {
                Ok(TypeExpr::Primitive(PrimitiveName::Any))
            } else {
                Ok(TypeExpr::TypeOf(ValueRef {
                    path,
                    type_args: Vec::new(),
                }))
            }
        }
        Expression::CallExpression(call) => {
            // fn() → ReturnType<typeof fn>
            let callee_type =
                infer_expression_type_ctx(&call.callee, source, policy, budget, depth + 1)?;
            if matches!(callee_type, TypeExpr::Primitive(PrimitiveName::Any)) {
                Ok(TypeExpr::Primitive(PrimitiveName::Any))
            } else {
                Ok(TypeExpr::Ref {
                    name: Arc::from("ReturnType"),
                    type_arguments: Arc::from(vec![callee_type]),
                })
            }
        }
        _ => Ok(TypeExpr::Primitive(PrimitiveName::Any)),
    }
}

/// Collect a dotted member path from a static member expression chain.
/// `a.b.c` → `["a", "b", "c"]` (in order). Non-identifier roots abort (clear path).
fn collect_static_member_path(
    member: &oxc_ast::ast::StaticMemberExpression<'_>,
    path: &mut Vec<String>,
) {
    let mut properties = Vec::new();
    let mut current = member;
    loop {
        properties.push(current.property.name.as_str().to_string());
        match &current.object {
            Expression::Identifier(identifier) => {
                path.push(identifier.name.as_str().to_string());
                break;
            }
            Expression::StaticMemberExpression(parent) => current = parent,
            _ => {
                path.clear();
                return;
            }
        }
    }
    properties.reverse();
    path.extend(properties);
}

fn collect_static_member_path_with_budget(
    member: &oxc_ast::ast::StaticMemberExpression<'_>,
    path: &mut Vec<String>,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<()> {
    let mut properties = Vec::new();
    let mut current = member;
    let mut nesting = 0usize;
    loop {
        budget.visit(depth + nesting)?;
        properties.push(current.property.name.as_str().to_string());
        match &current.object {
            Expression::Identifier(identifier) => {
                path.push(identifier.name.as_str().to_string());
                break;
            }
            Expression::StaticMemberExpression(parent) => {
                current = parent;
                nesting += 1;
            }
            _ => {
                path.clear();
                return Ok(());
            }
        }
    }
    properties.reverse();
    path.extend(properties);
    Ok(())
}

fn append_spread_array_element_types(
    expr: &Expression<'_>,
    source: &str,
    element_types: &mut Vec<TypeExpr>,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<()> {
    let spread_ty =
        infer_expression_type_ctx(expr, source, MemberLiteralPolicy::Widen, budget, depth)?;
    if let Some(spread_elements) = collect_array_element_types_from_type(&spread_ty) {
        element_types.extend(spread_elements);
    } else {
        element_types.push(TypeExpr::Primitive(PrimitiveName::Any));
    }
    Ok(())
}

fn collect_array_element_types_from_type(ty: &TypeExpr) -> Option<Vec<TypeExpr>> {
    match ty {
        TypeExpr::Array { element, .. } => {
            let mut members = Vec::new();
            append_union_members(&mut members, element.as_ref().clone());
            Some(members)
        }
        TypeExpr::Tuple { elements, .. } => {
            let mut members = Vec::new();
            for element in elements.iter() {
                append_union_members(&mut members, element.ty.clone());
            }
            Some(members)
        }
        TypeExpr::Union(members) => {
            let mut collected = Vec::new();
            for member in members.iter() {
                let nested = collect_array_element_types_from_type(member)?;
                collected.extend(nested);
            }
            Some(collected)
        }
        _ => None,
    }
}

fn append_union_members(into: &mut Vec<TypeExpr>, ty: TypeExpr) {
    // `TypeExpr` implements `Drop`; flatten a union by borrowing + cloning
    // its (refcounted) members, otherwise push the whole value by move.
    if let TypeExpr::Union(members) = &ty {
        into.extend(members.iter().cloned());
    } else {
        into.push(ty);
    }
}

fn widen_literal_type(expr: TypeExpr) -> InferenceResult<TypeExpr> {
    let mut budget = InferenceBudget::default();
    widen_literal_type_with_budget(expr, &mut budget, 0)
}

fn widen_literal_type_with_budget(
    expr: TypeExpr,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<TypeExpr> {
    budget.visit(depth)?;
    // `TypeExpr` implements `Drop`, so the compound arms below cannot bind
    // their children by-move out of an owned `expr`. Match on a borrow and
    // clone the (refcounted) children; the catch-all forwards `expr` whole
    // (a full-value move, which `Drop` permits).
    match &expr {
        TypeExpr::Literal(verter_type_expr::LiteralValue::String(_)) => {
            Ok(TypeExpr::Primitive(PrimitiveName::String))
        }
        TypeExpr::Literal(verter_type_expr::LiteralValue::Number(_)) => {
            Ok(TypeExpr::Primitive(PrimitiveName::Number))
        }
        TypeExpr::Literal(verter_type_expr::LiteralValue::Boolean(_)) => {
            Ok(TypeExpr::Primitive(PrimitiveName::Boolean))
        }
        TypeExpr::Literal(verter_type_expr::LiteralValue::BigInt(_)) => {
            Ok(TypeExpr::Primitive(PrimitiveName::BigInt))
        }
        TypeExpr::Union(members) => {
            let mut widened = Vec::with_capacity(members.len());
            for member in members.iter().cloned() {
                widened.push(widen_literal_type_with_budget(member, budget, depth + 1)?);
            }
            Ok(TypeExpr::union(dedupe_type_exprs(widened)))
        }
        TypeExpr::Intersection(members) => {
            let mut widened = Vec::with_capacity(members.len());
            for member in members.iter().cloned() {
                widened.push(widen_literal_type_with_budget(member, budget, depth + 1)?);
            }
            Ok(TypeExpr::intersection(widened))
        }
        TypeExpr::Array { element, readonly } => Ok(TypeExpr::Array {
            element: Arc::new(widen_literal_type_with_budget(
                element.as_ref().clone(),
                budget,
                depth + 1,
            )?),
            readonly: *readonly,
        }),
        TypeExpr::Tuple { elements, readonly } => {
            let mut widened = Vec::with_capacity(elements.len());
            for mut element in elements.iter().cloned() {
                element.ty = widen_literal_type_with_budget(element.ty, budget, depth + 1)?;
                widened.push(element);
            }
            Ok(TypeExpr::Tuple {
                elements: Arc::from(widened),
                readonly: *readonly,
            })
        }
        TypeExpr::Object(obj) => {
            let mut properties = Vec::with_capacity(obj.properties.len());
            for property in obj.properties.iter().cloned() {
                properties.push(widen_object_member_with_budget(
                    property,
                    budget,
                    depth + 1,
                )?);
            }
            Ok(TypeExpr::Object(Arc::new(ObjectExpr { properties })))
        }
        TypeExpr::Function(function) => Ok(TypeExpr::Function(Arc::new(FunctionExpr::with_spans(
            function.parameters.clone(),
            function
                .return_type
                .as_ref()
                .map(|return_type| {
                    widen_literal_type_with_budget(return_type.as_ref().clone(), budget, depth + 1)
                        .map(Arc::new)
                })
                .transpose()?,
            function.type_parameters.clone(),
            function.spans,
        )))),
        // A bare constructor type (`new (...) => R`) carries the same
        // `FunctionExpr` payload as a function type, so its literal members
        // widen identically. Reconstruct as a `ConstructorType` so the
        // constructor-ness survives — never flatten it to a plain `Function`.
        // This runs on analyzer-side lowered IR (e.g. `value as new () => T`),
        // BEFORE the dispatch lower collapses `Function`/`ConstructorType`.
        TypeExpr::ConstructorType(function) => Ok(TypeExpr::ConstructorType(Arc::new(
            FunctionExpr::with_spans(
                function.parameters.clone(),
                function
                    .return_type
                    .as_ref()
                    .map(|return_type| {
                        widen_literal_type_with_budget(
                            return_type.as_ref().clone(),
                            budget,
                            depth + 1,
                        )
                        .map(Arc::new)
                    })
                    .transpose()?,
                function.type_parameters.clone(),
                function.spans,
            ),
        ))),
        _ => Ok(expr),
    }
}

fn widen_object_member_with_budget(
    member: ObjectMember,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<ObjectMember> {
    budget.visit(depth)?;
    match member {
        ObjectMember::Property(mut property) => {
            property.ty = widen_literal_type_with_budget(property.ty, budget, depth + 1)?;
            Ok(ObjectMember::Property(property))
        }
        ObjectMember::IndexSignature(mut signature) => {
            signature.value_type =
                widen_literal_type_with_budget(signature.value_type, budget, depth + 1)?;
            Ok(ObjectMember::IndexSignature(signature))
        }
        ObjectMember::CallSignature(function) => {
            Ok(ObjectMember::CallSignature(FunctionExpr::with_spans(
                function.parameters,
                function
                    .return_type
                    .as_ref()
                    .map(|return_type| {
                        widen_literal_type_with_budget(
                            return_type.as_ref().clone(),
                            budget,
                            depth + 1,
                        )
                        .map(Arc::new)
                    })
                    .transpose()?,
                function.type_parameters,
                function.spans,
            )))
        }
        ObjectMember::ConstructSignature(function) => {
            Ok(ObjectMember::ConstructSignature(FunctionExpr::with_spans(
                function.parameters,
                function
                    .return_type
                    .as_ref()
                    .map(|return_type| {
                        widen_literal_type_with_budget(
                            return_type.as_ref().clone(),
                            budget,
                            depth + 1,
                        )
                        .map(Arc::new)
                    })
                    .transpose()?,
                function.type_parameters,
                function.spans,
            )))
        }
        ObjectMember::Method(mut method) => {
            method.function = FunctionExpr::with_spans(
                method.function.parameters,
                method
                    .function
                    .return_type
                    .as_ref()
                    .map(|return_type| {
                        widen_literal_type_with_budget(
                            return_type.as_ref().clone(),
                            budget,
                            depth + 1,
                        )
                        .map(Arc::new)
                    })
                    .transpose()?,
                method.function.type_parameters,
                method.function.spans,
            );
            Ok(ObjectMember::Method(method))
        }
    }
}

fn dedupe_type_exprs(types: Vec<TypeExpr>) -> Vec<TypeExpr> {
    let mut unique = Vec::new();
    for ty in types {
        if !unique.contains(&ty) {
            unique.push(ty);
        }
    }
    unique
}

fn is_const_assertion_type_expr(expr: &TypeExpr) -> bool {
    matches!(
        expr,
        TypeExpr::Unknown { raw } if raw == "const"
    ) || matches!(
        expr,
        TypeExpr::Ref { name, type_arguments } if name.as_ref() == "const" && type_arguments.is_empty()
    )
}

fn lower_interface_member(sig: &TSSignature<'_>, source: &str) -> Option<ObjectMember> {
    match sig {
        TSSignature::TSPropertySignature(prop) => {
            let name = property_key_name(&prop.key)?;
            let ty = prop
                .type_annotation
                .as_ref()
                .map(|ta| lower_ts_type(&ta.type_annotation, source))
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
            let spans = MemberSpans {
                declaration: Some(prop.span.into()),
                name: Some(prop.key.span().into()),
                type_annotation: prop
                    .type_annotation
                    .as_ref()
                    .map(|ta| ta.type_annotation.span().into()),
            };
            Some(ObjectMember::Property(
                verter_type_expr::ObjectProperty::with_spans_public(
                    name,
                    ty,
                    prop.optional,
                    prop.readonly,
                    spans,
                ),
            ))
        }
        TSSignature::TSMethodSignature(method) => {
            let name = property_key_name(&method.key)?;
            let params = lower_function_params(&method.params, source);
            let return_type = method
                .return_type
                .as_ref()
                .map(|rt| lower_ts_type(&rt.type_annotation, source));
            let type_parameters = method
                .type_parameters
                .as_ref()
                .map(|tp| lower_type_param_decls(tp, source))
                .unwrap_or_default();
            let fn_spans = FunctionSpans {
                signature: Some(method.span.into()),
                return_type: method
                    .return_type
                    .as_ref()
                    .map(|rt| rt.type_annotation.span().into()),
            };
            let member_spans = MemberSpans {
                declaration: Some(method.span.into()),
                name: Some(method.key.span().into()),
                type_annotation: None,
            };
            Some(ObjectMember::Method(MethodSignature::with_spans_public(
                name,
                FunctionExpr::with_spans(
                    params,
                    return_type.map(Arc::new),
                    type_parameters,
                    fn_spans,
                ),
                method.optional,
                member_spans,
            )))
        }
        TSSignature::TSCallSignatureDeclaration(call) => {
            let params = lower_function_params(&call.params, source);
            let return_type = call
                .return_type
                .as_ref()
                .map(|rt| lower_ts_type(&rt.type_annotation, source));
            let type_parameters = call
                .type_parameters
                .as_ref()
                .map(|tp| lower_type_param_decls(tp, source))
                .unwrap_or_default();
            let fn_spans = FunctionSpans {
                signature: Some(call.span.into()),
                return_type: call
                    .return_type
                    .as_ref()
                    .map(|rt| rt.type_annotation.span().into()),
            };
            Some(ObjectMember::CallSignature(FunctionExpr::with_spans(
                params,
                return_type.map(Arc::new),
                type_parameters,
                fn_spans,
            )))
        }
        TSSignature::TSIndexSignature(idx) => {
            let (key_name, key_type, key_span) = if let Some(param) = idx.parameters.first() {
                (
                    param.name.to_string(),
                    lower_ts_type(&param.type_annotation.type_annotation, source),
                    Some(param.span.into()),
                )
            } else {
                (
                    "key".to_string(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    None,
                )
            };
            let value_type = lower_ts_type(&idx.type_annotation.type_annotation, source);
            let spans = IndexSignatureSpans {
                declaration: Some(idx.span.into()),
                key: key_span,
                value: Some(idx.type_annotation.type_annotation.span().into()),
            };
            Some(ObjectMember::IndexSignature(IndexSignature::with_spans(
                key_name,
                key_type,
                value_type,
                idx.readonly,
                spans,
            )))
        }
        TSSignature::TSConstructSignatureDeclaration(ctor) => {
            let params = lower_function_params(&ctor.params, source);
            let return_type = ctor
                .return_type
                .as_ref()
                .map(|rt| lower_ts_type(&rt.type_annotation, source));
            let type_parameters = ctor
                .type_parameters
                .as_ref()
                .map(|tp| lower_type_param_decls(tp, source))
                .unwrap_or_default();
            let fn_spans = FunctionSpans {
                signature: Some(ctor.span.into()),
                return_type: ctor
                    .return_type
                    .as_ref()
                    .map(|rt| rt.type_annotation.span().into()),
            };
            Some(ObjectMember::ConstructSignature(FunctionExpr::with_spans(
                params,
                return_type.map(Arc::new),
                type_parameters,
                fn_spans,
            )))
        }
    }
}

fn lower_function_params(params: &FormalParameters<'_>, source: &str) -> Vec<FunctionParam> {
    let mut budget = InferenceBudget::default();
    lower_function_params_with_budget(params, source, &mut budget, 0)
        .unwrap_or_else(|_| lower_function_params_without_initializer_inference(params, source))
}

fn lower_function_params_with_budget(
    params: &FormalParameters<'_>,
    source: &str,
    budget: &mut InferenceBudget,
    depth: usize,
) -> InferenceResult<Vec<FunctionParam>> {
    budget.visit(depth)?;
    let mut lowered_params =
        Vec::with_capacity(params.items.len() + usize::from(params.rest.is_some()));
    for param in &params.items {
        budget.visit(depth + 1)?;
        let name = match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
            _ => None,
        };
        // The OXC structural fact: did this parameter carry an explicit TS
        // type annotation? Captured here (the AST node is in hand), it is the
        // sole authority for JSDoc `@param` precedence downstream — an
        // explicit `: any` lowers to `Primitive(Any)` exactly like a missing
        // annotation, so the lowered `ty` cannot distinguish the two.
        let has_ts_annotation = param.type_annotation.is_some();
        let ty = if let Some(annotation) = &param.type_annotation {
            lower_ts_type(&annotation.type_annotation, source)
        } else if let Some(initializer) = &param.initializer {
            infer_declaration_expression_type_with_budget(
                initializer,
                source,
                false,
                budget,
                depth + 2,
            )?
        } else {
            TypeExpr::Primitive(PrimitiveName::Any)
        };
        let mut lowered = FunctionParam::with_span(
            name,
            ty,
            param.optional || param.initializer.is_some(),
            false,
            Some(param.span.into()),
            has_ts_annotation,
        );
        lowered.is_parameter_property =
            param.accessibility.is_some() || param.readonly || param.r#override;
        lowered_params.push(lowered);
    }
    if let Some(rest) = &params.rest {
        budget.visit(depth + 1)?;
        let name = match &rest.rest.argument {
            BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
            _ => None,
        };
        let has_ts_annotation = rest.type_annotation.is_some();
        let ty = rest
            .type_annotation
            .as_ref()
            .map(|ta| lower_ts_type(&ta.type_annotation, source))
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
        lowered_params.push(FunctionParam::with_span(
            name,
            ty,
            false,
            true,
            Some(rest.span.into()),
            has_ts_annotation,
        ));
    }
    Ok(lowered_params)
}

fn lower_function_params_without_initializer_inference(
    params: &FormalParameters<'_>,
    source: &str,
) -> Vec<FunctionParam> {
    let mut lowered = Vec::with_capacity(params.items.len() + usize::from(params.rest.is_some()));
    for param in &params.items {
        let name = match &param.pattern {
            BindingPattern::BindingIdentifier(identifier) => Some(identifier.name.to_string()),
            _ => None,
        };
        let has_ts_annotation = param.type_annotation.is_some();
        let ty = param
            .type_annotation
            .as_ref()
            .map(|annotation| lower_ts_type(&annotation.type_annotation, source))
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
        let mut parameter = FunctionParam::with_span(
            name,
            ty,
            param.optional || param.initializer.is_some(),
            false,
            Some(param.span.into()),
            has_ts_annotation,
        );
        parameter.is_parameter_property =
            param.accessibility.is_some() || param.readonly || param.r#override;
        lowered.push(parameter);
    }
    if let Some(rest) = &params.rest {
        let name = match &rest.rest.argument {
            BindingPattern::BindingIdentifier(identifier) => Some(identifier.name.to_string()),
            _ => None,
        };
        let has_ts_annotation = rest.type_annotation.is_some();
        let ty = rest
            .type_annotation
            .as_ref()
            .map(|annotation| lower_ts_type(&annotation.type_annotation, source))
            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
        lowered.push(FunctionParam::with_span(
            name,
            ty,
            false,
            true,
            Some(rest.span.into()),
            has_ts_annotation,
        ));
    }
    lowered
}

fn lower_type_param_decls(
    type_params: &TSTypeParameterDeclaration<'_>,
    source: &str,
) -> Vec<TypeParam> {
    type_params
        .params
        .iter()
        .map(|p| TypeParam {
            name: p.name.to_string(),
            constraint: p
                .constraint
                .as_ref()
                .map(|c| Arc::new(lower_ts_type(c, source))),
            default: p
                .default
                .as_ref()
                .map(|d| Arc::new(lower_ts_type(d, source))),
        })
        .collect()
}

pub fn parse_type_parameter_clause(clause: &str) -> Vec<TypeParam> {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::Statement;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let wrapped = format!("type __VerterGeneric__<{clause}> = void");
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &wrapped, SourceType::ts()).parse();
    let Some(Statement::TSTypeAliasDeclaration(alias)) = ret.program.body.first() else {
        return Vec::new();
    };
    alias
        .type_parameters
        .as_ref()
        .map(|params| lower_type_param_decls(params, &wrapped))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Expansion-based macro type evaluation
// ---------------------------------------------------------------------------

/// Scope hint for `expand_macro_types_impl_with_expander` — full component
/// meta uses `Full`, fallthrough resolution uses `Fallthrough` to skip work
/// the fallthrough pipeline doesn't need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroExpansionScope {
    Full,
    Fallthrough,
}

/// Field kind discriminator threaded into the closure passed to
/// [`expand_macro_types_impl_with_expander`].
///
/// The closure receives the [`TypeExpr`] alongside this discriminator;
/// session-side surface-id capture (sidecar propagation) needs to know
/// which output vector the result is destined for so the captured
/// `SemanticNodeId` lands in the correct `SurfaceNodeIdentities`
/// slot. Threading the discriminator at the closure-call boundary
/// keeps the verter_semantic API scope-aware without exposing
/// session-layer types upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FieldKind {
    /// `defineProps<T>()` field — populates `ExpandedComponentTypes.props`.
    Prop,
    /// `defineEmits<T>()` field — populates `ExpandedComponentTypes.emits`.
    Emit,
    /// `defineSlots<T>()` slot binding — populates
    /// `ExpandedComponentTypes.slot_bindings`.
    SlotBinding,
    /// `defineExpose<T>()` binding — populates
    /// `ExpandedComponentTypes.bindings`.
    Binding,
}

/// Exact local value declaration selected for a requested `defineExpose`
/// binding. The declaration owner is carried separately from the published
/// name so same-name module/setup bindings cannot alias during expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingExpansionEntry {
    pub name: String,
    pub owner: TopLevelOwnerId,
}

/// Path segment for [`FieldExpansionContext::output_path`] — a path from
/// the parent macro shell (e.g. `Props<T>`) to the specific field the
/// closure is being invoked for. The session-side closure converts this
/// into a `verter_session::semantic_query::PathSegment` slice when
/// constructing the dispatch projection query (plan Step 1 / D1.1).
///
/// `Member` is the only variant required for Step 1 — `defineProps`,
/// `defineEmits`, and `defineSlots` all expose fields at named members
/// of the macro's parent type. Future variants (`Index`, `KeyOf`) are
/// deferred until a consumer needs them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PathSegment {
    /// Named-member hop, e.g. `[Member("items")]` for the `items` prop
    /// field of `defineProps<Props>()`.
    Member(std::sync::Arc<str>),
}

/// Closure invocation context for
/// [`expand_macro_types_impl_with_expander`]'s `expand_field_expr`
/// callback (plan Step 1 / D1.1).
///
/// Replaces the previous bare `FieldKind` parameter so the closure has
/// enough context to drive a dispatch-mediated projection of the
/// macro's parent shell rather than re-resolving the field-level
/// `TypeExpr` in isolation:
///
/// - `kind` — destination output vector (Prop / Emit / SlotBinding / Binding).
/// - `macro_index` — index into the surrounding `AnalyzedFileSnapshot::macros`
///   slice. The closure consumes `macro.parsed_type_argument` (cached
///   shallow analysis output, plan D1.2) at this index to obtain the
///   parent shell as a [`TypeExpr`] without re-parsing.
/// - `output_path` — path from the parent shell to the field's value.
///   For props/emits this is `[Member(field_name)]`; for slot bindings
///   it is `[Member(slot_name), Member(binding_name)]`. The closure
///   passes the path through dispatch's `ProjectPath` query after
///   lowering the parent shell.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FieldExpansionContext {
    pub kind: FieldKind,
    pub macro_index: usize,
    pub scope_owner: TopLevelOwnerId,
    pub output_path: std::sync::Arc<[PathSegment]>,
}

pub fn expand_macro_types_impl_with_expander<F>(
    macros: &[crate::analysis::types::AnalyzedMacro],
    source: Option<&str>,
    binding_entries: &[BindingExpansionEntry],
    debug_env: Option<&mut EvalEnv>,
    scope: MacroExpansionScope,
    mut expand_field_expr: F,
) -> crate::analysis::type_expand::ExpandedComponentTypes
where
    F: FnMut(
        FieldExpansionContext,
        Option<&verter_type_expr::locators::MacroPayloadLocator>,
    ) -> crate::analysis::type_expand::ExpansionResult<
        crate::analysis::type_expand::ExpandedNormalizedExpr,
    >,
{
    use crate::analysis::type_expand::{ExpandedComponentTypes, ExpandedField};

    let mut result = ExpandedComponentTypes::default();
    let started = Instant::now();
    let start_steps = debug_env.as_deref().map(EvalEnv::steps).unwrap_or(0);

    type_expand_debug(|| {
        format!(
            "expand_macro_types:start macros={} source_present={} local_binding_filter={} steps={}",
            macros.len(),
            source.is_some(),
            binding_entries.len(),
            start_steps,
        )
    });

    for (macro_index, m) in macros.iter().enumerate() {
        // Expand prop field type annotations.
        //
        // The analyzer producer (`extract_fields_from_interface_body_like`)
        // lowers each prop's TS annotation directly from the OXC `TSType<'_>`
        // AST node and stores the result on `AnalyzedPropField.type_expr`.
        // Consumers read the typed form authoritatively — no string parsing.
        for field in &m.prop_fields {
            if let Some(ref payload) = field.payload {
                {
                    let item_started = Instant::now();
                    let stage_log = ExpandStageLog {
                        macro_index,
                        macro_kind: m.kind,
                        stage: "prop_field",
                        target: field.name.as_str(),
                        started: item_started,
                        start_steps: debug_env.as_deref().map(EvalEnv::steps).unwrap_or(0),
                    };
                    log_expand_stage_start(&stage_log);
                    let ctx = FieldExpansionContext {
                        kind: FieldKind::Prop,
                        macro_index,
                        scope_owner: m.owner,
                        output_path: std::sync::Arc::from(vec![PathSegment::Member(
                            std::sync::Arc::from(field.name.as_str()),
                        )]),
                    };
                    let expanded = expand_field_expr(ctx, Some(payload));
                    log_expand_stage(
                        stage_log,
                        expanded.exactness,
                        expanded.execution_status,
                        &expanded.diagnostics,
                        debug_env.as_deref(),
                    );
                    let shallow_source = Some(
                        verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(
                            payload.clone(),
                        ),
                    );
                    result.props.push(ExpandedField {
                        name: field.name.clone(),
                        r#type: verter_type_expr::facts::SourcePosition::Present(
                            expanded.value.expr,
                        ),
                        raw_type: field.type_annotation.clone(),
                        optional: field.is_optional,
                        exactness: expanded.exactness,
                        execution_status: expanded.execution_status,
                        diagnostics: expanded.diagnostics,
                        shallow_source,
                        declared_in_macro_type_arg: field.declared_in_macro_type_arg,
                    });
                }
            }
        }

        // NOTE: defineProps<T>(), defineEmits<T>(), defineSlots<T>() object-shape
        // production is owned by the query-engine phase in meta_resolve.rs.
        // This function handles field-level work only.

        // Expand emit payload types via the analyzer-populated typed form.
        for field in &m.emit_fields {
            if let Some(ref payload) = field.payload {
                {
                    let item_started = Instant::now();
                    let stage_log = ExpandStageLog {
                        macro_index,
                        macro_kind: m.kind,
                        stage: "emit_field",
                        target: field.name.as_str(),
                        started: item_started,
                        start_steps: debug_env.as_deref().map(EvalEnv::steps).unwrap_or(0),
                    };
                    log_expand_stage_start(&stage_log);
                    let ctx = FieldExpansionContext {
                        kind: FieldKind::Emit,
                        macro_index,
                        scope_owner: m.owner,
                        output_path: std::sync::Arc::from(vec![PathSegment::Member(
                            std::sync::Arc::from(field.name.as_str()),
                        )]),
                    };
                    let expanded = expand_field_expr(ctx, Some(payload));
                    log_expand_stage(
                        stage_log,
                        expanded.exactness,
                        expanded.execution_status,
                        &expanded.diagnostics,
                        debug_env.as_deref(),
                    );
                    let shallow_source = Some(
                        verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(
                            payload.clone(),
                        ),
                    );
                    result.emits.push(ExpandedField {
                        name: field.name.clone(),
                        r#type: verter_type_expr::facts::SourcePosition::Present(
                            expanded.value.expr,
                        ),
                        raw_type: field.payload_type.clone(),
                        optional: false,
                        exactness: expanded.exactness,
                        execution_status: expanded.execution_status,
                        diagnostics: expanded.diagnostics,
                        shallow_source,
                        // `AnalyzedEmitField` is the upstream type at this
                        // layer. It carries `name`, `payload_type`, and
                        // `payload_expr` — not own-body-vs-heritage
                        // provenance. The published-surface policies
                        // (`Refined` etc.) consult the bit only on the
                        // `props` axis; the emit surface does not gate on
                        // it. `false` is the structural truth at the emit
                        // ExpandedField layer because the producer type
                        // does not encode the distinction.
                        declared_in_macro_type_arg: false,
                    });
                }
            }
        }

        // Slot binding expansion is not needed for fallthrough-only meta.
        // Read the authored payload position emitted by the analyzer producer
        // in `extract_slot_bindings_from_oxc_type`.
        if scope == MacroExpansionScope::Full {
            for slot in &m.slot_fields {
                for binding in &slot.bindings {
                    if let Some(ref payload) = binding.payload {
                        {
                            let item_started = Instant::now();
                            let slot_binding_target = format!("{}.{}", slot.name, binding.name);
                            let stage_log = ExpandStageLog {
                                macro_index,
                                macro_kind: m.kind,
                                stage: "slot_binding",
                                target: slot_binding_target.as_str(),
                                started: item_started,
                                start_steps: debug_env.as_deref().map(EvalEnv::steps).unwrap_or(0),
                            };
                            log_expand_stage_start(&stage_log);
                            let ctx = FieldExpansionContext {
                                kind: FieldKind::SlotBinding,
                                macro_index,
                                scope_owner: m.owner,
                                output_path: std::sync::Arc::from(vec![
                                    PathSegment::Member(std::sync::Arc::from(slot.name.as_str())),
                                    PathSegment::Member(std::sync::Arc::from(
                                        binding.name.as_str(),
                                    )),
                                ]),
                            };
                            let expanded = expand_field_expr(ctx, Some(payload));
                            log_expand_stage(
                                stage_log,
                                expanded.exactness,
                                expanded.execution_status,
                                &expanded.diagnostics,
                                debug_env.as_deref(),
                            );
                            let shallow_source = Some(
                                verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(
                                    payload.clone(),
                                ),
                            );
                            result.slot_bindings.push(ExpandedField {
                                name: slot_binding_target,
                                r#type: verter_type_expr::facts::SourcePosition::Present(
                                    expanded.value.expr,
                                ),
                                raw_type: binding.type_annotation.clone(),
                                optional: false,
                                exactness: expanded.exactness,
                                execution_status: expanded.execution_status,
                                diagnostics: expanded.diagnostics,
                                shallow_source,
                                // SAFETY: slot bindings are positional
                                // parameters of a slot's function signature
                                // (not declared members of the macro T's own
                                // body). The fact is meaningful at the slot
                                // level, not the binding level — defining
                                // `declared_in_macro_type_arg = false` here
                                // is the structural truth.
                                declared_in_macro_type_arg: false,
                            });
                        }
                    }
                }
            }
        }
    }

    // Expose/value binding expansion is not needed for fallthrough-only meta.
    if scope == MacroExpansionScope::Full {
        for entry in binding_entries {
            let name = &entry.name;
            let item_started = Instant::now();
            let stage_log = ExpandStageLog {
                macro_index: usize::MAX,
                macro_kind: crate::analysis::types::AnalyzedMacroKind::DefineExpose,
                stage: "binding",
                target: name.as_str(),
                started: item_started,
                start_steps: debug_env.as_deref().map(EvalEnv::steps).unwrap_or(0),
            };
            log_expand_stage_start(&stage_log);
            // `defineExpose` binding entries are top-level value bindings
            // in the script-setup scope — there is no parent macro shell and
            // no authored macro-payload locator (the payload argument is
            // `None`). `kind: Binding` plus the binding NAME on `output_path`
            // tell the closure to resolve the top-level value binding by
            // name. `macro_index` carries the sentinel `usize::MAX` used
            // elsewhere for non-macro-anchored expose entries.
            let ctx = FieldExpansionContext {
                kind: FieldKind::Binding,
                macro_index: usize::MAX,
                scope_owner: entry.owner,
                output_path: std::sync::Arc::from(vec![PathSegment::Member(std::sync::Arc::from(
                    name.as_str(),
                ))]),
            };
            let expanded = expand_field_expr(ctx, None);
            log_expand_stage(
                stage_log,
                expanded.exactness,
                expanded.execution_status,
                &expanded.diagnostics,
                debug_env.as_deref(),
            );
            // `defineExpose` binding entries are top-level value bindings
            // outside any macro T (no declared/heritage distinction applies,
            // and no analyzer-side authored shallow source exists).
            result.bindings.push(ExpandedField {
                name: name.clone(),
                r#type: verter_type_expr::facts::SourcePosition::Present(expanded.value.expr),
                raw_type: None,
                optional: false,
                exactness: expanded.exactness,
                execution_status: expanded.execution_status,
                diagnostics: expanded.diagnostics,
                shallow_source: None,
                declared_in_macro_type_arg: false,
            });
        }
    }

    type_expand_debug(|| {
        format!(
            "expand_macro_types:end props={} define_props={} define_emits={} emits={} define_slots={} slot_bindings={} bindings={} steps_delta={} budget_exhausted={} took {:?}",
            result.props.len(),
            result.define_props.len(),
            result.define_emits.len(),
            result.emits.len(),
            result.define_slots.len(),
            result.slot_bindings.len(),
            result.bindings.len(),
            debug_env
                .as_deref()
                .map(|env| env.steps().saturating_sub(start_steps))
                .unwrap_or(0),
            debug_env
                .as_deref()
                .map(EvalEnv::budget_exhausted)
                .unwrap_or(false),
            started.elapsed(),
        )
    });

    result
}

pub fn has_named_shape_surface(shape: &crate::analysis::type_expand::ExpandedObjectShape) -> bool {
    !shape.properties.is_empty() || !shape.call_signatures.is_empty()
}

// ---------------------------------------------------------------------------
// Public convenience: parse source and build env
// ---------------------------------------------------------------------------

/// Parse a TypeScript source string and build an inventory environment.
///
/// This is a convenience function for tests and standalone usage.
/// In production, use `build_eval_env` with a pre-parsed OXC program and the
/// producing file's real canonical id; this convenience anchors at a
/// deterministic inline-fixture canonical.
pub fn parse_and_build_env(source: &str) -> EvalEnv {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, source, source_type).parse();
    build_eval_env(
        &ret.program,
        source,
        &BuildEvalEnvContext::new("inline:parse-and-build-env"),
    )
}

/// The TRANSIENT per-file lowering view: every declaration's fully-lowered
/// typed-IR parts, in registration order, BEFORE fact minting. Produced by the
/// SAME statement lowering [`build_eval_env`] registers through
/// ([`lower_statement_parts`]), so lowering tests characterize exactly the
/// typed IR the fact minting consumes. Never stored on any cache or inventory.
#[derive(Debug, Clone, Default)]
pub struct LoweredFileParts {
    pub type_decls: Vec<LoweredTypeDeclParts>,
    pub value_decls: Vec<LoweredValueDeclParts>,
    pub aug_type_decls: Vec<(AugmentationScopeKind, LoweredTypeDeclParts)>,
    pub aug_value_decls: Vec<(AugmentationScopeKind, LoweredValueDeclParts)>,
}

impl LoweredFileParts {
    /// The LAST registered file-scope type decl parts named `name` — the
    /// last-wins representative, mirroring `TypeDeclGroup::primary`.
    pub fn type_decl(&self, name: &str) -> Option<&LoweredTypeDeclParts> {
        self.type_decls
            .iter()
            .rev()
            .find(|parts| parts.name == name)
    }

    /// Every file-scope type contributor named `name`, in registration order.
    pub fn type_contributors(&self, name: &str) -> Vec<&LoweredTypeDeclParts> {
        self.type_decls
            .iter()
            .filter(|parts| parts.name == name)
            .collect()
    }

    /// The LAST registered file-scope value decl parts named `name`.
    pub fn value_decl(&self, name: &str) -> Option<&LoweredValueDeclParts> {
        self.value_decls
            .iter()
            .rev()
            .find(|parts| parts.name == name)
    }

    /// The LAST registered augmentation-scoped type decl parts under
    /// `(scope, name)`.
    pub fn aug_type_decl(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<&LoweredTypeDeclParts> {
        self.aug_type_decls
            .iter()
            .rev()
            .find(|(s, parts)| s == scope && parts.name == name)
            .map(|(_, parts)| parts)
    }

    /// Every augmentation-scoped type contributor under `(scope, name)`, in
    /// registration order.
    pub fn aug_type_contributors(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Vec<&LoweredTypeDeclParts> {
        self.aug_type_decls
            .iter()
            .filter(|(s, parts)| s == scope && parts.name == name)
            .map(|(_, parts)| parts)
            .collect()
    }

    /// The LAST registered augmentation-scoped value decl parts under
    /// `(scope, name)`.
    pub fn aug_value_decl(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<&LoweredValueDeclParts> {
        self.aug_value_decls
            .iter()
            .rev()
            .find(|(s, parts)| s == scope && parts.name == name)
            .map(|(_, parts)| parts)
    }
}

/// Parse a TypeScript source string and lower every declaration to its
/// TRANSIENT typed-IR parts through the SAME statement arms
/// [`parse_and_build_env`] registers through — including the JSDoc `@typedef`
/// registration under TS-decl precedence. In-crate lowering-test support; the
/// returned parts are the pre-fact-minting view and are never stored.
pub fn parse_and_lower_parts(source: &str) -> LoweredFileParts {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

    let mut out = LoweredFileParts::default();
    for stmt in &ret.program.body {
        let parts = lower_statement_parts(stmt, source);
        out.type_decls.extend(parts.type_decls);
        out.value_decls.extend(parts.value_decls);
        out.aug_type_decls.extend(parts.aug_type_decls);
        out.aug_value_decls.extend(parts.aug_value_decls);
    }
    // JSDoc `@typedef {T} Name` registration mirrors `build_eval_env`: after
    // the statement walk, under TS-decl precedence (a name a TS declaration
    // already claimed is skipped).
    for typedef in crate::analysis::jsdoc::collect_jsdoc_typedefs(&ret.program.comments, source) {
        if out
            .type_decls
            .iter()
            .any(|parts| parts.name == typedef.name)
        {
            continue;
        }
        out.type_decls.push(LoweredTypeDeclParts {
            name: typedef.name,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: typedef.body,
            direct_member_return_inference: Vec::new(),
        });
    }
    out
}

/// Parse a JavaScript/TypeScript value expression into a lightweight [`TypeExpr`].
///
/// This preserves finite string literals, object-literal top-level shapes, identifier
/// references via `typeof`, and conditional unions needed by the shared host-side
/// fallthrough resolver.
pub fn parse_value_expression_type(expression: &str) -> Option<TypeExpr> {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let wrapped = format!("const __verter_expr__ = {expression};");
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &wrapped, SourceType::ts()).parse();
    let stmt = ret.program.body.first()?;
    let Statement::VariableDeclaration(decl) = stmt else {
        return None;
    };
    let declarator = decl.declarations.first()?;
    let init = declarator.init.as_ref()?;
    lower_value_expression(init, &wrapped).ok()
}

fn lower_value_expression(expr: &Expression<'_>, source: &str) -> InferenceResult<TypeExpr> {
    infer_expression_type(expr, source)
}
