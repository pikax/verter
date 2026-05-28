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
    Argument, ArrowFunctionExpression, BindingPattern, CallExpression, Class, ClassElement,
    Declaration, ExportDefaultDeclarationKind, Expression, FormalParameters, Function,
    MethodDefinitionKind, ObjectExpression, ObjectPropertyKind, Program, Statement,
    TSAccessibility, TSInterfaceDeclaration, TSModuleDeclaration, TSModuleDeclarationBody,
    TSModuleDeclarationName, TSSignature, TSTypeAliasDeclaration, TSTypeParameterDeclaration,
    VariableDeclarationKind, VariableDeclarator,
};
use verter_type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, MethodSignature, ObjectExpr, ObjectMember,
    PrimitiveName, TypeExpr, TypeExprScope, TypeParam, ValueRef,
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
        match stmt {
            Statement::TSTypeAliasDeclaration(decl) => {
                extract_type_alias(decl, source, &mut env);
            }
            Statement::TSInterfaceDeclaration(decl) => {
                extract_interface(decl, source, &mut env);
            }
            Statement::TSModuleDeclaration(module) => {
                extract_module_declaration(module, source, &mut env, None);
            }
            Statement::ClassDeclaration(decl) => {
                extract_class(decl, source, &mut env);
            }
            Statement::FunctionDeclaration(func) => {
                extract_function(func, source, &mut env);
            }
            Statement::VariableDeclaration(var_decl) => {
                for decl in &var_decl.declarations {
                    extract_variable(decl, var_decl.kind, source, &mut env);
                }
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(ref decl) = export.declaration {
                    extract_from_declaration(decl, source, &mut env);
                }
            }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    extract_function(func, source, &mut env);
                }
                ExportDefaultDeclarationKind::ClassDeclaration(cls) => {
                    extract_class(cls, source, &mut env);
                }
                ExportDefaultDeclarationKind::TSInterfaceDeclaration(iface) => {
                    extract_interface(iface, source, &mut env);
                }
                other => {
                    if let Some(expr) = other.as_expression() {
                        extract_default_expression(expr, source, &mut env);
                    }
                }
            },
            _ => {}
        }
    }

    env
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
        Declaration::ClassDeclaration(cls) => {
            extract_class(cls, source, env);
        }
        Declaration::FunctionDeclaration(func) => {
            extract_function(func, source, env);
        }
        Declaration::VariableDeclaration(var_decl) => {
            for d in &var_decl.declarations {
                extract_variable(d, var_decl.kind, source, env);
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
    extract_named_type_alias(decl, source, env, name);
}

fn extract_named_type_alias(
    decl: &TSTypeAliasDeclaration<'_>,
    source: &str,
    env: &mut EvalEnv,
    name: String,
) {
    let type_parameters = decl
        .type_parameters
        .as_ref()
        .map(|tp| lower_type_param_decls(tp, source))
        .unwrap_or_default();
    let body = lower_ts_type(&decl.type_annotation, source);

    env.add_type(TypeDeclInfo {
        name,
        declaration_id: 0,
        kind: TypeDeclKind::Alias,
        type_parameters,
        body,
    });
}

fn extract_interface(decl: &TSInterfaceDeclaration<'_>, source: &str, env: &mut EvalEnv) {
    let name = decl.id.name.to_string();
    extract_named_interface(decl, source, env, name);
}

fn extract_named_interface(
    decl: &TSInterfaceDeclaration<'_>,
    source: &str,
    env: &mut EvalEnv,
    name: String,
) {
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

    env.add_type(TypeDeclInfo {
        name,
        declaration_id: 0,
        kind: TypeDeclKind::Interface,
        type_parameters,
        body,
    });
}

fn extract_module_declaration(
    decl: &TSModuleDeclaration<'_>,
    source: &str,
    env: &mut EvalEnv,
    prefix: Option<&str>,
) {
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

fn extract_namespaced_statement(
    stmt: &Statement<'_>,
    source: &str,
    env: &mut EvalEnv,
    namespace: &str,
) {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => {
            extract_named_type_alias(
                alias,
                source,
                env,
                qualified_name(namespace, &alias.id.name),
            );
        }
        Statement::TSInterfaceDeclaration(iface) => {
            extract_named_interface(
                iface,
                source,
                env,
                qualified_name(namespace, &iface.id.name),
            );
        }
        Statement::TSModuleDeclaration(module) => {
            extract_module_declaration(module, source, env, Some(namespace));
        }
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
            extract_named_type_alias(
                alias,
                source,
                env,
                qualified_name(namespace, &alias.id.name),
            );
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            extract_named_interface(
                iface,
                source,
                env,
                qualified_name(namespace, &iface.id.name),
            );
        }
        Declaration::TSModuleDeclaration(module) => {
            extract_module_declaration(module, source, env, Some(namespace));
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

fn extract_class(decl: &Class<'_>, source: &str, env: &mut EvalEnv) {
    let name = match &decl.id {
        Some(id) => id.name.to_string(),
        None => return,
    };

    // Extract public instance shape from class body
    let mut members = Vec::new();
    let mut ctor_sig = None;

    for element in &decl.body.body {
        match element {
            ClassElement::PropertyDefinition(prop) => {
                if matches!(prop.accessibility, None | Some(TSAccessibility::Public))
                    && !prop.r#static
                {
                    if let Some(prop_name) = property_key_name(&prop.key) {
                        let ty = prop
                            .type_annotation
                            .as_ref()
                            .map(|ta| lower_ts_type(&ta.type_annotation, source))
                            .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
                        members.push(ObjectMember::Property(verter_type_expr::ObjectProperty {
                            name: prop_name,
                            ty,
                            optional: prop.optional,
                            readonly: prop.readonly,
                        }));
                    }
                }
            }
            ClassElement::MethodDefinition(method) => {
                if matches!(method.accessibility, None | Some(TSAccessibility::Public))
                    && !method.r#static
                {
                    if method.kind == MethodDefinitionKind::Constructor {
                        ctor_sig = Some(extract_function_signature(&method.value, source));
                    } else if let Some(method_name) = property_key_name(&method.key) {
                        let func = extract_function_signature(&method.value, source);
                        members.push(ObjectMember::Method(MethodSignature {
                            name: method_name,
                            function: FunctionExpr {
                                parameters: func.parameters,
                                return_type: func.return_type.map(Arc::new),
                                type_parameters: func.type_parameters,
                            },
                            optional: method.optional,
                        }));
                    }
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
    // by every body-driven surface reader (eager OXC rail folds it
    // separately via `apply_class_heritage_edge`; this is the typed-IR
    // producer parity).
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
    let constructor_signature = ctor_sig.clone().unwrap_or_else(|| FunctionSignature {
        parameters: Vec::new(),
        return_type: Some(TypeExpr::named(name.clone())),
        type_parameters: Vec::new(),
    });
    let constructor_shape = ObjectExpr {
        properties: vec![ObjectMember::ConstructSignature(FunctionExpr {
            parameters: constructor_signature.parameters.clone(),
            return_type: constructor_signature.return_type.clone().map(Arc::new),
            type_parameters: constructor_signature.type_parameters.clone(),
        })],
    };

    env.add_value(ValueDeclInfo {
        name,
        declaration_id: 0,
        kind: ValueDeclKind::Class,
        type_annotation: None,
        function_signature: Some(constructor_signature),
        object_shape: Some(constructor_shape),
    });
}

// ---------------------------------------------------------------------------
// Value declarations
// ---------------------------------------------------------------------------

fn extract_function(func: &Function<'_>, source: &str, env: &mut EvalEnv) {
    let name = match &func.id {
        Some(id) => id.name.to_string(),
        None => return,
    };

    let sig = extract_function_signature(func, source);
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
        function_signature: Some(sig),
        object_shape: None,
    });
}

fn extract_variable(
    decl: &VariableDeclarator<'_>,
    kind: VariableDeclarationKind,
    source: &str,
    env: &mut EvalEnv,
) {
    let name = match &decl.id {
        BindingPattern::BindingIdentifier(id) => id.name.to_string(),
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

    // Extract function signature from arrow functions or function expressions
    let mut function_signature = None;
    let mut object_shape = None;

    if let Some(ref init) = decl.init {
        function_signature = extract_initializer_function_signature(init, source);
        object_shape = extract_initializer_object_shape(init, source);

        if type_annotation.is_none() {
            let mut inferred = infer_expression_type(init, source);
            if matches!(var_kind, ValueDeclKind::Let | ValueDeclKind::Var) {
                inferred = widen_literal_type(inferred);
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
        function_signature,
        object_shape,
    });
}

fn extract_default_expression(expr: &Expression<'_>, source: &str, env: &mut EvalEnv) {
    let function_signature = extract_initializer_function_signature(expr, source);
    let object_shape = extract_initializer_object_shape(expr, source);
    let type_annotation = Some(lower_value_expression(expr, source));

    env.add_value(ValueDeclInfo {
        name: "default".to_string(),
        declaration_id: 0,
        kind: ValueDeclKind::Const,
        type_annotation,
        function_signature,
        object_shape,
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

fn extract_initializer_object_shape(expr: &Expression<'_>, source: &str) -> Option<ObjectExpr> {
    match expr {
        Expression::ObjectExpression(obj) => Some(extract_object_literal(obj, source)),
        Expression::TSAsExpression(ts_as) => {
            extract_initializer_object_shape(&ts_as.expression, source)
        }
        Expression::TSSatisfiesExpression(sat) => {
            extract_initializer_object_shape(&sat.expression, source)
        }
        Expression::ParenthesizedExpression(paren) => {
            extract_initializer_object_shape(&paren.expression, source)
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

    FunctionSignature {
        parameters,
        return_type,
        type_parameters,
    }
}

fn extract_object_literal(obj: &ObjectExpression<'_>, source: &str) -> ObjectExpr {
    let mut members = Vec::new();
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                if let Some(name) = property_key_name(&p.key) {
                    let ty = infer_expression_type(&p.value, source);
                    push_object_property_with_override(
                        &mut members,
                        verter_type_expr::ObjectProperty {
                            name,
                            ty,
                            optional: false,
                            readonly: false,
                        },
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
fn extract_object_literal_as_type(obj: &ObjectExpression<'_>, source: &str) -> TypeExpr {
    let mut members = Vec::new();
    let mut spread_types: Vec<TypeExpr> = Vec::new();
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                if let Some(name) = property_key_name(&p.key) {
                    let ty = infer_expression_type(&p.value, source);
                    push_object_property_with_override(
                        &mut members,
                        verter_type_expr::ObjectProperty {
                            name,
                            ty,
                            optional: false,
                            readonly: false,
                        },
                    );
                }
            }
            ObjectPropertyKind::SpreadProperty(spread) => {
                let spread_ty = infer_expression_type(&spread.argument, source);
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
fn infer_expression_type(expr: &Expression<'_>, source: &str) -> TypeExpr {
    match expr {
        Expression::Identifier(ident) => TypeExpr::TypeOf(ValueRef {
            path: vec![ident.name.as_str().to_string()],
        }),
        Expression::StringLiteral(s) => TypeExpr::string_literal(s.value.as_str()),
        Expression::NumericLiteral(n) => TypeExpr::number_literal(n.value),
        Expression::BooleanLiteral(b) => TypeExpr::boolean_literal(b.value),
        Expression::NullLiteral(_) => TypeExpr::Primitive(PrimitiveName::Null),
        Expression::ConditionalExpression(cond) => TypeExpr::union(vec![
            infer_expression_type(&cond.consequent, source),
            infer_expression_type(&cond.alternate, source),
        ]),
        Expression::ParenthesizedExpression(paren) => {
            infer_expression_type(&paren.expression, source)
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
                                infer_expression_type(expr, source),
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
        Expression::ObjectExpression(obj) => extract_object_literal_as_type(obj, source),
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
            TypeExpr::Function(Arc::new(FunctionExpr {
                parameters: sig.parameters,
                return_type: sig.return_type.map(Arc::new),
                type_parameters: sig.type_parameters,
            }))
        }
        Expression::TSAsExpression(ts_as) => {
            // `as const` should preserve the underlying literal/object surface
            // instead of degrading the inferred type to an opaque `const` marker.
            let asserted = lower_ts_type(&ts_as.type_annotation, source);
            if is_const_assertion_type_expr(&asserted) {
                infer_expression_type(&ts_as.expression, source)
            } else {
                asserted
            }
        }
        Expression::TSSatisfiesExpression(sat) => {
            // const x = value satisfies SomeType → infer from the underlying value expression,
            // not the annotation. `satisfies` validates but doesn't widen.
            infer_expression_type(&sat.expression, source)
        }
        Expression::StaticMemberExpression(member) => {
            // obj.foo → typeof obj.foo (build a dotted path)
            let mut path = Vec::new();
            collect_static_member_path(member, &mut path);
            if path.is_empty() {
                TypeExpr::Primitive(PrimitiveName::Any)
            } else {
                TypeExpr::TypeOf(ValueRef { path })
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
    match ty {
        TypeExpr::Union(members) => into.extend(members.iter().cloned()),
        other => into.push(other),
    }
}

fn widen_literal_type(expr: TypeExpr) -> TypeExpr {
    match expr {
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
            readonly,
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
            readonly,
        },
        TypeExpr::Object(obj) => TypeExpr::Object(Arc::new(ObjectExpr {
            properties: obj
                .properties
                .iter()
                .cloned()
                .map(widen_object_member)
                .collect(),
        })),
        TypeExpr::Function(function) => TypeExpr::Function(Arc::new(FunctionExpr {
            parameters: function.parameters.clone(),
            return_type: function
                .return_type
                .as_ref()
                .map(|return_type| Arc::new(widen_literal_type(return_type.as_ref().clone()))),
            type_parameters: function.type_parameters.clone(),
        })),
        other => other,
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
        ObjectMember::CallSignature(function) => ObjectMember::CallSignature(FunctionExpr {
            parameters: function.parameters,
            return_type: function
                .return_type
                .as_ref()
                .map(|return_type| Arc::new(widen_literal_type(return_type.as_ref().clone()))),
            type_parameters: function.type_parameters,
        }),
        ObjectMember::ConstructSignature(function) => {
            ObjectMember::ConstructSignature(FunctionExpr {
                parameters: function.parameters,
                return_type: function
                    .return_type
                    .as_ref()
                    .map(|return_type| Arc::new(widen_literal_type(return_type.as_ref().clone()))),
                type_parameters: function.type_parameters,
            })
        }
        ObjectMember::Method(mut method) => {
            method.function =
                FunctionExpr {
                    parameters: method.function.parameters,
                    return_type: method.function.return_type.as_ref().map(|return_type| {
                        Arc::new(widen_literal_type(return_type.as_ref().clone()))
                    }),
                    type_parameters: method.function.type_parameters,
                };
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
            Some(ObjectMember::Property(verter_type_expr::ObjectProperty {
                name,
                ty,
                optional: prop.optional,
                readonly: prop.readonly,
            }))
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
            Some(ObjectMember::Method(MethodSignature {
                name,
                function: FunctionExpr {
                    parameters: params,
                    return_type: return_type.map(Arc::new),
                    type_parameters,
                },
                optional: method.optional,
            }))
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
            Some(ObjectMember::CallSignature(FunctionExpr {
                parameters: params,
                return_type: return_type.map(Arc::new),
                type_parameters,
            }))
        }
        TSSignature::TSIndexSignature(idx) => {
            let (key_name, key_type) = if let Some(param) = idx.parameters.first() {
                (
                    param.name.to_string(),
                    lower_ts_type(&param.type_annotation.type_annotation, source),
                )
            } else {
                (
                    "key".to_string(),
                    TypeExpr::Primitive(PrimitiveName::String),
                )
            };
            let value_type = lower_ts_type(&idx.type_annotation.type_annotation, source);
            Some(ObjectMember::IndexSignature(IndexSignature {
                key_name,
                key_type,
                value_type,
                readonly: idx.readonly,
            }))
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
            Some(ObjectMember::ConstructSignature(FunctionExpr {
                parameters: params,
                return_type: return_type.map(Arc::new),
                type_parameters,
            }))
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
            let ty = param
                .type_annotation
                .as_ref()
                .map(|ta| lower_ts_type(&ta.type_annotation, source))
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
            FunctionParam {
                name,
                ty,
                optional: param.optional,
                rest: false,
            }
        })
        .chain(params.rest.as_ref().map(|rest| {
            let name = match &rest.rest.argument {
                BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
                _ => None,
            };
            let ty = rest
                .type_annotation
                .as_ref()
                .map(|ta| lower_ts_type(&ta.type_annotation, source))
                .unwrap_or(TypeExpr::Primitive(PrimitiveName::Any));
            FunctionParam {
                name,
                ty,
                optional: false,
                rest: true,
            }
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
