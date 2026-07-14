use oxc_ast::ast::*;
use oxc_ast::Comment;

use oxc_span::GetSpan;
use verter_type_expr::TypeExpr;

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
///
/// This is the analyzer's OWN macro assembly (statement-order walk +
/// local-type-reference resolution + final-index locator stamping) — the one
/// macro-ordinal / field-ordinal addressing engine. The deref-side field
/// replay ([`lower_macro_field_payload_at`]) calls it over the retained
/// snapshot so the mint side and the deref side cannot drift.
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

    // Final normalization: stamp the authored macro-payload locators at each
    // macro's final index. Constructor sites record authored-annotation
    // presence via the scope pairing fields; the content-free POSITION (macro
    // index + field index) is only known once the macro list is final.
    for (macro_index, mac) in macros.iter_mut().enumerate() {
        stamp_macro_payload_locators(mac, u32::try_from(macro_index).unwrap_or(u32::MAX));
    }

    macros
}

/// Lower the authored generic TYPE ARGUMENT of the macro call whose
/// SFC-absolute span is `macro_span`, replaying the analyzer's own
/// statement-position walk over the retained `program` (the span comes from
/// the analyzer's `AnalyzedMacro.span`, so the mint side and the deref side
/// share one address) and lowering `type_arguments.params[0]` through the
/// same `lower_ts_type` producer the analyzer fingerprints. `None` when no
/// macro-shaped call sits at that span or it carries no type argument.
///
/// The walk covers exactly the analyzer's macro positions: a top-level
/// expression statement, a variable declarator initializer, and the INNER
/// call of a `withDefaults(defineProps<T>(), …)` wrapper.
#[must_use]
pub fn lower_macro_type_argument_at_span(
    program: &Program<'_>,
    source: &str,
    macro_span: verter_span::Span,
) -> Option<verter_type_expr::TypeExpr> {
    fn from_call<'a>(
        call: &'a oxc_ast::ast::CallExpression<'a>,
        source: &str,
        macro_span: verter_span::Span,
    ) -> Option<verter_type_expr::TypeExpr> {
        let call_span: verter_span::Span = call.span.into();
        if call_span == macro_span {
            let type_args = call.type_arguments.as_ref()?;
            let first = type_args.params.first()?;
            return Some(verter_type_expr_oxc::lower_ts_type(first, source));
        }
        // `withDefaults(defineProps<T>(), …)` — the INNER macro call is a
        // distinct analyzer macro at the inner call's span.
        for argument in &call.arguments {
            if let Some(Expression::CallExpression(inner)) = argument.as_expression() {
                if let Some(lowered) = from_call(inner, source, macro_span) {
                    return Some(lowered);
                }
            }
        }
        None
    }
    fn from_expression(
        expr: &Expression<'_>,
        source: &str,
        macro_span: verter_span::Span,
    ) -> Option<verter_type_expr::TypeExpr> {
        match expr {
            Expression::CallExpression(call) => from_call(call, source, macro_span),
            _ => None,
        }
    }

    for stmt in &program.body {
        let lowered = match stmt {
            Statement::ExpressionStatement(expr_stmt) => {
                from_expression(&expr_stmt.expression, source, macro_span)
            }
            Statement::VariableDeclaration(var_decl) => {
                var_decl.declarations.iter().find_map(|decl| {
                    decl.init
                        .as_ref()
                        .and_then(|init| from_expression(init, source, macro_span))
                })
            }
            _ => None,
        };
        if lowered.is_some() {
            return lowered;
        }
    }
    None
}

/// Outcome of [`lower_macro_field_payload_at`] — the deref-side re-derivation
/// of one authored per-field macro payload. Every non-body arm is a typed
/// absence, never a fabricated body.
#[derive(Debug, Clone)]
pub enum MacroFieldPayloadLowering {
    /// The addressed field carries an authored payload position, lowered to
    /// owned typed IR (a prop's type annotation, an emit's payload type / the
    /// call-signature payload tuple, a slot's return type, a `defineModel`
    /// type argument).
    Payload(TypeExpr),
    /// The addressed field exists but authors NO payload at its position —
    /// there is no authored TYPE body there.
    Unauthored,
    /// `(macro_index, field_index)` addresses no analyzer field at all (an
    /// out-of-range / drifted ordinal, a macro kind without a field
    /// vocabulary, or a field whose authored node no longer sits at the
    /// recorded position).
    NoField,
}

/// The lowered authored PER-FIELD payload of the macro at ordinal
/// `macro_index`, field ordinal `field_index` — the deref-side re-derivation
/// of the position a [`MacroPayloadPosition::Field`] payload locator
/// addresses.
///
/// Replays the analyzer's OWN macro assembly ([`analyze_macros_from_program`]
/// — the one macro-ordinal / field-ordinal addressing engine, so the mint
/// side and the deref side cannot drift), selects the field family by the
/// macro's kind (props for `defineProps` / `defineModel`, emits, slots), and
/// lowers the field's authored node through the same `lower_ts_type`
/// producer the analyzer fingerprints. The authored node is re-located by
/// the field's recorded byte span — the exact span the assembly just
/// recorded from THIS program, so the match is byte-precise:
///
/// - a PROP field's annotation lives on a `TSPropertySignature` (the macro
///   type-argument literal, an intersection arm, or a local interface /
///   alias body the analyzer resolved through its local registry) or on the
///   runtime object argument's authored `as PropType<T>` / function-type
///   assertion;
/// - a `defineModel` field's span IS the macro type-argument span;
/// - an EMIT property-signature field's payload is the member value type; a
///   call-signature field's payload is the tuple synthesised from the
///   parameters after the event name (the same shape the analyzer displays);
/// - a SLOT field's payload is the slot function's RETURN type (property or
///   method signature form).
///
/// [`MacroPayloadPosition::Field`]: verter_type_expr::locators::MacroPayloadPosition::Field
#[must_use]
pub fn lower_macro_field_payload_at(
    program: &Program<'_>,
    source: &str,
    macro_index: u32,
    field_index: u32,
) -> MacroFieldPayloadLowering {
    let macros = analyze_macros_from_program(program, source);
    let Some(mac) = macros.get(macro_index as usize) else {
        return MacroFieldPayloadLowering::NoField;
    };
    enum FieldTarget {
        Prop { span: verter_span::Span },
        ModelTypeArgument { macro_span: verter_span::Span },
        Emit { span: verter_span::Span },
        SlotReturn { span: verter_span::Span },
    }
    let target = match mac.kind {
        AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
            let Some(field) = mac.prop_fields.get(field_index as usize) else {
                return MacroFieldPayloadLowering::NoField;
            };
            if field.type_expr_scope.is_none() {
                return MacroFieldPayloadLowering::Unauthored;
            }
            FieldTarget::Prop { span: field.span }
        }
        AnalyzedMacroKind::DefineModel => {
            let Some(field) = mac.prop_fields.get(field_index as usize) else {
                return MacroFieldPayloadLowering::NoField;
            };
            if field.type_expr_scope.is_none() {
                return MacroFieldPayloadLowering::Unauthored;
            }
            FieldTarget::ModelTypeArgument {
                macro_span: mac.span,
            }
        }
        AnalyzedMacroKind::DefineEmits => {
            let Some(field) = mac.emit_fields.get(field_index as usize) else {
                return MacroFieldPayloadLowering::NoField;
            };
            if field.payload_expr_scope.is_none() {
                return MacroFieldPayloadLowering::Unauthored;
            }
            FieldTarget::Emit { span: field.span }
        }
        AnalyzedMacroKind::DefineSlots => {
            let Some(field) = mac.slot_fields.get(field_index as usize) else {
                return MacroFieldPayloadLowering::NoField;
            };
            if field.return_expr_scope.is_none() {
                return MacroFieldPayloadLowering::Unauthored;
            }
            FieldTarget::SlotReturn { span: field.span }
        }
        // `defineExpose` / `defineOptions` fields never stamp a payload
        // position (their inventories are runtime object forms).
        AnalyzedMacroKind::DefineExpose | AnalyzedMacroKind::DefineOptions => {
            return MacroFieldPayloadLowering::NoField;
        }
    };
    match target {
        FieldTarget::ModelTypeArgument { macro_span } => {
            match lower_macro_type_argument_at_span(program, source, macro_span) {
                Some(expr) => MacroFieldPayloadLowering::Payload(expr),
                None => MacroFieldPayloadLowering::NoField,
            }
        }
        FieldTarget::Prop { span } => lower_prop_field_payload_at_span(program, source, mac, span),
        FieldTarget::Emit { span } => lower_emit_field_payload_at_span(program, source, mac, span),
        FieldTarget::SlotReturn { span } => {
            lower_slot_return_payload_at_span(program, source, mac, span)
        }
    }
}

/// Collect every `TSSignature` member list an analyzer field span can live
/// in: the macro call's type-argument literal (plus intersection arms) and
/// every local registry declaration body (interface bodies and alias object
/// literals — the bodies `resolve_macro_type_references` draws fields from).
fn field_payload_member_lists<'a>(
    program: &'a Program<'a>,
    mac: &AnalyzedMacro,
) -> Vec<&'a [TSSignature<'a>]> {
    fn collect_from_type<'a>(ts_type: &'a TSType<'a>, out: &mut Vec<&'a [TSSignature<'a>]>) {
        match ts_type {
            TSType::TSTypeLiteral(literal) => out.push(&literal.members),
            TSType::TSIntersectionType(intersection) => {
                for arm in &intersection.types {
                    collect_from_type(arm, out);
                }
            }
            _ => {}
        }
    }
    let mut lists: Vec<&'a [TSSignature<'a>]> = Vec::new();
    if let Some(call) = find_macro_call_at_span(program, mac.span) {
        if let Some(type_args) = call.type_arguments.as_ref() {
            if let Some(first) = type_args.params.first() {
                collect_from_type(first, &mut lists);
            }
        }
    }
    for decl in build_local_type_registry(program).into_values() {
        match decl {
            LocalTypeDecl::Interface { body, .. } => lists.push(&body.body),
            LocalTypeDecl::Alias(ts_type) => collect_from_type(ts_type, &mut lists),
            LocalTypeDecl::Class => {}
        }
    }
    lists
}

/// Find the macro CALL node at `macro_span`, replaying the analyzer's own
/// statement-position walk (top-level expression statement, variable
/// declarator initializer, and the INNER call of a
/// `withDefaults(defineProps<T>(), …)` wrapper).
fn find_macro_call_at_span<'a>(
    program: &'a Program<'a>,
    macro_span: verter_span::Span,
) -> Option<&'a CallExpression<'a>> {
    fn from_call<'a>(
        call: &'a CallExpression<'a>,
        macro_span: verter_span::Span,
    ) -> Option<&'a CallExpression<'a>> {
        let call_span: verter_span::Span = call.span.into();
        if call_span == macro_span {
            return Some(call);
        }
        for argument in &call.arguments {
            if let Some(Expression::CallExpression(inner)) = argument.as_expression() {
                if let Some(found) = from_call(inner, macro_span) {
                    return Some(found);
                }
            }
        }
        None
    }
    fn from_expression<'a>(
        expr: &'a Expression<'a>,
        macro_span: verter_span::Span,
    ) -> Option<&'a CallExpression<'a>> {
        match expr {
            Expression::CallExpression(call) => from_call(call, macro_span),
            _ => None,
        }
    }
    for stmt in &program.body {
        let found = match stmt {
            Statement::ExpressionStatement(expr_stmt) => {
                from_expression(&expr_stmt.expression, macro_span)
            }
            Statement::VariableDeclaration(var_decl) => {
                var_decl.declarations.iter().find_map(|decl| {
                    decl.init
                        .as_ref()
                        .and_then(|init| from_expression(init, macro_span))
                })
            }
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Lower a PROP field's authored payload node at its recorded key span: a
/// `TSPropertySignature` annotation (type-argument literal / local decl
/// body) or the runtime object argument's authored `as PropType<T>` /
/// function-type assertion.
fn lower_prop_field_payload_at_span(
    program: &Program<'_>,
    source: &str,
    mac: &AnalyzedMacro,
    span: verter_span::Span,
) -> MacroFieldPayloadLowering {
    for members in field_payload_member_lists(program, mac) {
        for member in members {
            if let TSSignature::TSPropertySignature(prop) = member {
                let key_span: verter_span::Span = prop.key.span().into();
                if key_span == span {
                    return match prop.type_annotation.as_ref() {
                        Some(ta) => MacroFieldPayloadLowering::Payload(
                            verter_type_expr_oxc::lower_ts_type(&ta.type_annotation, source),
                        ),
                        None => MacroFieldPayloadLowering::Unauthored,
                    };
                }
            }
        }
    }
    // Runtime object argument: `name: { type: X as PropType<T> }` — the
    // authored assertion node the mint's scope pairing recorded.
    if let Some(call) = find_macro_call_at_span(program, mac.span) {
        if let Some(first_arg) = call.arguments.first() {
            if let Some(Expression::ObjectExpression(obj)) = first_arg.as_expression() {
                for prop in &obj.properties {
                    let ObjectPropertyKind::ObjectProperty(p) = prop else {
                        continue;
                    };
                    let key_span: verter_span::Span = p.key.span().into();
                    if key_span != span {
                        continue;
                    }
                    return lower_runtime_prop_assertion(&p.value, source);
                }
            }
        }
    }
    MacroFieldPayloadLowering::NoField
}

/// Lower the authored type of one runtime prop VALUE (the expanded object
/// form's `type:` sub-property carrying an `as PropType<T>` / `as () => T` /
/// `as new () => T` assertion — the exact shapes the mint stamps a payload
/// position for).
fn lower_runtime_prop_assertion(value: &Expression<'_>, source: &str) -> MacroFieldPayloadLowering {
    let Expression::ObjectExpression(val_obj) = value else {
        return MacroFieldPayloadLowering::Unauthored;
    };
    for sub_prop in &val_obj.properties {
        let ObjectPropertyKind::ObjectProperty(sp) = sub_prop else {
            continue;
        };
        let PropertyKey::StaticIdentifier(id) = &sp.key else {
            continue;
        };
        if id.name != "type" {
            continue;
        }
        let Expression::TSAsExpression(ts_as) = &sp.value else {
            continue;
        };
        if !has_authored_prop_type_assertion(ts_as) {
            continue;
        }
        let node = match &ts_as.type_annotation {
            TSType::TSTypeReference(type_ref) => type_ref
                .type_arguments
                .as_ref()
                .and_then(|args| args.params.first()),
            TSType::TSFunctionType(fn_type) => Some(&fn_type.return_type.type_annotation),
            TSType::TSConstructorType(ctor) => Some(&ctor.return_type.type_annotation),
            _ => None,
        };
        return match node {
            Some(node) => MacroFieldPayloadLowering::Payload(verter_type_expr_oxc::lower_ts_type(
                node, source,
            )),
            None => MacroFieldPayloadLowering::Unauthored,
        };
    }
    MacroFieldPayloadLowering::Unauthored
}

/// Lower an EMIT field's authored payload at its recorded span: a
/// property-signature member VALUE (`change: [id: number]` — span = the key)
/// or the call-signature payload TUPLE (`(e: 'change', id: number): void` —
/// span = the event-name string literal; the tuple synthesises from the
/// parameters after the event name, the same shape the analyzer displays).
fn lower_emit_field_payload_at_span(
    program: &Program<'_>,
    source: &str,
    mac: &AnalyzedMacro,
    span: verter_span::Span,
) -> MacroFieldPayloadLowering {
    for members in field_payload_member_lists(program, mac) {
        for member in members {
            match member {
                TSSignature::TSPropertySignature(prop) => {
                    let key_span: verter_span::Span = prop.key.span().into();
                    if key_span == span {
                        return match prop.type_annotation.as_ref() {
                            Some(ta) => MacroFieldPayloadLowering::Payload(
                                verter_type_expr_oxc::lower_ts_type(&ta.type_annotation, source),
                            ),
                            None => MacroFieldPayloadLowering::Unauthored,
                        };
                    }
                }
                TSSignature::TSCallSignatureDeclaration(call_sig) => {
                    let Some(first_param) = call_sig.params.items.first() else {
                        continue;
                    };
                    let Some(ta) = first_param.type_annotation.as_ref() else {
                        continue;
                    };
                    let TSType::TSLiteralType(lit) = &ta.type_annotation else {
                        continue;
                    };
                    let TSLiteral::StringLiteral(s) = &lit.literal else {
                        continue;
                    };
                    let name_span: verter_span::Span = s.span.into();
                    if name_span != span {
                        continue;
                    }
                    let elements: Vec<verter_type_expr::TupleElement> = call_sig
                        .params
                        .items
                        .iter()
                        .skip(1)
                        .map(|param| verter_type_expr::TupleElement {
                            label: match &param.pattern {
                                BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
                                _ => None,
                            },
                            ty: param
                                .type_annotation
                                .as_ref()
                                .map(|ta| {
                                    verter_type_expr_oxc::lower_ts_type(&ta.type_annotation, source)
                                })
                                .unwrap_or(TypeExpr::Primitive(
                                    verter_type_expr::PrimitiveName::Any,
                                )),
                            optional: param.optional,
                            rest: false,
                        })
                        .collect();
                    return MacroFieldPayloadLowering::Payload(TypeExpr::Tuple {
                        elements: std::sync::Arc::from(elements),
                        readonly: false,
                    });
                }
                _ => {}
            }
        }
    }
    MacroFieldPayloadLowering::NoField
}

/// Lower a SLOT field's authored RETURN payload at its recorded key span
/// (property-signature function-type return or method-signature return —
/// the exact positions the mint's `return_expr_scope` pairing records).
fn lower_slot_return_payload_at_span(
    program: &Program<'_>,
    source: &str,
    mac: &AnalyzedMacro,
    span: verter_span::Span,
) -> MacroFieldPayloadLowering {
    for members in field_payload_member_lists(program, mac) {
        for member in members {
            match member {
                TSSignature::TSPropertySignature(prop) => {
                    let key_span: verter_span::Span = prop.key.span().into();
                    if key_span == span {
                        let Some(ta) = prop.type_annotation.as_ref() else {
                            return MacroFieldPayloadLowering::Unauthored;
                        };
                        let TSType::TSFunctionType(fn_type) = &ta.type_annotation else {
                            return MacroFieldPayloadLowering::Unauthored;
                        };
                        return MacroFieldPayloadLowering::Payload(
                            verter_type_expr_oxc::lower_ts_type(
                                &fn_type.return_type.type_annotation,
                                source,
                            ),
                        );
                    }
                }
                TSSignature::TSMethodSignature(method) => {
                    let key_span: verter_span::Span = method.key.span().into();
                    if key_span == span {
                        return match method.return_type.as_ref() {
                            Some(rt) => MacroFieldPayloadLowering::Payload(
                                verter_type_expr_oxc::lower_ts_type(&rt.type_annotation, source),
                            ),
                            None => MacroFieldPayloadLowering::Unauthored,
                        };
                    }
                }
                _ => {}
            }
        }
    }
    MacroFieldPayloadLowering::NoField
}

/// Stamp the content-free authored payload locators onto a fully-assembled
/// macro at its FINAL index. A locator addresses the analyzer's replayable
/// field positions (`macros[macro_index].<fields>[field_index]`); the anchor
/// mirrors the analyzer's local-file scope convention (empty producing
/// canonical = the local file; the owning declaration is the component's
/// `default` value symbol). A field whose scope pairing is absent has no
/// authored annotation and keeps `payload: None` — never a fabricated
/// position. Slot BINDING payloads stay `None`: the flat field-position
/// vocabulary cannot address a nested (slot, binding) position honestly, so
/// typed binding demand is host-raised.
pub(crate) fn stamp_macro_payload_locators(mac: &mut AnalyzedMacro, macro_index: u32) {
    use verter_type_expr::locators::{
        AuthoredAnchor, LocatorSymbolSpace, MacroPayloadLocator, MacroPayloadPosition,
    };

    fn local_default_anchor() -> AuthoredAnchor {
        AuthoredAnchor {
            canonical_id: std::sync::Arc::from(""),
            symbol: std::sync::Arc::from("default"),
            space: LocatorSymbolSpace::Value,
        }
    }
    let field_locator = |macro_index: u32, field_index: usize| MacroPayloadLocator {
        anchor: local_default_anchor(),
        macro_index,
        payload: MacroPayloadPosition::Field {
            field_index: u32::try_from(field_index).unwrap_or(u32::MAX),
        },
    };

    if mac.parsed_type_argument_scope.is_some() {
        mac.parsed_type_argument = Some(MacroPayloadLocator {
            anchor: local_default_anchor(),
            macro_index,
            payload: MacroPayloadPosition::TypeArgument,
        });
    }
    for (field_index, field) in mac.prop_fields.iter_mut().enumerate() {
        if field.type_expr_scope.is_some() {
            field.payload = Some(field_locator(macro_index, field_index));
        }
    }
    for (field_index, field) in mac.emit_fields.iter_mut().enumerate() {
        if field.payload_expr_scope.is_some() {
            field.payload = Some(field_locator(macro_index, field_index));
        }
    }
    for (field_index, field) in mac.slot_fields.iter_mut().enumerate() {
        if field.return_expr_scope.is_some() {
            field.payload = Some(field_locator(macro_index, field_index));
        }
    }
    for (field_index, field) in mac.expose_fields.iter_mut().enumerate() {
        if field.type_expr_scope.is_some() {
            field.payload = Some(field_locator(macro_index, field_index));
        }
    }
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

/// Extract prop fields from an interface body. The `declared_in_macro_type_arg`
/// argument is propagated unchanged — callers know the contextual provenance
/// of the body being walked.
fn extract_fields_from_interface_body(
    body: &TSInterfaceBody<'_>,
    source: &str,
    comments: &[Comment],
    declared_in_macro_type_arg: bool,
) -> Vec<AnalyzedPropField> {
    extract_fields_from_interface_body_like(
        &body.body,
        source,
        comments,
        declared_in_macro_type_arg,
    )
}

/// Resolve prop fields from a TSType using the local type registry.
/// Returns `None` if the type cannot be resolved locally (triggers TS fallback).
///
/// `declared_in_macro_type_arg` is the provenance fact for every member
/// extracted from this type's own body. The recursive walk preserves it
/// through utility-type unwraps (`Partial`, `Required`), inline-literal arms
/// of intersections, and inline `TSTypeLiteral` direct expansions. When the
/// walker enters a `TSTypeReference` to a local interface, its heritage
/// (`extends`) parents are resolved with `declared_in_macro_type_arg = false`
/// (those names arrive via inheritance), while the referenced interface's own
/// body inherits the caller's provenance (the author wrote the name in their
/// declared shape, even if via a local-alias chain).
fn resolve_type_to_prop_fields(
    ts_type: &TSType<'_>,
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
    visited: &mut FxHashSet<String>,
    declared_in_macro_type_arg: bool,
) -> Option<Vec<AnalyzedPropField>> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => Some(extract_fields_from_interface_body_like(
            &literal.members,
            source,
            comments,
            declared_in_macro_type_arg,
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
                                first,
                                registry,
                                source,
                                comments,
                                visited,
                                declared_in_macro_type_arg,
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
                                first,
                                registry,
                                source,
                                comments,
                                visited,
                                declared_in_macro_type_arg,
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
                    // are still returned. Heritage members are NOT declared
                    // in the macro type arg.
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
                            false,
                        ) else {
                            continue;
                        };
                        for field in parent_fields {
                            if seen_names.insert(field.name.clone()) {
                                all_fields.push(field);
                            }
                        }
                    }
                    // Add own fields (child overrides parent). The interface's own
                    // body members preserve the caller's provenance — they are
                    // members the author wrote in the type they referenced from
                    // the macro's T argument.
                    let own_fields = extract_fields_from_interface_body(
                        body,
                        source,
                        comments,
                        declared_in_macro_type_arg,
                    );
                    for field in own_fields {
                        if seen_names.insert(field.name.clone()) {
                            all_fields.push(field);
                        }
                    }
                    Some(all_fields)
                }
                Some(LocalTypeDecl::Alias(aliased_type)) => resolve_type_to_prop_fields(
                    aliased_type,
                    registry,
                    source,
                    comments,
                    visited,
                    declared_in_macro_type_arg,
                ),
                Some(LocalTypeDecl::Class) => None, // Unresolvable
                None => None,                       // Not found locally
            };
            visited.remove(&name);
            result
        }
        TSType::TSIntersectionType(intersection) => {
            // TypeScript intersection semantics: `A & B` publishes the
            // union of A's and B's members. An arm we cannot resolve
            // locally (`None`) contributes nothing but MUST NOT
            // invalidate the contributions of resolvable arms — the
            // same non-fatal-unsupported merge the prepared-surface
            // projector applies on the cross-file path.
            let mut all_fields = Vec::new();
            let mut seen_names = FxHashSet::default();
            let mut any_resolved = false;
            for t in &intersection.types {
                if let Some(fields) = resolve_type_to_prop_fields(
                    t,
                    registry,
                    source,
                    comments,
                    visited,
                    declared_in_macro_type_arg,
                ) {
                    any_resolved = true;
                    for field in fields {
                        if seen_names.insert(field.name.clone()) {
                            all_fields.push(field);
                        }
                    }
                }
            }
            if any_resolved {
                Some(all_fields)
            } else {
                None
            }
        }
        TSType::TSUnionType(_) => None, // Union types aren't prop field sources
        _ => None,
    }
}

/// Extract prop fields from TSSignature members (shared between TSTypeLiteral and interface bodies).
///
/// `declared_in_macro_type_arg` is propagated to every constructed
/// `AnalyzedPropField` — callers know whether the members belong to the SFC
/// author's own `defineProps<T>()` body shape (`true`) or to inherited /
/// heritage / utility-expansion code (`false`).
fn extract_fields_from_interface_body_like(
    members: &[TSSignature<'_>],
    source: &str,
    comments: &[Comment],
    declared_in_macro_type_arg: bool,
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
                // Lower the OXC `TSType<'_>` AST node directly. Source slicing is
                // display-only.
                let (type_annotation, has_authored_annotation) = match prop.type_annotation.as_ref()
                {
                    Some(ta) => {
                        let start = ta.type_annotation.span().start as usize;
                        let end = ta.type_annotation.span().end as usize;
                        let display = if end <= source.len() {
                            let text = source[start..end].trim();
                            (!text.is_empty()).then(|| text.to_string())
                        } else {
                            None
                        };
                        (display, true)
                    }
                    None => (None, false),
                };
                // The scope pairing records authored-annotation PRESENCE (the
                // local-file empty scope); the payload position is stamped at
                // the macro's final index by `stamp_macro_payload_locators`.
                let type_expr_scope =
                    has_authored_annotation.then(|| verter_type_expr::TypeExprScope::new(""));
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
                    payload: None,
                    type_expr_scope,
                    declared_in_macro_type_arg,
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
        if let Some(fields) = resolve_type_to_prop_fields(
            type_param.1,
            registry,
            source,
            comments,
            &mut visited,
            true,
        ) {
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
                        true,
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
                            shape: local_type_ref_shape(type_ref),
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
                true,
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
                if let Some(fields) = resolve_interface_decl(
                    type_ref,
                    decl,
                    registry,
                    source,
                    comments,
                    &mut visited,
                    true,
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
                        shape: local_type_ref_shape(type_ref),
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
                        true,
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
    declared_in_macro_type_arg: bool,
) -> Option<Vec<AnalyzedPropField>> {
    match decl {
        LocalTypeDecl::Interface { body, .. } => Some(extract_fields_from_interface_body(
            body,
            source,
            comments,
            declared_in_macro_type_arg,
        )),
        LocalTypeDecl::Alias(aliased_type) => resolve_type_to_local_own_prop_fields(
            aliased_type,
            registry,
            source,
            comments,
            visited,
            declared_in_macro_type_arg,
        ),
        LocalTypeDecl::Class => None,
    }
}

fn resolve_type_to_local_own_prop_fields(
    ts_type: &TSType<'_>,
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
    visited: &mut FxHashSet<String>,
    declared_in_macro_type_arg: bool,
) -> Option<Vec<AnalyzedPropField>> {
    match ts_type {
        TSType::TSTypeLiteral(literal) => Some(extract_fields_from_interface_body_like(
            &literal.members,
            source,
            comments,
            declared_in_macro_type_arg,
        )),
        TSType::TSParenthesizedType(parenthesized) => resolve_type_to_local_own_prop_fields(
            &parenthesized.type_annotation,
            registry,
            source,
            comments,
            visited,
            declared_in_macro_type_arg,
        ),
        TSType::TSTypeReference(type_ref) => {
            let name = type_name_to_string(&type_ref.type_name);
            if name.is_empty() || !visited.insert(name.clone()) {
                return None;
            }
            let result = match registry.get(&name) {
                Some(LocalTypeDecl::Interface { body, .. }) => {
                    Some(extract_fields_from_interface_body(
                        body,
                        source,
                        comments,
                        declared_in_macro_type_arg,
                    ))
                }
                Some(LocalTypeDecl::Alias(aliased_type)) => resolve_type_to_local_own_prop_fields(
                    aliased_type,
                    registry,
                    source,
                    comments,
                    visited,
                    declared_in_macro_type_arg,
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
                if let Some(fields) = resolve_type_to_local_own_prop_fields(
                    ty,
                    registry,
                    source,
                    comments,
                    visited,
                    declared_in_macro_type_arg,
                ) {
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
///
/// `declared_in_macro_type_arg` flows to the interface's own body members.
/// Heritage (`extends`) parents are always resolved with `false` — their
/// members arrived via inheritance, not via the SFC author's declared shape.
fn resolve_interface_decl(
    name: &str,
    decl: &LocalTypeDecl<'_>,
    registry: &FxHashMap<String, LocalTypeDecl<'_>>,
    source: &str,
    comments: &[Comment],
    visited: &mut FxHashSet<String>,
    declared_in_macro_type_arg: bool,
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
                    false,
                )?;
                for field in parent_fields {
                    if seen_names.insert(field.name.clone()) {
                        fields.push(field);
                    }
                }
            }

            let own_fields = extract_fields_from_interface_body(
                body,
                source,
                comments,
                declared_in_macro_type_arg,
            );
            for field in own_fields {
                if seen_names.insert(field.name.clone()) {
                    fields.push(field);
                }
            }
            Some(fields)
        }
        LocalTypeDecl::Alias(aliased_type) => resolve_type_to_prop_fields(
            aliased_type,
            registry,
            source,
            comments,
            visited,
            declared_in_macro_type_arg,
        ),
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

/// The synthesized closed shape of a locally-resolved type reference: an
/// object expansion is never a primitive, so it stays a shallow named
/// reference resolved on demand (the analyzer's local-file scope convention is
/// the empty producing canonical).
fn local_type_ref_shape(type_ref: &str) -> verter_type_expr::facts::ResolvedLocalShape {
    verter_type_expr::facts::ResolvedLocalShape::Ref(
        verter_type_expr::locators::SymbolBodyLocator {
            anchor: verter_type_expr::locators::AuthoredAnchor {
                canonical_id: std::sync::Arc::from(""),
                symbol: std::sync::Arc::from(type_ref),
                space: verter_type_expr::locators::LocatorSymbolSpace::Type,
            },
        },
    )
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
            let (is_type_based, type_references, has_parsed_type_argument) =
                if let Some(ref type_args) = call.type_arguments {
                    if let Some(first) = type_args.params.first() {
                        // Capture the parent shell's authored POSITION once
                        // during shallow analysis: the scope pairing records
                        // presence; the payload locator is stamped at the
                        // macro's final index. The host-side closure demands
                        // the typed body through the shared dispatch.
                        (true, collect_type_references(first), true)
                    } else {
                        (true, Vec::new(), false)
                    }
                } else {
                    (false, Vec::new(), false)
                };
            let parsed_type_argument = None;
            let parsed_type_argument_scope =
                has_parsed_type_argument.then(|| verter_type_expr::TypeExprScope::new(""));

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
            } else if kind == AnalyzedMacroKind::DefineModel {
                extract_define_model_default_values(call, &model_name, source)
            } else {
                Vec::new()
            };

            let expose_fields = if kind == AnalyzedMacroKind::DefineExpose {
                extract_expose_fields(call, comments, source)
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
                parsed_type_argument_scope,
                span: call.span.into(),
            })
        }
        _ => None,
    }
}

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
    // Authored `defineModel<T>()` annotation: the scope pairing records
    // presence; the payload position is stamped at the macro's final index.
    let type_expr_scope = Some(verter_type_expr::TypeExprScope::new(""));
    vec![AnalyzedPropField {
        name,
        is_optional,
        span: first.span().into(),
        type_annotation: Some(type_text.to_string()),
        description: None,
        tags: Vec::new(),
        resolution_source: TypeResolutionSource::Rust,
        resolution_error: None,
        payload: None,
        type_expr_scope,
        // `defineModel<T>()` declares the model prop name explicitly at the
        // macro site.
        declared_in_macro_type_arg: true,
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

/// Extract the authored `default` value from a `defineModel()` options object.
///
/// Handles:
/// - `defineModel<T>({ default: ... })` — options as first arg
/// - `defineModel<T>('name', { default: ... })` — options as second arg
///
/// Mirrors [`extract_define_model_default_keys`]: the value entry comes from
/// the same `default` property that marks the model as defaulted, keyed by
/// the model name, so a present default key always pairs with a present
/// default value entry. The value carries the verbatim source text of the
/// default expression, exactly like `withDefaults()` defaults.
fn extract_define_model_default_values(
    call: &CallExpression<'_>,
    model_name: &Option<String>,
    source: &str,
) -> Vec<AnalyzedDefaultValue> {
    let name = model_name.as_deref().unwrap_or("modelValue");

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

    obj.properties
        .iter()
        .filter_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(p) = prop else {
                return None;
            };
            if !matches!(&p.key, PropertyKey::StaticIdentifier(id) if id.name == "default") {
                return None;
            }
            let value = default_value_source_text(&p.value, source).unwrap_or_default();
            Some(AnalyzedDefaultValue {
                key: name.to_string(),
                value,
                span: p.value.span().into(),
            })
        })
        .collect()
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
        // Inline `defineProps<{ count: number; ... }>()` — delegate to the
        // shared interface-body-like extractor so every prop carries the
        // typed `*_expr` form lowered via `lower_ts_type`. Members of an
        // inline literal at the macro type arg are declared by the author.
        TSType::TSTypeLiteral(literal) => {
            extract_fields_from_interface_body_like(&literal.members, source, comments, true)
        }
        TSType::TSTypeReference(_) => {
            // Interface reference — can't resolve inline, leave empty
            Vec::new()
        }
        TSType::TSIntersectionType(intersection) => {
            // Merge fields from all branches. Inline literal arms (`{ ... }`)
            // contribute author-declared members; reference arms return empty
            // here and are resolved later by `resolve_macro_type_references`,
            // where their provenance is preserved based on whether the
            // referenced declaration is local-author-declared or external.
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

/// Whether a runtime prop's `as` assertion carries an authored payload TYPE
/// position. Presence-only — the payload position is stamped at the macro's
/// final index; the typed body is demanded through the shared dispatch on read.
///
/// Rules:
/// - `X as PropType<T>` → the `T` argument position
/// - `X as () => T` / `X as new () => T` → the return-type position
/// - Other assertions → no authored payload (caller falls back to
///   `constructor_to_ts_type`)
fn has_authored_prop_type_assertion(ts_as: &TSAsExpression<'_>) -> bool {
    match &ts_as.type_annotation {
        TSType::TSTypeReference(type_ref) => {
            if let TSTypeName::IdentifierReference(id) = &type_ref.type_name {
                id.name == "PropType"
                    && type_ref
                        .type_arguments
                        .as_ref()
                        .is_some_and(|args| !args.params.is_empty())
            } else {
                false
            }
        }
        TSType::TSFunctionType(_) | TSType::TSConstructorType(_) => true,
        _ => false,
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

                let mut type_annotation: Option<String> = None;
                // Whether an authored `PropType<T>`-style assertion exists (the
                // payload position is stamped at the macro's final index).
                let mut has_authored_prop_type = false;
                // Vue semantics: props are optional by default unless `required: true` is set.
                let mut is_optional = true;

                // Check if value is a constructor (shorthand: `name: String`)
                if let Expression::Identifier(id) = &p.value {
                    if let Some(ts_text) = constructor_to_ts_type(&id.name) {
                        type_annotation = Some(ts_text.to_string());
                    }
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
                                    if has_authored_prop_type_assertion(ts_as) {
                                        // Display: slice the source span of the inner type-arg / return-type
                                        // so the wire payload still carries human-readable text.
                                        let display_span = match &ts_as.type_annotation {
                                            TSType::TSTypeReference(tr) => tr
                                                .type_arguments
                                                .as_ref()
                                                .and_then(|args| args.params.first())
                                                .map(|first| first.span()),
                                            TSType::TSFunctionType(ft) => {
                                                Some(ft.return_type.type_annotation.span())
                                            }
                                            TSType::TSConstructorType(ct) => {
                                                Some(ct.return_type.type_annotation.span())
                                            }
                                            _ => None,
                                        };
                                        type_annotation = display_span.and_then(|sp_| {
                                            let s = sp_.start as usize;
                                            let e = sp_.end as usize;
                                            (e <= source.len())
                                                .then(|| source[s..e].trim().to_string())
                                                .filter(|t| !t.is_empty())
                                        });
                                        has_authored_prop_type = true;
                                    } else if let Expression::Identifier(id) = &ts_as.expression {
                                        if let Some(ts_text) = constructor_to_ts_type(&id.name) {
                                            type_annotation = Some(ts_text.to_string());
                                        }
                                    }
                                } else if let Expression::Identifier(id) = &sp.value {
                                    if let Some(ts_text) = constructor_to_ts_type(&id.name) {
                                        type_annotation = Some(ts_text.to_string());
                                    }
                                }
                            }
                            "required" => {
                                // `required: true` makes the prop required (not optional).
                                if let Expression::BooleanLiteral(b) = &sp.value {
                                    is_optional = !b.value;
                                }
                            }
                            "default" => {
                                let val_text = default_value_source_text(&sp.value, source)
                                    .unwrap_or_default();
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

                let type_expr_scope =
                    has_authored_prop_type.then(|| verter_type_expr::TypeExprScope::new(""));
                fields.push(AnalyzedPropField {
                    name: key_name,
                    is_optional,
                    span: p.key.span().into(),
                    type_annotation,
                    description,
                    tags,
                    resolution_source: TypeResolutionSource::Rust,
                    resolution_error: None,
                    payload: None,
                    type_expr_scope,
                    // Runtime object form — the author wrote this prop name
                    // directly as a key in `defineProps({ ... })`.
                    declared_in_macro_type_arg: true,
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
                            payload: None,
                            type_expr_scope: None,
                            // Runtime array form — the author wrote the name
                            // directly as an array entry in `defineProps([...])`.
                            declared_in_macro_type_arg: true,
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
                let (payload_type, has_authored_payload) = match prop.type_annotation.as_ref() {
                    Some(ta) => {
                        let start = ta.type_annotation.span().start as usize;
                        let end = ta.type_annotation.span().end as usize;
                        let display = if end <= source.len() {
                            let text = source[start..end].trim();
                            (!text.is_empty()).then(|| text.to_string())
                        } else {
                            None
                        };
                        (display, true)
                    }
                    None => (None, false),
                };
                let payload_expr_scope =
                    has_authored_payload.then(|| verter_type_expr::TypeExprScope::new(""));
                let (description, tags) = extract_jsdoc_for(comments, prop.span().start, source);
                key_name.map(|name| AnalyzedEmitField {
                    name,
                    span: prop.key.span().into(),
                    payload_type,
                    description,
                    tags,
                    payload: None,
                    payload_expr_scope,
                })
            }
            // Call signature: `(e: 'change', id: number): void`
            TSSignature::TSCallSignatureDeclaration(call_sig) => {
                let first_param = call_sig.params.items.first()?;
                let type_ann = first_param.type_annotation.as_ref()?;
                if let TSType::TSLiteralType(lit) = &type_ann.type_annotation {
                    if let TSLiteral::StringLiteral(s) = &lit.literal {
                        // Display: `[id: number]` formed from the source slices of the
                        // remaining params.
                        let extra_params_text: Vec<String> = call_sig
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
                        let payload_type = Some(format!("[{}]", extra_params_text.join(", ")));
                        // The call-signature payload tuple is an authored
                        // structure at this emit position: the scope pairing
                        // records presence, the payload locator is stamped at
                        // the macro's final index, and the tuple materializes
                        // through the shared dispatch on demand.
                        let payload_expr_scope = Some(verter_type_expr::TypeExprScope::new(""));
                        let (description, tags) =
                            extract_jsdoc_for(comments, call_sig.span().start, source);
                        return Some(AnalyzedEmitField {
                            name: s.value.to_string(),
                            span: s.span.into(),
                            payload_type,
                            description,
                            tags,
                            payload: None,
                            payload_expr_scope,
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
                        payload: None,
                        payload_expr_scope: None,
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
                        payload: None,
                        payload_expr_scope: None,
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
                let value = default_value_source_text(&p.value, source).unwrap_or_default();
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

/// Extract the verbatim source text of a default-value expression.
///
/// Every expression kind — including string literals — yields the exact
/// source slice (`default: 'vertical'` publishes `'vertical'`, quotes
/// included), so display layers never re-infer quoting from the surrounding
/// type. Shared by the script-setup macro analyzer and the Options-API
/// analyzer; returns `None` when the span is out of range.
pub(crate) fn default_value_source_text(expr: &Expression<'_>, source: &str) -> Option<String> {
    let span = expr.span();
    source
        .get(span.start as usize..span.end as usize)
        .map(str::to_string)
}

/// Extract exposed field names and their leading JSDoc from
/// `defineExpose({ foo, bar })`.
///
/// Only parses object literal arguments. Identifier args (e.g., `defineExpose(myObj)`)
/// return empty since we can't resolve the value statically. Each field's
/// leading `/** ... */` block is captured at the field key span, exactly like
/// runtime prop fields.
fn extract_expose_fields(
    call: &CallExpression<'_>,
    comments: &[Comment],
    source: &str,
) -> Vec<AnalyzedExposeField> {
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
                let (description, tags) = extract_jsdoc_for(comments, p.key.span().start, source);
                key_name.map(|name| AnalyzedExposeField {
                    name,
                    span: Some(p.key.span().into()),
                    payload: None,
                    type_expr_scope: None,
                    description,
                    tags,
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
                let (return_type, has_authored_return) = prop
                    .type_annotation
                    .as_ref()
                    .map(|ta| extract_slot_return_from_fn(&ta.type_annotation, source))
                    .unwrap_or((None, false));
                let return_expr_scope =
                    has_authored_return.then(|| verter_type_expr::TypeExprScope::new(""));
                let (description, tags) = extract_jsdoc_for(comments, prop.span().start, source);
                key_name.map(|name| AnalyzedSlotField {
                    name,
                    is_required: !prop.optional,
                    span: prop.key.span().into(),
                    bindings,
                    return_type,
                    description,
                    tags,
                    payload: None,
                    return_expr_scope,
                })
            }
            TSSignature::TSMethodSignature(method) => {
                let key_name = match &method.key {
                    PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                    PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
                    _ => None,
                };
                let bindings = extract_slot_bindings_from_params(&method.params, source);
                let (return_type, has_authored_return) = match method.return_type.as_ref() {
                    Some(rt) => {
                        let start = rt.type_annotation.span().start as usize;
                        let end = rt.type_annotation.span().end as usize;
                        let display = if end <= source.len() {
                            let text = source[start..end].trim();
                            (!text.is_empty()).then(|| text.to_string())
                        } else {
                            None
                        };
                        (display, true)
                    }
                    None => (None, false),
                };
                let return_expr_scope =
                    has_authored_return.then(|| verter_type_expr::TypeExprScope::new(""));
                let (description, tags) = extract_jsdoc_for(comments, method.span().start, source);
                key_name.map(|name| AnalyzedSlotField {
                    name,
                    is_required: !method.optional,
                    span: method.key.span().into(),
                    bindings,
                    return_type,
                    description,
                    tags,
                    payload: None,
                    return_expr_scope,
                })
            }
            _ => None,
        })
        .collect()
}

/// Extract the display text of a `TSFunctionType`'s return type plus whether
/// an authored return position exists. Returns `(None, false)` for
/// non-function-type inputs. The return payload position is stamped at the
/// macro's final index; the typed body is demanded through the shared
/// dispatch on read.
///
/// Handles: `(props: { row: MyItem }) => VNode[]` → (`"VNode[]"`, true).
fn extract_slot_return_from_fn(ts_type: &TSType<'_>, source: &str) -> (Option<String>, bool) {
    if let TSType::TSFunctionType(fn_type) = ts_type {
        let start = fn_type.return_type.type_annotation.span().start as usize;
        let end = fn_type.return_type.type_annotation.span().end as usize;
        let display = if end <= source.len() {
            let text = source[start..end].trim();
            (!text.is_empty()).then(|| text.to_string())
        } else {
            None
        };
        return (display, true);
    }
    (None, false)
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
    // Fall back to recovering bindings from a `Pick<Object, Keys>` AST shape.
    extract_slot_bindings_from_pick_ast(&ta.type_annotation, source)
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
                let type_annotation = match prop.type_annotation.as_ref() {
                    Some(ta) => {
                        let start = ta.type_annotation.span().start as usize;
                        let end = ta.type_annotation.span().end as usize;
                        if end <= source.len() {
                            let text = source[start..end].trim();
                            (!text.is_empty()).then(|| text.to_string())
                        } else {
                            None
                        }
                    }
                    None => None,
                };
                // A nested (slot, binding) position is not addressable by the
                // flat field-position vocabulary — the typed binding channel
                // is host-raised; the binding carries display text only.
                key_name.map(|name| AnalyzedSlotFieldBinding {
                    name,
                    type_annotation,
                    span: prop.key.span().into(),
                    payload: None,
                    binding_expr_scope: None,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Recover slot bindings from an AST `Pick<Object, Keys>` type reference.
///
/// Walks the OXC `TSType` directly for the key inventory. A nested
/// (slot, binding) position is not addressable by the flat field-position
/// payload vocabulary, so each emitted binding carries DISPLAY TEXT only
/// (`type_annotation = "Object[Key]"`, sliced from the authored spans) with
/// `payload: None` — no stored typed shape; the typed binding channel is
/// host-raised on demand. For each key in the keys union (or a single key
/// reference):
/// - String-literal keys (`"name"`) emit one binding named after the literal.
/// - Userland alias keys (`type BindingKey = "name" | "value"`) emit ONE
///   binding named after the alias. Alias resolution is NOT analyzer scope —
///   enumerating the literal-union members happens downstream through the
///   host-raised typed binding channel.
///
/// Other shapes (non-Pick references, missing arguments, non-literal/non-ref keys)
/// return an empty vec.
fn extract_slot_bindings_from_pick_ast(
    ts_type: &TSType<'_>,
    source: &str,
) -> Vec<AnalyzedSlotFieldBinding> {
    let TSType::TSTypeReference(type_ref) = ts_type else {
        return Vec::new();
    };
    // Match `Pick<...>` by AST shape.
    let is_pick = matches!(
        &type_ref.type_name,
        TSTypeName::IdentifierReference(id) if id.name == "Pick"
    );
    if !is_pick {
        return Vec::new();
    }
    let Some(type_args) = type_ref.type_arguments.as_ref() else {
        return Vec::new();
    };
    if type_args.params.len() != 2 {
        return Vec::new();
    }
    let object_ts = &type_args.params[0];
    let keys_ts = &type_args.params[1];

    // Collect each key as either a literal-string key, or a userland alias Ref.
    let mut bindings = Vec::new();

    let push_for_key = |key_ts: &TSType<'_>, bindings: &mut Vec<AnalyzedSlotFieldBinding>| {
        match key_ts {
            // Literal string-key: `"name"`
            TSType::TSLiteralType(lit) => {
                if let TSLiteral::StringLiteral(s) = &lit.literal {
                    let key_name = s.value.to_string();
                    let key_text = {
                        let span = lit.span();
                        let st = span.start as usize;
                        let en = span.end as usize;
                        if en <= source.len() {
                            source[st..en].trim().to_string()
                        } else {
                            format!("\"{key_name}\"")
                        }
                    };
                    let object_text = {
                        let span = object_ts.span();
                        let st = span.start as usize;
                        let en = span.end as usize;
                        if en <= source.len() {
                            source[st..en].trim().to_string()
                        } else {
                            String::new()
                        }
                    };
                    let display =
                        (!object_text.is_empty()).then(|| format!("{object_text}[{key_text}]"));
                    // A nested (slot, binding) position is not addressable by
                    // the flat field-position vocabulary — the typed binding
                    // channel is host-raised; display text only.
                    bindings.push(AnalyzedSlotFieldBinding {
                        name: key_name,
                        type_annotation: display,
                        span: verter_span::Span::default(),
                        payload: None,
                        binding_expr_scope: None,
                    });
                }
            }
            // Userland alias: `type BindingKey = "name" | "value"` referenced
            // by name. The analyzer emits one display-text binding named after
            // the alias; resolving the alias to its literal-union body happens
            // downstream through the host-raised typed binding channel.
            TSType::TSTypeReference(key_ref) => {
                let alias_name = match &key_ref.type_name {
                    TSTypeName::IdentifierReference(id) => Some(id.name.to_string()),
                    _ => None,
                };
                if let Some(alias_name) = alias_name {
                    let key_text = {
                        let span = key_ts.span();
                        let st = span.start as usize;
                        let en = span.end as usize;
                        if en <= source.len() {
                            source[st..en].trim().to_string()
                        } else {
                            alias_name.clone()
                        }
                    };
                    let object_text = {
                        let span = object_ts.span();
                        let st = span.start as usize;
                        let en = span.end as usize;
                        if en <= source.len() {
                            source[st..en].trim().to_string()
                        } else {
                            String::new()
                        }
                    };
                    let display =
                        (!object_text.is_empty()).then(|| format!("{object_text}[{key_text}]"));
                    // A nested (slot, binding) position is not addressable by
                    // the flat field-position vocabulary — the typed binding
                    // channel is host-raised; display text only.
                    bindings.push(AnalyzedSlotFieldBinding {
                        // Bare alias reference: at analyzer scope we cannot
                        // enumerate the underlying literal-union members, and
                        // no typed shape is stored — a downstream consumer
                        // demands the typed surface through the host-raised
                        // binding channel and emits a per-binding entry for
                        // each resolved literal. The alias name stands in as
                        // the binding identifier; the consumer overrides it
                        // downstream.
                        name: alias_name,
                        type_annotation: display,
                        span: verter_span::Span::default(),
                        payload: None,
                        binding_expr_scope: None,
                    });
                }
            }
            _ => {}
        }
    };

    match keys_ts {
        // `Pick<X, "a" | "b">` — union of literal keys.
        TSType::TSUnionType(union) => {
            for arm in &union.types {
                push_for_key(arm, &mut bindings);
            }
        }
        // `Pick<X, "a">` or `Pick<X, BindingKey>` — single literal/ref key.
        single => push_for_key(single, &mut bindings),
    }

    bindings
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
