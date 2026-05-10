use oxc_ast::ast::*;
use oxc_ast::Comment;
use oxc_span::GetSpan;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::analysis::types::{
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

fn insert_local_type_decl_from_declaration<'a>(
    registry: &mut FxHashMap<String, LocalTypeDecl<'a>>,
    decl: &'a Declaration<'a>,
) {
    match decl {
        Declaration::TSInterfaceDeclaration(decl) => {
            let extends: &[TSInterfaceHeritage<'_>] = &decl.extends;
            registry.insert(
                decl.id.name.to_string(),
                LocalTypeDecl::Interface {
                    body: &decl.body,
                    extends,
                },
            );
        }
        Declaration::TSTypeAliasDeclaration(decl) => {
            registry.insert(
                decl.id.name.to_string(),
                LocalTypeDecl::Alias(&decl.type_annotation),
            );
        }
        Declaration::ClassDeclaration(decl) => {
            if let Some(ref id) = decl.id {
                registry.insert(id.name.to_string(), LocalTypeDecl::Class);
            }
        }
        _ => {}
    }
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
            Statement::ExportNamedDeclaration(export) => {
                if let Some(ref decl) = export.declaration {
                    insert_local_type_decl_from_declaration(&mut registry, decl);
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
                    // Resolve extends chain via direct registry lookup.
                    // Skip unresolvable heritage clauses rather than aborting,
                    // so that successfully-resolved parents and own fields
                    // are still returned.
                    let mut all_fields = Vec::new();
                    let mut seen_names = FxHashSet::default();
                    for heritage in *extends {
                        let Some(parent_name) = heritage_name(&heritage.expression) else {
                            continue;
                        };
                        let Some(parent_decl) = registry.get(&parent_name) else {
                            continue;
                        };
                        let Some(parent_fields) = resolve_interface_decl(
                            &parent_name,
                            parent_decl,
                            registry,
                            source,
                            comments,
                            visited,
                        ) else {
                            continue;
                        };
                        for field in parent_fields {
                            if seen_names.insert(field.name.clone()) {
                                all_fields.push(field);
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

    // Collect type param AST nodes for each macro by matching spans
    let props_type_params = collect_macro_call_type_params(program, "defineProps");
    let emits_type_params = collect_macro_call_type_params(program, "defineEmits");
    let slots_type_params = collect_macro_call_type_params(program, "defineSlots");

    for mac in macros.iter_mut() {
        if !mac.is_type_based || mac.type_references.is_empty() {
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

        match mac.kind {
            AnalyzedMacroKind::DefineProps => {
                resolve_local_define_props(
                    mac,
                    &props_type_params,
                    &registry,
                    source,
                    &program.comments,
                );
            }
            AnalyzedMacroKind::DefineEmits => {
                resolve_local_define_emits(
                    mac,
                    &emits_type_params,
                    &registry,
                    source,
                    &program.comments,
                );
            }
            AnalyzedMacroKind::DefineSlots => {
                resolve_local_define_slots(
                    mac,
                    &slots_type_params,
                    &registry,
                    source,
                    &program.comments,
                );
            }
            _ => continue,
        }
    }
}

/// Resolve local types for a defineProps macro.
fn resolve_local_define_props(
    mac: &mut AnalyzedMacro,
    type_params: &[(u32, &TSType<'_>)],
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
) {
    let mut visited = FxHashSet::default();
    let mut resolved_types = Vec::new();

    let mac_start = mac.span.start;
    if let Some(type_param) = type_params.iter().find(|tp| tp.0 == mac_start) {
        let direct_local_root_names = collect_direct_local_macro_root_names(type_param.1);
        if let Some(fields) =
            resolve_type_to_prop_fields(type_param.1, registry, source, comments, &mut visited)
        {
            for type_ref in &direct_local_root_names {
                if let Some(decl) = registry.get(type_ref.as_str()) {
                    visited.clear();
                    if let Some(ref_fields) = resolve_interface_decl(
                        type_ref,
                        decl,
                        registry,
                        source,
                        comments,
                        &mut visited,
                    ) {
                        let expanded = build_expanded_type_text(&ref_fields);
                        let span = match decl {
                            LocalTypeDecl::Interface { body, .. } => body.span.into(),
                            LocalTypeDecl::Alias(t) => t.span().into(),
                            LocalTypeDecl::Class => verter_span::Span::default(),
                        };
                        let type_expr = Some(build_expanded_type_expr(&ref_fields));
                        debug_assert!(
                            type_expr.is_some() || expanded.is_empty(),
                            "ResolvedLocalType.type_expr MUST be populated when expanded is non-empty"
                        );
                        resolved_types.push(ResolvedLocalType {
                            name: type_ref.clone(),
                            expanded,
                            type_expr,
                            span,
                        });
                    }
                }
            }
            mac.prop_fields = fields;
        } else {
            visited.clear();
            if let Some(fields) = resolve_type_to_local_own_prop_fields(
                type_param.1,
                registry,
                source,
                comments,
                &mut visited,
            ) {
                mac.prop_fields = fields;
            }
        }
    } else {
        // Fallback: resolve individual type references (single ref case)
        visited.clear();
        if mac.type_references.len() == 1 {
            let type_ref = &mac.type_references[0];
            if let Some(decl) = registry.get(type_ref.as_str()) {
                if let Some(fields) =
                    resolve_interface_decl(type_ref, decl, registry, source, comments, &mut visited)
                {
                    let expanded = build_expanded_type_text(&fields);
                    let span = match decl {
                        LocalTypeDecl::Interface { body, .. } => body.span.into(),
                        LocalTypeDecl::Alias(t) => t.span().into(),
                        LocalTypeDecl::Class => verter_span::Span::default(),
                    };
                    let type_expr = Some(build_expanded_type_expr(&fields));
                    debug_assert!(
                        type_expr.is_some() || expanded.is_empty(),
                        "ResolvedLocalType.type_expr MUST be populated when expanded is non-empty"
                    );
                    resolved_types.push(ResolvedLocalType {
                        name: type_ref.clone(),
                        expanded,
                        type_expr,
                        span,
                    });
                    mac.prop_fields = fields;
                } else {
                    visited.clear();
                    if let Some(fields) = resolve_local_decl_own_prop_fields(
                        decl,
                        registry,
                        source,
                        comments,
                        &mut visited,
                    ) {
                        mac.prop_fields = fields;
                    }
                }
            }
        }
    }

    mac.resolved_local_types = resolved_types;
}

fn collect_direct_local_macro_root_names(ts_type: &TSType<'_>) -> Vec<String> {
    fn collect(ts_type: &TSType<'_>, direct_roots: &mut Vec<String>) -> bool {
        match ts_type {
            TSType::TSParenthesizedType(parenthesized) => {
                collect(&parenthesized.type_annotation, direct_roots)
            }
            TSType::TSTypeReference(type_ref) => {
                let name = type_name_to_string(&type_ref.type_name);
                if name.is_empty() {
                    return false;
                }
                direct_roots.push(name);
                true
            }
            TSType::TSIntersectionType(intersection) => {
                let start_len = direct_roots.len();
                if intersection
                    .types
                    .iter()
                    .all(|inner| collect(inner, direct_roots))
                {
                    true
                } else {
                    direct_roots.truncate(start_len);
                    false
                }
            }
            _ => false,
        }
    }

    let mut direct_roots = Vec::new();
    if !collect(ts_type, &mut direct_roots) {
        return Vec::new();
    }

    let mut seen = FxHashSet::default();
    direct_roots
        .into_iter()
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

fn resolve_local_decl_own_prop_fields(
    decl: &LocalTypeDecl<'_>,
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
    visited: &mut FxHashSet<String>,
) -> Option<Vec<AnalyzedPropField>> {
    match decl {
        LocalTypeDecl::Interface { body, .. } => {
            Some(extract_fields_from_interface_body(body, source, comments))
        }
        LocalTypeDecl::Alias(aliased_type) => {
            resolve_type_to_local_own_prop_fields(aliased_type, registry, source, comments, visited)
        }
        LocalTypeDecl::Class => None,
    }
}

fn resolve_type_to_local_own_prop_fields(
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
        TSType::TSParenthesizedType(parenthesized) => resolve_type_to_local_own_prop_fields(
            &parenthesized.type_annotation,
            registry,
            source,
            comments,
            visited,
        ),
        TSType::TSTypeReference(type_ref) => {
            let name = type_name_to_string(&type_ref.type_name);
            if name.is_empty() || !visited.insert(name.clone()) {
                return None;
            }
            let result = match registry.get(&name) {
                Some(LocalTypeDecl::Interface { body, .. }) => {
                    Some(extract_fields_from_interface_body(body, source, comments))
                }
                Some(LocalTypeDecl::Alias(aliased_type)) => resolve_type_to_local_own_prop_fields(
                    aliased_type,
                    registry,
                    source,
                    comments,
                    visited,
                ),
                Some(LocalTypeDecl::Class) | None => None,
            };
            visited.remove(&name);
            result
        }
        TSType::TSIntersectionType(intersection) => {
            let mut all_fields = Vec::new();
            let mut seen_names = FxHashSet::default();
            for ty in &intersection.types {
                if let Some(fields) =
                    resolve_type_to_local_own_prop_fields(ty, registry, source, comments, visited)
                {
                    for field in fields {
                        if seen_names.insert(field.name.clone()) {
                            all_fields.push(field);
                        }
                    }
                }
            }
            (!all_fields.is_empty()).then_some(all_fields)
        }
        _ => None,
    }
}

/// Resolve local types for a defineEmits macro.
fn resolve_local_define_emits(
    mac: &mut AnalyzedMacro,
    type_params: &[(u32, &TSType<'_>)],
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
) {
    let mut visited = FxHashSet::default();
    let mac_start = mac.span.start;

    if let Some(type_param) = type_params.iter().find(|tp| tp.0 == mac_start) {
        if let Some(fields) =
            resolve_type_to_emit_fields(type_param.1, registry, source, comments, &mut visited)
        {
            mac.emit_fields = fields;
        }
    } else if mac.type_references.len() == 1 {
        let type_ref = &mac.type_references[0];
        if let Some(decl) = registry.get(type_ref.as_str()) {
            if let Some(fields) = resolve_interface_decl_generic(
                type_ref,
                decl,
                registry,
                source,
                comments,
                &mut visited,
                &extract_emit_fields_from_members,
            ) {
                mac.emit_fields = fields;
            }
        }
    }
}

/// Resolve local types for a defineSlots macro.
fn resolve_local_define_slots(
    mac: &mut AnalyzedMacro,
    type_params: &[(u32, &TSType<'_>)],
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
) {
    let mut visited = FxHashSet::default();
    let mac_start = mac.span.start;

    if let Some(type_param) = type_params.iter().find(|tp| tp.0 == mac_start) {
        if let Some(fields) =
            resolve_type_to_slot_fields(type_param.1, registry, source, comments, &mut visited)
        {
            mac.slot_fields = fields;
        }
    } else if mac.type_references.len() == 1 {
        let type_ref = &mac.type_references[0];
        if let Some(decl) = registry.get(type_ref.as_str()) {
            if let Some(fields) = resolve_interface_decl_generic(
                type_ref,
                decl,
                registry,
                source,
                comments,
                &mut visited,
                &extract_slot_fields_from_members,
            ) {
                mac.slot_fields = fields;
            }
        }
    }
}

/// Collect the type parameter AST nodes for all calls to `callee_name<T>()` in the program.
/// Returns `(call_span_start, &TSType)` pairs.
fn collect_macro_call_type_params<'a>(
    program: &'a Program<'a>,
    callee_name: &str,
) -> Vec<(u32, &'a TSType<'a>)> {
    let mut result = Vec::new();
    for stmt in &program.body {
        collect_macro_call_from_stmt(stmt, callee_name, &mut result);
    }
    result
}

fn collect_macro_call_from_stmt<'a>(
    stmt: &'a Statement<'a>,
    callee_name: &str,
    result: &mut Vec<(u32, &'a TSType<'a>)>,
) {
    match stmt {
        Statement::ExpressionStatement(es) => {
            collect_macro_call_from_expr(&es.expression, callee_name, result);
        }
        Statement::VariableDeclaration(decl) => {
            for d in &decl.declarations {
                if let Some(init) = &d.init {
                    collect_macro_call_from_expr(init, callee_name, result);
                }
            }
        }
        _ => {}
    }
}

fn collect_macro_call_from_expr<'a>(
    expr: &'a Expression<'a>,
    callee_name: &str,
    result: &mut Vec<(u32, &'a TSType<'a>)>,
) {
    if let Expression::CallExpression(call) = expr {
        let is_target =
            matches!(&call.callee, Expression::Identifier(id) if id.name == callee_name);
        if is_target {
            if let Some(ref type_args) = call.type_arguments {
                if let Some(first) = type_args.params.first() {
                    result.push((call.span.start, first));
                }
            }
        }
        // Also check for withDefaults(defineProps<T>(), ...) — only relevant for defineProps
        if callee_name == "defineProps" {
            if let Some(first_arg) = call.arguments.first() {
                if let Some(inner_expr) = first_arg.as_expression() {
                    collect_macro_call_from_expr(inner_expr, callee_name, result);
                }
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

            // resolve_interface_decl is strict: require ALL heritage to resolve.
            // This is used for publishing complete type expansions to
            // resolved_local_types, where partial expansions would be misleading.
            // (In contrast, resolve_type_to_prop_fields is tolerant and skips
            // unresolvable heritage to preserve own fields.)
            for heritage in *extends {
                let parent_name = heritage_name(&heritage.expression)?;
                let parent_decl = registry.get(&parent_name)?;
                let parent_fields = resolve_interface_decl(
                    &parent_name,
                    parent_decl,
                    registry,
                    source,
                    comments,
                    visited,
                )?;
                for field in parent_fields {
                    if seen_names.insert(field.name.clone()) {
                        fields.push(field);
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

// ── Generic local type resolution for emit/slot fields ──
// Single resolver shared by emits and slots. Differences are only in the
// member extraction function (what fields to extract from TSSignature members).

/// Trait for extracting a dedup key from a resolved field.
trait NamedField {
    fn field_name(&self) -> &str;
}

impl NamedField for AnalyzedEmitField {
    fn field_name(&self) -> &str {
        &self.name
    }
}

impl NamedField for AnalyzedSlotField {
    fn field_name(&self) -> &str {
        &self.name
    }
}

/// Generic type-to-fields resolver. Shared walker for emit/slot fields.
/// Returns `None` if the type cannot be resolved locally (triggers host fallback).
///
/// Termination behavior: does not emit partial/guessed fields, does not fall back
/// to host resolution. Leaves the branch empty for unresolvable types.
#[allow(clippy::type_complexity)]
fn resolve_type_to_fields<T: NamedField + Clone>(
    ts_type: &TSType<'_>,
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
    visited: &mut FxHashSet<String>,
    extract_from_members: &dyn Fn(&[TSSignature<'_>], &str, &[Comment]) -> Vec<T>,
) -> Option<Vec<T>> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => {
            Some(extract_from_members(&literal.members, source, comments))
        }
        TSType::TSTypeReference(ref_type) => {
            let name = type_name_to_string(&ref_type.type_name);
            if visited.contains(&name) {
                return Some(Vec::new());
            }
            // Utility types are unresolvable locally — stop without inventing fields
            if ref_type.type_arguments.is_some() {
                match name.as_str() {
                    "Partial" | "Required" | "Pick" | "Omit" | "ReturnType" | "InstanceType"
                    | "Record" | "Extract" | "Exclude" | "NonNullable" => {
                        return None;
                    }
                    _ => {}
                }
            }
            visited.insert(name.clone());
            let result = match registry.get(&name) {
                Some(LocalTypeDecl::Interface { body, extends }) => {
                    let mut all_fields = Vec::new();
                    let mut seen_names = FxHashSet::default();
                    let own_fields = extract_from_members(&body.body, source, comments);
                    for field in own_fields {
                        if seen_names.insert(field.field_name().to_string()) {
                            all_fields.push(field);
                        }
                    }
                    for heritage in *extends {
                        let Some(parent_name) = heritage_name(&heritage.expression) else {
                            continue;
                        };
                        let Some(parent_decl) = registry.get(&parent_name) else {
                            continue;
                        };
                        let Some(parent_fields) = resolve_interface_decl_generic(
                            &parent_name,
                            parent_decl,
                            registry,
                            source,
                            comments,
                            visited,
                            extract_from_members,
                        ) else {
                            continue;
                        };
                        for field in parent_fields {
                            if seen_names.insert(field.field_name().to_string()) {
                                all_fields.push(field);
                            }
                        }
                    }
                    Some(all_fields)
                }
                Some(LocalTypeDecl::Alias(aliased_type)) => resolve_type_to_fields(
                    aliased_type,
                    registry,
                    source,
                    comments,
                    visited,
                    extract_from_members,
                ),
                Some(LocalTypeDecl::Class) | None => None,
            };
            visited.remove(&name);
            result
        }
        TSType::TSIntersectionType(intersection) => {
            let mut all_fields = Vec::new();
            let mut seen_names = FxHashSet::default();
            for t in &intersection.types {
                if let Some(fields) = resolve_type_to_fields(
                    t,
                    registry,
                    source,
                    comments,
                    visited,
                    extract_from_members,
                ) {
                    for field in fields {
                        if seen_names.insert(field.field_name().to_string()) {
                            all_fields.push(field);
                        }
                    }
                }
            }
            Some(all_fields)
        }
        _ => None,
    }
}

/// Generic interface declaration resolver. Shared by emit/slot resolution.
#[allow(clippy::type_complexity)]
fn resolve_interface_decl_generic<T: NamedField + Clone>(
    name: &str,
    decl: &LocalTypeDecl<'_>,
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
    visited: &mut FxHashSet<String>,
    extract_from_members: &dyn Fn(&[TSSignature<'_>], &str, &[Comment]) -> Vec<T>,
) -> Option<Vec<T>> {
    if visited.contains(name) {
        return Some(Vec::new());
    }
    visited.insert(name.to_string());
    let result = match decl {
        LocalTypeDecl::Interface { body, extends } => {
            let mut fields = Vec::new();
            let mut seen_names = FxHashSet::default();
            let own_fields = extract_from_members(&body.body, source, comments);
            for field in own_fields {
                if seen_names.insert(field.field_name().to_string()) {
                    fields.push(field);
                }
            }
            for heritage in *extends {
                let Some(parent_name) = heritage_name(&heritage.expression) else {
                    continue;
                };
                let Some(parent_decl) = registry.get(&parent_name) else {
                    continue;
                };
                let Some(parent_fields) = resolve_interface_decl_generic(
                    &parent_name,
                    parent_decl,
                    registry,
                    source,
                    comments,
                    visited,
                    extract_from_members,
                ) else {
                    continue;
                };
                for field in parent_fields {
                    if seen_names.insert(field.field_name().to_string()) {
                        fields.push(field);
                    }
                }
            }
            Some(fields)
        }
        LocalTypeDecl::Alias(aliased_type) => resolve_type_to_fields(
            aliased_type,
            registry,
            source,
            comments,
            visited,
            extract_from_members,
        ),
        LocalTypeDecl::Class => None,
    };
    visited.remove(name);
    result
}

/// Resolve emit fields from a TSType using the shared generic resolver.
fn resolve_type_to_emit_fields(
    ts_type: &TSType<'_>,
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
    visited: &mut FxHashSet<String>,
) -> Option<Vec<AnalyzedEmitField>> {
    resolve_type_to_fields(
        ts_type,
        registry,
        source,
        comments,
        visited,
        &extract_emit_fields_from_members,
    )
}

/// Resolve slot fields from a TSType using the shared generic resolver.
fn resolve_type_to_slot_fields(
    ts_type: &TSType<'_>,
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
    visited: &mut FxHashSet<String>,
) -> Option<Vec<AnalyzedSlotField>> {
    resolve_type_to_fields(
        ts_type,
        registry,
        source,
        comments,
        visited,
        &extract_slot_fields_from_members,
    )
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

/// Build a structured expanded object from resolved prop fields.
fn build_expanded_type_expr(fields: &[AnalyzedPropField]) -> verter_type_expr::TypeExpr {
    use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

    let properties = fields
        .iter()
        .map(|field| {
            let ty = field
                .type_annotation
                .as_deref()
                .map(verter_type_expr_oxc::parse_type_annotation)
                .unwrap_or(TypeExpr::Unknown {
                    raw: "unknown".to_string(),
                });
            ObjectMember::Property(ObjectProperty {
                name: field.name.clone(),
                ty,
                optional: field.is_optional,
                readonly: false,
            })
        })
        .collect();

    TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
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
            let (is_type_based, type_references, parsed_type_argument) =
                if let Some(ref type_args) = call.type_arguments {
                    if let Some(first) = type_args.params.first() {
                        // D1.2: capture the parent shell as a TypeExpr
                        // once during shallow analysis. The host-side
                        // closure consumes this field to drive a
                        // dispatch projection of the macro's fields
                        // without re-parsing.
                        let raw_span = first.span();
                        let start = raw_span.start as usize;
                        let end = raw_span.end as usize;
                        let parsed = if start <= end && end <= source.len() {
                            let text = source[start..end].trim();
                            if text.is_empty() {
                                None
                            } else {
                                let lowered = verter_type_expr_oxc::parse_type_annotation(text);
                                if matches!(lowered, verter_type_expr::TypeExpr::Unknown { .. }) {
                                    None
                                } else {
                                    Some(std::sync::Arc::new(lowered))
                                }
                            }
                        } else {
                            None
                        };
                        (true, collect_type_references(first), parsed)
                    } else {
                        (true, Vec::new(), None)
                    }
                } else {
                    (false, Vec::new(), None)
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
                parsed_type_argument,
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
    let is_optional = !define_model_is_required(call);
    vec![AnalyzedPropField {
        name,
        is_optional,
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

fn define_model_is_required(call: &CallExpression<'_>) -> bool {
    let options_obj = call.arguments.iter().find_map(|arg| {
        if let Argument::ObjectExpression(obj) = arg {
            Some(obj)
        } else {
            None
        }
    });

    let Some(obj) = options_obj else {
        return false;
    };

    obj.properties.iter().any(|prop| {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            return false;
        };
        let is_required_key =
            matches!(&p.key, PropertyKey::StaticIdentifier(id) if id.name == "required");
        let is_true = matches!(
            &p.value,
            Expression::BooleanLiteral(lit) if lit.value
        );
        is_required_key && is_true
    })
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
        TSType::TSTypeLiteral(literal) => {
            extract_emit_fields_from_members(&literal.members, source, comments)
        }
        TSType::TSTypeReference(_) => Vec::new(),
        TSType::TSIntersectionType(intersection) => intersection
            .types
            .iter()
            .flat_map(|t| extract_emit_fields_from_type(t, comments, source))
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract emit fields from TSSignature members (shared between TSTypeLiteral and interface bodies).
fn extract_emit_fields_from_members(
    members: &[TSSignature<'_>],
    source: &str,
    comments: &[Comment],
) -> Vec<AnalyzedEmitField> {
    members
        .iter()
        .filter_map(|member| match member {
            // Property signature: `custom: [payload: string]`
            TSSignature::TSPropertySignature(prop) => {
                let key_name = match &prop.key {
                    PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                    PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                    _ => None,
                };
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
                let (description, tags) = extract_jsdoc_for(comments, prop.span().start, source);
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
                let first_param = call_sig.params.items.first()?;
                let type_ann = first_param.type_annotation.as_ref()?;
                if let TSType::TSLiteralType(lit) = &type_ann.type_annotation {
                    if let TSLiteral::StringLiteral(s) = &lit.literal {
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
        .collect()
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
        TSType::TSTypeLiteral(literal) => {
            extract_slot_fields_from_members(&literal.members, source, comments)
        }
        TSType::TSTypeReference(_) => Vec::new(),
        TSType::TSIntersectionType(intersection) => intersection
            .types
            .iter()
            .flat_map(|t| extract_slot_fields_from_type(t, source, comments))
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract slot fields from TSSignature members (shared between TSTypeLiteral and interface bodies).
fn extract_slot_fields_from_members(
    members: &[TSSignature<'_>],
    source: &str,
    comments: &[Comment],
) -> Vec<AnalyzedSlotField> {
    members
        .iter()
        .filter_map(|member| match member {
            TSSignature::TSPropertySignature(prop) => {
                let key_name = match &prop.key {
                    PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                    PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                    _ => None,
                };
                let bindings = prop
                    .type_annotation
                    .as_ref()
                    .map(|ta| extract_slot_bindings_from_fn_type(&ta.type_annotation, source))
                    .unwrap_or_default();
                let return_type = prop
                    .type_annotation
                    .as_ref()
                    .and_then(|ta| extract_slot_return_type_from_fn(&ta.type_annotation, source));
                let (description, tags) = extract_jsdoc_for(comments, prop.span().start, source);
                key_name.map(|name| AnalyzedSlotField {
                    name,
                    is_required: !prop.optional,
                    span: prop.key.span().into(),
                    bindings,
                    return_type,
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
                let return_type = method.return_type.as_ref().and_then(|rt| {
                    let start = rt.type_annotation.span().start as usize;
                    let end = rt.type_annotation.span().end as usize;
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
                let (description, tags) = extract_jsdoc_for(comments, method.span().start, source);
                key_name.map(|name| AnalyzedSlotField {
                    name,
                    is_required: !method.optional,
                    span: method.key.span().into(),
                    bindings,
                    return_type,
                    description,
                    tags,
                })
            }
            _ => None,
        })
        .collect()
}

/// Extract the return type text from a `TSFunctionType` annotation.
///
/// Handles: `(props: { row: MyItem }) => VNode[]` → `"VNode[]"`
fn extract_slot_return_type_from_fn(ts_type: &TSType<'_>, source: &str) -> Option<String> {
    if let TSType::TSFunctionType(fn_type) = ts_type {
        let start = fn_type.return_type.type_annotation.span().start as usize;
        let end = fn_type.return_type.type_annotation.span().end as usize;
        if end <= source.len() {
            let text = source[start..end].trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
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
    let bindings = extract_slot_bindings_from_type_literal(&ta.type_annotation, source);
    if !bindings.is_empty() {
        return bindings;
    }
    extract_slot_bindings_from_source_type(&ta.type_annotation, source)
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

fn extract_slot_bindings_from_source_type(
    ts_type: &TSType<'_>,
    source: &str,
) -> Vec<AnalyzedSlotFieldBinding> {
    let start = ts_type.span().start as usize;
    let end = ts_type.span().end as usize;
    if start >= end || end > source.len() {
        return Vec::new();
    }

    extract_slot_bindings_from_type_text(source[start..end].trim())
}

fn extract_slot_bindings_from_type_text(type_text: &str) -> Vec<AnalyzedSlotFieldBinding> {
    extract_slot_bindings_from_pick_type(type_text).unwrap_or_default()
}

fn extract_slot_bindings_from_pick_type(type_text: &str) -> Option<Vec<AnalyzedSlotFieldBinding>> {
    let text = type_text.trim();
    if !text.starts_with("Pick<") || !text.ends_with('>') {
        return None;
    }

    let args = split_top_level_type_segments(&text["Pick<".len()..text.len() - 1], ',');
    if args.len() != 2 {
        return None;
    }

    let object = args[0].trim();
    let keys = split_top_level_type_segments(args[1].trim(), '|');
    let mut bindings = Vec::new();
    for key in keys {
        let key = key.trim();
        let name = extract_type_string_literal_name(key)?;
        bindings.push(AnalyzedSlotFieldBinding {
            name,
            type_annotation: Some(format!("{object}[{key}]")),
            span: verter_span::Span::default(),
        });
    }

    (!bindings.is_empty()).then_some(bindings)
}

fn split_top_level_type_segments(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut angle_depth = 0i32;
    let mut in_string = false;
    let mut string_delim = '\0';
    let mut escape = false;

    for (index, ch) in text.char_indices() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == string_delim {
                in_string = false;
            }
            continue;
        }

        match ch {
            '\'' | '"' | '`' => {
                in_string = true;
                string_delim = ch;
            }
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            '<' => angle_depth += 1,
            '>' => angle_depth -= 1,
            _ if ch == separator
                && paren_depth == 0
                && bracket_depth == 0
                && brace_depth == 0
                && angle_depth == 0 =>
            {
                parts.push(text[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    let tail = text[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

fn extract_type_string_literal_name(text: &str) -> Option<String> {
    let text = text.trim();
    if text.len() >= 2
        && ((text.starts_with('\'') && text.ends_with('\''))
            || (text.starts_with('"') && text.ends_with('"')))
    {
        return Some(text[1..text.len() - 1].to_string());
    }
    None
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
/// Extract JSDoc description and tags for a given AST node position.
pub(crate) fn extract_jsdoc_for(
    comments: &[Comment],
    target_start: u32,
    source: &str,
) -> (Option<String>, Vec<JsdocTag>) {
    crate::analysis::jsdoc::extract_jsdoc_for_comments(comments, target_start, source)
}

#[cfg(test)]
#[path = "macros_tests.rs"]
mod macros_tests;
