use oxc_ast::ast::*;
use oxc_ast::{Comment, CommentContent};
use oxc_span::GetSpan;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::types::{
    AnalyzedDefaultValue, AnalyzedEmitField, AnalyzedExposeField, AnalyzedMacro, AnalyzedMacroKind,
    AnalyzedPropField, AnalyzedSlotField, AnalyzedSlotFieldBinding, JsdocTag, ResolvedLocalType,
    TypeResolutionSource,
};

/// Classify a callee name as a Vue compiler macro.
fn classify_macro(name: &str) -> Option<AnalyzedMacroKind> {
    match name {
        "defineProps" => Some(AnalyzedMacroKind::DefineProps),
        "defineEmits" => Some(AnalyzedMacroKind::DefineEmits),
        "defineModel" => Some(AnalyzedMacroKind::DefineModel),
        "defineExpose" => Some(AnalyzedMacroKind::DefineExpose),
        "defineOptions" => Some(AnalyzedMacroKind::DefineOptions),
        "defineSlots" => Some(AnalyzedMacroKind::DefineSlots),
        "withDefaults" => Some(AnalyzedMacroKind::WithDefaults),
        _ => None,
    }
}

/// Collect all type reference names from a TypeScript type annotation.
///
/// Walks recursively through the type AST and collects every `TSTypeReference`
/// identifier name. This includes both user-defined types and built-in generics
/// like `Partial`, `Required`, etc.
///
/// # Examples
/// - `{foo: MyType}` → `["MyType"]`
/// - `MyType` → `["MyType"]`
/// - `MyType & OtherType` → `["MyType", "OtherType"]`
/// - `{foo: string, bar: number}` → `[]`
/// - `Partial<MyType>` → `["Partial", "MyType"]`
pub fn collect_type_references(ts_type: &TSType<'_>) -> Vec<String> {
    let mut refs = Vec::new();
    collect_type_references_recursive(ts_type, &mut refs);
    refs
}

fn collect_type_references_recursive(ts_type: &TSType<'_>, refs: &mut Vec<String>) {
    match ts_type {
        TSType::TSTypeReference(type_ref) => {
            // Collect the type name
            match &type_ref.type_name {
                TSTypeName::IdentifierReference(id) => {
                    refs.push(id.name.to_string());
                }
                TSTypeName::QualifiedName(qualified) => {
                    collect_qualified_name_root(qualified, refs);
                }
                _ => {}
            }
            // Also recurse into type arguments: e.g., Partial<MyType>
            if let Some(params) = &type_ref.type_arguments {
                for param in &params.params {
                    collect_type_references_recursive(param, refs);
                }
            }
        }
        TSType::TSUnionType(union_type) => {
            for ty in &union_type.types {
                collect_type_references_recursive(ty, refs);
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                collect_type_references_recursive(ty, refs);
            }
        }
        TSType::TSTypeLiteral(literal) => {
            for member in &literal.members {
                match member {
                    TSSignature::TSPropertySignature(prop) => {
                        if let Some(ref ta) = prop.type_annotation {
                            collect_type_references_recursive(&ta.type_annotation, refs);
                        }
                    }
                    TSSignature::TSMethodSignature(method) => {
                        if let Some(ref ret) = method.return_type {
                            collect_type_references_recursive(&ret.type_annotation, refs);
                        }
                        // In OXC 0.112, type annotations are on FormalParameter, not BindingPattern
                        for param in &method.params.items {
                            if let Some(ref ta) = param.type_annotation {
                                collect_type_references_recursive(&ta.type_annotation, refs);
                            }
                        }
                    }
                    TSSignature::TSCallSignatureDeclaration(call) => {
                        if let Some(ref ret) = call.return_type {
                            collect_type_references_recursive(&ret.type_annotation, refs);
                        }
                        for param in &call.params.items {
                            if let Some(ref ta) = param.type_annotation {
                                collect_type_references_recursive(&ta.type_annotation, refs);
                            }
                        }
                    }
                    TSSignature::TSIndexSignature(idx) => {
                        collect_type_references_recursive(
                            &idx.type_annotation.type_annotation,
                            refs,
                        );
                    }
                    TSSignature::TSConstructSignatureDeclaration(_) => {}
                }
            }
        }
        TSType::TSArrayType(arr) => {
            collect_type_references_recursive(&arr.element_type, refs);
        }
        TSType::TSTupleType(tuple) => {
            for elem in &tuple.element_types {
                match elem {
                    TSTupleElement::TSOptionalType(opt) => {
                        collect_type_references_recursive(&opt.type_annotation, refs);
                    }
                    TSTupleElement::TSRestType(rest) => {
                        collect_type_references_recursive(&rest.type_annotation, refs);
                    }
                    TSTupleElement::TSNamedTupleMember(named) => {
                        if let Some(t) = named.element_type.as_ts_type() {
                            collect_type_references_recursive(t, refs);
                        }
                    }
                    _ => {
                        if let Some(t) = elem.as_ts_type() {
                            collect_type_references_recursive(t, refs);
                        }
                    }
                }
            }
        }
        TSType::TSConditionalType(cond) => {
            collect_type_references_recursive(&cond.check_type, refs);
            collect_type_references_recursive(&cond.extends_type, refs);
            collect_type_references_recursive(&cond.true_type, refs);
            collect_type_references_recursive(&cond.false_type, refs);
        }
        TSType::TSMappedType(mapped) => {
            // In OXC 0.112, constraint is directly on TSMappedType (not optional)
            collect_type_references_recursive(&mapped.constraint, refs);
            if let Some(ref type_annotation) = mapped.type_annotation {
                collect_type_references_recursive(type_annotation, refs);
            }
        }
        TSType::TSIndexedAccessType(idx) => {
            collect_type_references_recursive(&idx.object_type, refs);
            collect_type_references_recursive(&idx.index_type, refs);
        }
        TSType::TSTypeOperatorType(op) => {
            collect_type_references_recursive(&op.type_annotation, refs);
        }
        TSType::TSParenthesizedType(paren) => {
            collect_type_references_recursive(&paren.type_annotation, refs);
        }
        TSType::TSTemplateLiteralType(tpl) => {
            for ty in &tpl.types {
                collect_type_references_recursive(ty, refs);
            }
        }
        TSType::TSFunctionType(func) => {
            // return_type is Box<TSTypeAnnotation>, not optional
            collect_type_references_recursive(&func.return_type.type_annotation, refs);
            for param in &func.params.items {
                if let Some(ref ta) = param.type_annotation {
                    collect_type_references_recursive(&ta.type_annotation, refs);
                }
            }
        }
        TSType::TSConstructorType(ctor) => {
            // return_type is Box<TSTypeAnnotation>, not optional
            collect_type_references_recursive(&ctor.return_type.type_annotation, refs);
        }
        TSType::TSInferType(_) => {}
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                refs.push(ident.name.to_string());
            }
        }
        TSType::TSImportType(_) => {}
        // Primitives and literals — no type references
        _ => {}
    }
}

fn collect_qualified_name_root(name: &TSQualifiedName<'_>, refs: &mut Vec<String>) {
    match &name.left {
        TSTypeName::IdentifierReference(id) => {
            refs.push(id.name.to_string());
        }
        TSTypeName::QualifiedName(inner) => {
            collect_qualified_name_root(inner, refs);
        }
        _ => {}
    }
}

/// Detect Vue macros from a parsed program's body.
/// Returns analyzed macros with type reference information.
#[cfg(test)]
fn analyze_macros_from_program(program: &Program<'_>, source: &str) -> Vec<AnalyzedMacro> {
    let mut macros = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::ExpressionStatement(expr_stmt) => {
                try_extract_macro_from_expr(
                    &expr_stmt.expression,
                    &mut macros,
                    source,
                    &program.comments,
                );
            }
            Statement::VariableDeclaration(var_decl) => {
                for decl in &var_decl.declarations {
                    try_extract_macro_from_var_decl(decl, &mut macros, source, &program.comments);
                }
            }
            _ => {}
        }
    }

    // Post-processing: resolve local type references in prop fields
    resolve_macro_type_references(program, &mut macros, source);

    macros
}

// ── Local type registry for resolving TSTypeReference in defineProps ──

/// A local type declaration found in the same script block.
enum LocalTypeDecl<'a> {
    Interface {
        body: &'a TSInterfaceBody<'a>,
        extends: &'a [TSInterfaceHeritage<'a>],
    },
    Alias(&'a TSType<'a>),
    Class,
}

/// Build a registry of local type declarations from the program.
fn build_local_type_registry<'a>(program: &'a Program<'a>) -> FxHashMap<String, LocalTypeDecl<'a>> {
    let mut registry = FxHashMap::default();
    for stmt in &program.body {
        match stmt {
            Statement::TSInterfaceDeclaration(decl) => {
                let extends: &[TSInterfaceHeritage<'_>] = &decl.extends;
                registry.insert(
                    decl.id.name.to_string(),
                    LocalTypeDecl::Interface {
                        body: &decl.body,
                        extends,
                    },
                );
            }
            Statement::TSTypeAliasDeclaration(decl) => {
                registry.insert(
                    decl.id.name.to_string(),
                    LocalTypeDecl::Alias(&decl.type_annotation),
                );
            }
            Statement::ClassDeclaration(decl) => {
                if let Some(ref id) = decl.id {
                    registry.insert(id.name.to_string(), LocalTypeDecl::Class);
                }
            }
            _ => {}
        }
    }
    registry
}

/// Extract prop fields from an interface body.
fn extract_fields_from_interface_body(
    body: &TSInterfaceBody<'_>,
    source: &str,
    comments: &[Comment],
) -> Vec<AnalyzedPropField> {
    body.body
        .iter()
        .filter_map(|member| {
            if let TSSignature::TSPropertySignature(prop) = member {
                let key_name = match &prop.key {
                    PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                    PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                    _ => None,
                };
                let type_annotation = prop.type_annotation.as_ref().and_then(|ta| {
                    let start = ta.type_annotation.span().start as usize;
                    let end = ta.type_annotation.span().end as usize;
                    if end <= source.len() {
                        let text = source[start..end].trim();
                        if !text.is_empty() {
                            Some(text.to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });
                let (description, tags) = extract_jsdoc_for(comments, prop.span().start, source);
                key_name.map(|name| AnalyzedPropField {
                    name,
                    is_optional: prop.optional,
                    span: prop.key.span().into(),
                    type_annotation,
                    description,
                    tags,
                    resolution_source: TypeResolutionSource::Rust,
                    resolution_error: None,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Resolve prop fields from a TSType using the local type registry.
/// Returns `None` if the type cannot be resolved locally (triggers TS fallback).
fn resolve_type_to_prop_fields(
    ts_type: &TSType<'_>,
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
    visited: &mut FxHashSet<String>,
) -> Option<Vec<AnalyzedPropField>> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => Some(extract_fields_from_interface_body_like(
            &literal.members,
            source,
            comments,
        )),
        TSType::TSTypeReference(ref_type) => {
            let name = type_name_to_string(&ref_type.type_name);

            // Recursion guard
            if visited.contains(&name) {
                return Some(Vec::new());
            }

            // Check for known utility types
            if let Some(ref type_args) = ref_type.type_arguments {
                match name.as_str() {
                    "Partial" => {
                        if let Some(first) = type_args.params.first() {
                            visited.insert(name.clone());
                            let result = resolve_type_to_prop_fields(
                                first, registry, source, comments, visited,
                            );
                            visited.remove(&name);
                            return result.map(|fields| {
                                fields
                                    .into_iter()
                                    .map(|mut f| {
                                        f.is_optional = true;
                                        f
                                    })
                                    .collect()
                            });
                        }
                    }
                    "Required" => {
                        if let Some(first) = type_args.params.first() {
                            visited.insert(name.clone());
                            let result = resolve_type_to_prop_fields(
                                first, registry, source, comments, visited,
                            );
                            visited.remove(&name);
                            return result.map(|fields| {
                                fields
                                    .into_iter()
                                    .map(|mut f| {
                                        f.is_optional = false;
                                        f
                                    })
                                    .collect()
                            });
                        }
                    }
                    "Pick" | "Omit" | "ReturnType" | "InstanceType" | "Record" | "Extract"
                    | "Exclude" | "NonNullable" => {
                        return None; // Unresolvable by Rust
                    }
                    _ => {}
                }
            }

            // Look up in local registry
            visited.insert(name.clone());
            let result = match registry.get(&name) {
                Some(LocalTypeDecl::Interface { body, extends }) => {
                    // Resolve extends chain first via direct registry lookup
                    let mut all_fields = Vec::new();
                    let mut seen_names = FxHashSet::default();
                    for heritage in *extends {
                        let Some(parent_name) = heritage_name(&heritage.expression) else {
                            continue;
                        };
                        if let Some(parent_decl) = registry.get(&parent_name) {
                            if let Some(parent_fields) = resolve_interface_decl(
                                &parent_name,
                                parent_decl,
                                registry,
                                source,
                                comments,
                                visited,
                            ) {
                                for field in parent_fields {
                                    if seen_names.insert(field.name.clone()) {
                                        all_fields.push(field);
                                    }
                                }
                            }
                        }
                    }
                    // Add own fields (child overrides parent)
                    let own_fields = extract_fields_from_interface_body(body, source, comments);
                    for field in own_fields {
                        if seen_names.insert(field.name.clone()) {
                            all_fields.push(field);
                        }
                    }
                    Some(all_fields)
                }
                Some(LocalTypeDecl::Alias(aliased_type)) => {
                    resolve_type_to_prop_fields(aliased_type, registry, source, comments, visited)
                }
                Some(LocalTypeDecl::Class) => None, // Unresolvable
                None => None,                       // Not found locally
            };
            visited.remove(&name);
            result
        }
        TSType::TSIntersectionType(intersection) => {
            let mut all_fields = Vec::new();
            let mut seen_names = FxHashSet::default();
            for t in &intersection.types {
                match resolve_type_to_prop_fields(t, registry, source, comments, visited) {
                    Some(fields) => {
                        for field in fields {
                            if seen_names.insert(field.name.clone()) {
                                all_fields.push(field);
                            }
                        }
                    }
                    None => {
                        // One branch unresolvable — mark the entire intersection as unresolvable
                        return None;
                    }
                }
            }
            Some(all_fields)
        }
        TSType::TSUnionType(_) => None, // Union types aren't prop field sources
        _ => None,
    }
}

/// Extract prop fields from TSSignature members (shared between TSTypeLiteral and interface bodies).
fn extract_fields_from_interface_body_like(
    members: &[TSSignature<'_>],
    source: &str,
    comments: &[Comment],
) -> Vec<AnalyzedPropField> {
    members
        .iter()
        .filter_map(|member| {
            if let TSSignature::TSPropertySignature(prop) = member {
                let key_name = match &prop.key {
                    PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                    PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                    _ => None,
                };
                let type_annotation = prop.type_annotation.as_ref().and_then(|ta| {
                    let start = ta.type_annotation.span().start as usize;
                    let end = ta.type_annotation.span().end as usize;
                    if end <= source.len() {
                        let text = source[start..end].trim();
                        if !text.is_empty() {
                            Some(text.to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });
                let (description, tags) = extract_jsdoc_for(comments, prop.span().start, source);
                key_name.map(|name| AnalyzedPropField {
                    name,
                    is_optional: prop.optional,
                    span: prop.key.span().into(),
                    type_annotation,
                    description,
                    tags,
                    resolution_source: TypeResolutionSource::Rust,
                    resolution_error: None,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Convert a `TSTypeName` to a string.
fn type_name_to_string(type_name: &TSTypeName<'_>) -> String {
    match type_name {
        TSTypeName::IdentifierReference(id) => id.name.to_string(),
        TSTypeName::QualifiedName(qualified) => {
            format!(
                "{}.{}",
                type_name_to_string(&qualified.left),
                qualified.right.name
            )
        }
        _ => String::new(),
    }
}

/// Extract an identifier name from an expression (for `extends` heritage).
fn heritage_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// Post-process macros to resolve local type references in prop fields.
///
/// For `defineProps<Props>()` where `Props` is a local interface, this resolves
/// the interface members into prop fields. Also populates `resolved_local_types`
/// for the schema layer.
pub(crate) fn resolve_macro_type_references(
    program: &Program<'_>,
    macros: &mut [AnalyzedMacro],
    source: &str,
) {
    let registry = build_local_type_registry(program);
    if registry.is_empty() {
        return;
    }

    // Collect type param AST nodes for each defineProps macro by matching spans
    let type_params = collect_define_props_type_params(program);

    for mac in macros.iter_mut() {
        if mac.kind != AnalyzedMacroKind::DefineProps || !mac.is_type_based {
            continue;
        }

        // Skip if no type references to resolve
        if mac.type_references.is_empty() {
            continue;
        }

        // Check if any type references are in our registry (need resolution)
        let has_local_refs = mac
            .type_references
            .iter()
            .any(|r| registry.contains_key(r.as_str()));
        if !has_local_refs {
            continue;
        }

        let mut visited = FxHashSet::default();
        let mut resolved_types = Vec::new();

        // Try to find the actual type param AST and resolve the full type
        let mac_start = mac.span.start;
        if let Some(type_param) = type_params.iter().find(|tp| tp.0 == mac_start) {
            if let Some(fields) = resolve_type_to_prop_fields(
                type_param.1,
                &registry,
                source,
                &program.comments,
                &mut visited,
            ) {
                // Build resolved_local_types for each type reference
                for type_ref in &mac.type_references {
                    if let Some(decl) = registry.get(type_ref.as_str()) {
                        visited.clear();
                        if let Some(ref_fields) = resolve_interface_decl(
                            type_ref,
                            decl,
                            &registry,
                            source,
                            &program.comments,
                            &mut visited,
                        ) {
                            let expanded = build_expanded_type_text(&ref_fields);
                            let span = match decl {
                                LocalTypeDecl::Interface { body, .. } => body.span.into(),
                                LocalTypeDecl::Alias(t) => t.span().into(),
                                LocalTypeDecl::Class => verter_span::Span::default(),
                            };
                            resolved_types.push(ResolvedLocalType {
                                name: type_ref.clone(),
                                expanded,
                                span,
                            });
                        }
                    }
                }
                mac.prop_fields = fields;
            }
        } else {
            // Fallback: resolve individual type references (single ref case)
            visited.clear();
            if mac.type_references.len() == 1 {
                let type_ref = &mac.type_references[0];
                if let Some(decl) = registry.get(type_ref.as_str()) {
                    if let Some(fields) = resolve_interface_decl(
                        type_ref,
                        decl,
                        &registry,
                        source,
                        &program.comments,
                        &mut visited,
                    ) {
                        let expanded = build_expanded_type_text(&fields);
                        let span = match decl {
                            LocalTypeDecl::Interface { body, .. } => body.span.into(),
                            LocalTypeDecl::Alias(t) => t.span().into(),
                            LocalTypeDecl::Class => verter_span::Span::default(),
                        };
                        resolved_types.push(ResolvedLocalType {
                            name: type_ref.clone(),
                            expanded,
                            span,
                        });
                        mac.prop_fields = fields;
                    }
                }
            }
        }

        mac.resolved_local_types = resolved_types;
    }
}

/// Collect the type parameter AST nodes for all `defineProps<T>()` calls in the program.
/// Returns `(call_span_start, &TSType)` pairs.
fn collect_define_props_type_params<'a>(program: &'a Program<'a>) -> Vec<(u32, &'a TSType<'a>)> {
    let mut result = Vec::new();
    for stmt in &program.body {
        collect_define_props_from_stmt(stmt, &mut result);
    }
    result
}

fn collect_define_props_from_stmt<'a>(
    stmt: &'a Statement<'a>,
    result: &mut Vec<(u32, &'a TSType<'a>)>,
) {
    match stmt {
        Statement::ExpressionStatement(es) => {
            collect_define_props_from_expr(&es.expression, result);
        }
        Statement::VariableDeclaration(decl) => {
            for d in &decl.declarations {
                if let Some(init) = &d.init {
                    collect_define_props_from_expr(init, result);
                }
            }
        }
        _ => {}
    }
}

fn collect_define_props_from_expr<'a>(
    expr: &'a Expression<'a>,
    result: &mut Vec<(u32, &'a TSType<'a>)>,
) {
    if let Expression::CallExpression(call) = expr {
        let is_define_props =
            matches!(&call.callee, Expression::Identifier(id) if id.name == "defineProps");
        if is_define_props {
            if let Some(ref type_args) = call.type_arguments {
                if let Some(first) = type_args.params.first() {
                    result.push((call.span.start, first));
                }
            }
        }
        // Also check for withDefaults(defineProps<T>(), ...)
        if let Some(first_arg) = call.arguments.first() {
            if let Some(inner_expr) = first_arg.as_expression() {
                collect_define_props_from_expr(inner_expr, result);
            }
        }
    }
}

/// Resolve an interface declaration to prop fields (recursive helper).
fn resolve_interface_decl(
    name: &str,
    decl: &LocalTypeDecl<'_>,
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
    visited: &mut FxHashSet<String>,
) -> Option<Vec<AnalyzedPropField>> {
    if visited.contains(name) {
        return Some(Vec::new());
    }
    visited.insert(name.to_string());
    let result = match decl {
        LocalTypeDecl::Interface { body, extends } => {
            let mut fields = Vec::new();
            let mut seen_names = FxHashSet::default();

            for heritage in *extends {
                let Some(parent_name) = heritage_name(&heritage.expression) else {
                    continue;
                };
                if let Some(parent_decl) = registry.get(&parent_name) {
                    if let Some(parent_fields) = resolve_interface_decl(
                        &parent_name,
                        parent_decl,
                        registry,
                        source,
                        comments,
                        visited,
                    ) {
                        for field in parent_fields {
                            if seen_names.insert(field.name.clone()) {
                                fields.push(field);
                            }
                        }
                    }
                }
            }

            let own_fields = extract_fields_from_interface_body(body, source, comments);
            for field in own_fields {
                if seen_names.insert(field.name.clone()) {
                    fields.push(field);
                }
            }
            Some(fields)
        }
        LocalTypeDecl::Alias(aliased_type) => {
            resolve_type_to_prop_fields(aliased_type, registry, source, comments, visited)
        }
        LocalTypeDecl::Class => None,
    };
    visited.remove(name);
    result
}

/// Build an expanded type text like `"{ title: string; isbn: string }"` from prop fields.
fn build_expanded_type_text(fields: &[AnalyzedPropField]) -> String {
    let mut parts = Vec::new();
    for f in fields {
        let opt = if f.is_optional { "?" } else { "" };
        let ty = f.type_annotation.as_deref().unwrap_or("unknown");
        parts.push(format!("{}{}: {}", f.name, opt, ty));
    }
    format!("{{ {} }}", parts.join("; "))
}

/// Try to extract macros from an expression statement.
/// Called per-statement from the single-pass AST walk.
pub(crate) fn try_extract_macro_from_expr(
    expression: &Expression<'_>,
    macros: &mut Vec<AnalyzedMacro>,
    source: &str,
    comments: &[Comment],
) {
    if let Some(m) = try_extract_macro(expression, None, source, comments) {
        if m.kind == AnalyzedMacroKind::WithDefaults {
            try_extract_inner_macro(expression, macros, source, comments);
        }
        macros.push(m);
    }
}

/// Try to extract macros from a variable declarator.
/// Called per-declaration from the single-pass AST walk.
pub(crate) fn try_extract_macro_from_var_decl(
    decl: &VariableDeclarator<'_>,
    macros: &mut Vec<AnalyzedMacro>,
    source: &str,
    comments: &[Comment],
) {
    if let Some(ref init) = decl.init {
        let binding_name = if let BindingPattern::BindingIdentifier(id) = &decl.id {
            Some(id.name.to_string())
        } else {
            None
        };
        if let Some(m) = try_extract_macro(init, binding_name, source, comments) {
            if m.kind == AnalyzedMacroKind::WithDefaults {
                try_extract_inner_macro(init, macros, source, comments);
            }
            macros.push(m);
        }
    }
}

/// For `withDefaults(defineProps<...>(), {...})`, extract the inner macro
/// (e.g. `defineProps`) from the first argument.
fn try_extract_inner_macro(
    expr: &Expression<'_>,
    macros: &mut Vec<AnalyzedMacro>,
    source: &str,
    comments: &[Comment],
) {
    if let Expression::CallExpression(call) = expr {
        if let Some(first_arg) = call.arguments.first() {
            if let Some(inner_expr) = first_arg.as_expression() {
                if let Some(m) = try_extract_macro(inner_expr, None, source, comments) {
                    macros.push(m);
                }
            }
        }
    }
}

/// Try to extract a macro call from an expression.
fn try_extract_macro(
    expr: &Expression<'_>,
    binding_name: Option<String>,
    source: &str,
    comments: &[Comment],
) -> Option<AnalyzedMacro> {
    match expr {
        Expression::CallExpression(call) => {
            let callee_name = match &call.callee {
                Expression::Identifier(id) => Some(id.name.as_str()),
                _ => None,
            }?;

            let kind = classify_macro(callee_name)?;

            // In OXC 0.112, type parameters on call expressions are `.type_arguments`
            let (is_type_based, type_references) = if let Some(ref type_args) = call.type_arguments
            {
                if let Some(first) = type_args.params.first() {
                    (true, collect_type_references(first))
                } else {
                    (true, Vec::new())
                }
            } else {
                (false, Vec::new())
            };

            // Extract model name from defineModel('name') first string argument
            let model_name = if kind == AnalyzedMacroKind::DefineModel {
                call.arguments.first().and_then(|arg| {
                    if let Some(Expression::StringLiteral(lit)) = arg.as_expression() {
                        Some(lit.value.to_string())
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            // Detect defineOptions({ inheritAttrs: false })
            let has_inherit_attrs_false =
                kind == AnalyzedMacroKind::DefineOptions && has_inherit_attrs_false_in_args(call);

            let prop_extraction = if kind == AnalyzedMacroKind::DefineProps {
                extract_prop_fields(call, source, comments)
            } else if kind == AnalyzedMacroKind::DefineModel {
                PropFieldExtraction {
                    fields: extract_define_model_type(call, source, &model_name),
                    default_keys: Vec::new(),
                    default_values: Vec::new(),
                }
            } else {
                PropFieldExtraction {
                    fields: Vec::new(),
                    default_keys: Vec::new(),
                    default_values: Vec::new(),
                }
            };
            let prop_fields = prop_extraction.fields;

            let emit_fields = if kind == AnalyzedMacroKind::DefineEmits {
                extract_emit_fields(call, comments, source)
            } else {
                Vec::new()
            };

            let slot_fields = if kind == AnalyzedMacroKind::DefineSlots {
                extract_slot_fields(call, source, comments)
            } else {
                Vec::new()
            };

            let default_keys = if kind == AnalyzedMacroKind::WithDefaults {
                extract_with_defaults_keys(call)
            } else if kind == AnalyzedMacroKind::DefineProps {
                prop_extraction.default_keys
            } else if kind == AnalyzedMacroKind::DefineModel {
                extract_define_model_default_keys(call, &model_name)
            } else {
                Vec::new()
            };
            let default_values = if kind == AnalyzedMacroKind::WithDefaults {
                extract_with_defaults_values(call, source)
            } else if kind == AnalyzedMacroKind::DefineProps {
                prop_extraction.default_values
            } else {
                Vec::new()
            };

            let expose_fields = if kind == AnalyzedMacroKind::DefineExpose {
                extract_expose_fields(call)
            } else {
                Vec::new()
            };

            Some(AnalyzedMacro {
                kind,
                is_type_based,
                type_references,
                binding_name,
                model_name,
                has_inherit_attrs_false,
                prop_fields,
                emit_fields,
                slot_fields,
                default_keys,
                expose_fields,
                default_values,
                resolved_local_types: Vec::new(),
                span: call.span.into(),
            })
        }
        _ => None,
    }
}

/// Extract the type parameter from `defineModel<T>()` as a single `AnalyzedPropField`.
///
/// Unlike `defineProps<{ count: number }>()` where the type param is a `TSTypeLiteral`,
/// `defineModel<string>()` has a single type (e.g., `TSStringKeyword`, `TSTypeReference`).
/// We extract the source text of the first type param as the `type_annotation`.
fn extract_define_model_type(
    call: &CallExpression<'_>,
    source: &str,
    model_name: &Option<String>,
) -> Vec<AnalyzedPropField> {
    let Some(ref type_args) = call.type_arguments else {
        return Vec::new();
    };
    let Some(first) = type_args.params.first() else {
        return Vec::new();
    };
    let start = first.span().start as usize;
    let end = first.span().end as usize;
    if end > source.len() {
        return Vec::new();
    }
    let type_text = source[start..end].trim();
    if type_text.is_empty() {
        return Vec::new();
    }
    let name = model_name.as_deref().unwrap_or("modelValue").to_string();
    vec![AnalyzedPropField {
        name,
        is_optional: false,
        span: first.span().into(),
        type_annotation: Some(type_text.to_string()),
        description: None,
        tags: Vec::new(),
        resolution_source: TypeResolutionSource::Rust,
        resolution_error: None,
    }]
}

/// Check if a `defineModel()` call has a `default` key in its options object.
///
/// Handles:
/// - `defineModel<T>({ default: ... })` — options as first arg
/// - `defineModel<T>('name', { default: ... })` — options as second arg
///
/// Returns a vec containing the model name if `default` is present, empty otherwise.
fn extract_define_model_default_keys(
    call: &CallExpression<'_>,
    model_name: &Option<String>,
) -> Vec<String> {
    let name = model_name.as_deref().unwrap_or("modelValue").to_string();

    // Find the options object argument (skip string literal name argument)
    let options_obj = call.arguments.iter().find_map(|arg| {
        if let Argument::ObjectExpression(obj) = arg {
            Some(obj)
        } else {
            None
        }
    });

    let Some(obj) = options_obj else {
        return Vec::new();
    };

    // Check if the object has a "default" property
    let has_default = obj.properties.iter().any(|prop| {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            matches!(&p.key, PropertyKey::StaticIdentifier(id) if id.name == "default")
        } else {
            false
        }
    });

    if has_default {
        vec![name]
    } else {
        Vec::new()
    }
}

/// Result of extracting prop fields from a `defineProps` call.
struct PropFieldExtraction {
    fields: Vec<AnalyzedPropField>,
    default_keys: Vec<String>,
    default_values: Vec<AnalyzedDefaultValue>,
}

/// Extract individual prop field names and spans from a `defineProps` call.
///
/// Handles:
/// - Type-based: `defineProps<{ count: number, name: string }>()`
/// - Runtime object: `defineProps({ count: { type: Number }, name: String })`
/// - Runtime array: `defineProps(['count', 'name'])`
fn extract_prop_fields(
    call: &CallExpression<'_>,
    source: &str,
    comments: &[Comment],
) -> PropFieldExtraction {
    // Type-based: extract from type parameters
    if let Some(ref type_args) = call.type_arguments {
        if let Some(first) = type_args.params.first() {
            return PropFieldExtraction {
                fields: extract_prop_fields_from_type(first, source, comments),
                default_keys: Vec::new(),
                default_values: Vec::new(),
            };
        }
    }

    // Runtime: extract from first argument
    if let Some(first_arg) = call.arguments.first() {
        if let Some(expr) = first_arg.as_expression() {
            let rt = extract_prop_fields_from_runtime(expr, source, comments);
            return PropFieldExtraction {
                fields: rt.fields,
                default_keys: rt.default_keys,
                default_values: rt.default_values,
            };
        }
    }

    PropFieldExtraction {
        fields: Vec::new(),
        default_keys: Vec::new(),
        default_values: Vec::new(),
    }
}

/// Extract prop fields from a TypeScript type parameter (e.g., `{ count: number }`).
fn extract_prop_fields_from_type(
    ts_type: &TSType<'_>,
    source: &str,
    comments: &[Comment],
) -> Vec<AnalyzedPropField> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => literal
            .members
            .iter()
            .filter_map(|member| {
                if let TSSignature::TSPropertySignature(prop) = member {
                    let key_name = match &prop.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    };
                    // Extract type annotation text from source span
                    let type_annotation = prop.type_annotation.as_ref().and_then(|ta| {
                        let start = ta.type_annotation.span().start as usize;
                        let end = ta.type_annotation.span().end as usize;
                        if end <= source.len() {
                            let text = source[start..end].trim();
                            if !text.is_empty() {
                                Some(text.to_string())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    });
                    // Extract JSDoc from leading comment
                    let (description, tags) =
                        extract_jsdoc_for(comments, prop.span().start, source);
                    key_name.map(|name| AnalyzedPropField {
                        name,
                        is_optional: prop.optional,
                        span: prop.key.span().into(),
                        type_annotation,
                        description,
                        tags,
                        resolution_source: TypeResolutionSource::Rust,
                        resolution_error: None,
                    })
                } else {
                    None
                }
            })
            .collect(),
        TSType::TSTypeReference(_) => {
            // Interface reference — can't resolve inline, leave empty
            Vec::new()
        }
        TSType::TSIntersectionType(intersection) => {
            // Merge fields from all branches
            intersection
                .types
                .iter()
                .flat_map(|t| extract_prop_fields_from_type(t, source, comments))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Result of extracting prop fields from a runtime defineProps argument.
struct RuntimePropExtraction {
    fields: Vec<AnalyzedPropField>,
    default_keys: Vec<String>,
    default_values: Vec<AnalyzedDefaultValue>,
}

/// Map a runtime constructor name to its TypeScript type string.
fn constructor_to_ts_type(name: &str) -> Option<&'static str> {
    match name {
        "String" => Some("string"),
        "Number" => Some("number"),
        "Boolean" => Some("boolean"),
        "Array" => Some("Array<any>"),
        "Object" => Some("object"),
        "Function" => Some("Function"),
        "Symbol" => Some("symbol"),
        "Date" => Some("Date"),
        "RegExp" => Some("RegExp"),
        "Promise" => Some("Promise<any>"),
        _ => None,
    }
}

/// Extract the meaningful TypeScript type from a `TSAsExpression` on a runtime prop `type:` field.
///
/// Rules:
/// - `X as PropType<T>`  → `T` (extracts the first type argument)
/// - `X as () => T`      → `T` (extracts the return type, not the callable)
/// - `X as new () => T`  → `T` (extracts the return type, not the constructor)
/// - Other assertions    → `None` (caller falls back to `constructor_to_ts_type`)
fn extract_ts_as_type(ts_as: &TSAsExpression<'_>, source: &str) -> Option<String> {
    match &ts_as.type_annotation {
        TSType::TSTypeReference(type_ref) => {
            // `X as PropType<T>` → extract T
            if let TSTypeName::IdentifierReference(id) = &type_ref.type_name {
                if id.name == "PropType" {
                    if let Some(args) = &type_ref.type_arguments {
                        if let Some(first) = args.params.first() {
                            let span = first.span();
                            return Some(
                                source[span.start as usize..span.end as usize].to_string(),
                            );
                        }
                    }
                }
            }
            None
        }
        TSType::TSFunctionType(fn_type) => {
            // `X as () => T` → extract T (the return type, not the callable signature)
            let span = fn_type.return_type.type_annotation.span();
            Some(source[span.start as usize..span.end as usize].to_string())
        }
        TSType::TSConstructorType(ctor_type) => {
            // `X as new () => T` → extract T (the return type, not the constructor signature)
            let span = ctor_type.return_type.type_annotation.span();
            Some(source[span.start as usize..span.end as usize].to_string())
        }
        _ => None,
    }
}

/// Extract prop fields from a runtime argument (object or array).
///
/// For object form, detects both shorthand (`name: String`) and expanded
/// (`name: { type: String, default: 'Hello' }`) property definitions.
fn extract_prop_fields_from_runtime(
    expr: &Expression<'_>,
    source: &str,
    comments: &[Comment],
) -> RuntimePropExtraction {
    match expr {
        Expression::ObjectExpression(obj) => {
            let mut fields = Vec::new();
            let mut default_keys = Vec::new();
            let mut default_values = Vec::new();

            for prop in &obj.properties {
                let ObjectPropertyKind::ObjectProperty(p) = prop else {
                    continue;
                };
                let key_name = match &p.key {
                    PropertyKey::StaticIdentifier(id) => id.name.to_string(),
                    PropertyKey::StringLiteral(lit) => lit.value.to_string(),
                    _ => continue,
                };

                let mut type_annotation = None;
                // Vue semantics: props are optional by default unless `required: true` is set.
                let mut is_optional = true;

                // Check if value is a constructor (shorthand: `name: String`)
                if let Expression::Identifier(id) = &p.value {
                    type_annotation = constructor_to_ts_type(&id.name).map(String::from);
                }

                // Check if value is an expanded object: `name: { type: String, default: 'Hello' }`
                if let Expression::ObjectExpression(val_obj) = &p.value {
                    for sub_prop in &val_obj.properties {
                        let ObjectPropertyKind::ObjectProperty(sp) = sub_prop else {
                            continue;
                        };
                        let sub_key = match &sp.key {
                            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
                            _ => continue,
                        };
                        match sub_key {
                            "type" => {
                                // Try to extract an explicit type assertion first (`X as PropType<T>`,
                                // `X as () => T`, `X as new () => T`), then fall back to mapping the
                                // base constructor identifier via `constructor_to_ts_type`.
                                if let Expression::TSAsExpression(ts_as) = &sp.value {
                                    if let Some(extracted) = extract_ts_as_type(ts_as, source) {
                                        type_annotation = Some(extracted);
                                    } else if let Expression::Identifier(id) = &ts_as.expression {
                                        type_annotation =
                                            constructor_to_ts_type(&id.name).map(String::from);
                                    }
                                } else if let Expression::Identifier(id) = &sp.value {
                                    type_annotation =
                                        constructor_to_ts_type(&id.name).map(String::from);
                                }
                            }
                            "required" => {
                                // `required: true` makes the prop required (not optional).
                                if let Expression::BooleanLiteral(b) = &sp.value {
                                    is_optional = !b.value;
                                }
                            }
                            "default" => {
                                let val_text = extract_default_value_text(&sp.value, source);
                                default_keys.push(key_name.clone());
                                default_values.push(AnalyzedDefaultValue {
                                    key: key_name.clone(),
                                    value: val_text,
                                    span: sp.value.span().into(),
                                });
                            }
                            _ => {}
                        }
                    }
                }

                let (description, tags) = extract_jsdoc_for(comments, p.key.span().start, source);

                fields.push(AnalyzedPropField {
                    name: key_name,
                    is_optional,
                    span: p.key.span().into(),
                    type_annotation,
                    description,
                    tags,
                    resolution_source: TypeResolutionSource::Rust,
                    resolution_error: None,
                });
            }

            RuntimePropExtraction {
                fields,
                default_keys,
                default_values,
            }
        }
        Expression::ArrayExpression(arr) => RuntimePropExtraction {
            fields: arr
                .elements
                .iter()
                .filter_map(|elem| {
                    if let ArrayExpressionElement::StringLiteral(lit) = elem {
                        Some(AnalyzedPropField {
                            name: lit.value.to_string(),
                            // Array form has no type or required info — optional by Vue default.
                            is_optional: true,
                            span: lit.span.into(),
                            type_annotation: None,
                            description: None,
                            tags: Vec::new(),
                            resolution_source: TypeResolutionSource::Rust,
                            resolution_error: None,
                        })
                    } else {
                        None
                    }
                })
                .collect(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
        },
        _ => RuntimePropExtraction {
            fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
        },
    }
}

/// Extract individual emit field names and spans from a `defineEmits` call.
///
/// Handles:
/// - Type-based property-signature: `defineEmits<{ custom: [payload: string]; click: [] }>()`
/// - Type-based call-signature: `defineEmits<{ (e: 'change', id: number): void }>()`
/// - Runtime array: `defineEmits(['custom', 'click'])`
/// - Runtime object: `defineEmits({ custom: null })`
fn extract_emit_fields(
    call: &CallExpression<'_>,
    comments: &[Comment],
    source: &str,
) -> Vec<AnalyzedEmitField> {
    // Type-based: extract from type parameters
    if let Some(ref type_args) = call.type_arguments {
        if let Some(first) = type_args.params.first() {
            return extract_emit_fields_from_type(first, comments, source);
        }
    }

    // Runtime: extract from first argument
    if let Some(first_arg) = call.arguments.first() {
        if let Some(expr) = first_arg.as_expression() {
            return extract_emit_fields_from_runtime(expr);
        }
    }

    Vec::new()
}

/// Extract emit fields from a TypeScript type parameter.
///
/// Handles two TSTypeLiteral member shapes:
/// 1. `TSPropertySignature` — key name is the event name (e.g., `custom: [payload: string]`)
/// 2. `TSCallSignatureDeclaration` — first param's string literal type is the event name
fn extract_emit_fields_from_type(
    ts_type: &TSType<'_>,
    comments: &[Comment],
    source: &str,
) -> Vec<AnalyzedEmitField> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => literal
            .members
            .iter()
            .filter_map(|member| match member {
                // Property signature: `custom: [payload: string]`
                TSSignature::TSPropertySignature(prop) => {
                    let key_name = match &prop.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    };
                    // Extract payload type from the value type annotation
                    let payload_type = prop.type_annotation.as_ref().and_then(|ta| {
                        let start = ta.type_annotation.span().start as usize;
                        let end = ta.type_annotation.span().end as usize;
                        if end <= source.len() {
                            let text = source[start..end].trim();
                            if !text.is_empty() {
                                Some(text.to_string())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    });
                    let (description, tags) =
                        extract_jsdoc_for(comments, prop.span().start, source);
                    key_name.map(|name| AnalyzedEmitField {
                        name,
                        span: prop.key.span().into(),
                        payload_type,
                        description,
                        tags,
                    })
                }
                // Call signature: `(e: 'change', id: number): void`
                TSSignature::TSCallSignatureDeclaration(call_sig) => {
                    // First param should be string literal type: `e: 'change'`
                    let first_param = call_sig.params.items.first()?;
                    let type_ann = first_param.type_annotation.as_ref()?;
                    if let TSType::TSLiteralType(lit) = &type_ann.type_annotation {
                        if let TSLiteral::StringLiteral(s) = &lit.literal {
                            // Extract payload params (all params after the event name)
                            let payload_type = {
                                let extra_params: Vec<String> = call_sig
                                    .params
                                    .items
                                    .iter()
                                    .skip(1)
                                    .map(|p| {
                                        let start = p.span().start as usize;
                                        let end = p.span().end as usize;
                                        if end <= source.len() {
                                            source[start..end].to_string()
                                        } else {
                                            "unknown".to_string()
                                        }
                                    })
                                    .collect();
                                Some(format!("[{}]", extra_params.join(", ")))
                            };
                            let (description, tags) =
                                extract_jsdoc_for(comments, call_sig.span().start, source);
                            return Some(AnalyzedEmitField {
                                name: s.value.to_string(),
                                span: s.span.into(),
                                payload_type,
                                description,
                                tags,
                            });
                        }
                    }
                    None
                }
                _ => None,
            })
            .collect(),
        TSType::TSTypeReference(_) => Vec::new(),
        TSType::TSIntersectionType(intersection) => intersection
            .types
            .iter()
            .flat_map(|t| extract_emit_fields_from_type(t, comments, source))
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract emit fields from a runtime argument (object keys or array string elements).
fn extract_emit_fields_from_runtime(expr: &Expression<'_>) -> Vec<AnalyzedEmitField> {
    match expr {
        Expression::ObjectExpression(obj) => obj
            .properties
            .iter()
            .filter_map(|prop| {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    let key_name = match &p.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    };
                    key_name.map(|name| AnalyzedEmitField {
                        name,
                        span: p.key.span().into(),
                        payload_type: None,
                        description: None,
                        tags: Vec::new(),
                    })
                } else {
                    None
                }
            })
            .collect(),
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|elem| {
                if let ArrayExpressionElement::StringLiteral(lit) = elem {
                    Some(AnalyzedEmitField {
                        name: lit.value.to_string(),
                        span: lit.span.into(),
                        payload_type: None,
                        description: None,
                        tags: Vec::new(),
                    })
                } else {
                    None
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract object property key names from the second argument of `withDefaults()`.
///
/// `withDefaults(defineProps<{...}>(), { foo: 'bar', baz: 42 })` → `["foo", "baz"]`
fn extract_with_defaults_keys(call: &CallExpression<'_>) -> Vec<String> {
    let Some(second_arg) = call.arguments.get(1) else {
        return Vec::new();
    };
    let Some(Expression::ObjectExpression(obj)) = second_arg.as_expression() else {
        return Vec::new();
    };
    obj.properties
        .iter()
        .filter_map(|prop| {
            if let ObjectPropertyKind::ObjectProperty(p) = prop {
                match &p.key {
                    PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                    PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                    _ => None,
                }
            } else {
                None
            }
        })
        .collect()
}

/// Extract default value key-value pairs from `withDefaults(defineProps<T>(), { key: value })`.
fn extract_with_defaults_values(
    call: &CallExpression<'_>,
    source: &str,
) -> Vec<AnalyzedDefaultValue> {
    let Some(second_arg) = call.arguments.get(1) else {
        return Vec::new();
    };
    let Some(Expression::ObjectExpression(obj)) = second_arg.as_expression() else {
        return Vec::new();
    };
    obj.properties
        .iter()
        .filter_map(|prop| {
            if let ObjectPropertyKind::ObjectProperty(p) = prop {
                let key = match &p.key {
                    PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                    PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                    _ => None,
                }?;
                let value = extract_default_value_text(&p.value, source);
                Some(AnalyzedDefaultValue {
                    key,
                    value,
                    span: p.value.span().into(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Extract default value source text. For string literals, extracts the inner value.
fn extract_default_value_text(expr: &Expression<'_>, source: &str) -> String {
    match expr {
        Expression::StringLiteral(s) => s.value.to_string(),
        Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => {
            let start = expr.span().start as usize;
            let end = expr.span().end as usize;
            if end <= source.len() {
                source[start..end].to_string()
            } else {
                String::new()
            }
        }
        _ => {
            let start = expr.span().start as usize;
            let end = expr.span().end as usize;
            if end <= source.len() {
                source[start..end].to_string()
            } else {
                String::new()
            }
        }
    }
}

/// Extract exposed field names from `defineExpose({ foo, bar })`.
///
/// Only parses object literal arguments. Identifier args (e.g., `defineExpose(myObj)`)
/// return empty since we can't resolve the value statically.
fn extract_expose_fields(call: &CallExpression<'_>) -> Vec<AnalyzedExposeField> {
    let Some(first_arg) = call.arguments.first() else {
        return Vec::new();
    };
    let Some(Expression::ObjectExpression(obj)) = first_arg.as_expression() else {
        return Vec::new();
    };
    obj.properties
        .iter()
        .filter_map(|prop| {
            if let ObjectPropertyKind::ObjectProperty(p) = prop {
                let key_name = match &p.key {
                    PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                    PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                    _ => None,
                };
                key_name.map(|name| AnalyzedExposeField {
                    name,
                    span: p.key.span().into(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Extract individual slot field names, required status, bindings, and spans from a `defineSlots` call.
///
/// Handles:
/// - Type-based: `defineSlots<{ default(props: { item: string }): any; header?(props: {}): any }>()`
/// - Empty / no type params → empty vec
fn extract_slot_fields(
    call: &CallExpression<'_>,
    source: &str,
    comments: &[Comment],
) -> Vec<AnalyzedSlotField> {
    if let Some(ref type_args) = call.type_arguments {
        if let Some(first) = type_args.params.first() {
            return extract_slot_fields_from_type(first, source, comments);
        }
    }
    Vec::new()
}

/// Extract slot fields from a TypeScript type parameter.
///
/// Handles:
/// - `TSPropertySignature`: `default: (props: { row: MyItem }) => any`
/// - `TSMethodSignature`: `default(props: { item: string }): any`
/// - `TSIntersectionType`: merges fields from all branches
fn extract_slot_fields_from_type(
    ts_type: &TSType<'_>,
    source: &str,
    comments: &[Comment],
) -> Vec<AnalyzedSlotField> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => literal
            .members
            .iter()
            .filter_map(|member| match member {
                TSSignature::TSPropertySignature(prop) => {
                    let key_name = match &prop.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    };
                    // For property signatures, extract bindings from function type annotation
                    let bindings = prop
                        .type_annotation
                        .as_ref()
                        .map(|ta| extract_slot_bindings_from_fn_type(&ta.type_annotation, source))
                        .unwrap_or_default();
                    let (description, tags) =
                        extract_jsdoc_for(comments, prop.span().start, source);
                    key_name.map(|name| AnalyzedSlotField {
                        name,
                        is_required: !prop.optional,
                        span: prop.key.span().into(),
                        bindings,
                        description,
                        tags,
                    })
                }
                TSSignature::TSMethodSignature(method) => {
                    let key_name = match &method.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    };
                    let bindings = extract_slot_bindings_from_params(&method.params, source);
                    let (description, tags) =
                        extract_jsdoc_for(comments, method.span().start, source);
                    key_name.map(|name| AnalyzedSlotField {
                        name,
                        is_required: !method.optional,
                        span: method.key.span().into(),
                        bindings,
                        description,
                        tags,
                    })
                }
                _ => None,
            })
            .collect(),
        TSType::TSTypeReference(_) => Vec::new(),
        TSType::TSIntersectionType(intersection) => intersection
            .types
            .iter()
            .flat_map(|t| extract_slot_fields_from_type(t, source, comments))
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract binding types from a `TSFunctionType` annotation on a property signature.
///
/// Handles: `default: (props: { row: MyItem }) => any`
fn extract_slot_bindings_from_fn_type(
    ts_type: &TSType<'_>,
    source: &str,
) -> Vec<AnalyzedSlotFieldBinding> {
    if let TSType::TSFunctionType(fn_type) = ts_type {
        extract_slot_bindings_from_params(&fn_type.params, source)
    } else {
        Vec::new()
    }
}

/// Extract slot binding names and types from a function's first parameter type annotation.
///
/// Given `(props: { item: string, index: number })`, extracts:
/// `[{name: "item", type_annotation: Some("string")}, {name: "index", type_annotation: Some("number")}]`
fn extract_slot_bindings_from_params(
    params: &FormalParameters<'_>,
    source: &str,
) -> Vec<AnalyzedSlotFieldBinding> {
    let Some(first_param) = params.items.first() else {
        return Vec::new();
    };
    let Some(ref ta) = first_param.type_annotation else {
        return Vec::new();
    };
    extract_slot_bindings_from_type_literal(&ta.type_annotation, source)
}

/// Extract binding names and types from a `TSTypeLiteral` (object type).
fn extract_slot_bindings_from_type_literal(
    ts_type: &TSType<'_>,
    source: &str,
) -> Vec<AnalyzedSlotFieldBinding> {
    let TSType::TSTypeLiteral(literal) = ts_type else {
        return Vec::new();
    };
    literal
        .members
        .iter()
        .filter_map(|member| {
            if let TSSignature::TSPropertySignature(prop) = member {
                let key_name = match &prop.key {
                    PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                    PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                    _ => None,
                };
                let type_annotation = prop.type_annotation.as_ref().and_then(|ta| {
                    let start = ta.type_annotation.span().start as usize;
                    let end = ta.type_annotation.span().end as usize;
                    if end <= source.len() {
                        let text = source[start..end].trim();
                        if !text.is_empty() {
                            Some(text.to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });
                key_name.map(|name| AnalyzedSlotFieldBinding {
                    name,
                    type_annotation,
                    span: prop.key.span().into(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Check if a `defineOptions()` call has `inheritAttrs: false` in its first object argument.
fn has_inherit_attrs_false_in_args(call: &CallExpression<'_>) -> bool {
    let Some(first_arg) = call.arguments.first() else {
        return false;
    };
    let Some(Expression::ObjectExpression(obj)) = first_arg.as_expression() else {
        return false;
    };
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let is_inherit_attrs = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name == "inheritAttrs",
                PropertyKey::StringLiteral(lit) => lit.value == "inheritAttrs",
                _ => false,
            };
            if is_inherit_attrs {
                if let Expression::BooleanLiteral(b) = &p.value {
                    return !b.value; // inheritAttrs: false
                }
            }
        }
    }
    false
}

// ── JSDoc extraction helpers ─────────────────────────────────────────

/// Find a leading JSDoc comment for a declaration at the given byte offset.
///
/// OXC's `Comment.attached_to` is the byte offset of the token the comment precedes.
fn find_leading_jsdoc<'a>(
    comments: &[Comment],
    target_start: u32,
    source: &'a str,
) -> Option<&'a str> {
    for comment in comments {
        if comment.attached_to == target_start
            && comment.is_block()
            && matches!(
                comment.content,
                CommentContent::Jsdoc | CommentContent::JsdocLegal
            )
        {
            let start = comment.span.start as usize;
            let end = comment.span.end as usize;
            if end <= source.len() {
                return Some(&source[start..end]);
            }
        }
    }
    None
}

/// Parse a raw JSDoc comment text into a description and a list of tags.
///
/// Input is the full comment including `/**` and `*/` delimiters.
/// Returns `(description, tags)`.
fn parse_jsdoc(raw: &str) -> (Option<String>, Vec<JsdocTag>) {
    // Strip /** and */ delimiters
    let inner = raw
        .strip_prefix("/**")
        .unwrap_or(raw)
        .strip_suffix("*/")
        .unwrap_or(raw);

    // Clean up each line: strip leading whitespace and `*`
    let lines: Vec<&str> = inner
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            trimmed.strip_prefix('*').unwrap_or(trimmed).trim()
        })
        .collect();

    let mut description_parts = Vec::new();
    let mut tags = Vec::new();
    let mut current_tag: Option<(String, Vec<String>)> = None;

    for line in &lines {
        if let Some(rest) = line.strip_prefix('@') {
            // Flush current tag
            if let Some((name, text_parts)) = current_tag.take() {
                let text = text_parts.join(" ");
                tags.push(JsdocTag {
                    name,
                    text: if text.is_empty() { None } else { Some(text) },
                });
            }
            // Parse new tag
            let (tag_name, tag_text) =
                if let Some(space_idx) = rest.find(|c: char| c.is_whitespace()) {
                    (&rest[..space_idx], rest[space_idx..].trim())
                } else {
                    (rest.trim(), "")
                };
            current_tag = Some((
                tag_name.to_string(),
                if tag_text.is_empty() {
                    Vec::new()
                } else {
                    vec![tag_text.to_string()]
                },
            ));
        } else if let Some(ref mut tag) = current_tag {
            // Continuation of a tag
            if !line.is_empty() {
                tag.1.push(line.to_string());
            }
        } else if !line.is_empty() {
            // Part of description
            description_parts.push(*line);
        }
    }

    // Flush last tag
    if let Some((name, text_parts)) = current_tag {
        let text = text_parts.join(" ");
        tags.push(JsdocTag {
            name,
            text: if text.is_empty() { None } else { Some(text) },
        });
    }

    let description = if description_parts.is_empty() {
        None
    } else {
        Some(description_parts.join(" "))
    };

    (description, tags)
}

/// Extract JSDoc description and tags for a given AST node position.
pub(crate) fn extract_jsdoc_for(
    comments: &[Comment],
    target_start: u32,
    source: &str,
) -> (Option<String>, Vec<JsdocTag>) {
    match find_leading_jsdoc(comments, target_start, source) {
        Some(raw) => parse_jsdoc(raw),
        None => (None, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_parser::{ParseOptions, Parser};
    use oxc_span::SourceType;

    use super::*;

    fn parse_and_extract(alloc: &Allocator, source: &str) -> Vec<AnalyzedMacro> {
        let parser =
            Parser::new(alloc, source, SourceType::ts()).with_options(ParseOptions::default());
        let result = parser.parse();
        assert!(!result.panicked, "failed to parse: {source}");
        analyze_macros_from_program(&result.program, source)
    }

    fn parse_type_refs(type_annotation: &str) -> Vec<String> {
        // Parse as a type annotation inside a variable declaration
        let code = format!("let _x: {type_annotation};");
        let alloc = Allocator::new();
        let parser =
            Parser::new(&alloc, &code, SourceType::ts()).with_options(ParseOptions::default());
        let result = parser.parse();
        assert!(!result.panicked, "failed to parse: {}", code);

        // In OXC 0.112, type annotations are on VariableDeclarator, not BindingPattern
        for stmt in &result.program.body {
            if let Statement::VariableDeclaration(var_decl) = stmt {
                if let Some(decl) = var_decl.declarations.first() {
                    if let Some(ref ta) = decl.type_annotation {
                        return collect_type_references(&ta.type_annotation);
                    }
                }
            }
        }
        panic!("could not find type annotation in parsed code");
    }

    /// @ai-generated - Object literal with type reference
    #[test]
    fn object_with_type_ref() {
        let refs = parse_type_refs("{foo: MyType}");
        assert_eq!(refs, vec!["MyType"]);
    }

    /// @ai-generated - Simple type reference
    #[test]
    fn simple_type_ref() {
        let refs = parse_type_refs("MyType");
        assert_eq!(refs, vec!["MyType"]);
    }

    /// @ai-generated - Intersection type
    #[test]
    fn intersection_type() {
        let refs = parse_type_refs("MyType & OtherType");
        assert_eq!(refs, vec!["MyType", "OtherType"]);
    }

    /// @ai-generated - Only primitives, no type references
    #[test]
    fn only_primitives() {
        let refs = parse_type_refs("{foo: string, bar: number}");
        assert!(refs.is_empty());
    }

    /// @ai-generated - Generic type with nested type reference
    #[test]
    fn generic_with_nested_ref() {
        let refs = parse_type_refs("Partial<MyType>");
        assert_eq!(refs, vec!["Partial", "MyType"]);
    }

    /// @ai-generated - Union type
    #[test]
    fn union_type() {
        let refs = parse_type_refs("MyType | OtherType");
        assert_eq!(refs, vec!["MyType", "OtherType"]);
    }

    /// @ai-generated - Array of type reference
    #[test]
    fn array_type() {
        let refs = parse_type_refs("MyType[]");
        assert_eq!(refs, vec!["MyType"]);
    }

    /// @ai-generated - Nested object with type refs
    #[test]
    fn nested_object() {
        let refs = parse_type_refs("{foo: {bar: MyType}, baz: OtherType}");
        assert_eq!(refs, vec!["MyType", "OtherType"]);
    }

    /// @ai-generated - Conditional type
    #[test]
    fn conditional_type() {
        let refs = parse_type_refs("A extends B ? C : D");
        assert_eq!(refs, vec!["A", "B", "C", "D"]);
    }

    /// @ai-generated - Mapped type
    #[test]
    fn mapped_type() {
        let refs = parse_type_refs("{[K in keyof T]: V}");
        assert_eq!(refs, vec!["T", "V"]);
    }

    /// @ai-generated - Indexed access type
    #[test]
    fn indexed_access() {
        let refs = parse_type_refs("T[K]");
        assert_eq!(refs, vec!["T", "K"]);
    }

    #[test]
    fn tuple_with_optional_element() {
        // Regression: TSTupleElement::TSOptionalType panics with to_ts_type()
        let refs = parse_type_refs("[string, MyType?]");
        assert_eq!(refs, vec!["MyType"]);
    }

    #[test]
    fn tuple_with_rest_element() {
        // Regression: TSTupleElement::TSRestType panics with to_ts_type()
        let refs = parse_type_refs("[string, ...MyType[]]");
        assert_eq!(refs, vec!["MyType"]);
    }

    #[test]
    fn tuple_with_named_element() {
        let refs = parse_type_refs("[name: string, value: MyType]");
        assert_eq!(refs, vec!["MyType"]);
    }

    fn parse_macros(code: &str) -> Vec<AnalyzedMacro> {
        let alloc = Allocator::new();
        let parser =
            Parser::new(&alloc, code, SourceType::ts()).with_options(ParseOptions::default());
        let result = parser.parse();
        assert!(!result.panicked, "failed to parse: {}", code);
        analyze_macros_from_program(&result.program, code)
    }

    /// @ai-generated - Detect defineProps with type param
    #[test]
    fn detect_define_props_type_based() {
        let macros = parse_macros("const props = defineProps<{foo: MyType}>()");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineProps);
        assert!(macros[0].is_type_based);
        assert_eq!(macros[0].type_references, vec!["MyType"]);
        assert_eq!(macros[0].binding_name.as_deref(), Some("props"));
    }

    /// @ai-generated - Detect defineProps without type param
    #[test]
    fn detect_define_props_runtime() {
        let macros = parse_macros("const props = defineProps({foo: String})");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineProps);
        assert!(!macros[0].is_type_based);
        assert!(macros[0].type_references.is_empty());
    }

    /// @ai-generated - Detect defineEmits
    #[test]
    fn detect_define_emits() {
        let macros = parse_macros("const emit = defineEmits<{(e: 'click'): void}>()");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineEmits);
        assert!(macros[0].is_type_based);
    }

    /// @ai-generated - Detect defineModel
    #[test]
    fn detect_define_model() {
        let macros = parse_macros("const model = defineModel<string>()");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineModel);
        assert!(macros[0].is_type_based);
    }

    /// @ai-generated - Bare macro call (no binding)
    #[test]
    fn bare_macro_call() {
        let macros = parse_macros("defineExpose({})");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineExpose);
        assert!(macros[0].binding_name.is_none());
    }

    /// @ai-generated - Imported type reference in defineProps
    #[test]
    fn imported_type_in_define_props() {
        let macros = parse_macros("defineProps<MyImportedType>()");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].type_references, vec!["MyImportedType"]);
    }

    /// @ai-generated - Multiple macros
    #[test]
    fn multiple_macros() {
        let code = r#"
const props = defineProps<{foo: string}>()
const emit = defineEmits<{(e: 'click'): void}>()
defineExpose({ props })
"#;
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 3);
    }

    /// @ai-generated - withDefaults wrapping defineProps extracts both macros
    #[test]
    fn with_defaults_extracts_inner_define_props() {
        let code = r#"const props = withDefaults(defineProps<{foo: MyType}>(), { foo: 'bar' })"#;
        let macros = parse_macros(code);
        assert!(
            macros.len() >= 2,
            "should extract both withDefaults and defineProps, got {}",
            macros.len()
        );
        assert!(
            macros
                .iter()
                .any(|m| m.kind == AnalyzedMacroKind::WithDefaults),
            "should have withDefaults"
        );
        assert!(
            macros
                .iter()
                .any(|m| m.kind == AnalyzedMacroKind::DefineProps),
            "should have defineProps"
        );
        // The inner defineProps should capture type references
        let define_props = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert!(
            define_props.is_type_based,
            "inner defineProps should be type-based"
        );
        assert!(
            define_props.type_references.contains(&"MyType".to_string()),
            "inner defineProps should capture type references"
        );
    }

    /// @ai-generated - import("./foo").Bar in type refs returns empty (not tracked)
    #[test]
    fn import_type_in_type_refs_returns_empty() {
        // TSImportType is intentionally not tracked — returns empty
        let refs = parse_type_refs("import('./foo').Bar");
        assert!(
            refs.is_empty(),
            "import() type references should not be collected, got: {:?}",
            refs
        );
    }

    /// @ai-generated - typeof X in type refs extracts the identifier
    #[test]
    fn typeof_in_type_refs_extracts_identifier() {
        // TSTypeQuery (typeof) should collect the referenced identifier
        let refs = parse_type_refs("typeof X");
        assert_eq!(refs, vec!["X".to_string()]);
    }

    // =========================================================================
    // Prop field extraction tests
    // =========================================================================

    #[test]
    fn prop_fields_type_based_literal() {
        let code = "defineProps<{ count: number, name: string }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].prop_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 prop fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "count");
        assert_eq!(fields[1].name, "name");
        // Verify spans point to prop keys
        assert_eq!(
            &code[fields[0].span.start as usize..fields[0].span.end as usize],
            "count"
        );
        assert_eq!(
            &code[fields[1].span.start as usize..fields[1].span.end as usize],
            "name"
        );
    }

    #[test]
    fn prop_fields_type_based_with_assignment() {
        let code = "const props = defineProps<{ msg: string }>()";
        let macros = parse_macros(code);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert_eq!(dp.prop_fields.len(), 1);
        assert_eq!(dp.prop_fields[0].name, "msg");
        assert_eq!(
            &code[dp.prop_fields[0].span.start as usize..dp.prop_fields[0].span.end as usize],
            "msg"
        );
    }

    #[test]
    fn prop_fields_runtime_object() {
        let code = "defineProps({ count: { type: Number }, name: String })";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].prop_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 runtime prop fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "count");
        assert_eq!(fields[1].name, "name");
        assert_eq!(
            &code[fields[0].span.start as usize..fields[0].span.end as usize],
            "count"
        );
        assert_eq!(
            &code[fields[1].span.start as usize..fields[1].span.end as usize],
            "name"
        );
    }

    #[test]
    fn prop_fields_runtime_array() {
        let code = "defineProps(['count', 'name'])";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].prop_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 array prop fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "count");
        assert_eq!(fields[1].name, "name");
    }

    #[test]
    fn prop_fields_with_defaults() {
        let code = "withDefaults(defineProps<{ msg: string, count: number }>(), { msg: 'hi' })";
        let macros = parse_macros(code);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert_eq!(
            dp.prop_fields.len(),
            2,
            "withDefaults inner defineProps should have prop fields: {:?}",
            dp.prop_fields
        );
        assert_eq!(dp.prop_fields[0].name, "msg");
        assert_eq!(dp.prop_fields[1].name, "count");
    }

    #[test]
    fn prop_fields_non_define_props_empty() {
        let code = "defineEmits<{(e: 'click'): void}>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].prop_fields.is_empty(),
            "defineEmits should have no prop fields"
        );
    }

    #[test]
    fn prop_fields_type_reference_empty() {
        // Interface reference — can't resolve inline, prop_fields is empty
        let code = "defineProps<MyProps>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].prop_fields.is_empty(),
            "type reference should yield empty prop fields"
        );
    }

    // =========================================================================
    // Emit field extraction tests
    // =========================================================================

    #[test]
    fn emit_fields_type_based_property_signature() {
        let code = "defineEmits<{ custom: [payload: string]; click: [] }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].emit_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 emit fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "custom");
        assert_eq!(fields[1].name, "click");
        // Verify spans point to event name keys
        assert_eq!(
            &code[fields[0].span.start as usize..fields[0].span.end as usize],
            "custom"
        );
        assert_eq!(
            &code[fields[1].span.start as usize..fields[1].span.end as usize],
            "click"
        );
    }

    #[test]
    fn emit_fields_type_based_call_signature() {
        let code = "defineEmits<{ (e: 'change', id: number): void }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].emit_fields;
        assert_eq!(
            fields.len(),
            1,
            "should extract 1 emit field from call signature: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "change");
    }

    #[test]
    fn emit_fields_type_based_mixed_signatures() {
        let code = "defineEmits<{ (e: 'change', id: number): void; custom: [payload: string] }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].emit_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 emit fields from mixed signatures: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "change");
        assert_eq!(fields[1].name, "custom");
    }

    #[test]
    fn emit_fields_runtime_array() {
        let code = "defineEmits(['custom', 'click'])";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].emit_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 runtime array emit fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "custom");
        assert_eq!(fields[1].name, "click");
    }

    #[test]
    fn emit_fields_runtime_object() {
        let code = "defineEmits({ custom: null })";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].emit_fields;
        assert_eq!(
            fields.len(),
            1,
            "should extract 1 runtime object emit field: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "custom");
    }

    #[test]
    fn emit_fields_non_define_emits_empty() {
        let code = "defineProps<{ count: number }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].emit_fields.is_empty(),
            "defineProps should have no emit fields"
        );
    }

    #[test]
    fn emit_fields_type_reference_empty() {
        // Interface reference — can't resolve inline, emit_fields is empty
        let code = "defineEmits<MyEvents>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].emit_fields.is_empty(),
            "type reference should yield empty emit fields"
        );
    }

    #[test]
    fn prop_fields_intersection_type() {
        let code = "defineProps<{ a: string } & { b: number }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].prop_fields;
        assert_eq!(
            fields.len(),
            2,
            "intersection should merge fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "a");
        assert_eq!(fields[1].name, "b");
    }

    // =========================================================================
    // Type annotation extraction tests
    // =========================================================================

    #[test]
    fn prop_field_type_annotation_string_literal_union() {
        let code = "defineProps<{ variant: 'primary' | 'secondary' }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let field = &macros[0].prop_fields[0];
        assert_eq!(field.name, "variant");
        assert_eq!(
            field.type_annotation.as_deref(),
            Some("'primary' | 'secondary'"),
            "should capture string literal union type annotation"
        );
    }

    #[test]
    fn prop_field_type_annotation_primitive() {
        let code = "defineProps<{ count: number }>()";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert_eq!(field.name, "count");
        assert_eq!(
            field.type_annotation.as_deref(),
            Some("number"),
            "should capture primitive type annotation"
        );
    }

    #[test]
    fn prop_field_type_annotation_runtime_constructor() {
        let code = "defineProps({ count: Number })";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert_eq!(field.name, "count");
        // Runtime constructor shorthand is mapped to TS type
        assert_eq!(field.type_annotation.as_deref(), Some("number"));
    }

    #[test]
    fn prop_field_type_annotation_multiple() {
        let code = "defineProps<{ variant: 'a' | 'b', size: 'sm' | 'lg' }>()";
        let macros = parse_macros(code);
        let fields = &macros[0].prop_fields;
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].type_annotation.as_deref(), Some("'a' | 'b'"));
        assert_eq!(fields[1].type_annotation.as_deref(), Some("'sm' | 'lg'"));
    }

    // =========================================================================
    // Slot field extraction tests
    // =========================================================================

    #[test]
    fn slot_fields_property_signature() {
        let code = "defineSlots<{ default(props: {}): any; header?(props: {}): any }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].slot_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 slot fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "default");
        assert!(fields[0].is_required, "default should be required (no ?)");
        assert_eq!(fields[1].name, "header");
        assert!(!fields[1].is_required, "header should be optional (has ?)");
        // Negative: non-defineSlots macros should NOT have slot_fields
        assert!(
            macros[0].prop_fields.is_empty(),
            "defineSlots should not have prop_fields"
        );
    }

    #[test]
    fn slot_fields_method_signature() {
        // Method shorthand syntax: `default(props: {}): any` vs `default?(props: {}): any`
        let code =
            "defineSlots<{ default(props: { item: string }): any; footer?(props: {}): any }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].slot_fields;
        assert_eq!(
            fields.len(),
            2,
            "should extract 2 slot fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "default");
        assert!(
            fields[0].is_required,
            "default (method, no ?) should be required"
        );
        assert_eq!(fields[1].name, "footer");
        assert!(
            !fields[1].is_required,
            "footer? (method, ?) should be optional"
        );
    }

    #[test]
    fn slot_fields_intersection_type() {
        let code = "defineSlots<{ default(p: {}): any } & { sidebar?(p: {}): any }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].slot_fields;
        assert_eq!(
            fields.len(),
            2,
            "intersection should merge slot fields: {:?}",
            fields
        );
        assert_eq!(fields[0].name, "default");
        assert!(fields[0].is_required);
        assert_eq!(fields[1].name, "sidebar");
        assert!(!fields[1].is_required);
    }

    #[test]
    fn slot_fields_type_reference_empty() {
        let code = "defineSlots<MySlots>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].slot_fields.is_empty(),
            "type reference should yield empty slot fields"
        );
    }

    #[test]
    fn slot_fields_no_type_params() {
        let code = "defineSlots()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].slot_fields.is_empty(),
            "no type params should yield empty slot fields"
        );
    }

    #[test]
    fn slot_fields_not_on_other_macros() {
        let code = "defineProps<{ count: number }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].slot_fields.is_empty(),
            "defineProps should not have slot_fields"
        );
    }

    #[test]
    fn slot_fields_all_required() {
        let code = "defineSlots<{ default(p: {}): any; header(p: {}): any; footer(p: {}): any }>()";
        let macros = parse_macros(code);
        let fields = &macros[0].slot_fields;
        assert_eq!(fields.len(), 3);
        for field in fields {
            assert!(
                field.is_required,
                "slot '{}' should be required",
                field.name
            );
        }
    }

    #[test]
    fn slot_fields_all_optional() {
        let code = "defineSlots<{ default?(p: {}): any; header?(p: {}): any }>()";
        let macros = parse_macros(code);
        let fields = &macros[0].slot_fields;
        assert_eq!(fields.len(), 2);
        for field in fields {
            assert!(
                !field.is_required,
                "slot '{}' should be optional",
                field.name
            );
        }
    }

    // =========================================================================
    // Slot field binding extraction tests
    // =========================================================================

    #[test]
    fn slot_fields_method_bindings() {
        let code = "defineSlots<{ default(props: { item: string, index: number }): any }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].slot_fields;
        assert_eq!(fields.len(), 1);
        let bindings = &fields[0].bindings;
        assert_eq!(
            bindings.len(),
            2,
            "should extract 2 bindings: {:?}",
            bindings
        );
        assert_eq!(bindings[0].name, "item");
        assert_eq!(bindings[0].type_annotation.as_deref(), Some("string"));
        assert_eq!(bindings[1].name, "index");
        assert_eq!(bindings[1].type_annotation.as_deref(), Some("number"));
        // Negative: no binding named "props" (that's the param name, not a binding)
        assert!(
            !bindings.iter().any(|b| b.name == "props"),
            "should not include 'props' as a binding name"
        );
    }

    #[test]
    fn slot_fields_property_fn_bindings() {
        let code = "defineSlots<{ default: (props: { row: MyItem }) => any }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].slot_fields;
        assert_eq!(fields.len(), 1);
        let bindings = &fields[0].bindings;
        assert_eq!(
            bindings.len(),
            1,
            "should extract 1 binding: {:?}",
            bindings
        );
        assert_eq!(bindings[0].name, "row");
        // Negative: type_annotation must NOT be None
        assert!(
            bindings[0].type_annotation.is_some(),
            "type_annotation should be present, not None"
        );
        assert_eq!(bindings[0].type_annotation.as_deref(), Some("MyItem"));
    }

    #[test]
    fn slot_fields_no_params_empty_bindings() {
        let code = "defineSlots<{ header(): any }>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].slot_fields;
        assert_eq!(fields.len(), 1);
        assert!(
            fields[0].bindings.is_empty(),
            "slot with no params should have empty bindings"
        );
    }

    #[test]
    fn slot_fields_complex_type_bindings() {
        let code =
            "defineSlots<{ default(props: { items: string[], active: boolean | null }): any }>()";
        let macros = parse_macros(code);
        let fields = &macros[0].slot_fields;
        assert_eq!(fields.len(), 1);
        let bindings = &fields[0].bindings;
        assert_eq!(
            bindings.len(),
            2,
            "should extract 2 bindings: {:?}",
            bindings
        );
        assert_eq!(bindings[0].name, "items");
        assert_eq!(bindings[0].type_annotation.as_deref(), Some("string[]"));
        assert_eq!(bindings[1].name, "active");
        assert_eq!(
            bindings[1].type_annotation.as_deref(),
            Some("boolean | null")
        );
    }

    #[test]
    fn slot_fields_multiple_slots_bindings() {
        let code = "defineSlots<{ default(props: { item: string }): any; header(props: { title: number }): any }>()";
        let macros = parse_macros(code);
        let fields = &macros[0].slot_fields;
        assert_eq!(fields.len(), 2);
        // First slot
        assert_eq!(fields[0].bindings.len(), 1);
        assert_eq!(fields[0].bindings[0].name, "item");
        assert_eq!(
            fields[0].bindings[0].type_annotation.as_deref(),
            Some("string")
        );
        // Second slot
        assert_eq!(fields[1].bindings.len(), 1);
        assert_eq!(fields[1].bindings[0].name, "title");
        assert_eq!(
            fields[1].bindings[0].type_annotation.as_deref(),
            Some("number")
        );
        // Negative: no cross-contamination
        assert!(
            !fields[0].bindings.iter().any(|b| b.name == "title"),
            "default slot should not have header's bindings"
        );
        assert!(
            !fields[1].bindings.iter().any(|b| b.name == "item"),
            "header slot should not have default's bindings"
        );
    }

    #[test]
    fn slot_fields_intersection_bindings() {
        let code =
            "defineSlots<{ default(p: { a: string }): any } & { footer(p: { b: number }): any }>()";
        let macros = parse_macros(code);
        let fields = &macros[0].slot_fields;
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "default");
        assert_eq!(fields[0].bindings.len(), 1);
        assert_eq!(fields[0].bindings[0].name, "a");
        assert_eq!(
            fields[0].bindings[0].type_annotation.as_deref(),
            Some("string")
        );
        assert_eq!(fields[1].name, "footer");
        assert_eq!(fields[1].bindings.len(), 1);
        assert_eq!(fields[1].bindings[0].name, "b");
        assert_eq!(
            fields[1].bindings[0].type_annotation.as_deref(),
            Some("number")
        );
    }

    #[test]
    fn slot_field_binding_span_is_correct() {
        let code = "defineSlots<{ default: (props: { item: string, index: number }) => any }>()";
        let macros = parse_macros(code);
        let bindings = &macros[0].slot_fields[0].bindings;
        assert_eq!(bindings.len(), 2);

        // Verify span points to the binding key in source
        assert_eq!(
            &code[bindings[0].span.start as usize..bindings[0].span.end as usize],
            "item"
        );
        assert_eq!(
            &code[bindings[1].span.start as usize..bindings[1].span.end as usize],
            "index"
        );

        // Negative: span should not be zero
        assert!(
            bindings[0].span.start > 0 || bindings[0].span.end > 0,
            "item span should be non-zero"
        );
        assert!(
            bindings[1].span.start > 0 || bindings[1].span.end > 0,
            "index span should be non-zero"
        );
    }

    // ── JSDoc extraction tests ───────────────────────────────────

    #[test]
    fn jsdoc_on_prop_fields() {
        let code = r#"defineProps<{
            /** The display label */
            label: string
            /** Size variant
             * @default 'md'
             */
            size: string
            noDoc: number
        }>()"#;
        let macros = parse_macros(code);
        let fields = &macros[0].prop_fields;

        assert_eq!(fields.len(), 3);

        // label has description, no tags
        assert_eq!(fields[0].description.as_deref(), Some("The display label"));
        assert!(fields[0].tags.is_empty());

        // size has description and @default tag
        assert_eq!(fields[1].description.as_deref(), Some("Size variant"));
        assert_eq!(fields[1].tags.len(), 1);
        assert_eq!(fields[1].tags[0].name, "default");
        assert_eq!(fields[1].tags[0].text.as_deref(), Some("'md'"));

        // noDoc has no JSDoc
        assert!(fields[2].description.is_none());
        assert!(fields[2].tags.is_empty());
    }

    #[test]
    fn jsdoc_on_runtime_prop_fields() {
        let code = r#"defineProps({
            /** The display label */
            label: String,
            /** Size variant
             * @default 'md'
             */
            size: { type: String, default: 'md' },
            noDoc: Number,
        })"#;
        let macros = parse_macros(code);
        let fields = &macros[0].prop_fields;
        assert_eq!(fields.len(), 3);

        // Positive: label has description, no tags
        assert_eq!(fields[0].description.as_deref(), Some("The display label"));
        assert!(fields[0].tags.is_empty());

        // Positive: size has description and @default tag
        assert_eq!(fields[1].description.as_deref(), Some("Size variant"));
        assert_eq!(fields[1].tags.len(), 1);
        assert_eq!(fields[1].tags[0].name, "default");
        assert_eq!(fields[1].tags[0].text.as_deref(), Some("'md'"));

        // Negative: noDoc has no JSDoc
        assert!(fields[2].description.is_none());
        assert!(fields[2].tags.is_empty());
    }

    #[test]
    fn jsdoc_on_emit_fields() {
        let code = r#"defineEmits<{
            /** Fired on click */
            click: []
            /** @deprecated use 'input' instead */
            change: [value: string]
        }>()"#;
        let macros = parse_macros(code);
        let fields = &macros[0].emit_fields;

        assert_eq!(fields.len(), 2);

        assert_eq!(fields[0].description.as_deref(), Some("Fired on click"));
        assert!(fields[0].tags.is_empty());

        assert!(
            fields[1].description.is_none(),
            "tag-only JSDoc should not have description"
        );
        assert_eq!(fields[1].tags.len(), 1);
        assert_eq!(fields[1].tags[0].name, "deprecated");
        assert_eq!(
            fields[1].tags[0].text.as_deref(),
            Some("use 'input' instead")
        );
    }

    #[test]
    fn jsdoc_on_slot_fields() {
        let code = r#"defineSlots<{
            /** The main content area */
            default(props: { item: string }): any
        }>()"#;
        let macros = parse_macros(code);
        let fields = &macros[0].slot_fields;

        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0].description.as_deref(),
            Some("The main content area")
        );
        assert!(fields[0].tags.is_empty());
    }

    #[test]
    fn jsdoc_with_multiple_tags() {
        let code = r#"defineProps<{
            /**
             * User identifier
             * @param {string} id - The user ID
             * @deprecated Use userId instead
             * @see https://example.com
             */
            id: string
        }>()"#;
        let macros = parse_macros(code);
        let fields = &macros[0].prop_fields;

        assert_eq!(fields[0].description.as_deref(), Some("User identifier"));
        assert_eq!(fields[0].tags.len(), 3);
        assert_eq!(fields[0].tags[0].name, "param");
        assert_eq!(
            fields[0].tags[0].text.as_deref(),
            Some("{string} id - The user ID")
        );
        assert_eq!(fields[0].tags[1].name, "deprecated");
        assert_eq!(
            fields[0].tags[1].text.as_deref(),
            Some("Use userId instead")
        );
        assert_eq!(fields[0].tags[2].name, "see");
        assert_eq!(
            fields[0].tags[2].text.as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn no_jsdoc_produces_none_and_empty() {
        let code = r#"defineProps<{ count: number }>()"#;
        let macros = parse_macros(code);
        let fields = &macros[0].prop_fields;

        assert_eq!(fields.len(), 1);
        assert!(fields[0].description.is_none());
        assert!(fields[0].tags.is_empty());
    }

    // =========================================================================
    // Issue 1: Prop field is_optional
    // =========================================================================

    #[test]
    fn prop_field_optional_type_based() {
        let code = "defineProps<{ name?: string, count: number }>()";
        let macros = parse_macros(code);
        let fields = &macros[0].prop_fields;
        assert_eq!(fields.len(), 2);
        assert!(fields[0].is_optional, "name? should be optional");
        assert!(
            !fields[1].is_optional,
            "count (no ?) should NOT be optional"
        );
    }

    #[test]
    fn prop_field_optional_runtime_default() {
        // Vue semantics: runtime props are optional by default (unless required: true)
        let code = "defineProps({ count: Number })";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert!(
            field.is_optional,
            "runtime props without required:true should be optional (Vue default)"
        );
    }

    #[test]
    fn prop_field_optional_array_default() {
        // Vue semantics: array-form props have no required info → optional by default
        let code = "defineProps(['count'])";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert!(
            field.is_optional,
            "array-form props should be optional by default"
        );
    }

    // =========================================================================
    // Issue 2: withDefaults default_keys
    // =========================================================================

    #[test]
    fn with_defaults_extracts_default_keys() {
        let code = r#"withDefaults(defineProps<{ foo: string, bar: number, baz: boolean }>(), { foo: 'hello', baz: true })"#;
        let macros = parse_macros(code);
        let wd = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::WithDefaults)
            .unwrap();
        let mut keys = wd.default_keys.clone();
        keys.sort();
        assert_eq!(
            keys,
            vec!["baz", "foo"],
            "should extract default keys from object literal"
        );
        // Negative: defineProps should NOT have default_keys
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert!(
            dp.default_keys.is_empty(),
            "defineProps should have empty default_keys"
        );
    }

    #[test]
    fn with_defaults_no_object_arg_empty_keys() {
        // withDefaults with non-object second arg (rare, but should not crash)
        let code = "withDefaults(defineProps<{ foo: string }>(), defaults)";
        let macros = parse_macros(code);
        let wd = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::WithDefaults)
            .unwrap();
        assert!(
            wd.default_keys.is_empty(),
            "non-object second arg should yield empty default_keys"
        );
    }

    #[test]
    fn with_defaults_extracts_default_values() {
        let code = r#"withDefaults(defineProps<{ foo: string, bar: number, baz: boolean }>(), { foo: 'hello', baz: true })"#;
        let macros = parse_macros(code);
        let wd = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::WithDefaults)
            .unwrap();
        assert_eq!(
            wd.default_values.len(),
            2,
            "should extract 2 default values"
        );
        let foo_val = wd.default_values.iter().find(|d| d.key == "foo").unwrap();
        assert_eq!(foo_val.value, "hello", "string default should strip quotes");
        let baz_val = wd.default_values.iter().find(|d| d.key == "baz").unwrap();
        assert_eq!(baz_val.value, "true");
    }

    // =========================================================================
    // Issue 4: defineExpose expose_fields
    // =========================================================================

    #[test]
    fn define_expose_extracts_fields() {
        let code = "defineExpose({ foo, bar, baz: computed(() => 1) })";
        let macros = parse_macros(code);
        let de = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineExpose)
            .unwrap();
        assert_eq!(de.expose_fields.len(), 3, "should extract 3 expose fields");
        assert_eq!(de.expose_fields[0].name, "foo");
        assert_eq!(de.expose_fields[1].name, "bar");
        assert_eq!(de.expose_fields[2].name, "baz");
    }

    #[test]
    fn define_expose_empty_object() {
        let code = "defineExpose({})";
        let macros = parse_macros(code);
        let de = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineExpose)
            .unwrap();
        assert!(
            de.expose_fields.is_empty(),
            "empty object should yield empty expose_fields"
        );
    }

    #[test]
    fn define_expose_no_args() {
        let code = "defineExpose()";
        let macros = parse_macros(code);
        let de = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineExpose)
            .unwrap();
        assert!(
            de.expose_fields.is_empty(),
            "no args should yield empty expose_fields"
        );
    }

    #[test]
    fn define_expose_identifier_arg_empty() {
        let code = "defineExpose(myObj)";
        let macros = parse_macros(code);
        let de = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineExpose)
            .unwrap();
        assert!(
            de.expose_fields.is_empty(),
            "identifier arg should yield empty expose_fields (can't resolve)"
        );
    }

    #[test]
    fn expose_fields_not_on_other_macros() {
        let code = "defineProps<{ count: number }>()";
        let macros = parse_macros(code);
        assert!(
            macros[0].expose_fields.is_empty(),
            "defineProps should not have expose_fields"
        );
    }

    // =========================================================================
    // Issue 5: Emit field payload_type
    // =========================================================================

    #[test]
    fn emit_field_payload_type_property_signature() {
        let code = "defineEmits<{ change: [id: number]; click: [] }>()";
        let macros = parse_macros(code);
        let fields = &macros[0].emit_fields;
        assert_eq!(fields.len(), 2);
        assert_eq!(
            fields[0].payload_type.as_deref(),
            Some("[id: number]"),
            "change should have payload type"
        );
        assert_eq!(
            fields[1].payload_type.as_deref(),
            Some("[]"),
            "click should have empty tuple payload"
        );
    }

    #[test]
    fn emit_field_payload_type_call_signature() {
        let code = "defineEmits<{ (e: 'change', id: number): void }>()";
        let macros = parse_macros(code);
        let fields = &macros[0].emit_fields;
        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0].payload_type.as_deref(),
            Some("[id: number]"),
            "call signature should extract params after event name as tuple"
        );
    }

    #[test]
    fn emit_field_payload_type_call_signature_no_payload() {
        let code = "defineEmits<{ (e: 'click'): void }>()";
        let macros = parse_macros(code);
        let fields = &macros[0].emit_fields;
        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0].payload_type.as_deref(),
            Some("[]"),
            "call signature with no extra params should have empty tuple"
        );
    }

    #[test]
    fn emit_field_payload_type_runtime_none() {
        let code = "defineEmits(['click'])";
        let macros = parse_macros(code);
        let fields = &macros[0].emit_fields;
        assert_eq!(fields.len(), 1);
        assert!(
            fields[0].payload_type.is_none(),
            "runtime emits should have no payload type"
        );
    }

    #[test]
    fn parse_jsdoc_unit_tests() {
        // Simple description
        let (desc, tags) = parse_jsdoc("/** Hello world */");
        assert_eq!(desc.as_deref(), Some("Hello world"));
        assert!(tags.is_empty());

        // Tag only
        let (desc, tags) = parse_jsdoc("/** @deprecated */");
        assert!(desc.is_none());
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "deprecated");
        assert!(tags[0].text.is_none());

        // Tag with text
        let (desc, tags) = parse_jsdoc("/** @default 'hello' */");
        assert!(desc.is_none());
        assert_eq!(tags[0].name, "default");
        assert_eq!(tags[0].text.as_deref(), Some("'hello'"));

        // Multi-line
        let (desc, tags) = parse_jsdoc(
            "/**\n * A description\n * @param name - the name\n * @returns nothing\n */",
        );
        assert_eq!(desc.as_deref(), Some("A description"));
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].name, "param");
        assert_eq!(tags[0].text.as_deref(), Some("name - the name"));
        assert_eq!(tags[1].name, "returns");
        assert_eq!(tags[1].text.as_deref(), Some("nothing"));
    }

    // =========================================================================
    // defineModel type extraction
    // =========================================================================

    #[test]
    fn define_model_type_string() {
        let code = "defineModel<string>()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].kind, AnalyzedMacroKind::DefineModel);
        let fields = &macros[0].prop_fields;
        assert_eq!(
            fields.len(),
            1,
            "defineModel<string> should produce 1 prop field"
        );
        assert_eq!(fields[0].name, "modelValue");
        assert_eq!(
            fields[0].type_annotation.as_deref(),
            Some("string"),
            "type_annotation should be 'string'"
        );
        assert!(!fields[0].is_optional);
    }

    #[test]
    fn define_model_named_with_type() {
        let code = "defineModel<number>('count')";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let fields = &macros[0].prop_fields;
        assert_eq!(fields.len(), 1);
        assert_eq!(
            fields[0].name, "count",
            "named model should use the name argument"
        );
        assert_eq!(fields[0].type_annotation.as_deref(), Some("number"));
    }

    #[test]
    fn define_model_complex_type() {
        let code = "defineModel<string | number>()";
        let macros = parse_macros(code);
        let fields = &macros[0].prop_fields;
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "modelValue");
        assert_eq!(
            fields[0].type_annotation.as_deref(),
            Some("string | number")
        );
    }

    #[test]
    fn define_model_no_type_param() {
        let code = "defineModel()";
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        assert!(
            macros[0].prop_fields.is_empty(),
            "defineModel without type param should have no prop_fields"
        );
    }

    // ── Type resolution tests ──

    #[test]
    fn resolve_local_interface_in_define_props() {
        let source = r#"
            interface Props { title: string; count: number; active?: boolean }
            defineProps<Props>()
        "#;
        let alloc = Allocator::default();
        let macros = parse_and_extract(&alloc, source);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert_eq!(
            dp.prop_fields.len(),
            3,
            "should resolve 3 fields from local interface"
        );
        assert_eq!(dp.prop_fields[0].name, "title");
        assert_eq!(dp.prop_fields[0].type_annotation.as_deref(), Some("string"));
        assert!(!dp.prop_fields[0].is_optional);
        assert_eq!(dp.prop_fields[1].name, "count");
        assert_eq!(dp.prop_fields[1].type_annotation.as_deref(), Some("number"));
        assert_eq!(dp.prop_fields[2].name, "active");
        assert!(dp.prop_fields[2].is_optional);
        assert!(
            !dp.prop_fields.iter().any(|f| f.resolution_error.is_some()),
            "all fields should be resolved without errors"
        );
    }

    #[test]
    fn resolve_local_type_alias_in_define_props() {
        let source = r#"
            type MyProps = { name: string; age?: number }
            defineProps<MyProps>()
        "#;
        let alloc = Allocator::default();
        let macros = parse_and_extract(&alloc, source);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert_eq!(
            dp.prop_fields.len(),
            2,
            "should resolve 2 fields from type alias"
        );
        assert_eq!(dp.prop_fields[0].name, "name");
        assert!(dp.prop_fields[1].is_optional);
    }

    #[test]
    fn resolve_interface_extends_chain() {
        let source = r#"
            interface Base { id: number; name: string }
            interface Extended extends Base { email: string; active?: boolean }
            defineProps<Extended>()
        "#;
        let alloc = Allocator::default();
        let macros = parse_and_extract(&alloc, source);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert_eq!(
            dp.prop_fields.len(),
            4,
            "should have all 4 fields (2 inherited + 2 own)"
        );
        let names: Vec<&str> = dp.prop_fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"id"), "should have inherited 'id'");
        assert!(names.contains(&"name"), "should have inherited 'name'");
        assert!(names.contains(&"email"), "should have own 'email'");
        assert!(names.contains(&"active"), "should have own 'active'");
    }

    #[test]
    fn resolve_mixed_intersection_type() {
        let source = r#"
            interface Identifiable { id: number }
            type Named = { name: string; label?: string }
            defineProps<Identifiable & Named & { extra: boolean }>()
        "#;
        let alloc = Allocator::default();
        let macros = parse_and_extract(&alloc, source);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert_eq!(
            dp.prop_fields.len(),
            4,
            "should merge all 4 fields from intersection"
        );
        let names: Vec<&str> = dp.prop_fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"id"));
        assert!(names.contains(&"name"));
        assert!(names.contains(&"label"));
        assert!(names.contains(&"extra"));
        let label = dp.prop_fields.iter().find(|f| f.name == "label").unwrap();
        assert!(label.is_optional, "label should be optional");
    }

    #[test]
    fn resolve_partial_wrapping_local_interface() {
        let source = r#"
            interface Props { title: string; count: number }
            defineProps<Partial<Props>>()
        "#;
        let alloc = Allocator::default();
        let macros = parse_and_extract(&alloc, source);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert_eq!(dp.prop_fields.len(), 2);
        assert!(
            dp.prop_fields.iter().all(|f| f.is_optional),
            "Partial should make all fields optional"
        );
    }

    #[test]
    fn unresolvable_class_type_returns_empty() {
        let source = r#"
            class UserModel { constructor(public id: number) {} }
            defineProps<{ model: UserModel }>()
        "#;
        let alloc = Allocator::default();
        let macros = parse_and_extract(&alloc, source);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        // The inline literal has one field "model" with type "UserModel"
        assert_eq!(dp.prop_fields.len(), 1);
        assert_eq!(dp.prop_fields[0].name, "model");
    }

    #[test]
    fn resolved_local_types_populated_for_interface() {
        let source = r#"
            interface Props { title: string; count: number }
            defineProps<Props>()
        "#;
        let alloc = Allocator::default();
        let macros = parse_and_extract(&alloc, source);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        assert_eq!(
            dp.resolved_local_types.len(),
            1,
            "should have one resolved local type"
        );
        assert_eq!(dp.resolved_local_types[0].name, "Props");
        assert!(
            dp.resolved_local_types[0]
                .expanded
                .contains("title: string"),
            "expanded text should contain field definitions"
        );
    }

    #[test]
    fn pick_omit_return_none_unresolvable() {
        let source = r#"
            interface Full { id: number; name: string; password: string }
            defineProps<{ display: Pick<Full, 'id' | 'name'> }>()
        "#;
        let alloc = Allocator::default();
        let macros = parse_and_extract(&alloc, source);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();
        // The inline literal extracts "display" field but cannot resolve Pick<Full,...>
        assert_eq!(dp.prop_fields.len(), 1);
        assert_eq!(dp.prop_fields[0].name, "display");
    }

    #[test]
    fn runtime_define_props_extracts_type_and_default() {
        let source = r#"defineProps({ message: { type: String, default: 'Hello from JS' }, count: Number, active: { type: Boolean, default: true } })"#;
        let alloc = Allocator::default();
        let macros = parse_and_extract(&alloc, source);
        let dp = macros
            .iter()
            .find(|m| m.kind == AnalyzedMacroKind::DefineProps)
            .unwrap();

        // Should extract 3 prop fields
        assert_eq!(dp.prop_fields.len(), 3);

        // message: { type: String, default: 'Hello from JS' }
        let msg = dp.prop_fields.iter().find(|f| f.name == "message").unwrap();
        assert_eq!(msg.type_annotation.as_deref(), Some("string"));

        // count: Number (shorthand)
        let cnt = dp.prop_fields.iter().find(|f| f.name == "count").unwrap();
        assert_eq!(cnt.type_annotation.as_deref(), Some("number"));

        // active: { type: Boolean, default: true }
        let act = dp.prop_fields.iter().find(|f| f.name == "active").unwrap();
        assert_eq!(act.type_annotation.as_deref(), Some("boolean"));

        // Should have default keys
        assert!(dp.default_keys.contains(&"message".to_string()));
        assert!(dp.default_keys.contains(&"active".to_string()));
        assert!(!dp.default_keys.contains(&"count".to_string()));

        // Should have default values
        let msg_default = dp
            .default_values
            .iter()
            .find(|d| d.key == "message")
            .unwrap();
        assert_eq!(msg_default.value, "Hello from JS");

        let act_default = dp
            .default_values
            .iter()
            .find(|d| d.key == "active")
            .unwrap();
        assert_eq!(act_default.value, "true");
    }

    // =========================================================================
    // TSAsExpression type assertion extraction tests
    // =========================================================================

    #[test]
    fn prop_ts_as_prop_type_angle() {
        // `Object as PropType<typeof Card>` should extract `typeof Card`, not `object`
        let code = "defineProps({ foz: { type: Object as PropType<typeof Card> } })";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert_eq!(field.name, "foz");
        assert_eq!(
            field.type_annotation.as_deref(),
            Some("typeof Card"),
            "PropType<T> assertion should yield T, not the base constructor type"
        );
        assert!(
            field.type_annotation.as_deref() != Some("object"),
            "should not degrade to 'object'"
        );
    }

    #[test]
    fn prop_ts_as_arrow_return() {
        // `Object as () => typeof Card` should extract `typeof Card` (the return type)
        let code = "defineProps({ baz: { type: Object as () => typeof Card } })";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert_eq!(field.name, "baz");
        assert_eq!(
            field.type_annotation.as_deref(),
            Some("typeof Card"),
            "() => T assertion should yield T, not the callable type"
        );
        assert!(
            field.type_annotation.as_deref() != Some("object"),
            "should not degrade to 'object'"
        );
        assert!(
            field.type_annotation.as_deref() != Some("Function"),
            "should not degrade to 'Function'"
        );
    }

    #[test]
    fn prop_ts_as_new_ctor_return() {
        // `Object as new () => typeof Card` should extract `typeof Card` (the return type)
        let code = "defineProps({ comp: { type: Object as new () => typeof Card } })";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert_eq!(field.name, "comp");
        assert_eq!(
            field.type_annotation.as_deref(),
            Some("typeof Card"),
            "new () => T assertion should yield T"
        );
        assert!(
            field.type_annotation.as_deref() != Some("object"),
            "should not degrade to 'object'"
        );
    }

    // =========================================================================
    // Runtime prop optionality tests (Vue semantics: optional unless required:true)
    // =========================================================================

    #[test]
    fn prop_shorthand_defaults_to_optional() {
        // `bar: Number` — no required field → is_optional: true (Vue default)
        let code = "defineProps({ bar: Number })";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert_eq!(field.name, "bar");
        assert!(
            field.is_optional,
            "shorthand runtime prop without required:true should be optional"
        );
    }

    #[test]
    fn prop_required_true_is_not_optional() {
        // `required: true` → is_optional: false
        let code = "defineProps({ foo: { type: String, required: true } })";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert_eq!(field.name, "foo");
        assert!(
            !field.is_optional,
            "runtime prop with required:true should not be optional"
        );
    }

    #[test]
    fn prop_required_false_is_optional() {
        // `required: false` → is_optional: true
        let code = "defineProps({ bar: { type: String, required: false } })";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert!(
            field.is_optional,
            "runtime prop with required:false should be optional"
        );
    }

    #[test]
    fn prop_with_default_is_optional() {
        // Props with a default value are optional (no required:true)
        let code = "defineProps({ count: { type: Number, default: 0 } })";
        let macros = parse_macros(code);
        let field = &macros[0].prop_fields[0];
        assert!(
            field.is_optional,
            "runtime prop with default but no required:true should be optional"
        );
    }

    #[test]
    fn prop_array_form_is_optional() {
        // Array form props have no type or required info → all optional
        let code = "defineProps(['title', 'active'])";
        let macros = parse_macros(code);
        let fields = &macros[0].prop_fields;
        assert_eq!(fields.len(), 2);
        assert!(
            fields[0].is_optional,
            "array-form props should be optional by default"
        );
        assert!(
            fields[1].is_optional,
            "array-form props should be optional by default"
        );
    }

    #[test]
    fn prop_mixed_fixture() {
        // Full regression fixture covering PropType<T>, () => T, required:true, and defaults
        let code = r#"defineProps({
  bar: Number,
  foo: { type: String, required: true },
  baz: { type: Object as () => typeof Card, default: () => { return Card } },
  foz: { type: Object as PropType<typeof Card>, default: () => { return Card } }
})"#;
        let macros = parse_macros(code);
        assert_eq!(macros.len(), 1);
        let dp = &macros[0];
        assert_eq!(dp.prop_fields.len(), 4);

        let bar = dp.prop_fields.iter().find(|f| f.name == "bar").unwrap();
        assert_eq!(bar.type_annotation.as_deref(), Some("number"));
        assert!(
            bar.is_optional,
            "bar has no required:true, should be optional"
        );

        let foo = dp.prop_fields.iter().find(|f| f.name == "foo").unwrap();
        assert_eq!(foo.type_annotation.as_deref(), Some("string"));
        assert!(
            !foo.is_optional,
            "foo has required:true, should not be optional"
        );

        let baz = dp.prop_fields.iter().find(|f| f.name == "baz").unwrap();
        assert_eq!(
            baz.type_annotation.as_deref(),
            Some("typeof Card"),
            "baz: Object as () => typeof Card should extract 'typeof Card'"
        );
        assert!(
            baz.is_optional,
            "baz has default, no required:true — should be optional"
        );
        assert!(
            baz.type_annotation.as_deref() != Some("object"),
            "baz must not degrade to 'object'"
        );
        assert!(
            baz.type_annotation.as_deref() != Some("Function"),
            "baz must not degrade to 'Function'"
        );
        assert!(
            baz.type_annotation.as_deref() != Some("unknown"),
            "baz must not degrade to 'unknown'"
        );

        let foz = dp.prop_fields.iter().find(|f| f.name == "foz").unwrap();
        assert_eq!(
            foz.type_annotation.as_deref(),
            Some("typeof Card"),
            "foz: Object as PropType<typeof Card> should extract 'typeof Card'"
        );
        assert!(
            foz.is_optional,
            "foz has default, no required:true — should be optional"
        );
        assert!(
            foz.type_annotation.as_deref() != Some("object"),
            "foz must not degrade to 'object'"
        );
        assert!(
            foz.type_annotation.as_deref() != Some("Function"),
            "foz must not degrade to 'Function'"
        );
        assert!(
            foz.type_annotation.as_deref() != Some("unknown"),
            "foz must not degrade to 'unknown'"
        );

        // Default values should contain the arrow function source
        let baz_default = dp.default_values.iter().find(|d| d.key == "baz").unwrap();
        assert!(
            baz_default.value.contains("=>"),
            "baz default should preserve arrow function source"
        );
        let foz_default = dp.default_values.iter().find(|d| d.key == "foz").unwrap();
        assert!(
            foz_default.value.contains("=>"),
            "foz default should preserve arrow function source"
        );
    }
}
