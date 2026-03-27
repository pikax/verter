//! Build an [`EvalEnv`] from an OXC program AST.
//!
//! Walks top-level declarations and populates the type and value
//! symbol tables so the evaluator can resolve references.

use std::io::Write;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use crate::type_eval::*;
use crate::type_expr::{
    self, FunctionExpr, FunctionParam, IndexSignature, MethodSignature, ObjectExpr, ObjectMember,
    PrimitiveName, TypeExpr, TypeParam, ValueRef,
};
use crate::type_expr_lower::{lower_ts_type, property_key_name};
use oxc_ast::ast::{
    Argument, ArrowFunctionExpression, BindingPattern, CallExpression, Class, ClassElement,
    Declaration, ExportDefaultDeclarationKind, Expression, FormalParameters, Function,
    MethodDefinitionKind, ObjectExpression, ObjectPropertyKind, Program, Statement,
    TSAccessibility, TSInterfaceDeclaration, TSSignature, TSTypeAliasDeclaration,
    TSTypeParameterDeclaration, VariableDeclarationKind, VariableDeclarator,
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
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                function_signature = Some(extract_arrow_signature(arrow, source));
            }
            Expression::FunctionExpression(func) => {
                function_signature = Some(extract_function_signature(func, source));
            }
            Expression::ObjectExpression(obj) => {
                object_shape = Some(extract_object_literal(obj, source));
            }
            _ => {}
        }

        if type_annotation.is_none() {
            let inferred = infer_expression_type(init, source);
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
    let function_signature = match expr {
        Expression::ArrowFunctionExpression(arrow) => Some(extract_arrow_signature(arrow, source)),
        Expression::FunctionExpression(func) => Some(extract_function_signature(func, source)),
        _ => None,
    };
    let object_shape = match expr {
        Expression::ObjectExpression(obj) => Some(extract_object_literal(obj, source)),
        _ => None,
    };
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
                    // Try to get type from type annotation or infer from value
                    let ty = infer_expression_type(&p.value, source);
                    members.push(ObjectMember::Property(type_expr::ObjectProperty {
                        name,
                        ty,
                        optional: false,
                        readonly: false,
                    }));
                }
            }
            ObjectPropertyKind::SpreadProperty(_) => {
                // Can't statically extract spread properties
            }
        }
    }
    ObjectExpr {
        properties: members,
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
        Expression::ArrayExpression(_) => TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::Any)),
            readonly: false,
        },
        Expression::ObjectExpression(obj) => {
            TypeExpr::Object(Arc::new(extract_object_literal(obj, source)))
        }
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            let mut value = String::new();
            for quasi in &tpl.quasis {
                value.push_str(quasi.value.raw.as_str());
            }
            TypeExpr::string_literal(value)
        }
        Expression::ArrowFunctionExpression(arrow) => {
            let sig = extract_arrow_signature(arrow, source);
            TypeExpr::Function(Arc::new(FunctionExpr {
                parameters: sig.parameters,
                return_type: sig.return_type.map(Arc::new),
                type_parameters: sig.type_parameters,
            }))
        }
        Expression::TSAsExpression(ts_as) => {
            // const x = value as SomeType → use the asserted type
            lower_ts_type(&ts_as.type_annotation, source)
        }
        Expression::TSSatisfiesExpression(sat) => {
            // const x = value satisfies SomeType → use the satisfies type
            lower_ts_type(&sat.type_annotation, source)
        }
        _ => TypeExpr::Primitive(PrimitiveName::Any),
    }
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
/// type annotation using the new expansion service with default budget.
pub fn evaluate_macro_types(
    macros: &[crate::types::AnalyzedMacro],
    source: &str,
) -> crate::type_expand::ExpandedComponentTypes {
    let mut env = parse_and_build_env(source);
    let budget = crate::type_expand::ExpansionBudget::default();
    expand_macro_types(macros, Some(source), &mut env, None, &budget)
}

// ---------------------------------------------------------------------------
// Expansion-based macro type evaluation
// ---------------------------------------------------------------------------

/// Expand all macro-backed type annotations using the new expander service.
///
/// Replaces `evaluate_macro_types_impl`. Uses `expand_object_shape` for
/// defineProps type parameters and `expand_normalized_expr` for individual
/// prop/emit/slot/binding annotations.
pub fn expand_macro_types(
    macros: &[crate::types::AnalyzedMacro],
    source: Option<&str>,
    env: &mut EvalEnv,
    local_binding_names: Option<&rustc_hash::FxHashSet<String>>,
    budget: &crate::type_expand::ExpansionBudget,
) -> crate::type_expand::ExpandedComponentTypes {
    let mut lookup = crate::type_eval::NoopEvalLookup;
    expand_macro_types_with_lookup(
        macros,
        source,
        env,
        local_binding_names,
        budget,
        &mut lookup,
    )
}

pub fn expand_macro_types_with_lookup(
    macros: &[crate::types::AnalyzedMacro],
    source: Option<&str>,
    env: &mut EvalEnv,
    local_binding_names: Option<&rustc_hash::FxHashSet<String>>,
    budget: &crate::type_expand::ExpansionBudget,
    lookup: &mut dyn crate::type_eval::EvalLookup,
) -> crate::type_expand::ExpandedComponentTypes {
    use crate::type_expand::{
        expand_normalized_expr_with_lookup, expand_object_shape_with_lookup,
        ExpandedComponentTypes, ExpandedField, ExpandedMacroObjectShape, ExpandedMacroProps,
    };
    use crate::type_expr_lower::parse_type_annotation;

    let mut result = ExpandedComponentTypes::default();
    let macro_type_params = source.map(collect_define_macro_type_params);
    let mut define_props_index = 0usize;
    let mut define_emits_index = 0usize;
    let mut define_slots_index = 0usize;
    let started = Instant::now();
    let start_steps = env.steps();

    type_expand_debug(|| {
        format!(
            "expand_macro_types:start macros={} source_present={} local_binding_filter={} steps={}",
            macros.len(),
            source.is_some(),
            local_binding_names.map(|names| names.len()).unwrap_or(0),
            start_steps,
        )
    });

    for (macro_index, m) in macros.iter().enumerate() {
        // Expand prop field type annotations
        for field in &m.prop_fields {
            if let Some(ref type_ann) = field.type_annotation {
                let parsed = parse_type_annotation(type_ann);
                if !parsed.is_unknown() {
                    let expanded = expand_normalized_expr_with_lookup(&parsed, env, budget, lookup);
                    result.props.push(ExpandedField {
                        name: field.name.clone(),
                        r#type: expanded.value.expr,
                        raw_type: Some(type_ann.clone()),
                        optional: field.is_optional,
                        completeness: expanded.completeness,
                        diagnostics: expanded.diagnostics,
                    });
                }
            }
        }

        // Expand defineProps<T>() type parameter into object shape
        if m.kind == crate::types::AnalyzedMacroKind::DefineProps && m.is_type_based {
            if let Some(type_params) = macro_type_params
                .as_ref()
                .map(|params| &params.define_props)
            {
                if let Some(lowered) = type_params.get(define_props_index) {
                    let shape_result =
                        expand_object_shape_with_lookup(lowered, env, budget, lookup);
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

        if m.kind == crate::types::AnalyzedMacroKind::DefineEmits && m.is_type_based {
            if let Some(type_params) = macro_type_params
                .as_ref()
                .map(|params| &params.define_emits)
            {
                if let Some(lowered) = type_params.get(define_emits_index) {
                    let shape_result =
                        expand_object_shape_with_lookup(lowered, env, budget, lookup);
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

        // Expand emit payload types
        for field in &m.emit_fields {
            if let Some(ref payload) = field.payload_type {
                let parsed = parse_type_annotation(payload);
                if !parsed.is_unknown() {
                    let expanded = expand_normalized_expr_with_lookup(&parsed, env, budget, lookup);
                    result.emits.push(ExpandedField {
                        name: field.name.clone(),
                        r#type: expanded.value.expr,
                        raw_type: Some(payload.clone()),
                        optional: false,
                        completeness: expanded.completeness,
                        diagnostics: expanded.diagnostics,
                    });
                }
            }
        }

        if m.kind == crate::types::AnalyzedMacroKind::DefineSlots && m.is_type_based {
            if let Some(type_params) = macro_type_params
                .as_ref()
                .map(|params| &params.define_slots)
            {
                if let Some(lowered) = type_params.get(define_slots_index) {
                    let shape_result =
                        expand_object_shape_with_lookup(lowered, env, budget, lookup);
                    if !shape_result.value.properties.is_empty() {
                        result.define_slots.push(ExpandedMacroObjectShape {
                            macro_index,
                            result: shape_result,
                        });
                    }
                }
            }
            define_slots_index += 1;
        }

        // Expand slot binding types (no skip heuristic — expander handles complexity)
        for slot in &m.slot_fields {
            for binding in &slot.bindings {
                if let Some(ref type_ann) = binding.type_annotation {
                    let parsed = parse_type_annotation(type_ann);
                    if !parsed.is_unknown() {
                        let expanded =
                            expand_normalized_expr_with_lookup(&parsed, env, budget, lookup);
                        result.slot_bindings.push(ExpandedField {
                            name: format!("{}.{}", slot.name, binding.name),
                            r#type: expanded.value.expr,
                            raw_type: Some(type_ann.clone()),
                            optional: false,
                            completeness: expanded.completeness,
                            diagnostics: expanded.diagnostics,
                        });
                    }
                }
            }
        }
    }

    // Expand binding type annotations (for expose/value lookups)
    let binding_entries: Vec<(String, TypeExpr)> = env
        .value_symbols
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
        .collect();
    for (name, type_ann) in binding_entries {
        let expanded = expand_normalized_expr_with_lookup(&type_ann, env, budget, lookup);
        result.bindings.push(ExpandedField {
            name,
            r#type: expanded.value.expr,
            raw_type: None,
            optional: false,
            completeness: expanded.completeness,
            diagnostics: expanded.diagnostics,
        });
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
            env.steps().saturating_sub(start_steps),
            env.budget_exhausted(),
            started.elapsed(),
        )
    });

    result
}

fn has_named_shape_surface(shape: &crate::type_expand::ExpandedObjectShape) -> bool {
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

/// Parse and evaluate a value expression against an existing evaluation environment.
pub fn evaluate_value_expression(expression: &str, env: &mut EvalEnv) -> Option<TypeExpr> {
    let mut lookup = crate::type_eval::NoopEvalLookup;
    evaluate_value_expression_with_lookup(expression, env, &mut lookup)
}

pub fn evaluate_value_expression_with_lookup(
    expression: &str,
    env: &mut EvalEnv,
    lookup: &mut dyn crate::type_eval::EvalLookup,
) -> Option<TypeExpr> {
    let lowered = parse_value_expression_type(expression)?;
    Some(crate::type_eval::evaluate_with_lookup(
        &lowered, env, lookup,
    ))
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
