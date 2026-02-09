//! Type resolution for Vue macro type parameters.
//!
//! This module resolves TypeScript type annotations from Vue macros like
//! `defineProps<{ title: string; count: number }>()` into structured type
//! information that can be used for code generation.
//!
//! Based on Vue's `resolveType.ts` implementation.

#![allow(dead_code)]

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use crate::common::Span;

/// Find the first TSType from a call expression's type parameters at the given span.
///
/// This walks the program AST to find a CallExpression whose span matches,
/// then extracts the first type parameter.
pub fn find_macro_type_param<'a>(
    program: &'a Program<'a>,
    macro_span: Span,
) -> Option<&'a TSType<'a>> {
    for stmt in &program.body {
        if let Some(ts_type) = find_call_type_param_in_statement(stmt, macro_span) {
            return Some(ts_type);
        }
    }
    None
}

fn find_call_type_param_in_statement<'a>(
    stmt: &'a Statement<'a>,
    target: Span,
) -> Option<&'a TSType<'a>> {
    match stmt {
        Statement::ExpressionStatement(expr_stmt) => {
            find_call_type_param_in_expr(&expr_stmt.expression, target)
        }
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                if let Some(init) = &decl.init {
                    if let Some(ts_type) = find_call_type_param_in_expr(init, target) {
                        return Some(ts_type);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn find_call_type_param_in_expr<'a>(
    expr: &'a Expression<'a>,
    target: Span,
) -> Option<&'a TSType<'a>> {
    match expr {
        Expression::CallExpression(call) => {
            // Check if this is our target call
            if call.span.start == target.start && call.span.end == target.end {
                if let Some(type_args) = &call.type_arguments {
                    return type_args.params.first();
                }
            }
            // Check nested calls in arguments
            for arg in &call.arguments {
                if let Argument::SpreadElement(spread) = arg {
                    if let Some(ts_type) = find_call_type_param_in_expr(&spread.argument, target) {
                        return Some(ts_type);
                    }
                } else if let Some(expr) = arg.as_expression() {
                    if let Some(ts_type) = find_call_type_param_in_expr(expr, target) {
                        return Some(ts_type);
                    }
                }
            }
            // Check callee if it's an expression
            find_call_type_param_in_expr(&call.callee, target)
        }
        Expression::ParenthesizedExpression(paren) => {
            find_call_type_param_in_expr(&paren.expression, target)
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                if let Some(ts_type) = find_call_type_param_in_expr(expr, target) {
                    return Some(ts_type);
                }
            }
            None
        }
        Expression::ConditionalExpression(cond) => {
            find_call_type_param_in_expr(&cond.consequent, target)
                .or_else(|| find_call_type_param_in_expr(&cond.alternate, target))
        }
        _ => None,
    }
}

/// Runtime type that can be inferred from TypeScript types.
/// These correspond to JavaScript constructor functions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuntimeType {
    String,
    Number,
    Boolean,
    Object,
    Array,
    Function,
    Symbol,
    /// Built-in types like Date, Map, Set, etc.
    BuiltIn(String),
    /// Null type
    Null,
    /// Type that couldn't be resolved
    Unknown,
}

impl RuntimeType {
    /// Convert to the JavaScript constructor name
    pub fn as_str(&self) -> &str {
        match self {
            RuntimeType::String => "String",
            RuntimeType::Number => "Number",
            RuntimeType::Boolean => "Boolean",
            RuntimeType::Object => "Object",
            RuntimeType::Array => "Array",
            RuntimeType::Function => "Function",
            RuntimeType::Symbol => "Symbol",
            RuntimeType::BuiltIn(name) => name,
            RuntimeType::Null => "null",
            RuntimeType::Unknown => "null",
        }
    }
}

/// Format runtime types as a Vue prop type value.
/// Single type: `String`
/// Multiple types: `[String, Number]`
pub fn format_runtime_types(types: &[RuntimeType]) -> String {
    // Filter out Unknown types
    let valid_types: Vec<_> = types
        .iter()
        .filter(|t| !matches!(t, RuntimeType::Unknown))
        .collect();

    if valid_types.is_empty() {
        return "null".to_string();
    }

    if valid_types.len() == 1 {
        return valid_types[0].as_str().to_string();
    }

    // Multiple types: [Type1, Type2]
    let type_strs: Vec<_> = valid_types.iter().map(|t| t.as_str()).collect();
    format!("[{}]", type_strs.join(", "))
}

/// A resolved property from a type literal.
#[derive(Debug, Clone)]
pub struct ResolvedProp {
    /// Span of the entire property signature (from key to end of type)
    pub span: Span,
    /// Span of the property key (name) in the source
    pub key: Span,
    /// Whether the property is optional (has `?`)
    pub optional: bool,
    /// Inferred runtime types for this property
    pub types: Vec<RuntimeType>,
}

/// A resolved emit event from defineEmits type parameter.
/// Supports both call signature style `{ (e: 'change', id: number): void }`
/// and shorthand style `{ change: [id: number] }`.
#[derive(Debug, Clone)]
pub struct ResolvedEmit {
    /// Span of the entire emit signature
    pub span: Span,
    /// The event name (extracted from first parameter literal or property key)
    pub name: String,
    /// Span of the event name in source (if available, for string literal params)
    pub name_span: Option<Span>,
}

/// Result of resolving type elements from a type annotation.
#[derive(Debug, Default)]
pub struct ResolvedElements {
    /// Resolved properties from the type
    pub props: Vec<ResolvedProp>,
    /// Resolved emit events from call signatures or shorthand properties
    pub emits: Vec<ResolvedEmit>,
    /// Whether this type has call signatures (is callable)
    pub has_call_signature: bool,
}

// =============================================================================
// Type Resolution Context (for resolving type references)
// =============================================================================

/// Location/origin of a diagnostic - identifies which plugin/module created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLocation {
    /// Type resolution (resolve_type.rs)
    TypeResolution,
    /// Props macro processing (defineProps, withDefaults)
    PropsMacro,
    /// Emits macro processing (defineEmits)
    EmitsMacro,
    /// Model macro processing (defineModel)
    ModelMacro,
    /// Options macro processing (defineOptions)
    OptionsMacro,
    /// Expose macro processing (defineExpose)
    ExposeMacro,
    /// Slots macro processing (defineSlots)
    SlotsMacro,
    /// Script setup processing
    ScriptSetup,
    /// Script options API processing
    ScriptOptions,
    /// Template processing
    Template,
    /// Style processing
    Style,
}

impl DiagnosticLocation {
    /// Get a short name for this location
    pub const fn name(&self) -> &'static str {
        match self {
            Self::TypeResolution => "type-resolution",
            Self::PropsMacro => "props-macro",
            Self::EmitsMacro => "emits-macro",
            Self::ModelMacro => "model-macro",
            Self::OptionsMacro => "options-macro",
            Self::ExposeMacro => "expose-macro",
            Self::SlotsMacro => "slots-macro",
            Self::ScriptSetup => "script-setup",
            Self::ScriptOptions => "script-options",
            Self::Template => "template",
            Self::Style => "style",
        }
    }
}

/// Diagnostic kind for resolution errors.
/// Uses enum-based messages to avoid String allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionDiagnosticKind {
    /// Type reference could not be resolved
    UnresolvedTypeReference,
    /// Empty type literal (no props)
    EmptyTypeLiteral,
    /// Intersection resulted in no usable types
    EmptyIntersection,
}

impl ResolutionDiagnosticKind {
    /// Get static message for this diagnostic kind - no allocation
    pub const fn message(&self) -> &'static str {
        match self {
            Self::UnresolvedTypeReference => "Could not resolve type reference",
            Self::EmptyTypeLiteral => "Empty type literal has no properties",
            Self::EmptyIntersection => "Intersection type resolved to no properties",
        }
    }
}

/// Diagnostic with enum-based message (no String allocations)
#[derive(Debug, Clone)]
pub struct ResolutionDiagnostic {
    /// Location in source code
    pub span: Span,
    /// What kind of diagnostic this is
    pub kind: ResolutionDiagnosticKind,
    /// Which plugin/module created this diagnostic
    pub location: DiagnosticLocation,
}

/// Context for type resolution with available type information.
/// Uses Span-based lookups with &[u8] comparisons to avoid String allocations.
#[derive(Debug)]
pub struct TypeResolutionContext<'a> {
    /// Source bytes for name comparisons
    pub source: &'a [u8],
    /// Local type alias declarations: (name_span, type_node)
    pub type_aliases: Vec<(Span, &'a TSType<'a>)>,
    /// Local interface declarations: (name_span, interface_body_members)
    pub interfaces: Vec<(Span, &'a oxc_allocator::Vec<'a, TSSignature<'a>>)>,
    /// Generic type parameters with constraints: (name_span, constraint_type)
    pub type_params: Vec<(Span, Option<&'a TSType<'a>>)>,
    /// Diagnostics collected during resolution
    pub diagnostics: Vec<ResolutionDiagnostic>,
}

impl<'a> TypeResolutionContext<'a> {
    /// Create a new empty context
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            type_aliases: Vec::new(),
            interfaces: Vec::new(),
            type_params: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Look up a type alias by comparing spans against source bytes
    pub fn find_type_alias(&self, name: &[u8]) -> Option<&'a TSType<'a>> {
        self.type_aliases
            .iter()
            .find(|(span, _)| &self.source[span.start as usize..span.end as usize] == name)
            .map(|(_, ty)| *ty)
    }

    /// Look up an interface by comparing spans against source bytes
    pub fn find_interface(
        &self,
        name: &[u8],
    ) -> Option<&'a oxc_allocator::Vec<'a, TSSignature<'a>>> {
        self.interfaces
            .iter()
            .find(|(span, _)| &self.source[span.start as usize..span.end as usize] == name)
            .map(|(_, members)| *members)
    }

    /// Look up a type parameter constraint by comparing spans against source bytes
    pub fn find_type_param(&self, name: &[u8]) -> Option<&'a TSType<'a>> {
        self.type_params
            .iter()
            .find(|(span, _)| &self.source[span.start as usize..span.end as usize] == name)
            .and_then(|(_, constraint)| *constraint)
    }
}

/// Build type resolution context from a parsed program.
/// Collects type aliases and interfaces for later lookup.
pub fn build_type_context<'a>(
    program: &'a Program<'a>,
    source: &'a [u8],
    base_offset: u32,
) -> TypeResolutionContext<'a> {
    let mut ctx = TypeResolutionContext::new(source);

    for stmt in &program.body {
        match stmt {
            // Collect type aliases: `type Foo = { bar: string }`
            Statement::TSTypeAliasDeclaration(alias) => {
                let name_span = Span {
                    start: alias.id.span.start + base_offset,
                    end: alias.id.span.end + base_offset,
                };
                ctx.type_aliases.push((name_span, &alias.type_annotation));
            }
            // Collect interfaces: `interface Foo { bar: string }`
            Statement::TSInterfaceDeclaration(interface) => {
                let name_span = Span {
                    start: interface.id.span.start + base_offset,
                    end: interface.id.span.end + base_offset,
                };
                ctx.interfaces.push((name_span, &interface.body.body));
            }
            _ => {}
        }
    }

    ctx
}

/// Resolve type elements from a TSType node.
///
/// This extracts property information from type literals, interfaces,
/// and other TypeScript type constructs.
///
/// # Arguments
/// * `node` - The TSType node to resolve
/// * `base_offset` - The document offset to apply to all spans
pub fn resolve_type_elements(node: &TSType, base_offset: u32) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner(node, base_offset, &mut result);
    result
}

/// Resolve type elements with a type resolution context.
/// This version can resolve local type aliases and interfaces.
///
/// # Arguments
/// * `node` - The TSType node to resolve
/// * `base_offset` - The document offset to apply to all spans
/// * `ctx` - Type resolution context with local type definitions
pub fn resolve_type_elements_with_ctx<'a>(
    node: &'a TSType<'a>,
    base_offset: u32,
    ctx: &mut TypeResolutionContext<'a>,
) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner_with_ctx(node, base_offset, &mut result, ctx);
    result
}

/// Resolve type elements with an immutable type resolution context.
/// This version doesn't collect diagnostics, making it suitable for
/// contexts where we only need the resolved types without error tracking.
///
/// # Arguments
/// * `node` - The TSType node to resolve
/// * `base_offset` - The document offset to apply to all spans
/// * `ctx` - Immutable type resolution context with local type definitions
pub fn resolve_type_elements_with_ctx_ref<'a>(
    node: &'a TSType<'a>,
    base_offset: u32,
    ctx: &TypeResolutionContext<'a>,
) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner_with_ctx_ref(node, base_offset, &mut result, ctx);
    result
}

fn resolve_type_elements_inner(node: &TSType, base_offset: u32, result: &mut ResolvedElements) {
    match node {
        // { prop: Type }
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members(&lit.members, base_offset, result);
        }

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => {
            resolve_type_elements_inner(&paren.type_annotation, base_offset, result);
        }

        // Union: Type1 | Type2
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                resolve_type_elements_inner(ty, base_offset, result);
            }
        }

        // Intersection: Type1 & Type2
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                resolve_type_elements_inner(ty, base_offset, result);
            }
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(_type_ref) => {
            // For now, we can't resolve type references without a scope
            // This would require tracking type declarations
            // Mark as unknown for now
        }

        // Function type: () => Type
        TSType::TSFunctionType(_) => {
            result.has_call_signature = true;
        }

        _ => {}
    }
}

/// Inner resolution function that uses the context for type reference lookup.
fn resolve_type_elements_inner_with_ctx<'a>(
    node: &'a TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &mut TypeResolutionContext<'a>,
) {
    match node {
        // { prop: Type }
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members(&lit.members, base_offset, result);
        }

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => {
            resolve_type_elements_inner_with_ctx(&paren.type_annotation, base_offset, result, ctx);
        }

        // Union: Type1 | Type2
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                resolve_type_elements_inner_with_ctx(ty, base_offset, result, ctx);
            }
        }

        // Intersection: Type1 & Type2
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                resolve_type_elements_inner_with_ctx(ty, base_offset, result, ctx);
            }
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(type_ref) => {
            // Get the type name for lookup
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            // 1. Check local type aliases
            if let Some(aliased_type) = ctx.find_type_alias(type_name_bytes) {
                resolve_type_elements_inner_with_ctx(aliased_type, base_offset, result, ctx);
                return;
            }

            // 2. Check local interfaces
            if let Some(interface_members) = ctx.find_interface(type_name_bytes) {
                resolve_type_literal_members(interface_members, base_offset, result);
                return;
            }

            // 3. Check generic type parameter constraints
            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                resolve_type_elements_inner_with_ctx(constraint, base_offset, result, ctx);
                return;
            }

            // 4. Couldn't resolve - add diagnostic
            // Note: We don't add to result.props here because we can't determine the structure
            ctx.diagnostics.push(ResolutionDiagnostic {
                span: Span {
                    start: type_ref.span.start + base_offset,
                    end: type_ref.span.end + base_offset,
                },
                kind: ResolutionDiagnosticKind::UnresolvedTypeReference,
                location: DiagnosticLocation::TypeResolution,
            });
        }

        // Function type: () => Type
        TSType::TSFunctionType(_) => {
            result.has_call_signature = true;
        }

        _ => {}
    }
}

/// Inner resolution function that uses an immutable context (doesn't collect diagnostics).
fn resolve_type_elements_inner_with_ctx_ref<'a>(
    node: &'a TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'a>,
) {
    match node {
        // { prop: Type }
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members(&lit.members, base_offset, result);
        }

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => {
            resolve_type_elements_inner_with_ctx_ref(
                &paren.type_annotation,
                base_offset,
                result,
                ctx,
            );
        }

        // Union: Type1 | Type2
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                resolve_type_elements_inner_with_ctx_ref(ty, base_offset, result, ctx);
            }
        }

        // Intersection: Type1 & Type2
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                resolve_type_elements_inner_with_ctx_ref(ty, base_offset, result, ctx);
            }
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(type_ref) => {
            // Get the type name for lookup
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            // 1. Check local type aliases
            if let Some(aliased_type) = ctx.find_type_alias(type_name_bytes) {
                resolve_type_elements_inner_with_ctx_ref(aliased_type, base_offset, result, ctx);
                return;
            }

            // 2. Check local interfaces
            if let Some(interface_members) = ctx.find_interface(type_name_bytes) {
                resolve_type_literal_members(interface_members, base_offset, result);
                return;
            }

            // 3. Check generic type parameter constraints
            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                resolve_type_elements_inner_with_ctx_ref(constraint, base_offset, result, ctx);
            }

            // 4. Couldn't resolve - skip silently (no diagnostics in immutable version)
        }

        // Function type: () => Type
        TSType::TSFunctionType(_) => {
            result.has_call_signature = true;
        }

        _ => {}
    }
}

/// Resolve members from a type literal's members array.
fn resolve_type_literal_members(
    members: &[TSSignature],
    base_offset: u32,
    result: &mut ResolvedElements,
) {
    for member in members {
        match member {
            TSSignature::TSPropertySignature(prop) => {
                // Check if this is a shorthand emit: { change: [id: number] }
                // Properties with tuple/array type values are treated as emits
                if let Some(emit) = resolve_property_as_emit(prop, base_offset) {
                    result.emits.push(emit);
                } else if let Some(resolved) = resolve_property_signature(prop, base_offset) {
                    result.props.push(resolved);
                }
            }
            TSSignature::TSMethodSignature(method) => {
                if let Some(resolved) = resolve_method_signature(method, base_offset) {
                    result.props.push(resolved);
                }
            }
            TSSignature::TSCallSignatureDeclaration(call_sig) => {
                result.has_call_signature = true;
                // Extract emit from call signature: (e: 'change', id: number): void
                if let Some(emit) = resolve_call_signature_as_emit(call_sig, base_offset) {
                    result.emits.push(emit);
                }
            }
            _ => {}
        }
    }
}

/// Try to resolve a property signature as an emit (shorthand style).
/// Shorthand style: `{ change: [id: number] }` or `{ update: [] }`
fn resolve_property_as_emit(prop: &TSPropertySignature, base_offset: u32) -> Option<ResolvedEmit> {
    // Get the property key as the event name
    let name = get_property_key_name(&prop.key)?;
    let key_span = get_property_key_span(&prop.key, base_offset)?;

    // Check if the type is a tuple type - this indicates emit shorthand
    // Note: Only TSTupleType (e.g., `[id: number]`) is emit shorthand.
    // TSArrayType (e.g., `string[]`) is a regular array prop type.
    if let Some(ann) = &prop.type_annotation {
        if let TSType::TSTupleType(_) = &ann.type_annotation {
            return Some(ResolvedEmit {
                span: Span {
                    start: prop.span.start + base_offset,
                    end: prop.span.end + base_offset,
                },
                name,
                name_span: Some(key_span),
            });
        }
    }

    None
}

/// Resolve a call signature as an emit event.
/// Call signature style: `(e: 'change', id: number): void`
/// The event name is extracted from the first parameter's type if it's a string literal.
fn resolve_call_signature_as_emit(
    call_sig: &TSCallSignatureDeclaration,
    base_offset: u32,
) -> Option<ResolvedEmit> {
    // Get the first parameter - should be like `e: 'eventName'`
    let first_param = call_sig.params.items.first()?;

    // The type annotation is on the FormalParameter, not the pattern
    let type_ann = first_param.type_annotation.as_ref()?;

    // Extract event name from string literal type
    if let TSType::TSLiteralType(lit) = &type_ann.type_annotation {
        if let TSLiteral::StringLiteral(s) = &lit.literal {
            return Some(ResolvedEmit {
                span: Span {
                    start: call_sig.span.start + base_offset,
                    end: call_sig.span.end + base_offset,
                },
                name: s.value.to_string(),
                name_span: Some(Span {
                    start: s.span.start + base_offset,
                    end: s.span.end + base_offset,
                }),
            });
        }
    }

    None
}

/// Get the name of a property key as a string.
fn get_property_key_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => n.raw.as_ref().map(|r| r.to_string()),
        _ => None,
    }
}

/// Resolve a property signature to a ResolvedProp.
fn resolve_property_signature(
    prop: &TSPropertySignature,
    base_offset: u32,
) -> Option<ResolvedProp> {
    let key = get_property_key_span(&prop.key, base_offset)?;
    let optional = prop.optional;

    let types = prop
        .type_annotation
        .as_ref()
        .map(|ann| infer_runtime_type(&ann.type_annotation))
        .unwrap_or_else(|| vec![RuntimeType::Unknown]);

    // Full span from the property signature, adjusted by base_offset
    let span = Span {
        start: prop.span.start + base_offset,
        end: prop.span.end + base_offset,
    };

    Some(ResolvedProp {
        span,
        key,
        optional,
        types,
    })
}

/// Resolve a method signature to a ResolvedProp (methods are function-typed properties).
fn resolve_method_signature(method: &TSMethodSignature, base_offset: u32) -> Option<ResolvedProp> {
    let key = get_property_key_span(&method.key, base_offset)?;
    let optional = method.optional;

    // Full span from the method signature, adjusted by base_offset
    let span = Span {
        start: method.span.start + base_offset,
        end: method.span.end + base_offset,
    };

    Some(ResolvedProp {
        span,
        key,
        optional,
        types: vec![RuntimeType::Function],
    })
}

/// Extract the span of a property key.
fn get_property_key_span(key: &PropertyKey, base_offset: u32) -> Option<Span> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(Span {
            start: id.span.start + base_offset,
            end: id.span.end + base_offset,
        }),
        PropertyKey::StringLiteral(s) => Some(Span {
            start: s.span.start + base_offset,
            end: s.span.end + base_offset,
        }),
        PropertyKey::NumericLiteral(n) => Some(Span {
            start: n.span.start + base_offset,
            end: n.span.end + base_offset,
        }),
        // Computed keys are not supported
        _ => None,
    }
}

/// Infer runtime type(s) from a TypeScript type annotation.
///
/// Returns a list of possible runtime types. For union types,
/// this returns all possible types. For simple types, returns a single type.
pub fn infer_runtime_type(node: &TSType) -> Vec<RuntimeType> {
    match node {
        // Primitive types
        TSType::TSStringKeyword(_) => vec![RuntimeType::String],
        TSType::TSNumberKeyword(_) => vec![RuntimeType::Number],
        TSType::TSBooleanKeyword(_) => vec![RuntimeType::Boolean],
        TSType::TSObjectKeyword(_) => vec![RuntimeType::Object],
        TSType::TSSymbolKeyword(_) => vec![RuntimeType::Symbol],
        TSType::TSNullKeyword(_) => vec![RuntimeType::Null],
        TSType::TSUndefinedKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSVoidKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSAnyKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSUnknownKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSNeverKeyword(_) => vec![RuntimeType::Unknown],
        TSType::TSBigIntKeyword(_) => vec![RuntimeType::Number],

        // Literal types
        TSType::TSLiteralType(lit) => infer_literal_type(lit),

        // Object/interface types
        TSType::TSTypeLiteral(_) => vec![RuntimeType::Object],

        // Array types
        TSType::TSArrayType(_) | TSType::TSTupleType(_) => vec![RuntimeType::Array],

        // Function types
        TSType::TSFunctionType(_) | TSType::TSConstructorType(_) => vec![RuntimeType::Function],

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => infer_runtime_type(&paren.type_annotation),

        // Union: Type1 | Type2
        TSType::TSUnionType(union) => {
            let mut types = Vec::new();
            for ty in &union.types {
                for t in infer_runtime_type(ty) {
                    if !types.contains(&t) {
                        types.push(t);
                    }
                }
            }
            types
        }

        // Intersection: Type1 & Type2 - typically results in Object
        TSType::TSIntersectionType(intersection) => {
            // For intersections, try to infer from all types
            let mut types = Vec::new();
            for ty in &intersection.types {
                for t in infer_runtime_type(ty) {
                    if t != RuntimeType::Unknown && !types.contains(&t) {
                        types.push(t);
                    }
                }
            }
            if types.is_empty() {
                vec![RuntimeType::Object]
            } else {
                types
            }
        }

        // Type reference: SomeType or SomeType<T>
        TSType::TSTypeReference(type_ref) => infer_type_reference(type_ref),

        // Conditional type: T extends U ? X : Y
        TSType::TSConditionalType(_) => vec![RuntimeType::Unknown],

        // Mapped type: { [K in keyof T]: T[K] }
        TSType::TSMappedType(_) => vec![RuntimeType::Object],

        // Indexed access: T[K]
        TSType::TSIndexedAccessType(_) => vec![RuntimeType::Unknown],

        // Template literal type: `${string}`
        TSType::TSTemplateLiteralType(_) => vec![RuntimeType::String],

        // Type query: typeof x
        TSType::TSTypeQuery(_) => vec![RuntimeType::Unknown],

        // Import type: import("...").Type
        TSType::TSImportType(_) => vec![RuntimeType::Unknown],

        // Type operator: keyof T, readonly T, unique symbol
        TSType::TSTypeOperatorType(op) => {
            if matches!(op.operator, TSTypeOperatorOperator::Keyof) {
                // keyof usually results in string | number | symbol
                vec![
                    RuntimeType::String,
                    RuntimeType::Number,
                    RuntimeType::Symbol,
                ]
            } else {
                infer_runtime_type(&op.type_annotation)
            }
        }

        // Infer type: infer T
        TSType::TSInferType(_) => vec![RuntimeType::Unknown],

        // This type
        TSType::TSThisType(_) => vec![RuntimeType::Object],

        // Intrinsic keyword
        TSType::TSIntrinsicKeyword(_) => vec![RuntimeType::Unknown],

        // Catch-all for any new types
        _ => vec![RuntimeType::Unknown],
    }
}

/// Infer runtime type from a literal type.
fn infer_literal_type(lit: &TSLiteralType) -> Vec<RuntimeType> {
    match &lit.literal {
        TSLiteral::StringLiteral(_) => vec![RuntimeType::String],
        TSLiteral::NumericLiteral(_) => vec![RuntimeType::Number],
        TSLiteral::BooleanLiteral(_) => vec![RuntimeType::Boolean],
        TSLiteral::BigIntLiteral(_) => vec![RuntimeType::Number],
        TSLiteral::TemplateLiteral(_) => vec![RuntimeType::String],
        TSLiteral::UnaryExpression(unary) => {
            // -1, +1, etc.
            match &unary.argument {
                Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => {
                    vec![RuntimeType::Number]
                }
                _ => vec![RuntimeType::Unknown],
            }
        }
    }
}

/// Infer runtime type from a type reference.
fn infer_type_reference(type_ref: &TSTypeReference) -> Vec<RuntimeType> {
    let name = get_type_reference_name(&type_ref.type_name);

    match name.as_str() {
        // Built-in JavaScript types
        "Array" | "ReadonlyArray" => vec![RuntimeType::Array],
        "Function" => vec![RuntimeType::Function],
        "Object" => vec![RuntimeType::Object],
        "String" => vec![RuntimeType::String],
        "Number" => vec![RuntimeType::Number],
        "Boolean" => vec![RuntimeType::Boolean],
        "Symbol" => vec![RuntimeType::Symbol],

        // Built-in object types
        "Date" | "RegExp" | "Error" | "Map" | "Set" | "WeakMap" | "WeakSet" | "Promise" => {
            vec![RuntimeType::BuiltIn(name)]
        }

        // TypeScript utility types
        "Partial" | "Required" | "Readonly" | "Record" | "Pick" | "Omit" | "InstanceType" => {
            vec![RuntimeType::Object]
        }
        "Parameters" | "ConstructorParameters" => vec![RuntimeType::Array],
        "ReturnType" => vec![RuntimeType::Unknown],
        "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" => vec![RuntimeType::String],
        "NonNullable" => {
            // Try to infer from the type parameter
            if let Some(args) = &type_ref.type_arguments {
                if let Some(first) = args.params.first() {
                    return infer_runtime_type(first)
                        .into_iter()
                        .filter(|t| *t != RuntimeType::Null)
                        .collect();
                }
            }
            vec![RuntimeType::Unknown]
        }
        "Extract" => {
            // Extract<T, U> - returns U
            if let Some(args) = &type_ref.type_arguments {
                if let Some(second) = args.params.get(1) {
                    return infer_runtime_type(second);
                }
            }
            vec![RuntimeType::Unknown]
        }
        "Exclude" | "OmitThisParameter" => {
            // Exclude<T, U> - returns T without U
            if let Some(args) = &type_ref.type_arguments {
                if let Some(first) = args.params.first() {
                    return infer_runtime_type(first);
                }
            }
            vec![RuntimeType::Unknown]
        }

        // Unknown type reference - can't resolve without scope
        _ => vec![RuntimeType::Unknown],
    }
}

/// Get the name from a type reference's type name.
fn get_type_reference_name(type_name: &TSTypeName) -> String {
    match type_name {
        TSTypeName::IdentifierReference(id) => id.name.to_string(),
        TSTypeName::QualifiedName(qualified) => {
            // For qualified names like Foo.Bar, just use the last part for now
            qualified.right.name.to_string()
        }
        TSTypeName::ThisExpression(_) => "this".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    /// Result of parsing a type string, includes source for key extraction
    struct ParsedType {
        source: String,
        resolved: ResolvedElements,
    }

    impl ParsedType {
        /// Get the key name from a prop by extracting from source
        fn key_name(&self, prop: &ResolvedProp) -> &str {
            &self.source[prop.key.start as usize..prop.key.end as usize]
        }

        /// Find a prop by key name
        fn find_prop(&self, name: &str) -> Option<&ResolvedProp> {
            self.resolved
                .props
                .iter()
                .find(|p| self.key_name(p) == name)
        }
    }

    /// Helper to parse a type string and return the result with source
    fn parse_type(type_str: &str) -> Option<ParsedType> {
        let allocator = Allocator::default();
        // Wrap in a type alias to parse
        let source = format!("type T = {}", type_str);
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, &source, source_type);
        let result = parser.parse();

        if !result.errors.is_empty() {
            return None;
        }

        // Find the type alias declaration
        for stmt in &result.program.body {
            if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                return Some(ParsedType {
                    source: source.clone(),
                    resolved: resolve_type_elements(&alias.type_annotation, 0),
                });
            }
        }
        None
    }

    /// Helper to infer runtime types from a type string
    fn infer_type(type_str: &str) -> Vec<RuntimeType> {
        let allocator = Allocator::default();
        let source = format!("type T = {}", type_str);
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, &source, source_type);
        let result = parser.parse();

        if !result.errors.is_empty() {
            return vec![RuntimeType::Unknown];
        }

        for stmt in &result.program.body {
            if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                return infer_runtime_type(&alias.type_annotation);
            }
        }
        vec![RuntimeType::Unknown]
    }

    #[test]
    fn test_primitive_types() {
        assert_eq!(infer_type("string"), vec![RuntimeType::String]);
        assert_eq!(infer_type("number"), vec![RuntimeType::Number]);
        assert_eq!(infer_type("boolean"), vec![RuntimeType::Boolean]);
        assert_eq!(infer_type("symbol"), vec![RuntimeType::Symbol]);
        assert_eq!(infer_type("null"), vec![RuntimeType::Null]);
        assert_eq!(infer_type("bigint"), vec![RuntimeType::Number]);
    }

    #[test]
    fn test_literal_types() {
        assert_eq!(infer_type("'hello'"), vec![RuntimeType::String]);
        assert_eq!(infer_type("42"), vec![RuntimeType::Number]);
        assert_eq!(infer_type("true"), vec![RuntimeType::Boolean]);
        assert_eq!(infer_type("false"), vec![RuntimeType::Boolean]);
    }

    #[test]
    fn test_array_types() {
        assert_eq!(infer_type("string[]"), vec![RuntimeType::Array]);
        assert_eq!(infer_type("Array<number>"), vec![RuntimeType::Array]);
        assert_eq!(infer_type("[string, number]"), vec![RuntimeType::Array]);
    }

    #[test]
    fn test_function_types() {
        assert_eq!(infer_type("() => void"), vec![RuntimeType::Function]);
        assert_eq!(
            infer_type("(x: number) => string"),
            vec![RuntimeType::Function]
        );
        assert_eq!(infer_type("Function"), vec![RuntimeType::Function]);
    }

    #[test]
    fn test_object_types() {
        assert_eq!(infer_type("{ foo: string }"), vec![RuntimeType::Object]);
        assert_eq!(infer_type("object"), vec![RuntimeType::Object]);
        assert_eq!(infer_type("Object"), vec![RuntimeType::Object]);
    }

    #[test]
    fn test_union_types() {
        let types = infer_type("string | number");
        assert!(types.contains(&RuntimeType::String));
        assert!(types.contains(&RuntimeType::Number));
        assert_eq!(types.len(), 2);
    }

    #[test]
    fn test_builtin_types() {
        assert_eq!(
            infer_type("Date"),
            vec![RuntimeType::BuiltIn("Date".to_string())]
        );
        assert_eq!(
            infer_type("Map<string, number>"),
            vec![RuntimeType::BuiltIn("Map".to_string())]
        );
        assert_eq!(
            infer_type("Set<string>"),
            vec![RuntimeType::BuiltIn("Set".to_string())]
        );
        assert_eq!(
            infer_type("Promise<void>"),
            vec![RuntimeType::BuiltIn("Promise".to_string())]
        );
    }

    #[test]
    fn test_utility_types() {
        assert_eq!(
            infer_type("Partial<{ foo: string }>"),
            vec![RuntimeType::Object]
        );
        assert_eq!(
            infer_type("Required<{ foo?: string }>"),
            vec![RuntimeType::Object]
        );
        assert_eq!(
            infer_type("Parameters<() => void>"),
            vec![RuntimeType::Array]
        );
    }

    #[test]
    fn test_resolve_type_literal() {
        let parsed = parse_type("{ title: string; count: number }").unwrap();
        assert_eq!(parsed.resolved.props.len(), 2);

        let title = parsed.find_prop("title").unwrap();
        assert_eq!(title.types, vec![RuntimeType::String]);
        assert!(!title.optional);

        let count = parsed.find_prop("count").unwrap();
        assert_eq!(count.types, vec![RuntimeType::Number]);
        assert!(!count.optional);
    }

    #[test]
    fn test_resolve_optional_props() {
        let parsed = parse_type("{ required: string; optional?: number }").unwrap();
        assert_eq!(parsed.resolved.props.len(), 2);

        let required = parsed.find_prop("required").unwrap();
        assert!(!required.optional);

        let optional = parsed.find_prop("optional").unwrap();
        assert!(optional.optional);
    }

    #[test]
    fn test_resolve_method_signatures() {
        let parsed = parse_type("{ onClick(): void; onChange(value: string): void }").unwrap();
        assert_eq!(parsed.resolved.props.len(), 2);

        for prop in &parsed.resolved.props {
            assert_eq!(prop.types, vec![RuntimeType::Function]);
        }
    }

    #[test]
    fn test_resolve_union_prop_types() {
        let parsed = parse_type("{ value: string | number }").unwrap();
        assert_eq!(parsed.resolved.props.len(), 1);

        let value = &parsed.resolved.props[0];
        assert!(value.types.contains(&RuntimeType::String));
        assert!(value.types.contains(&RuntimeType::Number));
    }

    #[test]
    fn test_resolve_call_signature() {
        let parsed = parse_type("{ (): void }").unwrap();
        assert!(parsed.resolved.has_call_signature);
    }

    #[test]
    fn test_complex_props_type() {
        let parsed = parse_type(
            r#"{
            title: string;
            count?: number;
            items: string[];
            metadata: { key: string };
            onClick: () => void;
            onUpdate(value: string): void;
        }"#,
        )
        .unwrap();

        assert_eq!(parsed.resolved.props.len(), 6);

        let title = parsed.find_prop("title").unwrap();
        assert_eq!(title.types, vec![RuntimeType::String]);
        assert!(!title.optional);

        let count = parsed.find_prop("count").unwrap();
        assert_eq!(count.types, vec![RuntimeType::Number]);
        assert!(count.optional);

        let items = parsed.find_prop("items").unwrap();
        assert_eq!(items.types, vec![RuntimeType::Array]);

        let metadata = parsed.find_prop("metadata").unwrap();
        assert_eq!(metadata.types, vec![RuntimeType::Object]);

        let onclick = parsed.find_prop("onClick").unwrap();
        assert_eq!(onclick.types, vec![RuntimeType::Function]);

        let onupdate = parsed.find_prop("onUpdate").unwrap();
        assert_eq!(onupdate.types, vec![RuntimeType::Function]);
    }

    #[test]
    fn test_format_runtime_types_single() {
        assert_eq!(format_runtime_types(&[RuntimeType::String]), "String");
        assert_eq!(format_runtime_types(&[RuntimeType::Number]), "Number");
        assert_eq!(format_runtime_types(&[RuntimeType::Boolean]), "Boolean");
        assert_eq!(format_runtime_types(&[RuntimeType::Array]), "Array");
        assert_eq!(format_runtime_types(&[RuntimeType::Function]), "Function");
        assert_eq!(format_runtime_types(&[RuntimeType::Object]), "Object");
    }

    #[test]
    fn test_format_runtime_types_multiple() {
        assert_eq!(
            format_runtime_types(&[RuntimeType::String, RuntimeType::Number]),
            "[String, Number]"
        );
        assert_eq!(
            format_runtime_types(&[
                RuntimeType::String,
                RuntimeType::Number,
                RuntimeType::Boolean
            ]),
            "[String, Number, Boolean]"
        );
    }

    #[test]
    fn test_format_runtime_types_filters_unknown() {
        // Unknown types should be filtered out
        assert_eq!(
            format_runtime_types(&[RuntimeType::String, RuntimeType::Unknown]),
            "String"
        );
        assert_eq!(format_runtime_types(&[RuntimeType::Unknown]), "null");
    }

    #[test]
    fn test_format_runtime_types_builtin() {
        assert_eq!(
            format_runtime_types(&[RuntimeType::BuiltIn("Date".to_string())]),
            "Date"
        );
        assert_eq!(
            format_runtime_types(&[RuntimeType::BuiltIn("Map".to_string()), RuntimeType::Null]),
            "[Map, null]"
        );
    }

    // =========================================================================
    // Tests for TypeResolutionContext
    // =========================================================================

    #[test]
    fn test_build_type_context_collects_type_aliases() {
        let allocator = Allocator::default();
        let source = r#"type Props = { foo: string };
type Options = { bar: number };"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let ctx = build_type_context(&result.program, source.as_bytes(), 0);

        assert_eq!(ctx.type_aliases.len(), 2);
        // Check that we can find Props
        assert!(ctx.find_type_alias(b"Props").is_some());
        assert!(ctx.find_type_alias(b"Options").is_some());
        assert!(ctx.find_type_alias(b"Unknown").is_none());
    }

    #[test]
    fn test_build_type_context_collects_interfaces() {
        let allocator = Allocator::default();
        let source = r#"interface Props { foo: string }
interface Options { bar: number }"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let ctx = build_type_context(&result.program, source.as_bytes(), 0);

        assert_eq!(ctx.interfaces.len(), 2);
        assert!(ctx.find_interface(b"Props").is_some());
        assert!(ctx.find_interface(b"Options").is_some());
        assert!(ctx.find_interface(b"Unknown").is_none());
    }

    #[test]
    fn test_resolve_type_alias_with_context() {
        let allocator = Allocator::default();
        let source = r#"type Props = { foo: string; bar: number };
type Test = Props;"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);

        // Find the type alias for Test and resolve it
        // First, manually get the Test type alias
        for stmt in &result.program.body {
            if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                if alias.id.name.as_str() == "Test" {
                    let resolved =
                        resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
                    // Test should resolve to Props which has 2 props
                    assert_eq!(
                        resolved.props.len(),
                        2,
                        "Should resolve Props type alias with 2 props"
                    );
                    assert!(ctx.diagnostics.is_empty(), "Should have no diagnostics");
                }
            }
        }
    }

    #[test]
    fn test_resolve_interface_with_context() {
        let allocator = Allocator::default();
        let source = r#"interface Props { foo: string; bar: number }
type Test = Props;"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);

        // Find the type alias for Test and resolve it
        for stmt in &result.program.body {
            if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                if alias.id.name.as_str() == "Test" {
                    let resolved =
                        resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
                    // Test should resolve to Props interface which has 2 props
                    assert_eq!(
                        resolved.props.len(),
                        2,
                        "Should resolve Props interface with 2 props"
                    );
                    assert!(ctx.diagnostics.is_empty(), "Should have no diagnostics");
                }
            }
        }
    }

    #[test]
    fn test_unresolved_type_emits_diagnostic() {
        let allocator = Allocator::default();
        let source = r#"type Test = UnknownType;"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);

        for stmt in &result.program.body {
            if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                let _resolved = resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
                // Should have a diagnostic for unresolved type
                assert_eq!(ctx.diagnostics.len(), 1, "Should have 1 diagnostic");
                assert_eq!(
                    ctx.diagnostics[0].kind,
                    ResolutionDiagnosticKind::UnresolvedTypeReference
                );
                assert_eq!(
                    ctx.diagnostics[0].location,
                    DiagnosticLocation::TypeResolution,
                    "Diagnostic should come from TypeResolution"
                );
            }
        }
    }

    #[test]
    fn test_resolve_intersection_with_context() {
        let allocator = Allocator::default();
        let source = r#"type A = { foo: string };
type B = { bar: number };
type Test = A & B;"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);

        for stmt in &result.program.body {
            if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                if alias.id.name.as_str() == "Test" {
                    let resolved =
                        resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
                    // Test should resolve to A & B which has 2 props total
                    assert_eq!(
                        resolved.props.len(),
                        2,
                        "Should resolve intersection with 2 props"
                    );
                    assert!(ctx.diagnostics.is_empty(), "Should have no diagnostics");
                }
            }
        }
    }
}
