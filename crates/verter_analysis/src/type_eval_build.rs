//! Build an [`EvalEnv`] from an OXC program AST.
//!
//! Walks top-level declarations and populates the type and value
//! symbol tables so the evaluator can resolve references.

use oxc_ast::ast::{
    Argument, ArrowFunctionExpression, BindingPattern, CallExpression, Class, ClassElement,
    Declaration, ExportDefaultDeclarationKind, Expression, FormalParameters, Function,
    MethodDefinitionKind, ObjectExpression, ObjectPropertyKind, Program, Statement,
    TSAccessibility, TSInterfaceDeclaration, TSSignature, TSTypeAliasDeclaration,
    TSTypeParameterDeclaration, VariableDeclarationKind, VariableDeclarator,
};
use oxc_span::GetSpan;

use crate::type_eval::*;
use crate::type_expr::{
    self, FunctionExpr, FunctionParam, IndexSignature, MethodSignature, ObjectExpr, ObjectMember,
    PrimitiveName, TypeExpr, TypeParam,
};
use crate::type_expr_lower::{has_immediate_vue_ignore_comment, lower_ts_type, property_key_name};

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
                _ => {}
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
    let mut body = TypeExpr::Object(ObjectExpr {
        properties: members,
    });

    if !decl.extends.is_empty() {
        let mut parts = Vec::new();
        for heritage in &decl.extends {
            if has_immediate_vue_ignore_comment(source, heritage.span().start) {
                continue;
            }
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
                                return_type: func.return_type.map(Box::new),
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

    let body = TypeExpr::Object(ObjectExpr {
        properties: members,
    });

    env.add_type(TypeDeclInfo {
        name: name.clone(),
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
            return_type: constructor_signature.return_type.clone().map(Box::new),
            type_parameters: constructor_signature.type_parameters.clone(),
        })],
    };

    env.add_value(ValueDeclInfo {
        name,
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
    let type_annotation = decl
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
    }

    env.add_value(ValueDeclInfo {
        name,
        kind: var_kind,
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
        Expression::StringLiteral(s) => TypeExpr::string_literal(s.value.as_str()),
        Expression::NumericLiteral(n) => TypeExpr::number_literal(n.value),
        Expression::BooleanLiteral(b) => TypeExpr::boolean_literal(b.value),
        Expression::NullLiteral(_) => TypeExpr::Primitive(PrimitiveName::Null),
        Expression::ArrayExpression(_) => TypeExpr::Array {
            element: Box::new(TypeExpr::Primitive(PrimitiveName::Any)),
            readonly: false,
        },
        Expression::ObjectExpression(obj) => TypeExpr::Object(extract_object_literal(obj, source)),
        Expression::ArrowFunctionExpression(arrow) => {
            let sig = extract_arrow_signature(arrow, source);
            TypeExpr::Function(FunctionExpr {
                parameters: sig.parameters,
                return_type: sig.return_type.map(Box::new),
                type_parameters: sig.type_parameters,
            })
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
                    return_type: return_type.map(Box::new),
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
                return_type: return_type.map(Box::new),
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
                return_type: return_type.map(Box::new),
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
                .map(|c| Box::new(lower_ts_type(c, source))),
            default: p
                .default
                .as_ref()
                .map(|d| Box::new(lower_ts_type(d, source))),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Snapshot evaluation: evaluate type annotations from an analysis snapshot
// ---------------------------------------------------------------------------

/// Evaluated type annotations for a component's metadata fields.
///
/// Serialized alongside the analysis snapshot so JS consumers can
/// use structured types instead of parsing raw type annotation strings.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedComponentTypes {
    /// Evaluated prop types, keyed by prop name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<EvaluatedField>,
    /// Evaluated full defineProps object shapes keyed by macro index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub define_props: Vec<EvaluatedMacroProps>,
    /// Evaluated emit payload types, keyed by event name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emits: Vec<EvaluatedField>,
    /// Evaluated slot binding types, keyed by "slotName.bindingName".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_bindings: Vec<EvaluatedField>,
    /// Evaluated binding types (for expose/value lookups), keyed by binding name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<EvaluatedField>,
}

/// A single evaluated type field.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedField {
    /// The field name (prop name, event name, or slot.binding key).
    pub name: String,
    /// The evaluated type expression.
    pub r#type: TypeExpr,
    /// Whether the source field is optional.
    #[serde(default)]
    pub optional: bool,
}

/// Evaluated full prop object for a specific defineProps macro.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedMacroProps {
    pub macro_index: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<EvaluatedField>,
}

/// Evaluate all type annotations in the given macros list.
///
/// Builds an `EvalEnv` from the source, then evaluates each prop/emit/slot
/// type annotation and returns the structured results.
///
/// Works with macros from either `ScriptAnalysisSnapshot` or `FileAnalysisSnapshot`.
pub fn evaluate_macro_types(
    macros: &[crate::types::AnalyzedMacro],
    source: &str,
) -> EvaluatedComponentTypes {
    let mut env = parse_and_build_env(source);
    evaluate_macro_types_with_env_and_source(macros, source, &mut env)
}

/// Evaluate all macro-backed type annotations with a caller-provided environment.
///
/// This lets higher layers extend the environment with imported declarations
/// or cached project state before evaluating the macro type annotations.
pub fn evaluate_macro_types_with_env(
    macros: &[crate::types::AnalyzedMacro],
    env: &mut EvalEnv,
) -> EvaluatedComponentTypes {
    evaluate_macro_types_impl(macros, None, env, None)
}

/// Evaluate macro-backed type annotations and the full defineProps macro type.
///
/// Parses the source to recover each defineProps type parameter so callers can
/// synthesize prop names even when `AnalyzedMacro.prop_fields` is empty or partial.
pub fn evaluate_macro_types_with_env_and_source(
    macros: &[crate::types::AnalyzedMacro],
    source: &str,
    env: &mut EvalEnv,
) -> EvaluatedComponentTypes {
    evaluate_macro_types_impl(macros, Some(source), env, None)
}

pub fn evaluate_macro_types_with_env_and_source_and_local_bindings(
    macros: &[crate::types::AnalyzedMacro],
    source: &str,
    env: &mut EvalEnv,
    local_binding_names: &rustc_hash::FxHashSet<String>,
) -> EvaluatedComponentTypes {
    evaluate_macro_types_impl(macros, Some(source), env, Some(local_binding_names))
}

fn evaluate_macro_types_impl(
    macros: &[crate::types::AnalyzedMacro],
    source: Option<&str>,
    env: &mut EvalEnv,
    local_binding_names: Option<&rustc_hash::FxHashSet<String>>,
) -> EvaluatedComponentTypes {
    use crate::type_eval::evaluate;
    use crate::type_expr_lower::parse_type_annotation;

    let mut result = EvaluatedComponentTypes::default();
    let define_props_type_params = source.map(collect_define_props_type_params);
    let mut define_props_index = 0usize;

    for (macro_index, m) in macros.iter().enumerate() {
        // Evaluate prop field types
        for field in &m.prop_fields {
            if let Some(ref type_ann) = field.type_annotation {
                let parsed = parse_type_annotation(type_ann);
                if !parsed.is_unknown() {
                    let evaluated = evaluate(&parsed, env);
                    result.props.push(EvaluatedField {
                        name: field.name.clone(),
                        r#type: evaluated,
                        optional: field.is_optional,
                    });
                }
            }
        }

        if m.kind == crate::types::AnalyzedMacroKind::DefineProps && m.is_type_based {
            if let Some(type_params) = define_props_type_params.as_ref() {
                if let Some(lowered) = type_params.get(define_props_index) {
                    let saved_max_depth = env.limits.max_depth;
                    env.limits.max_depth = env.limits.max_depth.min(8);
                    let evaluated = evaluate(lowered, env);
                    env.limits.max_depth = saved_max_depth;
                    let fields = collect_define_props_fields(&evaluated);
                    if !fields.is_empty() {
                        result.define_props.push(EvaluatedMacroProps {
                            macro_index,
                            fields,
                        });
                    }
                }
            }
            define_props_index += 1;
        }

        // Evaluate emit payload types
        for field in &m.emit_fields {
            if let Some(ref payload) = field.payload_type {
                let parsed = parse_type_annotation(payload);
                if !parsed.is_unknown() {
                    let evaluated = evaluate(&parsed, env);
                    result.emits.push(EvaluatedField {
                        name: field.name.clone(),
                        r#type: evaluated,
                        optional: false,
                    });
                }
            }
        }

        // Evaluate slot binding types
        for slot in &m.slot_fields {
            for binding in &slot.bindings {
                if let Some(ref type_ann) = binding.type_annotation {
                    let parsed = parse_type_annotation(type_ann);
                    if !parsed.is_unknown() && should_evaluate_slot_binding_type(&parsed) {
                        let evaluated = evaluate(&parsed, env);
                        result.slot_bindings.push(EvaluatedField {
                            name: format!("{}.{}", slot.name, binding.name),
                            r#type: evaluated,
                            optional: false,
                        });
                    }
                }
            }
        }
    }

    // Evaluate binding type annotations (for expose/value lookups)
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
        let evaluated = evaluate(&type_ann, env);
        result.bindings.push(EvaluatedField {
            name,
            r#type: evaluated,
            optional: false,
        });
    }

    result
}

fn should_evaluate_slot_binding_type(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) | TypeExpr::Unknown { .. } => true,
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
            should_evaluate_slot_binding_type(inner)
        }
        TypeExpr::Array { element, .. } => should_evaluate_slot_binding_type(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| should_evaluate_slot_binding_type(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().all(should_evaluate_slot_binding_type)
        }
        TypeExpr::Object(obj) => obj.properties.iter().all(|member| match member {
            ObjectMember::Property(prop) => should_evaluate_slot_binding_type(&prop.ty),
            ObjectMember::Method(method) => {
                method
                    .function
                    .parameters
                    .iter()
                    .all(|param| should_evaluate_slot_binding_type(&param.ty))
                    && method
                        .function
                        .return_type
                        .as_ref()
                        .map(|ret| should_evaluate_slot_binding_type(ret))
                        .unwrap_or(true)
            }
            ObjectMember::IndexSignature(sig) => {
                should_evaluate_slot_binding_type(&sig.key_type)
                    && should_evaluate_slot_binding_type(&sig.value_type)
            }
            ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                func.parameters
                    .iter()
                    .all(|param| should_evaluate_slot_binding_type(&param.ty))
                    && func
                        .return_type
                        .as_ref()
                        .map(|ret| should_evaluate_slot_binding_type(ret))
                        .unwrap_or(true)
            }
        }),
        TypeExpr::Function(func) => {
            func.parameters
                .iter()
                .all(|param| should_evaluate_slot_binding_type(&param.ty))
                && func
                    .return_type
                    .as_ref()
                    .map(|ret| should_evaluate_slot_binding_type(ret))
                    .unwrap_or(true)
        }
        TypeExpr::Ref { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TemplateLiteral { .. }
        | TypeExpr::Infer { .. } => false,
    }
}

fn collect_define_props_fields(ty: &TypeExpr) -> Vec<EvaluatedField> {
    let variants = collect_define_props_variants(ty);
    if variants.is_empty() {
        return Vec::new();
    }

    #[derive(Default)]
    struct FieldState {
        present_in: usize,
        optional: bool,
        types: Vec<TypeExpr>,
    }

    let mut order = Vec::<String>::new();
    let mut states = std::collections::HashMap::<String, FieldState>::new();

    for variant in &variants {
        let mut seen_in_variant = std::collections::HashSet::<String>::new();
        for member in &variant.properties {
            let ObjectMember::Property(prop) = member else {
                continue;
            };

            let state = states.entry(prop.name.clone()).or_insert_with(|| {
                order.push(prop.name.clone());
                FieldState::default()
            });
            if seen_in_variant.insert(prop.name.clone()) {
                state.present_in += 1;
            }
            state.optional |= prop.optional;
            if !state.types.iter().any(|existing| existing == &prop.ty) {
                state.types.push(prop.ty.clone());
            }
        }
    }

    let variant_count = variants.len();
    order
        .into_iter()
        .filter_map(|name| {
            let state = states.remove(&name)?;
            Some(EvaluatedField {
                name,
                r#type: TypeExpr::union(state.types),
                optional: state.optional || state.present_in < variant_count,
            })
        })
        .collect()
}

fn collect_define_props_variants(ty: &TypeExpr) -> Vec<ObjectExpr> {
    match ty {
        TypeExpr::Union(types) => types
            .iter()
            .flat_map(collect_define_props_variants)
            .collect(),
        TypeExpr::Parenthesized(inner) => collect_define_props_variants(inner),
        _ => extract_object_shape(ty).into_iter().collect(),
    }
}

fn collect_define_props_type_params(source: &str) -> Vec<TypeExpr> {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn is_define_props_call(call: &CallExpression<'_>) -> bool {
        matches!(&call.callee, Expression::Identifier(id) if id.name == "defineProps")
    }

    fn walk_expr(expr: &Expression<'_>, source: &str, result: &mut Vec<TypeExpr>) {
        match expr {
            Expression::CallExpression(call) => {
                if is_define_props_call(call) {
                    if let Some(type_args) = &call.type_arguments {
                        if let Some(first) = type_args.params.first() {
                            result.push(lower_ts_type(first, source));
                        }
                    }
                }
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

    fn walk_stmt(stmt: &Statement<'_>, source: &str, result: &mut Vec<TypeExpr>) {
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
    let mut result = Vec::new();
    for stmt in &ret.program.body {
        walk_stmt(stmt, source, &mut result);
    }
    result
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
