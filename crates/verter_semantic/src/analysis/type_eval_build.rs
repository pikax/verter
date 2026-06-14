//! Build an [`EvalEnv`] from an OXC program AST.
//!
//! Walks top-level declarations and populates the type and value
//! symbol tables so the evaluator can resolve references.

use std::io::Write;
use std::sync::{Arc, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::analysis::type_eval::*;
use oxc_ast::ast::{
    Argument, ArrowFunctionExpression, BinaryOperator, BindingPattern, CallExpression, Class,
    ClassElement, Declaration, ExportDefaultDeclarationKind, Expression, FormalParameters,
    Function, MethodDefinitionKind, ObjectExpression, ObjectPropertyKind, Program, Statement,
    TSAccessibility, TSEnumDeclaration, TSGlobalDeclaration, TSInterfaceDeclaration, TSModuleBlock,
    TSModuleDeclaration, TSModuleDeclarationBody, TSModuleDeclarationName, TSSignature,
    TSTypeAliasDeclaration, TSTypeParameterDeclaration, UnaryOperator, VariableDeclarationKind,
    VariableDeclarator,
};
use oxc_span::GetSpan;
use verter_type_expr::{
    FunctionExpr, FunctionParam, FunctionSpans, IndexSignature, IndexSignatureSpans, MemberSpans,
    MemberVisibility, MethodSignature, ObjectExpr, ObjectMember, PrimitiveName, TypeExpr,
    TypeExprScope, TypeParam, ValueRef,
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

/// Build an evaluation environment from an OXC program AST.
///
/// Extracts:
/// - Type aliases → `TypeDeclInfo`
/// - Interfaces → `TypeDeclInfo`
/// - Classes → `TypeDeclInfo` (body from constructor/public members)
/// - Functions → `ValueDeclInfo` with function signatures
/// - Variable declarations → `ValueDeclInfo` with type annotations / object shapes
pub fn build_eval_env(program: &Program<'_>, source: &str) -> EvalEnv {
    let mut env = EvalEnv::new();

    for stmt in &program.body {
        lower_top_level_statement(stmt, source, &mut env);
    }

    // JSDoc `@typedef {T} Name` declarations are first-class REGULAR types: a
    // `/** @typedef {{a: number}} Alias */` block declares `Alias` exactly like
    // a TS `type Alias = { a: number }`. Register them on the SAME type-symbol
    // registry the TS declarations above populated, so a later `@type {Alias}`
    // or bare `Alias` reference resolves through the shared dispatch with no
    // JSDoc-specific path. This runs AFTER the statement walk so a real TS
    // declaration of the same name always wins (TS-decl precedence).
    register_jsdoc_typedefs(&program.comments, source, &mut env);

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
pub fn lower_top_level_statement(stmt: &Statement<'_>, source: &str, env: &mut EvalEnv) {
    match stmt {
        Statement::TSTypeAliasDeclaration(decl) => {
            extract_type_alias(decl, source, env);
        }
        Statement::TSInterfaceDeclaration(decl) => {
            extract_interface(decl, source, env);
        }
        Statement::TSModuleDeclaration(module) => {
            extract_module_declaration(module, source, env, None);
        }
        Statement::TSGlobalDeclaration(global) => {
            extract_global_declaration(global, source, env);
        }
        Statement::ClassDeclaration(decl) => {
            extract_class(decl, source, env);
        }
        Statement::TSEnumDeclaration(decl) => {
            extract_enum(decl, env);
        }
        Statement::FunctionDeclaration(func) => {
            extract_function(func, source, env);
        }
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                extract_variable(decl, var_decl.kind, source, env, None);
            }
        }
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                extract_from_declaration(decl, source, env);
            }
        }
        Statement::ExportDefaultDeclaration(export) => match &export.declaration {
            ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                extract_function(func, source, env);
            }
            ExportDefaultDeclarationKind::ClassDeclaration(cls) => {
                extract_class(cls, source, env);
                // `export default class Props { … }` exports the class under
                // the `default` export name (the named identifier is NOT a
                // separate export — see ShallowFileState's default-export
                // contract), but `extract_class` keys the instance shape under
                // the declared name `Props`. A barrel that reaches this file
                // resolves the `(canonical, "default")` route, so the class
                // body must also be reachable under `default`. Alias the
                // declared-name type symbol into a `default` entry (same body,
                // same params) so the prepared-decl lookup at the resolved
                // default route hydrates the class.
                if let Some(name) = class_or_function_default_name(&cls.id) {
                    alias_default_export_type_symbol(env, &name);
                }
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(iface) => {
                extract_interface(iface, source, env);
                alias_default_export_type_symbol(env, iface.id.name.as_str());
            }
            other => {
                if let Some(expr) = other.as_expression() {
                    extract_default_expression(expr, source, env);
                }
            }
        },
        _ => {}
    }
}

/// Register the JSDoc `@typedef {T} Name` declaration named `name` into
/// `env`, applying the same TS-decl precedence as the whole-env walk: a
/// name a TS declaration already claimed in `env` is skipped. Returns
/// `true` when a typedef body was registered.
///
/// The selective counterpart to the whole-env typedef registration inside
/// [`build_eval_env`] — a demanded symbol that exists only as a `@typedef`
/// lowers exactly its own `{T}` payload.
pub fn lower_jsdoc_typedef_named(
    comments: &[oxc_ast::Comment],
    source: &str,
    name: &str,
    env: &mut EvalEnv,
) -> bool {
    if env.type_symbols.contains_key(name) {
        return false;
    }
    for typedef in crate::analysis::jsdoc::collect_jsdoc_typedefs(comments, source) {
        if typedef.name != name {
            continue;
        }
        env.add_type(TypeDeclInfo {
            name: typedef.name,
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: typedef.body,
        });
        return true;
    }
    false
}

/// Register each JSDoc `@typedef {T} Name` from the program's comments as a
/// `TypeDeclInfo` alias, skipping any name a TS declaration already claimed
/// (TS-decl precedence).
fn register_jsdoc_typedefs(comments: &[oxc_ast::Comment], source: &str, env: &mut EvalEnv) {
    for typedef in crate::analysis::jsdoc::collect_jsdoc_typedefs(comments, source) {
        if env.type_symbols.contains_key(&typedef.name) {
            // A real TS `type`/`interface`/`class` of this name was registered
            // during the statement walk; it is authoritative.
            continue;
        }
        env.add_type(TypeDeclInfo {
            name: typedef.name,
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: typedef.body,
        });
    }
}

fn extract_from_declaration(decl: &Declaration<'_>, source: &str, env: &mut EvalEnv) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            extract_type_alias(alias, source, env);
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            extract_interface(iface, source, env);
        }
        Declaration::TSModuleDeclaration(module) => {
            extract_module_declaration(module, source, env, None);
        }
        Declaration::TSGlobalDeclaration(global) => {
            extract_global_declaration(global, source, env);
        }
        Declaration::ClassDeclaration(cls) => {
            extract_class(cls, source, env);
        }
        Declaration::TSEnumDeclaration(decl) => {
            extract_enum(decl, env);
        }
        Declaration::FunctionDeclaration(func) => {
            extract_function(func, source, env);
        }
        Declaration::VariableDeclaration(var_decl) => {
            for d in &var_decl.declarations {
                extract_variable(d, var_decl.kind, source, env, None);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Type declarations
// ---------------------------------------------------------------------------

fn extract_type_alias(decl: &TSTypeAliasDeclaration<'_>, source: &str, env: &mut EvalEnv) {
    let name = decl.id.name.to_string();
    env.add_type(build_named_type_alias_decl(decl, source, name));
}

fn build_named_type_alias_decl(
    decl: &TSTypeAliasDeclaration<'_>,
    source: &str,
    name: String,
) -> TypeDeclInfo {
    let type_parameters = decl
        .type_parameters
        .as_ref()
        .map(|tp| lower_type_param_decls(tp, source))
        .unwrap_or_default();
    let body = lower_ts_type(&decl.type_annotation, source);

    TypeDeclInfo {
        name,
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters,
        body,
    }
}

fn extract_interface(decl: &TSInterfaceDeclaration<'_>, source: &str, env: &mut EvalEnv) {
    let name = decl.id.name.to_string();
    env.add_type(build_named_interface_decl(decl, source, name));
}

fn build_named_interface_decl(
    decl: &TSInterfaceDeclaration<'_>,
    source: &str,
    name: String,
) -> TypeDeclInfo {
    let type_parameters = decl
        .type_parameters
        .as_ref()
        .map(|tp| lower_type_param_decls(tp, source))
        .unwrap_or_default();

    // Build the body from the interface members
    let mut members = Vec::new();
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
        let mut parts = Vec::new();
        for heritage in &decl.extends {
            let base_name = match &heritage.expression {
                Expression::Identifier(id) => id.name.to_string(),
                _ => continue,
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

    TypeDeclInfo {
        name,
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters,
        body,
    }
}

fn extract_module_declaration(
    decl: &TSModuleDeclaration<'_>,
    source: &str,
    env: &mut EvalEnv,
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
            extract_augmentation_block(
                block,
                source,
                env,
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
            extract_module_declaration(inner, source, env, Some(module_name.as_str()));
        }
        TSModuleDeclarationBody::TSModuleBlock(block) => {
            for stmt in &block.body {
                extract_namespaced_statement(stmt, source, env, module_name.as_str());
            }
        }
    }
}

/// Retain the inner declarations of an ambient augmentation block
/// (`declare module "X" { ... }` or `declare global { ... }`) into the scoped
/// augmentation inventory under `scope`. Inner interfaces/type-aliases keep
/// their UNQUALIFIED names (an augmenter contributes `interface Config`, not
/// `external-spec.Config`) and never enter file-scope `type_symbols`.
fn extract_augmentation_block(
    block: &TSModuleBlock<'_>,
    source: &str,
    env: &mut EvalEnv,
    scope: AugmentationScopeKind,
) {
    for stmt in &block.body {
        match stmt {
            Statement::TSInterfaceDeclaration(iface) => {
                let name = iface.id.name.to_string();
                env.add_augmentation_type(
                    scope.clone(),
                    build_named_interface_decl(iface, source, name),
                );
            }
            Statement::TSTypeAliasDeclaration(alias) => {
                let name = alias.id.name.to_string();
                env.add_augmentation_type(
                    scope.clone(),
                    build_named_type_alias_decl(alias, source, name),
                );
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = export.declaration.as_ref() {
                    extract_augmentation_declaration(decl, source, env, &scope);
                }
            }
            // Value-space declarations (`const`/`let`/`var`, `function`,
            // `class`) augment the target module's VALUE surface. Reuse the
            // file-scope extractors into a throwaway env so the full retained
            // body is built exactly as for a top-level declaration, then move
            // the produced value declarations into the augmentation value
            // scope (never file-scope `value_symbols`).
            Statement::VariableDeclaration(_)
            | Statement::FunctionDeclaration(_)
            | Statement::ClassDeclaration(_) => {
                retain_value_statement_into_augmentation(stmt, source, env, &scope);
            }
            _ => {}
        }
    }
}

/// Route a `Declaration` inside an ambient augmentation block to the correct
/// augmentation inventory: interfaces / type-aliases to the type scope, value
/// declarations to the value scope (via a throwaway env).
fn extract_augmentation_declaration(
    decl: &Declaration<'_>,
    source: &str,
    env: &mut EvalEnv,
    scope: &AugmentationScopeKind,
) {
    match decl {
        Declaration::TSInterfaceDeclaration(iface) => {
            let name = iface.id.name.to_string();
            env.add_augmentation_type(
                scope.clone(),
                build_named_interface_decl(iface, source, name),
            );
        }
        Declaration::TSTypeAliasDeclaration(alias) => {
            let name = alias.id.name.to_string();
            env.add_augmentation_type(
                scope.clone(),
                build_named_type_alias_decl(alias, source, name),
            );
        }
        Declaration::VariableDeclaration(_)
        | Declaration::FunctionDeclaration(_)
        | Declaration::ClassDeclaration(_) => {
            let mut tmp = EvalEnv::new();
            extract_from_declaration(decl, source, &mut tmp);
            move_value_symbols_into_augmentation(tmp, env, scope);
        }
        _ => {}
    }
}

/// Reuse the file-scope extractors (via a throwaway env) to build the full
/// retained value declaration(s) for a value-space statement, then move them
/// into the augmentation value scope.
fn retain_value_statement_into_augmentation(
    stmt: &Statement<'_>,
    source: &str,
    env: &mut EvalEnv,
    scope: &AugmentationScopeKind,
) {
    let mut tmp = EvalEnv::new();
    match stmt {
        Statement::ClassDeclaration(decl) => extract_class(decl, source, &mut tmp),
        Statement::FunctionDeclaration(func) => extract_function(func, source, &mut tmp),
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                extract_variable(decl, var_decl.kind, source, &mut tmp, None);
            }
        }
        _ => {}
    }
    move_value_symbols_into_augmentation(tmp, env, scope);
}

/// Drain the value declarations a throwaway env collected and append them to
/// the augmentation value scope (the type side a `class` also produces is
/// intentionally dropped — an ambient `declare module` class augments the
/// value surface; its instance type is not stitched cross-file today).
fn move_value_symbols_into_augmentation(
    tmp: EvalEnv,
    env: &mut EvalEnv,
    scope: &AugmentationScopeKind,
) {
    for (_name, group) in tmp.value_symbols {
        for decl in group.contributors {
            env.add_augmentation_value(scope.clone(), decl);
        }
    }
}

/// Retain a `declare global { ... }` block's inner declarations under the
/// global augmentation scope.
fn extract_global_declaration(decl: &TSGlobalDeclaration<'_>, source: &str, env: &mut EvalEnv) {
    extract_augmentation_block(&decl.body, source, env, AugmentationScopeKind::Global);
}

fn extract_namespaced_statement(
    stmt: &Statement<'_>,
    source: &str,
    env: &mut EvalEnv,
    namespace: &str,
) {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => {
            env.add_type(build_named_type_alias_decl(
                alias,
                source,
                qualified_name(namespace, &alias.id.name),
            ));
        }
        Statement::TSInterfaceDeclaration(iface) => {
            env.add_type(build_named_interface_decl(
                iface,
                source,
                qualified_name(namespace, &iface.id.name),
            ));
        }
        Statement::TSModuleDeclaration(module) => {
            extract_module_declaration(module, source, env, Some(namespace));
        }
        // Namespace value indexing is EXPORT-ONLY: a non-exported
        // `namespace N { const hidden = … }` is private to the namespace body
        // (TS: `N.hidden` does not exist on `typeof N`), so a DIRECT
        // `Statement::VariableDeclaration` is intentionally NOT indexed under
        // its qualified name. Only the exported path below
        // (`export const VERSION = …` → `extract_namespaced_declaration`)
        // registers a qualified value member such as `N.VERSION`.
        Statement::ExportNamedDeclaration(export) => {
            if let Some(ref decl) = export.declaration {
                extract_namespaced_declaration(decl, source, env, namespace);
            }
        }
        _ => {}
    }
}

fn extract_namespaced_declaration(
    decl: &Declaration<'_>,
    source: &str,
    env: &mut EvalEnv,
    namespace: &str,
) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            env.add_type(build_named_type_alias_decl(
                alias,
                source,
                qualified_name(namespace, &alias.id.name),
            ));
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            env.add_type(build_named_interface_decl(
                iface,
                source,
                qualified_name(namespace, &iface.id.name),
            ));
        }
        Declaration::TSModuleDeclaration(module) => {
            extract_module_declaration(module, source, env, Some(namespace));
        }
        // A namespaced value member (`namespace NS { export const M = … }`)
        // registers under its QUALIFIED name `NS.M` so `typeof NS.M` binds.
        Declaration::VariableDeclaration(var_decl) => {
            for declarator in &var_decl.declarations {
                extract_variable(declarator, var_decl.kind, source, env, Some(namespace));
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
/// name. The cloned [`TypeDeclInfo`] carries the SAME body / params (only the
/// `name` key changes to `default`); it is a no-op when the declared symbol was
/// not registered (e.g. an empty class body produced no type symbol).
fn alias_default_export_type_symbol(env: &mut EvalEnv, declared_name: &str) {
    if env.type_symbols.contains_key("default") {
        return;
    }
    let Some(group) = env.type_symbols.get(declared_name) else {
        return;
    };
    let decl = group.primary();
    let aliased = TypeDeclInfo {
        name: "default".to_string(),
        declaration_id: 0,
        kind: decl.kind,
        type_parameters: decl.type_parameters.clone(),
        body: decl.body.clone(),
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

fn extract_class(decl: &Class<'_>, source: &str, env: &mut EvalEnv) {
    let name = match &decl.id {
        Some(id) => id.name.to_string(),
        None => return,
    };

    // Extract the public instance shape AND the value-side static surface
    // from the class body. Instance members go to the TYPE-space body;
    // static members ride INSIDE the value-side constructor-shape
    // `ObjectExpr` (the `typeof C` constructor-object model) next to the
    // `ConstructSignature` — never a separate field.
    let mut members = Vec::new();
    let mut static_members = Vec::new();
    let mut ctor_sig = None;
    let mut ctor_fn_spans = FunctionSpans::default();

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
                        .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
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
                        static_members.push(ObjectMember::Method(
                            MethodSignature::with_visibility(
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
                            ),
                        ));
                    }
                } else if method.kind == MethodDefinitionKind::Constructor {
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
                    members.push(ObjectMember::Method(MethodSignature::with_visibility(
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
                    )));
                }
            }
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
    // carried only its own members and the cross-file heritage was dropped
    // by every body-driven surface reader (the eager OXC rail folds class
    // heritage separately via `resolve_class_with_heritage_ctx_ref` in
    // `verter_parser`'s `utils/oxc/vue/script/resolve_type/decl.rs`; this
    // is the typed-IR producer parity).
    let own_body = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }));
    let body = match &decl.super_class {
        Some(Expression::Identifier(base_id)) => {
            let base_name = base_id.name.to_string();
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

    env.add_type(TypeDeclInfo {
        name: name.clone(),
        declaration_id: 0,
        kind: TypeDeclKind::Class,
        type_parameters,
        body,
    });

    // Also register as a value (for typeof ClassName / InstanceType)
    let mut constructor_signature = ctor_sig.clone().unwrap_or_else(|| FunctionSignature {
        parameters: Vec::new(),
        return_type: Some(TypeExpr::named(name.clone())),
        type_parameters: Vec::new(),
        has_implementation_body: true,
    });
    // A DECLARED constructor carries no return annotation — its construct
    // "return" IS the class instance. Backfill the instance reference so
    // `InstanceType<typeof C>` reads the instance type from the construct
    // signature exactly as it does from the synthesized default.
    if constructor_signature.return_type.is_none() {
        constructor_signature.return_type = Some(TypeExpr::named(name.clone()));
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

    env.add_value(ValueDeclInfo {
        name,
        declaration_id: 0,
        kind: ValueDeclKind::Class,
        type_annotation: None,
        signatures: vec![constructor_signature],
        object_shape: Some(constructor_shape),
        enum_members: None,
    });
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
fn degraded_member_domain(initializer: Option<&Expression<'_>>) -> TypeExpr {
    let number = || TypeExpr::Primitive(PrimitiveName::Number);
    let string = || TypeExpr::Primitive(PrimitiveName::String);
    let unknown = || TypeExpr::Primitive(PrimitiveName::Unknown);
    let Some(expr) = initializer else {
        // A bare member is only deferred when the running auto-increment value
        // is unknown; the auto-increment series is always NUMERIC.
        return number();
    };
    match expr {
        // A plain string or template literal (NO tag) is a string-valued
        // expression. A TAGGED template (`tag`...``) is deliberately EXCLUDED:
        // it is a call to `tag`, which can return any type, so `string` is not a
        // sound bound — it falls to the `_ => unknown()` arm below.
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => string(),
        Expression::NumericLiteral(_) => number(),
        Expression::UnaryExpression(unary) => match unary.operator {
            UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus | UnaryOperator::BitwiseNot => {
                number()
            }
            // `!x` (boolean), `typeof`/`void`/`delete` — not a sound numeric or
            // string enum value; no narrower domain than `unknown` is provable.
            _ => unknown(),
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
            | BinaryOperator::Exponential => number(),
            // `+` is numeric add OR string concat — the soundest bound is the
            // union of both.
            BinaryOperator::Addition => TypeExpr::union(vec![number(), string()]),
            // Comparison / logical / `in` / `instanceof` produce booleans —
            // never a sound enum value.
            _ => unknown(),
        },
        // A parenthesized wrapper carries no domain of its own — classify the
        // inner expression (`A = (1 << 2)` is still `number`).
        Expression::ParenthesizedExpression(paren) => {
            degraded_member_domain(Some(&paren.expression))
        }
        // Member-reference, call, identifier, anything else — unprovable here.
        _ => unknown(),
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
fn extract_enum(decl: &TSEnumDeclaration<'_>, env: &mut EvalEnv) {
    let name = decl.id.name.to_string();

    // The ordered member inventory: the NAME of EVERY statically-named member
    // plus its [`EnumMemberValue`] — `Folded` when statically foldable,
    // `Deferred` (carrying the degraded sound domain) otherwise. See the
    // `ValueDeclInfo::enum_members` field doc for the rail contract (the NAME
    // set is the presence-rail authority; the `Folded` subset is the foldable
    // rail; every member's projected type drives the type surfaces). The NAME
    // set must equal what `index_enum` records.
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
                EnumMemberValue::Folded(TypeExpr::string_literal(s.value.as_str()))
            }
            Some(Expression::NumericLiteral(n)) => {
                next_auto = Some(n.value + 1.0);
                EnumMemberValue::Folded(TypeExpr::number_literal(n.value))
            }
            // TS represents a signed numeric initializer (`A = -1`, `A = +2`)
            // as a unary expression over a numeric literal. Fold it to the
            // signed literal and reseed the auto-increment counter from it.
            Some(Expression::UnaryExpression(unary)) => {
                match (unary.operator, &unary.argument) {
                    (UnaryOperator::UnaryNegation, Expression::NumericLiteral(n)) => {
                        next_auto = Some(-n.value + 1.0);
                        EnumMemberValue::Folded(TypeExpr::number_literal(-n.value))
                    }
                    (UnaryOperator::UnaryPlus, Expression::NumericLiteral(n)) => {
                        next_auto = Some(n.value + 1.0);
                        EnumMemberValue::Folded(TypeExpr::number_literal(n.value))
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
                    EnumMemberValue::Folded(TypeExpr::number_literal(assigned))
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
    env.add_value(ValueDeclInfo {
        name: name.clone(),
        declaration_id: 0,
        kind: ValueDeclKind::Enum,
        type_annotation: None,
        signatures: Vec::new(),
        object_shape: None,
        enum_members: Some(members),
    });

    // Type-space: the enum used AS A TYPE is the union of its members' projected
    // types (folded literals plus degraded primitive arms for unfoldable
    // members) — but that union is DERIVED from the MERGED value members by
    // `ValueDeclGroup::enum_type_union` (the single source of truth), because a
    // per-declaration walk here cannot see same-name merged contributors (an
    // eager union would be last-wins and drop earlier declarations' members).
    // So this registers only the dual-space TYPE binding (kind `Alias` — there
    // is no dedicated enum `TypeDeclKind`, and a union carries no nominal
    // identity Verter models) with a NON-SERVED placeholder body; the
    // lazily-served declaration-body memo overrides it with the derived union
    // on demand.
    env.add_type(TypeDeclInfo {
        name,
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters: Vec::new(),
        body: TypeExpr::Primitive(PrimitiveName::Never),
    });
}

fn extract_function(func: &Function<'_>, source: &str, env: &mut EvalEnv) {
    let (name, name_offset) = match &func.id {
        Some(id) => (id.name.to_string(), id.span.start),
        None => return,
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

    env.add_value(ValueDeclInfo {
        name,
        declaration_id: 0,
        kind,
        type_annotation: None,
        signatures: vec![sig],
        object_shape: None,
        enum_members: None,
    });
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
    sig: &mut FunctionSignature,
    source: &str,
    name_offset: u32,
    has_ts_return: bool,
) {
    enrich_params_and_return_with_jsdoc(
        &mut sig.parameters,
        &mut sig.return_type,
        source,
        name_offset,
        has_ts_return,
    );
}

/// Backfill a parameter list + return type from a leading JSDoc block, for the
/// parameters / return that carried NO TS annotation. The shared core both
/// [`FunctionSignature`] (function declarations / initializer signatures) and
/// an inferred [`FunctionExpr`] `type_annotation` (an arrow / function-
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
) {
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
        }
    }
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
    enrich_params_and_return_with_jsdoc(
        &mut function.parameters,
        &mut return_type,
        source,
        name_offset,
        has_ts_return,
    );
    function.return_type = return_type.map(Arc::new);
}

fn extract_variable(
    decl: &VariableDeclarator<'_>,
    kind: VariableDeclarationKind,
    source: &str,
    env: &mut EvalEnv,
    namespace: Option<&str>,
) {
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
        _ => return,
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

    // Extract function signature from arrow functions or function expressions
    let mut function_signature = None;
    let mut object_shape = None;

    if let Some(ref init) = decl.init {
        function_signature = extract_initializer_function_signature(init, source);
        object_shape = extract_initializer_object_shape(init, source, MemberLiteralPolicy::Widen);

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
            let mut inferred = infer_expression_type(init, source);
            if matches!(var_kind, ValueDeclKind::Let | ValueDeclKind::Var) {
                inferred = widen_literal_type(inferred);
            }
            // The inferred `type_annotation` is the carrier query-time projection
            // consumes first (it precedes `function_signature`). When inference
            // produced a function type from a function-expression initializer,
            // enrich THAT function's params/return from the same JSDoc so the
            // projected signature is JSDoc-typed (not the un-enriched inference).
            if let TypeExpr::Function(function) = &mut inferred {
                enrich_function_expr_with_jsdoc(function, source, name_offset, has_ts_return);
            }
            if !matches!(inferred, TypeExpr::Primitive(PrimitiveName::Any)) {
                type_annotation = Some(inferred);
            }
        }
    }

    env.add_value(ValueDeclInfo {
        name,
        declaration_id: 0,
        kind: var_kind,
        type_annotation,
        signatures: function_signature.into_iter().collect(),
        object_shape,
        enum_members: None,
    });
}

fn extract_default_expression(expr: &Expression<'_>, source: &str, env: &mut EvalEnv) {
    let function_signature = extract_initializer_function_signature(expr, source);
    let object_shape = extract_initializer_object_shape(expr, source, MemberLiteralPolicy::Widen);
    let type_annotation = Some(lower_value_expression(expr, source));

    env.add_value(ValueDeclInfo {
        name: "default".to_string(),
        declaration_id: 0,
        kind: ValueDeclKind::Const,
        type_annotation,
        signatures: function_signature.into_iter().collect(),
        object_shape,
        enum_members: None,
    });
}

fn extract_initializer_function_signature(
    expr: &Expression<'_>,
    source: &str,
) -> Option<FunctionSignature> {
    match expr {
        Expression::ArrowFunctionExpression(arrow) => Some(extract_arrow_signature(arrow, source)),
        Expression::FunctionExpression(func) => Some(extract_function_signature(func, source)),
        Expression::TSAsExpression(ts_as) => {
            extract_initializer_function_signature(&ts_as.expression, source)
        }
        Expression::TSSatisfiesExpression(sat) => {
            extract_initializer_function_signature(&sat.expression, source)
        }
        Expression::ParenthesizedExpression(paren) => {
            extract_initializer_function_signature(&paren.expression, source)
        }
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
) -> Option<ObjectExpr> {
    match expr {
        Expression::ObjectExpression(obj) => Some(extract_object_literal(obj, source, policy)),
        Expression::TSAsExpression(ts_as) => {
            // `… as const` establishes a const context for the underlying
            // object shape (properties keep literals + become `readonly`).
            let inner_policy =
                if is_const_assertion_type_expr(&lower_ts_type(&ts_as.type_annotation, source)) {
                    MemberLiteralPolicy::ConstAssert
                } else {
                    policy
                };
            extract_initializer_object_shape(&ts_as.expression, source, inner_policy)
        }
        Expression::TSSatisfiesExpression(sat) => {
            // `satisfies` preserves members without widening, unless an
            // enclosing `as const` already pinned the readonly context.
            let inner_policy = if policy == MemberLiteralPolicy::ConstAssert {
                MemberLiteralPolicy::ConstAssert
            } else {
                MemberLiteralPolicy::Preserve
            };
            extract_initializer_object_shape(&sat.expression, source, inner_policy)
        }
        Expression::ParenthesizedExpression(paren) => {
            extract_initializer_object_shape(&paren.expression, source, policy)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn extract_function_signature(func: &Function<'_>, source: &str) -> FunctionSignature {
    let parameters = lower_function_params(&func.params, source);
    let return_type = func
        .return_type
        .as_ref()
        .map(|rt| lower_ts_type(&rt.type_annotation, source))
        .or_else(|| {
            // Infer return type from function body return statements
            func.body
                .as_ref()
                .and_then(|body| infer_return_type(body, source))
        });
    let type_parameters = func
        .type_parameters
        .as_ref()
        .map(|tp| lower_type_param_decls(tp, source))
        .unwrap_or_default();

    FunctionSignature {
        parameters,
        return_type,
        type_parameters,
        has_implementation_body: func.body.is_some(),
    }
}

fn extract_arrow_signature(arrow: &ArrowFunctionExpression<'_>, source: &str) -> FunctionSignature {
    let parameters = lower_function_params(&arrow.params, source);
    let return_type = arrow
        .return_type
        .as_ref()
        .map(|rt| lower_ts_type(&rt.type_annotation, source))
        .or_else(|| {
            // Infer return type from arrow body
            if arrow.expression {
                // () => expr — the body is a single expression
                if let Some(oxc_ast::ast::Statement::ExpressionStatement(expr)) =
                    arrow.body.statements.first()
                {
                    return Some(infer_expression_type(&expr.expression, source));
                }
            }
            infer_return_type(&arrow.body, source)
        });
    let type_parameters = arrow
        .type_parameters
        .as_ref()
        .map(|tp| lower_type_param_decls(tp, source))
        .unwrap_or_default();

    // An arrow function always carries an implementation body (expression or
    // block form).
    FunctionSignature {
        parameters,
        return_type,
        type_parameters,
        has_implementation_body: true,
    }
}

fn extract_object_literal(
    obj: &ObjectExpression<'_>,
    source: &str,
    policy: MemberLiteralPolicy,
) -> ObjectExpr {
    let mut members = Vec::new();
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                if let Some(name) = property_key_name(&p.key) {
                    let (ty, readonly) = object_member_value(&p.value, source, policy);
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
    ObjectExpr {
        properties: members,
    }
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
) -> TypeExpr {
    let mut members = Vec::new();
    let mut spread_types: Vec<TypeExpr> = Vec::new();
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                if let Some(name) = property_key_name(&p.key) {
                    let (ty, readonly) = object_member_value(&p.value, source, policy);
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
                let spread_ty = infer_expression_type_ctx(&spread.argument, source, policy);
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
        own_obj
    } else if matches!(&own_obj, TypeExpr::Object(obj) if obj.properties.is_empty()) {
        TypeExpr::intersection(spread_types)
    } else {
        spread_types.push(own_obj);
        TypeExpr::Intersection(spread_types.into())
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

/// Infer the return type of a function body by scanning return statements.
///
/// Returns `Some(TypeExpr)` if all return statements return the same shape.
/// Returns `None` if the body has no returns or returns are too complex.
fn infer_return_type(body: &oxc_ast::ast::FunctionBody<'_>, source: &str) -> Option<TypeExpr> {
    let mut return_types: Vec<TypeExpr> = Vec::new();

    for stmt in &body.statements {
        collect_return_types(stmt, source, &mut return_types);
    }

    if return_types.is_empty() {
        return None;
    }

    // If all returns produce the same type, use it; otherwise union them
    if return_types.len() == 1 {
        Some(return_types.into_iter().next().unwrap())
    } else {
        Some(TypeExpr::union(return_types))
    }
}

fn collect_return_types(
    stmt: &oxc_ast::ast::Statement<'_>,
    source: &str,
    results: &mut Vec<TypeExpr>,
) {
    use oxc_ast::ast::Statement;

    match stmt {
        Statement::ReturnStatement(ret) => {
            if let Some(ref arg) = ret.argument {
                results.push(infer_expression_type(arg, source));
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                collect_return_types(s, source, results);
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_return_types(&if_stmt.consequent, source, results);
            if let Some(ref alt) = if_stmt.alternate {
                collect_return_types(alt, source, results);
            }
        }
        _ => {}
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

fn infer_expression_type(expr: &Expression<'_>, source: &str) -> TypeExpr {
    infer_expression_type_ctx(expr, source, MemberLiteralPolicy::Widen)
}

/// Whether an expression is a `… as const` assertion (seen through a
/// parenthesised wrapper). Drives object-literal property widening: an
/// `as const`-asserted property keeps its literal type and is `readonly`; a
/// bare-literal property widens to its primitive.
fn expr_is_const_asserted(expr: &Expression<'_>, source: &str) -> bool {
    match expr {
        Expression::TSAsExpression(ts_as) => {
            is_const_assertion_type_expr(&lower_ts_type(&ts_as.type_annotation, source))
        }
        Expression::ParenthesizedExpression(paren) => {
            expr_is_const_asserted(&paren.expression, source)
        }
        _ => false,
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
) -> (TypeExpr, bool) {
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
    let raw = infer_expression_type_ctx(value, source, value_policy);
    // Widen a fresh TOP-LEVEL literal only under a plain `Widen` context (no
    // per-property `as const`); `Preserve` (satisfies) and `ConstAssert` keep it.
    let ty = if value_policy == MemberLiteralPolicy::Widen {
        widen_shallow_literal(raw)
    } else {
        raw
    };
    (ty, readonly)
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
) -> TypeExpr {
    match expr {
        Expression::Identifier(ident) => TypeExpr::TypeOf(ValueRef {
            path: vec![ident.name.as_str().to_string()],
            type_args: Vec::new(),
        }),
        Expression::StringLiteral(s) => TypeExpr::string_literal(s.value.as_str()),
        Expression::NumericLiteral(n) => TypeExpr::number_literal(n.value),
        Expression::BooleanLiteral(b) => TypeExpr::boolean_literal(b.value),
        Expression::NullLiteral(_) => TypeExpr::Primitive(PrimitiveName::Null),
        Expression::ConditionalExpression(cond) => TypeExpr::union(vec![
            infer_expression_type_ctx(&cond.consequent, source, policy),
            infer_expression_type_ctx(&cond.alternate, source, policy),
        ]),
        Expression::ParenthesizedExpression(paren) => {
            infer_expression_type_ctx(&paren.expression, source, policy)
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
                        );
                    }
                    oxc_ast::ast::ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(expr) = element.as_expression() {
                            append_union_members(
                                &mut element_types,
                                infer_expression_type_ctx(expr, source, policy),
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
            TypeExpr::Array {
                element: Arc::new(element),
                readonly: false,
            }
        }
        Expression::ObjectExpression(obj) => extract_object_literal_as_type(obj, source, policy),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            let mut value = String::new();
            for quasi in &tpl.quasis {
                value.push_str(quasi.value.raw.as_str());
            }
            TypeExpr::string_literal(value)
        }
        Expression::TemplateLiteral(_) => TypeExpr::Primitive(PrimitiveName::String),
        Expression::ArrowFunctionExpression(arrow) => {
            let sig = extract_arrow_signature(arrow, source);
            let fn_spans = FunctionSpans {
                signature: Some(arrow.span.into()),
                return_type: arrow
                    .return_type
                    .as_ref()
                    .map(|rt| rt.type_annotation.span().into()),
            };
            TypeExpr::Function(Arc::new(FunctionExpr::with_spans(
                sig.parameters,
                sig.return_type.map(Arc::new),
                sig.type_parameters,
                fn_spans,
            )))
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
                )
            } else {
                asserted
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
            infer_expression_type_ctx(&sat.expression, source, inner_policy)
        }
        Expression::StaticMemberExpression(member) => {
            // obj.foo → typeof obj.foo (build a dotted path)
            let mut path = Vec::new();
            collect_static_member_path(member, &mut path);
            if path.is_empty() {
                TypeExpr::Primitive(PrimitiveName::Any)
            } else {
                TypeExpr::TypeOf(ValueRef {
                    path,
                    type_args: Vec::new(),
                })
            }
        }
        Expression::CallExpression(call) => {
            // fn() → ReturnType<typeof fn>
            let callee_type = infer_expression_type(&call.callee, source);
            if matches!(callee_type, TypeExpr::Primitive(PrimitiveName::Any)) {
                TypeExpr::Primitive(PrimitiveName::Any)
            } else {
                TypeExpr::Ref {
                    name: Arc::from("ReturnType"),
                    type_arguments: Arc::from(vec![callee_type]),
                }
            }
        }
        _ => TypeExpr::Primitive(PrimitiveName::Any),
    }
}

/// Collect a dotted member path from a static member expression chain.
/// `a.b.c` → `["a", "b", "c"]` (in order). Non-identifier roots abort (clear path).
fn collect_static_member_path(
    member: &oxc_ast::ast::StaticMemberExpression<'_>,
    path: &mut Vec<String>,
) {
    match &member.object {
        Expression::Identifier(ident) => {
            path.push(ident.name.as_str().to_string());
        }
        Expression::StaticMemberExpression(parent) => {
            collect_static_member_path(parent, path);
            if path.is_empty() {
                return; // ancestor failed — propagate
            }
        }
        _ => {
            // Non-static root (e.g., computed, call) — can't build a simple path
            path.clear();
            return;
        }
    }
    path.push(member.property.name.as_str().to_string());
}

fn append_spread_array_element_types(
    expr: &Expression<'_>,
    source: &str,
    element_types: &mut Vec<TypeExpr>,
) {
    let spread_ty = infer_expression_type(expr, source);
    if let Some(spread_elements) = collect_array_element_types_from_type(&spread_ty) {
        element_types.extend(spread_elements);
    } else {
        element_types.push(TypeExpr::Primitive(PrimitiveName::Any));
    }
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

fn widen_literal_type(expr: TypeExpr) -> TypeExpr {
    // `TypeExpr` implements `Drop`, so the compound arms below cannot bind
    // their children by-move out of an owned `expr`. Match on a borrow and
    // clone the (refcounted) children; the catch-all forwards `expr` whole
    // (a full-value move, which `Drop` permits).
    match &expr {
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
        TypeExpr::Union(members) => TypeExpr::union(dedupe_type_exprs(
            members
                .iter()
                .cloned()
                .map(widen_literal_type)
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Intersection(members) => TypeExpr::intersection(
            members
                .iter()
                .cloned()
                .map(widen_literal_type)
                .collect::<Vec<_>>(),
        ),
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: Arc::new(widen_literal_type(element.as_ref().clone())),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: Arc::from(
                elements
                    .iter()
                    .cloned()
                    .map(|mut element| {
                        element.ty = widen_literal_type(element.ty);
                        element
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Object(obj) => TypeExpr::Object(Arc::new(ObjectExpr {
            properties: obj
                .properties
                .iter()
                .cloned()
                .map(widen_object_member)
                .collect(),
        })),
        TypeExpr::Function(function) => TypeExpr::Function(Arc::new(FunctionExpr::with_spans(
            function.parameters.clone(),
            function
                .return_type
                .as_ref()
                .map(|return_type| Arc::new(widen_literal_type(return_type.as_ref().clone()))),
            function.type_parameters.clone(),
            function.spans,
        ))),
        // A bare constructor type (`new (...) => R`) carries the same
        // `FunctionExpr` payload as a function type, so its literal members
        // widen identically. Reconstruct as a `ConstructorType` so the
        // constructor-ness survives — never flatten it to a plain `Function`.
        // This runs on analyzer-side lowered IR (e.g. `value as new () => T`),
        // BEFORE the dispatch lower collapses `Function`/`ConstructorType`.
        TypeExpr::ConstructorType(function) => {
            TypeExpr::ConstructorType(Arc::new(FunctionExpr::with_spans(
                function.parameters.clone(),
                function
                    .return_type
                    .as_ref()
                    .map(|return_type| Arc::new(widen_literal_type(return_type.as_ref().clone()))),
                function.type_parameters.clone(),
                function.spans,
            )))
        }
        _ => expr,
    }
}

fn widen_object_member(member: ObjectMember) -> ObjectMember {
    match member {
        ObjectMember::Property(mut property) => {
            property.ty = widen_literal_type(property.ty);
            ObjectMember::Property(property)
        }
        ObjectMember::IndexSignature(mut signature) => {
            signature.value_type = widen_literal_type(signature.value_type);
            ObjectMember::IndexSignature(signature)
        }
        ObjectMember::CallSignature(function) => {
            ObjectMember::CallSignature(FunctionExpr::with_spans(
                function.parameters,
                function
                    .return_type
                    .as_ref()
                    .map(|return_type| Arc::new(widen_literal_type(return_type.as_ref().clone()))),
                function.type_parameters,
                function.spans,
            ))
        }
        ObjectMember::ConstructSignature(function) => {
            ObjectMember::ConstructSignature(FunctionExpr::with_spans(
                function.parameters,
                function
                    .return_type
                    .as_ref()
                    .map(|return_type| Arc::new(widen_literal_type(return_type.as_ref().clone()))),
                function.type_parameters,
                function.spans,
            ))
        }
        ObjectMember::Method(mut method) => {
            method.function =
                FunctionExpr::with_spans(
                    method.function.parameters,
                    method.function.return_type.as_ref().map(|return_type| {
                        Arc::new(widen_literal_type(return_type.as_ref().clone()))
                    }),
                    method.function.type_parameters,
                    method.function.spans,
                );
            ObjectMember::Method(method)
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
    params
        .items
        .iter()
        .map(|param| {
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
            let ty = param
                .type_annotation
                .as_ref()
                .map(|ta| lower_ts_type(&ta.type_annotation, source))
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
            FunctionParam::with_span(
                name,
                ty,
                param.optional,
                false,
                Some(param.span.into()),
                has_ts_annotation,
            )
        })
        .chain(params.rest.as_ref().map(|rest| {
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
            FunctionParam::with_span(
                name,
                ty,
                false,
                true,
                Some(rest.span.into()),
                has_ts_annotation,
            )
        }))
        .collect()
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
    pub output_path: std::sync::Arc<[PathSegment]>,
}

pub fn expand_macro_types_impl_with_expander<F>(
    macros: &[crate::analysis::types::AnalyzedMacro],
    source: Option<&str>,
    binding_entries: &[(String, TypeExpr)],
    debug_env: Option<&mut EvalEnv>,
    scope: MacroExpansionScope,
    mut expand_field_expr: F,
) -> crate::analysis::type_expand::ExpandedComponentTypes
where
    F: FnMut(
        FieldExpansionContext,
        &TypeExpr,
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
            if let Some(ref typed) = field.type_expr {
                if !typed.is_unknown() {
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
                        output_path: std::sync::Arc::from(vec![PathSegment::Member(
                            std::sync::Arc::from(field.name.as_str()),
                        )]),
                    };
                    let expanded = expand_field_expr(ctx, typed);
                    log_expand_stage(
                        stage_log,
                        expanded.exactness,
                        expanded.execution_status,
                        &expanded.diagnostics,
                        debug_env.as_deref(),
                    );
                    let shallow_type_expr = field.type_expr.clone();
                    let shallow_type_expr_scope = field.type_expr_scope.clone();
                    debug_assert_eq!(
                        shallow_type_expr.is_some(),
                        shallow_type_expr_scope.is_some(),
                        "ExpandedField (prop) shallow_type_expr/shallow_type_expr_scope pairing violated for field `{}`",
                        field.name
                    );
                    result.props.push(ExpandedField {
                        name: field.name.clone(),
                        r#type: expanded.value.expr,
                        raw_type: field.type_annotation.clone(),
                        optional: field.is_optional,
                        exactness: expanded.exactness,
                        execution_status: expanded.execution_status,
                        diagnostics: expanded.diagnostics,
                        shallow_type_expr,
                        shallow_type_expr_scope,
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
            if let Some(ref typed) = field.payload_expr {
                if !typed.is_unknown() {
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
                        output_path: std::sync::Arc::from(vec![PathSegment::Member(
                            std::sync::Arc::from(field.name.as_str()),
                        )]),
                    };
                    let expanded = expand_field_expr(ctx, typed);
                    log_expand_stage(
                        stage_log,
                        expanded.exactness,
                        expanded.execution_status,
                        &expanded.diagnostics,
                        debug_env.as_deref(),
                    );
                    let shallow_type_expr = field.payload_expr.clone();
                    let shallow_type_expr_scope = field.payload_expr_scope.clone();
                    debug_assert_eq!(
                        shallow_type_expr.is_some(),
                        shallow_type_expr_scope.is_some(),
                        "ExpandedField (emit) shallow_type_expr/shallow_type_expr_scope pairing violated for emit `{}`",
                        field.name
                    );
                    result.emits.push(ExpandedField {
                        name: field.name.clone(),
                        r#type: expanded.value.expr,
                        raw_type: field.payload_type.clone(),
                        optional: false,
                        exactness: expanded.exactness,
                        execution_status: expanded.execution_status,
                        diagnostics: expanded.diagnostics,
                        shallow_type_expr,
                        shallow_type_expr_scope,
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
        // Read the typed form populated by the analyzer producer in
        // `extract_slot_bindings_from_oxc_type` (analyzer lowers the OXC
        // `TSType<'_>` AST node into `binding_expr`).
        if scope == MacroExpansionScope::Full {
            for slot in &m.slot_fields {
                for binding in &slot.bindings {
                    if let Some(ref typed) = binding.binding_expr {
                        if !typed.is_unknown() {
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
                                output_path: std::sync::Arc::from(vec![
                                    PathSegment::Member(std::sync::Arc::from(slot.name.as_str())),
                                    PathSegment::Member(std::sync::Arc::from(
                                        binding.name.as_str(),
                                    )),
                                ]),
                            };
                            let expanded = expand_field_expr(ctx, typed);
                            log_expand_stage(
                                stage_log,
                                expanded.exactness,
                                expanded.execution_status,
                                &expanded.diagnostics,
                                debug_env.as_deref(),
                            );
                            let shallow_type_expr = binding.binding_expr.clone();
                            let shallow_type_expr_scope = binding.binding_expr_scope.clone();
                            debug_assert_eq!(
                                shallow_type_expr.is_some(),
                                shallow_type_expr_scope.is_some(),
                                "ExpandedField (slot binding) shallow_type_expr/shallow_type_expr_scope pairing violated for binding `{}`",
                                slot_binding_target
                            );
                            result.slot_bindings.push(ExpandedField {
                                name: slot_binding_target,
                                r#type: expanded.value.expr,
                                raw_type: binding.type_annotation.clone(),
                                optional: false,
                                exactness: expanded.exactness,
                                execution_status: expanded.execution_status,
                                diagnostics: expanded.diagnostics,
                                shallow_type_expr,
                                shallow_type_expr_scope,
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
        for (name, type_ann) in binding_entries {
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
            // `defineExpose` binding entries are top-level value
            // bindings in the script-setup scope — there is no parent
            // macro shell. The closure recognises an empty
            // `output_path` as "no projection rewrite available; treat
            // `parsed` as the resolution target" and falls back to
            // legacy field-level resolution. `macro_index` carries the
            // sentinel `usize::MAX` used elsewhere for non-macro-anchored
            // expose entries (see binding stage label below).
            let ctx = FieldExpansionContext {
                kind: FieldKind::Binding,
                macro_index: usize::MAX,
                output_path: std::sync::Arc::from(Vec::<PathSegment>::new()),
            };
            let expanded = expand_field_expr(ctx, type_ann);
            log_expand_stage(
                stage_log,
                expanded.exactness,
                expanded.execution_status,
                &expanded.diagnostics,
                debug_env.as_deref(),
            );
            // `defineExpose` binding entries are top-level value bindings
            // with no analyzer-side shallow typed sidecar. The pairing
            // invariant holds trivially with both fields `None`.
            debug_assert_eq!(
                Option::<TypeExpr>::None.is_some(),
                Option::<TypeExprScope>::None.is_some(),
                "ExpandedField (expose binding) shallow_type_expr/shallow_type_expr_scope pairing violated for binding `{}`",
                name
            );
            // `defineExpose` binding entries are top-level value bindings
            // outside any macro T (no declared/heritage distinction
            // applies). `declared_in_macro_type_arg = false` is the
            // structural truth.
            result.bindings.push(ExpandedField {
                name: name.clone(),
                r#type: expanded.value.expr,
                raw_type: None,
                optional: false,
                exactness: expanded.exactness,
                execution_status: expanded.execution_status,
                diagnostics: expanded.diagnostics,
                shallow_type_expr: None,
                shallow_type_expr_scope: None,
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

#[derive(Default)]
pub struct CollectedMacroTypeParams {
    pub define_props: Vec<TypeExpr>,
    pub define_emits: Vec<TypeExpr>,
    pub define_slots: Vec<TypeExpr>,
}

pub fn collect_define_macro_type_params(source: &str) -> CollectedMacroTypeParams {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn collect_call_type_param(
        call: &CallExpression<'_>,
        source: &str,
        result: &mut CollectedMacroTypeParams,
    ) {
        let Expression::Identifier(id) = &call.callee else {
            return;
        };
        let Some(type_args) = &call.type_arguments else {
            return;
        };
        let Some(first) = type_args.params.first() else {
            return;
        };

        match id.name.as_str() {
            "defineProps" => result.define_props.push(lower_ts_type(first, source)),
            "defineEmits" => result.define_emits.push(lower_ts_type(first, source)),
            "defineSlots" => result.define_slots.push(lower_ts_type(first, source)),
            _ => {}
        }
    }

    fn walk_expr(expr: &Expression<'_>, source: &str, result: &mut CollectedMacroTypeParams) {
        match expr {
            Expression::CallExpression(call) => {
                collect_call_type_param(call, source, result);
                walk_expr(&call.callee, source, result);
                for arg in &call.arguments {
                    if let Argument::SpreadElement(spread) = arg {
                        walk_expr(&spread.argument, source, result);
                    } else if let Some(inner) = arg.as_expression() {
                        walk_expr(inner, source, result);
                    }
                }
            }
            Expression::ParenthesizedExpression(paren) => {
                walk_expr(&paren.expression, source, result)
            }
            Expression::ConditionalExpression(cond) => {
                walk_expr(&cond.test, source, result);
                walk_expr(&cond.consequent, source, result);
                walk_expr(&cond.alternate, source, result);
            }
            Expression::SequenceExpression(seq) => {
                for inner in &seq.expressions {
                    walk_expr(inner, source, result);
                }
            }
            _ => {}
        }
    }

    fn walk_stmt(stmt: &Statement<'_>, source: &str, result: &mut CollectedMacroTypeParams) {
        match stmt {
            Statement::ExpressionStatement(expr_stmt) => {
                walk_expr(&expr_stmt.expression, source, result)
            }
            Statement::VariableDeclaration(var_decl) => {
                for decl in &var_decl.declarations {
                    if let Some(init) = &decl.init {
                        walk_expr(init, source, result);
                    }
                }
            }
            _ => {}
        }
    }

    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, source, source_type).parse();
    let mut result = CollectedMacroTypeParams::default();
    for stmt in &ret.program.body {
        walk_stmt(stmt, source, &mut result);
    }
    result
}

pub fn collect_define_props_type_params(source: &str) -> Vec<TypeExpr> {
    collect_define_macro_type_params(source).define_props
}

// ---------------------------------------------------------------------------
// Public convenience: parse source and build env
// ---------------------------------------------------------------------------

/// Parse a TypeScript source string and build an evaluation environment.
///
/// This is a convenience function for tests and standalone usage.
/// In production, use `build_eval_env` with a pre-parsed OXC program.
pub fn parse_and_build_env(source: &str) -> EvalEnv {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, source, source_type).parse();
    build_eval_env(&ret.program, source)
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
    Some(lower_value_expression(init, &wrapped))
}

fn lower_value_expression(expr: &Expression<'_>, source: &str) -> TypeExpr {
    match expr {
        Expression::Identifier(ident) => TypeExpr::TypeOf(ValueRef {
            path: vec![ident.name.as_str().to_string()],
            type_args: Vec::new(),
        }),
        Expression::ConditionalExpression(cond) => TypeExpr::union(vec![
            lower_value_expression(&cond.consequent, source),
            lower_value_expression(&cond.alternate, source),
        ]),
        Expression::ParenthesizedExpression(paren) => {
            lower_value_expression(&paren.expression, source)
        }
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            let mut value = String::new();
            for quasi in &tpl.quasis {
                value.push_str(quasi.value.raw.as_str());
            }
            TypeExpr::string_literal(value)
        }
        Expression::TSAsExpression(ts_as) => lower_value_expression(&ts_as.expression, source),
        Expression::TSSatisfiesExpression(sat) => lower_value_expression(&sat.expression, source),
        _ => infer_expression_type(expr, source),
    }
}
