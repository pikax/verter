//! Build an [`EvalEnv`] from an OXC program AST.
//!
//! Walks top-level declarations and populates the type and value
//! symbol tables so the evaluator can resolve references.

use std::io::Write;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use crate::analysis::type_eval::*;
use crate::analysis::type_expr::{
    self, FunctionExpr, FunctionParam, IndexSignature, MethodSignature, ObjectExpr, ObjectMember,
    PrimitiveName, TypeExpr, TypeParam, ValueRef,
};
use crate::analysis::type_expr_lower::{lower_ts_type, property_key_name};
use oxc_ast::ast::{
    Argument, ArrowFunctionExpression, BindingPattern, CallExpression, Class, ClassElement,
    Declaration, ExportDefaultDeclarationKind, Expression, FormalParameters, Function,
    MethodDefinitionKind, ObjectExpression, ObjectPropertyKind, Program, Statement,
    TSAccessibility, TSInterfaceDeclaration, TSModuleDeclaration, TSModuleDeclarationBody,
    TSModuleDeclarationName, TSSignature, TSTypeAliasDeclaration, TSTypeParameterDeclaration,
    VariableDeclarationKind, VariableDeclarator,
};

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
                        members.push(ObjectMember::Property(type_expr::ObjectProperty {
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

    let body = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }));

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
                        type_expr::ObjectProperty {
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
                        type_expr::ObjectProperty {
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
    property: type_expr::ObjectProperty,
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
        TypeExpr::Literal(type_expr::LiteralValue::String(_)) => {
            TypeExpr::Primitive(PrimitiveName::String)
        }
        TypeExpr::Literal(type_expr::LiteralValue::Number(_)) => {
            TypeExpr::Primitive(PrimitiveName::Number)
        }
        TypeExpr::Literal(type_expr::LiteralValue::Boolean(_)) => {
            TypeExpr::Primitive(PrimitiveName::Boolean)
        }
        TypeExpr::Literal(type_expr::LiteralValue::BigInt(_)) => {
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
            Some(ObjectMember::Property(type_expr::ObjectProperty {
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
// Snapshot evaluation: evaluate type annotations from an analysis snapshot
// ---------------------------------------------------------------------------

/// Convenience wrapper: expand all macro type annotations from source.
///
/// Builds an `EvalEnv` from the source, then expands each prop/emit/slot
/// type annotation using the native solver with default limits.
pub fn evaluate_macro_types(
    macros: &[crate::analysis::types::AnalyzedMacro],
    source: &str,
) -> crate::analysis::type_expand::ExpandedComponentTypes {
    let mut env = parse_and_build_env(source);
    let solver_host = crate::analysis::type_solver::host::EvalEnvSolverHost::new(&env);
    expand_macro_types(macros, Some(source), &mut env, None, &solver_host)
}

// ---------------------------------------------------------------------------
// Expansion-based macro type evaluation
// ---------------------------------------------------------------------------

/// Expand all macro-backed type annotations using the native type solver.
///
/// All type resolution goes through `solve_type()` via the provided
/// `TypeSolverHost`. The host determines how type references are resolved:
/// - `EvalEnvSolverHost` for standalone/test contexts (resolves from local env)
/// - `SessionSolverHost` for production (resolves from host caches + owner env)
pub fn expand_macro_types(
    macros: &[crate::analysis::types::AnalyzedMacro],
    source: Option<&str>,
    env: &mut EvalEnv,
    local_binding_names: Option<&rustc_hash::FxHashSet<String>>,
    solver_host: &dyn crate::analysis::type_solver::host::TypeSolverHost,
) -> crate::analysis::type_expand::ExpandedComponentTypes {
    let binding_entries = collect_binding_entries_from_env(env, local_binding_names);
    let mut engine = crate::analysis::type_solver::query_engine::TypeQueryEngine::new(solver_host);
    let mut result = expand_macro_types_impl(
        macros,
        source,
        binding_entries.as_slice(),
        Some(env),
        MacroExpansionScope::Full,
        &mut engine,
    );
    // Standalone path: produce object shapes via the solver directly.
    // The session path uses the projection-first pipeline in meta_resolve.rs instead.
    expand_standalone_macro_object_shapes(macros, source, &mut result, &mut engine);
    result
}

/// Expand macro-backed type annotations using only pre-collected binding type
/// annotations plus the solver host.
///
/// This is the cache-owned production path used by `verter_session`, where
/// local binding types come from prepared value declarations rather than an
/// `EvalEnv`. Creates its own internal `TypeQueryEngine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroExpansionScope {
    Full,
    Fallthrough,
}

pub fn expand_macro_types_with_bindings(
    macros: &[crate::analysis::types::AnalyzedMacro],
    source: Option<&str>,
    binding_entries: &[(String, TypeExpr)],
    solver_host: &dyn crate::analysis::type_solver::host::TypeSolverHost,
) -> crate::analysis::type_expand::ExpandedComponentTypes {
    expand_macro_types_with_bindings_for_scope(
        macros,
        source,
        binding_entries,
        MacroExpansionScope::Full,
        solver_host,
    )
}

pub fn expand_macro_types_with_bindings_for_scope(
    macros: &[crate::analysis::types::AnalyzedMacro],
    source: Option<&str>,
    binding_entries: &[(String, TypeExpr)],
    scope: MacroExpansionScope,
    solver_host: &dyn crate::analysis::type_solver::host::TypeSolverHost,
) -> crate::analysis::type_expand::ExpandedComponentTypes {
    let mut engine = crate::analysis::type_solver::query_engine::TypeQueryEngine::new(solver_host);
    expand_macro_types_impl(macros, source, binding_entries, None, scope, &mut engine)
}

/// Like `expand_macro_types_with_bindings`, but accepts an external
/// request-scoped `TypeQueryEngine` so the caller can share one engine across
/// macro expansion and later registry projection.
pub fn expand_macro_types_with_engine(
    macros: &[crate::analysis::types::AnalyzedMacro],
    source: Option<&str>,
    binding_entries: &[(String, TypeExpr)],
    engine: &mut crate::analysis::type_solver::query_engine::TypeQueryEngine<'_>,
) -> crate::analysis::type_expand::ExpandedComponentTypes {
    expand_macro_types_with_engine_for_scope(
        macros,
        source,
        binding_entries,
        MacroExpansionScope::Full,
        engine,
    )
}

pub fn expand_macro_types_with_engine_for_scope(
    macros: &[crate::analysis::types::AnalyzedMacro],
    source: Option<&str>,
    binding_entries: &[(String, TypeExpr)],
    scope: MacroExpansionScope,
    engine: &mut crate::analysis::type_solver::query_engine::TypeQueryEngine<'_>,
) -> crate::analysis::type_expand::ExpandedComponentTypes {
    expand_macro_types_impl(macros, source, binding_entries, None, scope, engine)
}

fn collect_binding_entries_from_env(
    env: &EvalEnv,
    local_binding_names: Option<&rustc_hash::FxHashSet<String>>,
) -> Vec<(String, TypeExpr)> {
    env.value_symbols
        .iter()
        .filter(|(name, _)| {
            local_binding_names
                .map(|names| names.contains(name.as_str()))
                .unwrap_or(true)
        })
        .filter_map(|(name, decl)| {
            decl.type_annotation
                .as_ref()
                .map(|ta| (name.clone(), ta.clone()))
        })
        .collect()
}

fn expand_macro_types_impl(
    macros: &[crate::analysis::types::AnalyzedMacro],
    source: Option<&str>,
    binding_entries: &[(String, TypeExpr)],
    debug_env: Option<&mut EvalEnv>,
    scope: MacroExpansionScope,
    engine: &mut crate::analysis::type_solver::query_engine::TypeQueryEngine<'_>,
) -> crate::analysis::type_expand::ExpandedComponentTypes {
    use crate::analysis::type_expand::{
        solver_result_to_normalized_expansion, ExpandedComponentTypes, ExpandedField,
        ExpandedNormalizedExpr, ExpansionResult,
    };
    use crate::analysis::type_expr_lower::parse_type_annotation;
    use crate::analysis::type_solver::result::SolverResult;

    fn solver_to_expr_result(
        result: SolverResult<TypeExpr>,
    ) -> ExpansionResult<ExpandedNormalizedExpr> {
        solver_result_to_normalized_expansion(result)
    }

    fn expand_field_expr(
        engine: &mut crate::analysis::type_solver::query_engine::TypeQueryEngine<'_>,
        parsed: &TypeExpr,
    ) -> ExpansionResult<ExpandedNormalizedExpr> {
        if let Some(fast) = engine.try_fast_shallow_field_expr(parsed) {
            solver_to_expr_result(fast)
        } else if engine.should_preserve_shallow_field_expr(parsed) {
            ExpansionResult::exact_symbolic(ExpandedNormalizedExpr {
                expr: parsed.clone(),
            })
        } else {
            solver_to_expr_result(engine.solve_preserving_package_refs(parsed))
        }
    }

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
        // Expand prop field type annotations
        for field in &m.prop_fields {
            if let Some(ref type_ann) = field.type_annotation {
                let parsed = parse_type_annotation(type_ann);
                if !parsed.is_unknown() {
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
                    let expanded = expand_field_expr(engine, &parsed);
                    log_expand_stage(
                        stage_log,
                        expanded.exactness,
                        expanded.execution_status,
                        &expanded.diagnostics,
                        debug_env.as_deref(),
                    );
                    result.props.push(ExpandedField {
                        name: field.name.clone(),
                        r#type: expanded.value.expr,
                        raw_type: Some(type_ann.clone()),
                        optional: field.is_optional,
                        exactness: expanded.exactness,
                        execution_status: expanded.execution_status,
                        diagnostics: expanded.diagnostics,
                    });
                }
            }
        }

        // NOTE: defineProps<T>(), defineEmits<T>(), defineSlots<T>() object-shape
        // production is owned by the query-engine phase in meta_resolve.rs.
        // This function handles field-level work only.

        // Expand emit payload types
        for field in &m.emit_fields {
            if let Some(ref payload) = field.payload_type {
                let parsed = parse_type_annotation(payload);
                if !parsed.is_unknown() {
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
                    let expanded = expand_field_expr(engine, &parsed);
                    log_expand_stage(
                        stage_log,
                        expanded.exactness,
                        expanded.execution_status,
                        &expanded.diagnostics,
                        debug_env.as_deref(),
                    );
                    result.emits.push(ExpandedField {
                        name: field.name.clone(),
                        r#type: expanded.value.expr,
                        raw_type: Some(payload.clone()),
                        optional: false,
                        exactness: expanded.exactness,
                        execution_status: expanded.execution_status,
                        diagnostics: expanded.diagnostics,
                    });
                }
            }
        }

        // Slot binding expansion is not needed for fallthrough-only meta.
        if scope == MacroExpansionScope::Full {
            for slot in &m.slot_fields {
                for binding in &slot.bindings {
                    if let Some(ref type_ann) = binding.type_annotation {
                        let parsed = parse_type_annotation(type_ann);
                        if !parsed.is_unknown() {
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
                            let expanded = expand_field_expr(engine, &parsed);
                            log_expand_stage(
                                stage_log,
                                expanded.exactness,
                                expanded.execution_status,
                                &expanded.diagnostics,
                                debug_env.as_deref(),
                            );
                            result.slot_bindings.push(ExpandedField {
                                name: slot_binding_target,
                                r#type: expanded.value.expr,
                                raw_type: Some(type_ann.clone()),
                                optional: false,
                                exactness: expanded.exactness,
                                execution_status: expanded.execution_status,
                                diagnostics: expanded.diagnostics,
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
            let expanded = expand_field_expr(engine, type_ann);
            log_expand_stage(
                stage_log,
                expanded.exactness,
                expanded.execution_status,
                &expanded.diagnostics,
                debug_env.as_deref(),
            );
            result.bindings.push(ExpandedField {
                name: name.clone(),
                r#type: expanded.value.expr,
                raw_type: None,
                optional: false,
                exactness: expanded.exactness,
                execution_status: expanded.execution_status,
                diagnostics: expanded.diagnostics,
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

/// Solver-based macro object-shape production for the **standalone** path only
/// (WASM, playground, EvalEnv-backed tests).
///
/// The session path (`verter_session`) uses the projection-first pipeline in
/// `meta_resolve::produce_macro_object_shapes` instead. This function must NOT
/// be called from the session path.
fn expand_standalone_macro_object_shapes(
    macros: &[crate::analysis::types::AnalyzedMacro],
    source: Option<&str>,
    result: &mut crate::analysis::type_expand::ExpandedComponentTypes,
    engine: &mut crate::analysis::type_solver::query_engine::TypeQueryEngine<'_>,
) {
    use crate::analysis::type_expand::{
        solver_result_to_object_expansion, ExpandedMacroObjectShape, ExpandedMacroProps,
    };

    let macro_type_params = source.map(collect_define_macro_type_params);
    let mut define_props_index = 0usize;
    let mut define_emits_index = 0usize;
    let mut define_slots_index = 0usize;

    for (macro_index, m) in macros.iter().enumerate() {
        if m.kind == crate::analysis::types::AnalyzedMacroKind::DefineProps && m.is_type_based {
            if let Some(type_params) = macro_type_params
                .as_ref()
                .map(|params| &params.define_props)
            {
                if let Some(lowered) = type_params.get(define_props_index) {
                    let solved = engine.solve(lowered);
                    let shape_result = solver_result_to_object_expansion(solved);
                    if !shape_result.value.properties.is_empty()
                        || !shape_result.value.index_signatures.is_empty()
                    {
                        result.define_props.push(ExpandedMacroProps {
                            macro_index,
                            result: shape_result,
                        });
                    }
                }
            }
            define_props_index += 1;
        }

        if m.kind == crate::analysis::types::AnalyzedMacroKind::DefineEmits && m.is_type_based {
            if let Some(type_params) = macro_type_params
                .as_ref()
                .map(|params| &params.define_emits)
            {
                if let Some(lowered) = type_params.get(define_emits_index) {
                    let solved = engine.solve(lowered);
                    let shape_result = solver_result_to_object_expansion(solved);
                    if has_named_shape_surface(&shape_result.value) {
                        result.define_emits.push(ExpandedMacroObjectShape {
                            macro_index,
                            result: shape_result,
                        });
                    }
                }
            }
            define_emits_index += 1;
        }

        if m.kind == crate::analysis::types::AnalyzedMacroKind::DefineSlots && m.is_type_based {
            if let Some(type_params) = macro_type_params
                .as_ref()
                .map(|params| &params.define_slots)
            {
                if let Some(lowered) = type_params.get(define_slots_index) {
                    if m.slot_fields.is_empty() {
                        let mut solved = engine.solve(lowered);
                        solved.value = deep_resolve_slot_function_refs(&solved.value, engine);
                        let shape_result = solver_result_to_object_expansion(solved);
                        if !shape_result.value.properties.is_empty() {
                            result.define_slots.push(ExpandedMacroObjectShape {
                                macro_index,
                                result: shape_result,
                            });
                        }
                    }
                }
            }
            define_slots_index += 1;
        }
    }
}

pub fn has_named_shape_surface(shape: &crate::analysis::type_expand::ExpandedObjectShape) -> bool {
    !shape.properties.is_empty() || !shape.call_signatures.is_empty()
}

/// Deep-resolve remaining `Ref` nodes inside function signatures of a solved
/// slot object.  The solver intentionally keeps function parameter and return
/// types shallow (substitutions only, no ref expansion).  For `defineSlots`
/// results we need refs like `VNode` fully resolved to their object bodies.
pub fn deep_resolve_slot_function_refs(
    expr: &TypeExpr,
    engine: &mut crate::analysis::type_solver::query_engine::TypeQueryEngine<'_>,
) -> TypeExpr {
    if let TypeExpr::Object(obj) = expr {
        let properties: Vec<_> = obj
            .properties
            .iter()
            .map(|member| match member {
                crate::analysis::type_expr::ObjectMember::Property(p) => {
                    crate::analysis::type_expr::ObjectMember::Property(
                        crate::analysis::type_expr::ObjectProperty {
                            name: p.name.clone(),
                            ty: resolve_type_refs_deep(&p.ty, engine),
                            optional: p.optional,
                            readonly: p.readonly,
                        },
                    )
                }
                crate::analysis::type_expr::ObjectMember::Method(m) => {
                    crate::analysis::type_expr::ObjectMember::Method(
                        crate::analysis::type_expr::MethodSignature {
                            name: m.name.clone(),
                            function: resolve_fn_refs_deep(&m.function, engine),
                            optional: m.optional,
                        },
                    )
                }
                other => other.clone(),
            })
            .collect();
        TypeExpr::Object(std::sync::Arc::new(
            crate::analysis::type_expr::ObjectExpr { properties },
        ))
    } else {
        expr.clone()
    }
}

fn resolve_type_refs_deep(
    expr: &TypeExpr,
    engine: &mut crate::analysis::type_solver::query_engine::TypeQueryEngine<'_>,
) -> TypeExpr {
    match expr {
        TypeExpr::Ref { .. } => engine.solve(expr).value,
        TypeExpr::Function(func) => {
            TypeExpr::Function(std::sync::Arc::new(resolve_fn_refs_deep(func, engine)))
        }
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: std::sync::Arc::new(resolve_type_refs_deep(element, engine)),
            readonly: *readonly,
        },
        TypeExpr::Union(variants) => {
            let resolved: Vec<TypeExpr> = variants
                .iter()
                .map(|v| resolve_type_refs_deep(v, engine))
                .collect();
            TypeExpr::Union(std::sync::Arc::from(resolved))
        }
        TypeExpr::Intersection(parts) => {
            let resolved: Vec<TypeExpr> = parts
                .iter()
                .map(|p| resolve_type_refs_deep(p, engine))
                .collect();
            TypeExpr::Intersection(std::sync::Arc::from(resolved))
        }
        _ => expr.clone(),
    }
}

fn resolve_fn_refs_deep(
    func: &crate::analysis::type_expr::FunctionExpr,
    engine: &mut crate::analysis::type_solver::query_engine::TypeQueryEngine<'_>,
) -> crate::analysis::type_expr::FunctionExpr {
    crate::analysis::type_expr::FunctionExpr {
        parameters: func
            .parameters
            .iter()
            .map(|p| crate::analysis::type_expr::FunctionParam {
                name: p.name.clone(),
                ty: resolve_type_refs_deep(&p.ty, engine),
                optional: p.optional,
                rest: p.rest,
            })
            .collect(),
        return_type: func
            .return_type
            .as_ref()
            .map(|rt| std::sync::Arc::new(resolve_type_refs_deep(rt, engine))),
        type_parameters: func.type_parameters.clone(),
    }
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

/// Parse and evaluate a value expression against an existing evaluation environment.
pub fn evaluate_value_expression(expression: &str, env: &mut EvalEnv) -> Option<TypeExpr> {
    let lowered = parse_value_expression_type(expression)?;
    Some(crate::analysis::type_eval::evaluate(&lowered, env))
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
