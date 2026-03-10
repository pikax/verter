//! Cross-file type resolution for Vue compiler macros.
//!
//! Resolves TypeScript type annotations used as type parameters in Vue macros
//! (`defineProps<T>()`, `defineEmits<T>()`, `defineSlots<T>()`) into structured
//! [`ResolvedElements`] that drive runtime props/emits code generation.
//!
//! When `T` is defined inline (e.g. `defineProps<{ title: string }>()`), resolution
//! stays local. When `T` extends or references types from other files, the host
//! must pre-resolve those external types and pass them in via
//! [`VerterCompileOptions::external_types`](crate::VerterCompileOptions). The
//! resolved data is merged into [`TypeResolutionContext::companion_types`] so that
//! lookups for imported type names can fall back to pre-resolved definitions.
//!
//! Based on Vue's `resolveType.ts` implementation.

#![allow(dead_code)]

use oxc_ast::ast::*;
use oxc_span::GetSpan;
use rustc_hash::FxHashMap;

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
    /// Pre-resolved key name (set for external/cross-file types where spans
    /// reference a different source than the consuming SFC).
    pub key_name: Option<String>,
    /// Whether the property is optional (has `?`)
    pub optional: bool,
    /// Inferred runtime types for this property
    pub types: Vec<RuntimeType>,
    /// Span of the type annotation (excluding the `: ` prefix) in the source.
    /// Set for property signatures with explicit type annotations; `None` for
    /// method signatures and companion-script props.
    pub type_span: Option<Span>,
    /// Whether this span points into the current SFC source and can be used
    /// directly for local source maps.
    pub map_local: bool,
    /// Whether spans on this prop are already SFC-absolute.
    pub span_is_absolute: bool,
}

/// A resolved emit event from defineEmits type parameter.
/// Supports both call signature style `{ (e: 'change', id: number): void }`
/// and shorthand style `{ change: [id: number] }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedEmitSignature {
    /// Call signature payload params after the event name parameter.
    /// Empty string means the event carries no extra payload.
    Call { params_text: String },
    /// Shorthand tuple payload, including the surrounding `[...]`.
    Tuple { tuple_text: String },
}

#[derive(Debug, Clone)]
pub struct ResolvedEmit {
    /// Span of the entire emit signature
    pub span: Span,
    /// The event name (extracted from first parameter literal or property key)
    pub name: String,
    /// Span of the event name in source (if available, for string literal params)
    pub name_span: Option<Span>,
    /// The resolved payload signature, preserved as text so consumers can
    /// inline exact handler / `$emit` types even for cross-file imports.
    pub signature: ResolvedEmitSignature,
    /// Whether this span points into the current SFC source and can be used
    /// directly for local source maps.
    pub map_local: bool,
    /// Whether spans on this emit are already SFC-absolute.
    pub span_is_absolute: bool,
}

/// Result of resolving type elements from a type annotation.
#[derive(Debug, Default, Clone)]
pub struct ResolvedElements {
    /// Resolved properties from the type
    pub props: Vec<ResolvedProp>,
    /// Resolved emit events from call signatures or shorthand properties
    pub emits: Vec<ResolvedEmit>,
    /// Whether this type has call signatures (is callable)
    pub has_call_signature: bool,
    /// Runtime type inferred from the root type annotation being resolved.
    /// Used to distinguish valid empty object-like macro types from invalid
    /// primitives when cross-file resolution returns no concrete members.
    pub root_runtime_types: Vec<RuntimeType>,
}

impl ResolvedElements {
    /// Deduplicate props by key name (first occurrence wins).
    /// Matches Vue's `mergeElements()` behavior for union/intersection types.
    fn dedup_props(&mut self) {
        let mut seen = rustc_hash::FxHashSet::default();
        self.props.retain(|prop| {
            if let Some(ref name) = prop.key_name {
                seen.insert(name.clone())
            } else {
                // No key_name — always keep (shouldn't happen after resolution)
                true
            }
        });
    }
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
///
/// Two lifetime parameters:
/// - `'ctx`: borrow lifetime of the program reference (how long we hold references into the AST)
/// - `'a`: arena allocator lifetime (the AST node types are `TSType<'a>`, etc.)
#[derive(Debug)]
pub struct TypeResolutionContext<'ctx, 'a: 'ctx> {
    /// Source bytes for name comparisons
    pub source: &'ctx [u8],
    /// Local type alias declarations: (name_span, type_node)
    pub type_aliases: Vec<(Span, &'ctx TSType<'a>)>,
    /// Local interface declarations: (name_span, interface_body_members, extends_type_names)
    /// The extends_type_names are extracted from heritage clauses as String names,
    /// since we need to look them up recursively.
    pub interfaces: Vec<(
        Span,
        &'ctx oxc_allocator::Vec<'a, TSSignature<'a>>,
        Vec<String>,
    )>,
    /// Generic type parameters with constraints: (name_span, constraint_type)
    pub type_params: Vec<(Span, Option<&'ctx TSType<'a>>)>,
    /// Diagnostics collected during resolution
    pub diagnostics: Vec<ResolutionDiagnostic>,
    /// Pre-resolved types from companion `<script>` block.
    /// Keyed by type name string, value is the resolved elements.
    /// Used when a type reference can't be found in the local context.
    pub companion_types: rustc_hash::FxHashMap<String, ResolvedElements>,
}

impl<'ctx, 'a: 'ctx> TypeResolutionContext<'ctx, 'a> {
    /// Create a new empty context
    pub fn new(source: &'ctx [u8]) -> Self {
        Self {
            source,
            type_aliases: Vec::new(),
            interfaces: Vec::new(),
            type_params: Vec::new(),
            diagnostics: Vec::new(),
            companion_types: rustc_hash::FxHashMap::default(),
        }
    }

    /// Look up a type alias by comparing spans against source bytes
    pub fn find_type_alias(&self, name: &[u8]) -> Option<&'ctx TSType<'a>> {
        self.type_aliases
            .iter()
            .find(|(span, _)| &self.source[span.start as usize..span.end as usize] == name)
            .map(|(_, ty)| *ty)
    }

    /// Look up an interface by comparing spans against source bytes.
    /// Returns (body_members, extends_type_names).
    pub fn find_interface(
        &self,
        name: &[u8],
    ) -> Option<(&'ctx oxc_allocator::Vec<'a, TSSignature<'a>>, &[String])> {
        self.interfaces
            .iter()
            .find(|(span, _, _)| &self.source[span.start as usize..span.end as usize] == name)
            .map(|(_, members, extends)| (*members, extends.as_slice()))
    }

    /// Look up a type parameter constraint by comparing spans against source bytes
    pub fn find_type_param(&self, name: &[u8]) -> Option<&'ctx TSType<'a>> {
        self.type_params
            .iter()
            .find(|(span, _)| &self.source[span.start as usize..span.end as usize] == name)
            .and_then(|(_, constraint)| *constraint)
    }
}

/// Build type resolution context from a parsed program.
/// Collects type aliases and interfaces for later lookup.
pub fn build_type_context<'ctx, 'a: 'ctx>(
    program: &'ctx Program<'a>,
    source: &'ctx [u8],
    _content_offset: u32,
) -> TypeResolutionContext<'ctx, 'a> {
    let mut ctx = TypeResolutionContext::new(source);

    for stmt in &program.body {
        match stmt {
            // Collect type aliases: `type Foo = { bar: string }`
            Statement::TSTypeAliasDeclaration(alias) => {
                // alias.id.span is already adjusted by adjust_program_spans() to SFC coordinates,
                // so we use it directly — no additional offset needed.
                let name_span = Span::from(alias.id.span);
                ctx.type_aliases.push((name_span, &alias.type_annotation));
            }
            // Collect interfaces: `interface Foo { bar: string }`
            Statement::TSInterfaceDeclaration(interface) => {
                // interface.id.span is already adjusted by adjust_program_spans() to SFC coordinates.
                let name_span = Span::from(interface.id.span);
                let extends = extract_heritage_type_names(&interface.extends);
                ctx.interfaces
                    .push((name_span, &interface.body.body, extends));
            }
            // Collect exported type aliases and interfaces:
            // `export type Foo = { bar: string }` / `export interface Foo { bar: string }`
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    match decl {
                        Declaration::TSTypeAliasDeclaration(alias) => {
                            let name_span = Span::from(alias.id.span);
                            ctx.type_aliases.push((name_span, &alias.type_annotation));
                        }
                        Declaration::TSInterfaceDeclaration(interface) => {
                            let name_span = Span::from(interface.id.span);
                            let extends = extract_heritage_type_names(&interface.extends);
                            ctx.interfaces
                                .push((name_span, &interface.body.body, extends));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    ctx
}

/// Extract pre-resolved types from a companion `<script>` program.
///
/// Walks the program's statements and resolves any type aliases and interfaces,
/// returning a map from type name → resolved elements. This allows the setup
/// script's type resolver to look up types defined in the companion block.
pub fn extract_companion_types(
    program: &Program<'_>,
    source: &[u8],
    content_offset: u32,
) -> rustc_hash::FxHashMap<String, ResolvedElements> {
    // Build a full type context so we can resolve extends and cross-references
    let ctx = build_type_context(program, source, content_offset);

    let mut types = rustc_hash::FxHashMap::default();

    for stmt in &program.body {
        match stmt {
            Statement::TSTypeAliasDeclaration(alias) => {
                let name = alias.id.name.as_str().to_string();
                let resolved = resolve_type_elements_with_ctx_ref(
                    &alias.type_annotation,
                    content_offset,
                    &ctx,
                );
                types.insert(name, resolved);
            }
            Statement::TSInterfaceDeclaration(interface) => {
                let name = interface.id.name.as_str().to_string();
                let extends = extract_heritage_type_names(&interface.extends);
                let mut resolved = ResolvedElements::default();
                let mut guard = vec![name.clone()];
                resolve_interface_with_extends_ctx_ref(
                    &interface.body.body,
                    &extends,
                    content_offset,
                    &mut resolved,
                    &ctx,
                    &mut guard,
                );
                resolved.root_runtime_types = vec![RuntimeType::Object];
                types.insert(name, resolved);
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    match decl {
                        Declaration::TSTypeAliasDeclaration(alias) => {
                            let name = alias.id.name.as_str().to_string();
                            let resolved = resolve_type_elements_with_ctx_ref(
                                &alias.type_annotation,
                                content_offset,
                                &ctx,
                            );
                            types.insert(name, resolved);
                        }
                        Declaration::TSInterfaceDeclaration(interface) => {
                            let name = interface.id.name.as_str().to_string();
                            let extends = extract_heritage_type_names(&interface.extends);
                            let mut resolved = ResolvedElements::default();
                            let mut guard = vec![name.clone()];
                            resolve_interface_with_extends_ctx_ref(
                                &interface.body.body,
                                &extends,
                                content_offset,
                                &mut resolved,
                                &ctx,
                                &mut guard,
                            );
                            resolved.root_runtime_types = vec![RuntimeType::Object];
                            types.insert(name, resolved);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    types
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
    resolve_type_elements_inner(node, base_offset, &mut result, b"");
    result.root_runtime_types = infer_runtime_type(node);
    result
}

/// Resolve type elements with a type resolution context.
/// This version can resolve local type aliases and interfaces.
///
/// # Arguments
/// * `node` - The TSType node to resolve
/// * `base_offset` - The document offset to apply to all spans
/// * `ctx` - Type resolution context with local type definitions
pub fn resolve_type_elements_with_ctx<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner_with_ctx(node, base_offset, &mut result, ctx);
    result.root_runtime_types =
        resolve_root_runtime_type_with_ctx(node, ctx).unwrap_or_else(|| infer_runtime_type(node));
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
pub fn resolve_type_elements_with_ctx_ref<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> ResolvedElements {
    let mut result = ResolvedElements::default();
    resolve_type_elements_inner_with_ctx_ref(node, base_offset, &mut result, ctx);
    result.root_runtime_types = resolve_root_runtime_type_with_ctx_ref(node, ctx)
        .unwrap_or_else(|| infer_runtime_type(node));
    result
}

fn inferred_root_runtime_type_for_companion(companion: &ResolvedElements) -> Vec<RuntimeType> {
    if !companion.root_runtime_types.is_empty() {
        return companion.root_runtime_types.clone();
    }
    if !companion.props.is_empty() || !companion.emits.is_empty() {
        return vec![RuntimeType::Object];
    }
    if companion.has_call_signature {
        return vec![RuntimeType::Function];
    }
    vec![RuntimeType::Unknown]
}

fn resolve_root_runtime_type_with_ctx<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<Vec<RuntimeType>> {
    match node {
        TSType::TSTypeReference(type_ref) => {
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            if let Some(aliased_type) = ctx.find_type_alias(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx(aliased_type, ctx)
                        .unwrap_or_else(|| infer_runtime_type(aliased_type)),
                );
            }

            if ctx.find_interface(type_name_bytes).is_some() {
                return Some(vec![RuntimeType::Object]);
            }

            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx(constraint, ctx)
                        .unwrap_or_else(|| infer_runtime_type(constraint)),
                );
            }

            ctx.companion_types
                .get(type_name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        TSType::TSTypeQuery(query) => {
            let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name else {
                return None;
            };
            ctx.companion_types
                .get(ident.name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        _ => None,
    }
}

fn resolve_root_runtime_type_with_ctx_ref<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<Vec<RuntimeType>> {
    match node {
        TSType::TSTypeReference(type_ref) => {
            let type_name = get_type_reference_name(&type_ref.type_name);
            let type_name_bytes = type_name.as_bytes();

            if let Some(aliased_type) = ctx.find_type_alias(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx_ref(aliased_type, ctx)
                        .unwrap_or_else(|| infer_runtime_type(aliased_type)),
                );
            }

            if ctx.find_interface(type_name_bytes).is_some() {
                return Some(vec![RuntimeType::Object]);
            }

            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                return Some(
                    resolve_root_runtime_type_with_ctx_ref(constraint, ctx)
                        .unwrap_or_else(|| infer_runtime_type(constraint)),
                );
            }

            ctx.companion_types
                .get(type_name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        TSType::TSTypeQuery(query) => {
            let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name else {
                return None;
            };
            ctx.companion_types
                .get(ident.name.as_str())
                .map(inferred_root_runtime_type_for_companion)
        }
        _ => None,
    }
}

fn resolve_type_elements_inner(
    node: &TSType,
    base_offset: u32,
    result: &mut ResolvedElements,
    source: &[u8],
) {
    match node {
        // { prop: Type }
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members(&lit.members, base_offset, result, source);
        }

        // Parenthesized: (Type)
        TSType::TSParenthesizedType(paren) => {
            resolve_type_elements_inner(&paren.type_annotation, base_offset, result, source);
        }

        // Union: Type1 | Type2
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                resolve_type_elements_inner(ty, base_offset, result, source);
            }
            result.dedup_props();
        }

        // Intersection: Type1 & Type2
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                resolve_type_elements_inner(ty, base_offset, result, source);
            }
            result.dedup_props();
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

/// Resolve an interface including its extends clauses using mutable context.
/// Recursion guard prevents infinite loops from circular extends.
fn resolve_interface_with_extends_ctx<'ctx, 'a: 'ctx>(
    members: &[TSSignature],
    extends: &[String],
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    // Resolve own members
    resolve_type_literal_members(members, base_offset, result, ctx.source);

    // Resolve extends
    for base_name in extends {
        if recursion_guard.contains(base_name) {
            continue; // Avoid infinite recursion
        }
        recursion_guard.push(base_name.clone());

        let base_bytes = base_name.as_bytes();

        // Check local type aliases
        if let Some(aliased_type) = ctx.find_type_alias(base_bytes) {
            resolve_type_elements_inner_with_ctx(aliased_type, base_offset, result, ctx);
        }
        // Check local interfaces (need to clone extends to avoid borrow conflict)
        else if let Some((iface_members, iface_extends)) = ctx.find_interface(base_bytes) {
            let iface_extends_owned: Vec<String> = iface_extends.to_vec();
            resolve_interface_with_extends_ctx(
                iface_members,
                &iface_extends_owned,
                base_offset,
                result,
                ctx,
                recursion_guard,
            );
        }
        // Check companion types
        else if let Some(companion) = ctx.companion_types.get(base_name.as_str()) {
            result.props.extend(companion.props.iter().cloned());
            result.emits.extend(companion.emits.iter().cloned());
            if companion.has_call_signature {
                result.has_call_signature = true;
            }
        }

        recursion_guard.pop();
    }
}

/// Resolve an interface including its extends clauses using immutable context.
fn resolve_interface_with_extends_ctx_ref<'ctx, 'a: 'ctx>(
    members: &[TSSignature],
    extends: &[String],
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    // Resolve own members
    resolve_type_literal_members(members, base_offset, result, ctx.source);

    // Resolve extends
    for base_name in extends {
        if recursion_guard.contains(base_name) {
            continue;
        }
        recursion_guard.push(base_name.clone());

        let base_bytes = base_name.as_bytes();

        if let Some(aliased_type) = ctx.find_type_alias(base_bytes) {
            resolve_type_elements_inner_with_ctx_ref(aliased_type, base_offset, result, ctx);
        } else if let Some((iface_members, iface_extends)) = ctx.find_interface(base_bytes) {
            let iface_extends_owned: Vec<String> = iface_extends.to_vec();
            resolve_interface_with_extends_ctx_ref(
                iface_members,
                &iface_extends_owned,
                base_offset,
                result,
                ctx,
                recursion_guard,
            );
        } else if let Some(companion) = ctx.companion_types.get(base_name.as_str()) {
            result.props.extend(companion.props.iter().cloned());
            result.emits.extend(companion.emits.iter().cloned());
            if companion.has_call_signature {
                result.has_call_signature = true;
            }
        }

        recursion_guard.pop();
    }
}

/// Inner resolution function that uses the context for type reference lookup.
fn resolve_type_elements_inner_with_ctx<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
) {
    match node {
        // { prop: Type }
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members(&lit.members, base_offset, result, ctx.source);
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
            result.dedup_props();
        }

        // Intersection: Type1 & Type2
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                resolve_type_elements_inner_with_ctx(ty, base_offset, result, ctx);
            }
            result.dedup_props();
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

            // 2. Check local interfaces (with extends support)
            if let Some((interface_members, iface_extends)) = ctx.find_interface(type_name_bytes) {
                let extends_owned: Vec<String> = iface_extends.to_vec();
                let mut guard = vec![type_name.clone()];
                resolve_interface_with_extends_ctx(
                    interface_members,
                    &extends_owned,
                    base_offset,
                    result,
                    ctx,
                    &mut guard,
                );
                return;
            }

            // 3. Check generic type parameter constraints
            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                resolve_type_elements_inner_with_ctx(constraint, base_offset, result, ctx);
                return;
            }

            // 4. Check companion <script> block's pre-resolved types
            if let Some(companion) = ctx.companion_types.get(type_name.as_str()) {
                result.props.extend(companion.props.iter().cloned());
                result.emits.extend(companion.emits.iter().cloned());
                if companion.has_call_signature {
                    result.has_call_signature = true;
                }
                return;
            }

            // 5. Couldn't resolve - add diagnostic
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

        // Type query: typeof X — look up in companion types
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                let type_name = ident.name.as_str();
                if let Some(companion) = ctx.companion_types.get(type_name) {
                    result.props.extend(companion.props.iter().cloned());
                    result.emits.extend(companion.emits.iter().cloned());
                    if companion.has_call_signature {
                        result.has_call_signature = true;
                    }
                }
            }
        }

        // Function type: () => Type
        TSType::TSFunctionType(_) => {
            result.has_call_signature = true;
        }

        _ => {}
    }
}

/// Inner resolution function that uses an immutable context (doesn't collect diagnostics).
fn resolve_type_elements_inner_with_ctx_ref<'ctx, 'a: 'ctx>(
    node: &'ctx TSType<'a>,
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) {
    match node {
        // { prop: Type }
        TSType::TSTypeLiteral(lit) => {
            resolve_type_literal_members(&lit.members, base_offset, result, ctx.source);
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
            result.dedup_props();
        }

        // Intersection: Type1 & Type2
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                resolve_type_elements_inner_with_ctx_ref(ty, base_offset, result, ctx);
            }
            result.dedup_props();
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

            // 2. Check local interfaces (with extends support)
            if let Some((interface_members, iface_extends)) = ctx.find_interface(type_name_bytes) {
                let extends_owned: Vec<String> = iface_extends.to_vec();
                let mut guard = vec![type_name.clone()];
                resolve_interface_with_extends_ctx_ref(
                    interface_members,
                    &extends_owned,
                    base_offset,
                    result,
                    ctx,
                    &mut guard,
                );
                return;
            }

            // 3. Check generic type parameter constraints
            if let Some(constraint) = ctx.find_type_param(type_name_bytes) {
                resolve_type_elements_inner_with_ctx_ref(constraint, base_offset, result, ctx);
                return;
            }

            // 4. Check companion <script> block's pre-resolved types
            if let Some(companion) = ctx.companion_types.get(type_name.as_str()) {
                result.props.extend(companion.props.iter().cloned());
                result.emits.extend(companion.emits.iter().cloned());
                if companion.has_call_signature {
                    result.has_call_signature = true;
                }
            }

            // 5. Couldn't resolve - skip silently (no diagnostics in immutable version)
        }

        // Type query: typeof X — look up in companion types
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                let type_name = ident.name.as_str();
                if let Some(companion) = ctx.companion_types.get(type_name) {
                    result.props.extend(companion.props.iter().cloned());
                    result.emits.extend(companion.emits.iter().cloned());
                    if companion.has_call_signature {
                        result.has_call_signature = true;
                    }
                }
            }
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
    source: &[u8],
) {
    for member in members {
        match member {
            TSSignature::TSPropertySignature(prop) => {
                // Check if this is a shorthand emit: { change: [id: number] }
                // Properties with tuple/array type values are treated as emits
                if let Some(emit) = resolve_property_as_emit(prop, base_offset, source) {
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
                if let Some(emit) = resolve_call_signature_as_emit(call_sig, base_offset, source) {
                    result.emits.push(emit);
                }
            }
            _ => {}
        }
    }
}

/// Try to resolve a property signature as an emit (shorthand style).
/// Shorthand style: `{ change: [id: number] }` or `{ update: [] }`
fn resolve_property_as_emit(
    prop: &TSPropertySignature,
    base_offset: u32,
    source: &[u8],
) -> Option<ResolvedEmit> {
    // Get the property key as the event name
    let name = get_property_key_name(&prop.key)?;
    let key_span = get_property_key_span(&prop.key, base_offset)?;

    // Check if the type is a tuple type - this indicates emit shorthand
    // Note: Only TSTupleType (e.g., `[id: number]`) is emit shorthand.
    // TSArrayType (e.g., `string[]`) is a regular array prop type.
    if let Some(ann) = &prop.type_annotation {
        if let TSType::TSTupleType(_) = &ann.type_annotation {
            let tuple_text = slice_source_span(
                source,
                ann.type_annotation.span().start,
                ann.type_annotation.span().end,
            )?;
            return Some(ResolvedEmit {
                span: Span {
                    start: prop.span.start + base_offset,
                    end: prop.span.end + base_offset,
                },
                name,
                name_span: Some(key_span),
                signature: ResolvedEmitSignature::Tuple { tuple_text },
                map_local: true,
                span_is_absolute: base_offset != 0,
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
    source: &[u8],
) -> Option<ResolvedEmit> {
    // Get the first parameter - should be like `e: 'eventName'`
    let first_param = call_sig.params.items.first()?;

    // The type annotation is on the FormalParameter, not the pattern
    let type_ann = first_param.type_annotation.as_ref()?;

    // Extract event name from string literal type
    if let TSType::TSLiteralType(lit) = &type_ann.type_annotation {
        if let TSLiteral::StringLiteral(s) = &lit.literal {
            let mut params_text = String::new();
            for param in call_sig.params.items.iter().skip(1) {
                if !params_text.is_empty() {
                    params_text.push_str(", ");
                }
                params_text.push_str(&slice_source_span(
                    source,
                    param.span().start,
                    param.span().end,
                )?);
            }
            if let Some(rest) = &call_sig.params.rest {
                if !params_text.is_empty() {
                    params_text.push_str(", ");
                }
                params_text.push_str(&slice_source_span(source, rest.span.start, rest.span.end)?);
            }
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
                signature: ResolvedEmitSignature::Call { params_text },
                map_local: true,
                span_is_absolute: base_offset != 0,
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

fn slice_source_span(source: &[u8], start: u32, end: u32) -> Option<String> {
    let start = start as usize;
    let end = end as usize;
    if end > source.len() || start > end {
        return None;
    }
    std::str::from_utf8(&source[start..end])
        .ok()
        .map(|s| s.trim().to_string())
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

    let type_span = prop.type_annotation.as_ref().map(|ann| Span {
        start: ann.type_annotation.span().start + base_offset,
        end: ann.type_annotation.span().end + base_offset,
    });

    Some(ResolvedProp {
        span,
        key,
        key_name: get_property_key_name(&prop.key),
        optional,
        types,
        type_span,
        map_local: true,
        span_is_absolute: base_offset != 0,
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
        key_name: get_property_key_name(&method.key),
        optional,
        types: vec![RuntimeType::Function],
        type_span: None,
        map_local: true,
        span_is_absolute: base_offset != 0,
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

        // Conditional type: T extends U ? X : Y — union both branches
        TSType::TSConditionalType(cond) => {
            let mut types = infer_runtime_type(&cond.true_type);
            for t in infer_runtime_type(&cond.false_type) {
                if !types.contains(&t) {
                    types.push(t);
                }
            }
            if types.is_empty() {
                vec![RuntimeType::Unknown]
            } else {
                types
            }
        }

        // Mapped type: { [K in keyof T]: T[K] }
        TSType::TSMappedType(_) => vec![RuntimeType::Object],

        // Indexed access: T[K]
        TSType::TSIndexedAccessType(_) => vec![RuntimeType::Unknown],

        // Template literal type: `${string}`
        TSType::TSTemplateLiteralType(_) => vec![RuntimeType::String],

        // Type query: typeof x — in defineProps context, always refers to an object shape
        TSType::TSTypeQuery(_) => vec![RuntimeType::Object],

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

/// Extract type names from interface heritage/extends clauses.
fn extract_heritage_type_names(extends: &[TSInterfaceHeritage]) -> Vec<String> {
    extends
        .iter()
        .filter_map(|heritage| match &heritage.expression {
            Expression::Identifier(id) => Some(id.name.to_string()),
            _ => None,
        })
        .collect()
}

/// Get the name from a type reference's type name.
///
/// For qualified names like `Namespace.Props`, returns the full path
/// (`"Namespace.Props"`) by recursively walking the left side.
fn get_type_reference_name(type_name: &TSTypeName) -> String {
    match type_name {
        TSTypeName::IdentifierReference(id) => id.name.to_string(),
        TSTypeName::QualifiedName(qualified) => {
            let left = get_type_reference_name(&qualified.left);
            format!("{}.{}", left, qualified.right.name)
        }
        TSTypeName::ThisExpression(_) => "this".to_string(),
    }
}

/// Resolve a value declaration's type shape (for `typeof X` support).
///
/// Looks for variable declarations matching `type_name` in both exported and
/// non-exported positions. If the variable has a type annotation, resolves that.
/// Otherwise, if it has an object literal initializer, infers prop types from
/// the property values.
fn resolve_value_declaration_type<'a>(
    type_name: &str,
    program: &Program<'a>,
    source_bytes: &[u8],
    base_offset: u32,
    ctx: &TypeResolutionContext<'_, 'a>,
) -> Option<ResolvedElements> {
    let name_bytes = type_name.as_bytes();

    for stmt in &program.body {
        // Check both `export const X` and plain `const X`
        let var_decl = match stmt {
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(Declaration::VariableDeclaration(decl)) => Some(decl.as_ref()),
                _ => None,
            },
            Statement::VariableDeclaration(decl) => Some(decl.as_ref()),
            _ => None,
        };

        if let Some(decl) = var_decl {
            for declarator in &decl.declarations {
                let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                    continue;
                };
                if id.name.as_bytes() != name_bytes {
                    continue;
                }

                // 1. Type annotation on the declarator: `const X: { foo: string } = ...`
                if let Some(ref annotation) = declarator.type_annotation {
                    return Some(resolve_type_elements_with_ctx_ref(
                        &annotation.type_annotation,
                        base_offset,
                        ctx,
                    ));
                }

                // 2. Object literal initializer: `const X = { foo: 'str', bar: 42 }`
                if let Some(Expression::ObjectExpression(obj)) = &declarator.init {
                    return Some(infer_props_from_object_literal(obj, source_bytes));
                }
            }
        }
    }

    None
}

/// Infer prop types from an object literal's property values.
fn infer_props_from_object_literal(
    obj: &oxc_ast::ast::ObjectExpression<'_>,
    _source_bytes: &[u8],
) -> ResolvedElements {
    let mut result = ResolvedElements {
        root_runtime_types: vec![RuntimeType::Object],
        ..ResolvedElements::default()
    };

    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let key_span: oxc_span::Span = p.key.span();
        let runtime_type = match &p.value {
            Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => {
                vec![RuntimeType::String]
            }
            Expression::NumericLiteral(_) => vec![RuntimeType::Number],
            Expression::BooleanLiteral(_) => vec![RuntimeType::Boolean],
            Expression::ArrayExpression(_) => vec![RuntimeType::Array],
            Expression::ObjectExpression(_) => vec![RuntimeType::Object],
            Expression::NullLiteral(_) => vec![RuntimeType::Null],
            _ => vec![RuntimeType::Unknown],
        };

        result.props.push(ResolvedProp {
            span: crate::common::Span::new(key_span.start, key_span.end),
            key: crate::common::Span::new(key_span.start, key_span.end),
            key_name: None,
            types: runtime_type,
            optional: false,
            type_span: None,
            map_local: true,
            span_is_absolute: false,
        });
    }

    result
}

/// Resolve an imported type by name from a dependency file's source.
///
/// Parses the dep file, builds a type resolution context, finds the named type
/// (interface or type alias), and resolves it to structured property/emit information.
///
/// Returns `None` if the file can't be parsed or the named type isn't found.
pub fn resolve_external_type(
    type_name: &str,
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
) -> Option<ResolvedElements> {
    resolve_external_type_with_companion(type_name, dep_source, &FxHashMap::default(), allocator)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTypeBinding {
    pub local_name: String,
    pub imported_name: String,
    pub source: String,
}

pub fn extract_imported_type_bindings(
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
) -> Vec<ImportedTypeBinding> {
    let source_type = oxc_span::SourceType::ts();
    let parsed = oxc_parser::Parser::new(allocator, dep_source, source_type).parse();

    if parsed.panicked {
        return Vec::new();
    }

    let mut bindings = Vec::new();
    for stmt in &parsed.program.body {
        let Statement::ImportDeclaration(import_decl) = stmt else {
            continue;
        };
        let Some(specifiers) = &import_decl.specifiers else {
            continue;
        };
        for specifier in specifiers {
            let ImportDeclarationSpecifier::ImportSpecifier(import_spec) = specifier else {
                continue;
            };
            bindings.push(ImportedTypeBinding {
                local_name: import_spec.local.name.to_string(),
                imported_name: import_spec.imported.name().to_string(),
                source: import_decl.source.value.to_string(),
            });
        }
    }

    bindings
}

pub fn resolve_external_type_with_companion(
    type_name: &str,
    dep_source: &str,
    companion_types: &FxHashMap<String, ResolvedElements>,
    allocator: &oxc_allocator::Allocator,
) -> Option<ResolvedElements> {
    let source_type = oxc_span::SourceType::ts();
    let parsed = oxc_parser::Parser::new(allocator, dep_source, source_type).parse();

    if parsed.panicked {
        return None;
    }

    let source_bytes = dep_source.as_bytes();
    let mut ctx = build_type_context(&parsed.program, source_bytes, 0);
    for (name, resolved) in companion_types {
        ctx.companion_types
            .entry(name.clone())
            .or_insert_with(|| resolved.clone());
    }

    let name_bytes = type_name.as_bytes();

    let mut result = None;

    // Try type alias first
    if let Some(ts_type) = ctx.find_type_alias(name_bytes) {
        result = Some(resolve_type_elements_with_ctx_ref(ts_type, 0, &ctx));
    }

    // Try interface (with extends support)
    if result.is_none() {
        if let Some((members, extends)) = ctx.find_interface(name_bytes) {
            let mut r = ResolvedElements::default();
            let extends_owned: Vec<String> = extends.to_vec();
            let mut guard = vec![type_name.to_string()];
            resolve_interface_with_extends_ctx_ref(
                members,
                &extends_owned,
                0,
                &mut r,
                &ctx,
                &mut guard,
            );
            r.root_runtime_types = vec![RuntimeType::Object];
            result = Some(r);
        }
    }

    // Try exported variable declarations: `export const X: { prop: Type } = ...`
    // or non-exported `const X: { prop: Type } = ...` (for `typeof X`)
    if result.is_none() {
        result = resolve_value_declaration_type(type_name, &parsed.program, source_bytes, 0, &ctx);
    }

    // Populate key_name on all props since spans reference the external file,
    // not the consuming SFC. Consumers use key_name when available.
    result.map(|resolved| finalize_external_resolution(resolved, source_bytes))
}

fn finalize_external_resolution(
    mut resolved: ResolvedElements,
    source_bytes: &[u8],
) -> ResolvedElements {
    for prop in &mut resolved.props {
        let start = prop.key.start as usize;
        let end = prop.key.end as usize;
        if prop.key_name.is_none() && start < end && end <= source_bytes.len() {
            if let Ok(name) = std::str::from_utf8(&source_bytes[start..end]) {
                prop.key_name = Some(name.to_string());
            }
        }
        prop.map_local = false;
        prop.span_is_absolute = false;
    }
    for emit in &mut resolved.emits {
        emit.map_local = false;
        emit.span_is_absolute = false;
    }

    resolved
}

/// Hash the resolved type shape for cache comparison (SHA-256, truncated to 16 bytes).
///
/// Produces a stable hash from prop names + runtime types + optional flags + emits.
/// Two different source texts that resolve to the same prop shape produce the same hash.
///
/// # Arguments
/// * `resolved` - The resolved type elements
/// * `source` - Source bytes needed to extract prop key names from spans
pub fn hash_resolved_type(resolved: &ResolvedElements, source: &[u8]) -> [u8; 16] {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();

    // Hash props sorted by key name for stability
    let mut props: Vec<_> = resolved
        .props
        .iter()
        .map(|p| {
            let key_name = &source[p.key.start as usize..p.key.end as usize];
            let mut runtime_types: Vec<&str> = p.types.iter().map(|t| t.as_str()).collect();
            runtime_types.sort();
            (key_name, runtime_types, p.optional)
        })
        .collect();
    props.sort_by_key(|(name, _, _)| *name);

    hasher.update((props.len() as u32).to_le_bytes());
    for (name, types, optional) in &props {
        hasher.update((name.len() as u32).to_le_bytes());
        hasher.update(name);
        hasher.update((types.len() as u32).to_le_bytes());
        for t in types {
            hasher.update(t.as_bytes());
        }
        hasher.update([*optional as u8]);
    }

    // Hash emits sorted by name
    let mut emits: Vec<&str> = resolved.emits.iter().map(|e| e.name.as_str()).collect();
    emits.sort();

    hasher.update((emits.len() as u32).to_le_bytes());
    for name in &emits {
        hasher.update(name.as_bytes());
    }

    hasher.update([resolved.has_call_signature as u8]);

    let hash = hasher.finalize();
    let mut result = [0u8; 16];
    result.copy_from_slice(&hash[..16]);
    result
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

    // ═══════════════════════════════════════════════════════════
    // Cross-file type resolution (Tier 3)
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn resolve_external_type_interface() {
        let alloc = Allocator::default();
        let dep = "export interface Props { foo: string; bar: number }";
        let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
        assert_eq!(resolved.props.len(), 2);
    }

    #[test]
    fn resolve_external_type_alias() {
        let alloc = Allocator::default();
        let dep = "export type Props = { count: number; label?: string }";
        let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
        assert_eq!(resolved.props.len(), 2);
        let optional_count = resolved.props.iter().filter(|p| p.optional).count();
        assert_eq!(optional_count, 1);
    }

    #[test]
    fn resolve_external_type_alias_preserves_primitive_root_runtime_type() {
        let alloc = Allocator::default();
        let dep = "export type Props = string";
        let resolved = resolve_external_type("Props", dep, &alloc).unwrap();

        assert_eq!(resolved.props.len(), 0);
        assert_eq!(resolved.root_runtime_types, vec![RuntimeType::String]);
    }

    #[test]
    fn resolve_external_type_empty_interface_is_object_like() {
        let alloc = Allocator::default();
        let dep = "export interface Props {}";
        let resolved = resolve_external_type("Props", dep, &alloc).unwrap();

        assert_eq!(resolved.props.len(), 0);
        assert_eq!(resolved.root_runtime_types, vec![RuntimeType::Object]);
    }

    #[test]
    fn resolve_external_type_not_found() {
        let alloc = Allocator::default();
        let dep = "export interface Other { x: string }";
        assert!(resolve_external_type("Props", dep, &alloc).is_none());
    }

    #[test]
    fn resolve_external_type_non_exported_still_found() {
        let alloc = Allocator::default();
        // build_type_context collects both exported and non-exported declarations
        let dep = "interface Props { name: string }";
        let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
        assert_eq!(resolved.props.len(), 1);
    }

    #[test]
    fn resolve_external_type_with_intersection() {
        let alloc = Allocator::default();
        let dep = r#"
type A = { foo: string };
type B = { bar: number };
export type Props = A & B;
"#;
        let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
        assert_eq!(resolved.props.len(), 2);
    }

    #[test]
    fn resolve_external_type_parse_error_returns_none() {
        let alloc = Allocator::default();
        let dep = "export interface { broken syntax";
        // Should not panic, just return None
        assert!(resolve_external_type("Props", dep, &alloc).is_none());
    }

    #[test]
    fn hash_resolved_type_stable_across_formatting() {
        let alloc1 = Allocator::default();
        let dep1 = "export interface Props { foo: string; bar: number }";
        let resolved1 = resolve_external_type("Props", dep1, &alloc1).unwrap();
        let hash1 = hash_resolved_type(&resolved1, dep1.as_bytes());

        let alloc2 = Allocator::default();
        // Same interface with different whitespace
        let dep2 = "export interface Props {\n  foo: string;\n  bar: number;\n}";
        let resolved2 = resolve_external_type("Props", dep2, &alloc2).unwrap();
        let hash2 = hash_resolved_type(&resolved2, dep2.as_bytes());

        assert_eq!(hash1, hash2, "Same prop shape should produce same hash");
    }

    #[test]
    fn hash_resolved_type_differs_on_prop_added() {
        let alloc1 = Allocator::default();
        let dep1 = "export interface Props { foo: string }";
        let resolved1 = resolve_external_type("Props", dep1, &alloc1).unwrap();
        let hash1 = hash_resolved_type(&resolved1, dep1.as_bytes());

        let alloc2 = Allocator::default();
        let dep2 = "export interface Props { foo: string; bar: number }";
        let resolved2 = resolve_external_type("Props", dep2, &alloc2).unwrap();
        let hash2 = hash_resolved_type(&resolved2, dep2.as_bytes());

        assert_ne!(
            hash1, hash2,
            "Different prop count should produce different hash"
        );
    }

    #[test]
    fn hash_resolved_type_differs_on_type_changed() {
        let alloc1 = Allocator::default();
        let dep1 = "export interface Props { foo: string }";
        let resolved1 = resolve_external_type("Props", dep1, &alloc1).unwrap();
        let hash1 = hash_resolved_type(&resolved1, dep1.as_bytes());

        let alloc2 = Allocator::default();
        let dep2 = "export interface Props { foo: number }";
        let resolved2 = resolve_external_type("Props", dep2, &alloc2).unwrap();
        let hash2 = hash_resolved_type(&resolved2, dep2.as_bytes());

        assert_ne!(hash1, hash2, "Different type should produce different hash");
    }

    #[test]
    fn hash_resolved_type_differs_on_optional_changed() {
        let alloc1 = Allocator::default();
        let dep1 = "export interface Props { foo: string }";
        let resolved1 = resolve_external_type("Props", dep1, &alloc1).unwrap();
        let hash1 = hash_resolved_type(&resolved1, dep1.as_bytes());

        let alloc2 = Allocator::default();
        let dep2 = "export interface Props { foo?: string }";
        let resolved2 = resolve_external_type("Props", dep2, &alloc2).unwrap();
        let hash2 = hash_resolved_type(&resolved2, dep2.as_bytes());

        assert_ne!(
            hash1, hash2,
            "Optional change should produce different hash"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // Interface extends / heritage clause tests
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - interface B extends A should include A's props
    #[test]
    fn interface_extends_single() {
        let allocator = Allocator::default();
        let source = r#"interface A { foo: string }
interface B extends A { bar: number }
type Test = B;"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);

        for stmt in &result.program.body {
            if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                if alias.id.name.as_str() == "Test" {
                    let resolved =
                        resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
                    assert_eq!(
                        resolved.props.len(),
                        2,
                        "B extends A should have 2 props (foo + bar)"
                    );
                    assert!(ctx.diagnostics.is_empty());
                }
            }
        }
    }

    /// @ai-generated - interface extends multiple bases
    #[test]
    fn interface_extends_multiple() {
        let allocator = Allocator::default();
        let source = r#"interface A { foo: string }
interface B { bar: number }
interface C extends A, B { baz: boolean }
type Test = C;"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);

        for stmt in &result.program.body {
            if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                if alias.id.name.as_str() == "Test" {
                    let resolved =
                        resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
                    assert_eq!(
                        resolved.props.len(),
                        3,
                        "C extends A, B should have 3 props (foo + bar + baz)"
                    );
                    assert!(ctx.diagnostics.is_empty());
                }
            }
        }
    }

    /// @ai-generated - deep interface extends chain: C extends B extends A
    #[test]
    fn interface_extends_deep_chain() {
        let allocator = Allocator::default();
        let source = r#"interface A { a: string }
interface B extends A { b: number }
interface C extends B { c: boolean }
type Test = C;"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);

        for stmt in &result.program.body {
            if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                if alias.id.name.as_str() == "Test" {
                    let resolved =
                        resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
                    assert_eq!(
                        resolved.props.len(),
                        3,
                        "C extends B extends A should have 3 props (a + b + c)"
                    );
                    assert!(ctx.diagnostics.is_empty());
                }
            }
        }
    }

    /// @ai-generated - interface extends with companion types
    #[test]
    fn interface_extends_companion() {
        let allocator = Allocator::default();
        let source = r#"interface Local extends Base { own: string }
type Test = Local;"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);

        // Simulate companion type
        let mut base_resolved = ResolvedElements::default();
        base_resolved.props.push(ResolvedProp {
            span: Span { start: 0, end: 0 },
            key: Span { start: 0, end: 0 },
            key_name: Some("baseField".to_string()),
            optional: false,
            types: vec![RuntimeType::String],
            type_span: None,
            map_local: true,
            span_is_absolute: false,
        });
        ctx.companion_types
            .insert("Base".to_string(), base_resolved);

        for stmt in &result.program.body {
            if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                if alias.id.name.as_str() == "Test" {
                    let resolved =
                        resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
                    assert_eq!(
                        resolved.props.len(),
                        2,
                        "Local extends Base should have 2 props (baseField + own)"
                    );
                    assert!(ctx.diagnostics.is_empty());
                }
            }
        }
    }

    /// @ai-generated - resolve_external_type handles interface extends within same file
    #[test]
    fn resolve_external_type_interface_extends() {
        let alloc = Allocator::default();
        let dep = r#"
export interface Base { foo: string }
export interface Props extends Base { bar: number }
"#;
        let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
        assert_eq!(
            resolved.props.len(),
            2,
            "Props extends Base should have 2 props"
        );
    }

    /// @ai-generated - resolve_external_type handles deep extends chain
    #[test]
    fn resolve_external_type_deep_extends() {
        let alloc = Allocator::default();
        let dep = r#"
interface A { a: string }
interface B extends A { b: number }
export interface Props extends B { c: boolean }
"#;
        let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
        assert_eq!(
            resolved.props.len(),
            3,
            "Props extends B extends A should have 3 props"
        );
    }

    /// @ai-generated - resolve_external_type_with_companion supports imported aliases.
    #[test]
    fn resolve_external_type_with_companion_import_alias() {
        let alloc = Allocator::default();
        let dep = r#"
import type { BaseAction as LocalBase } from './base'

export interface Props extends LocalBase {
  label: string
}
"#;
        let mut companion_types = rustc_hash::FxHashMap::default();
        let mut base = ResolvedElements::default();
        base.props.push(ResolvedProp {
            span: Span::new(0, 0),
            key: Span::new(0, 0),
            key_name: Some("id".to_string()),
            optional: false,
            types: vec![RuntimeType::String],
            type_span: None,
            map_local: false,
            span_is_absolute: false,
        });
        companion_types.insert("LocalBase".to_string(), base);

        let resolved =
            resolve_external_type_with_companion("Props", dep, &companion_types, &alloc).unwrap();
        assert_eq!(
            resolved.props.len(),
            2,
            "Props should include both imported base props and local props"
        );
        assert!(resolved
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("id")));
        assert!(resolved
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("label")));
    }

    /// @ai-generated - resolve_external_type_with_companion supports transitive imported emits shapes.
    #[test]
    fn resolve_external_type_with_companion_transitive_emits_shape() {
        let alloc = Allocator::default();
        let dep = r#"
import type { BaseEmits } from './base'

export interface Emits extends BaseEmits {
  confirm: [id: number]
}
"#;
        let mut companion_types = rustc_hash::FxHashMap::default();
        let mut base = ResolvedElements::default();
        base.emits.push(ResolvedEmit {
            span: Span::new(0, 0),
            name: "submit".to_string(),
            name_span: None,
            signature: ResolvedEmitSignature::Call {
                params_text: "payload: string".to_string(),
            },
            map_local: false,
            span_is_absolute: false,
        });
        companion_types.insert("BaseEmits".to_string(), base);

        let resolved =
            resolve_external_type_with_companion("Emits", dep, &companion_types, &alloc).unwrap();
        assert_eq!(
            resolved.emits.len(),
            2,
            "Emits should include imported and local emits entries"
        );
        assert!(resolved.emits.iter().any(|emit| emit.name == "submit"));
        assert!(resolved.emits.iter().any(|emit| emit.name == "confirm"));
    }

    /// @ai-generated - extract_companion_types handles interface extends
    #[test]
    fn companion_types_interface_extends() {
        let allocator = Allocator::default();
        let source = r#"interface Base { base: string }
interface Extended extends Base { own: number }"#;
        let source_type = SourceType::ts();
        let parser = Parser::new(&allocator, source, source_type);
        let result = parser.parse();

        let types = extract_companion_types(&result.program, source.as_bytes(), 0);

        let extended = types.get("Extended").unwrap();
        assert_eq!(
            extended.props.len(),
            2,
            "Extended should include base + own props"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // Union/Intersection deduplication tests
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - intersection of types with shared props should deduplicate
    #[test]
    fn resolve_intersection_deduplicates_shared_props() {
        let parsed = parse_type("{ x: string; y: number } & { x: string; z: boolean }").unwrap();
        // x appears in both branches — should appear only once
        let x_count = parsed
            .resolved
            .props
            .iter()
            .filter(|p| parsed.key_name(p) == "x")
            .count();
        assert_eq!(
            x_count, 1,
            "Intersection should deduplicate shared prop 'x'"
        );
        assert_eq!(
            parsed.resolved.props.len(),
            3,
            "Should have 3 unique props: x, y, z"
        );
    }

    /// @ai-generated - union of types with shared props should deduplicate
    #[test]
    fn resolve_union_deduplicates_shared_props() {
        let parsed = parse_type("{ x: string; y: number } | { x: string; z: boolean }").unwrap();
        let x_count = parsed
            .resolved
            .props
            .iter()
            .filter(|p| parsed.key_name(p) == "x")
            .count();
        assert_eq!(x_count, 1, "Union should deduplicate shared prop 'x'");
        assert_eq!(
            parsed.resolved.props.len(),
            3,
            "Should have 3 unique props: x, y, z"
        );
    }

    /// @ai-generated - mixed union and intersection with overlapping props
    #[test]
    fn resolve_intersection_union_combo_deduplicates() {
        let parsed =
            parse_type("({ a: string } | { a: number; b: boolean }) & { a: string; c: number }")
                .unwrap();
        let a_count = parsed
            .resolved
            .props
            .iter()
            .filter(|p| parsed.key_name(p) == "a")
            .count();
        assert_eq!(
            a_count, 1,
            "Combined union+intersection should deduplicate shared prop 'a'"
        );
    }

    /// @ai-generated - intersection dedup with context (type references)
    #[test]
    fn resolve_intersection_dedup_with_context() {
        let allocator = Allocator::default();
        let source = r#"type A = { x: string; y: number };
type B = { x: string; z: boolean };
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
                    assert_eq!(
                        resolved.props.len(),
                        3,
                        "A & B with shared 'x' should have 3 unique props"
                    );
                }
            }
        }
    }

    /// @ai-generated - circular extends doesn't cause infinite recursion
    #[test]
    fn interface_extends_circular_no_panic() {
        let alloc = Allocator::default();
        // This is invalid TS but shouldn't crash the resolver
        let dep = r#"
interface A extends B { a: string }
interface B extends A { b: number }
"#;
        // Should return without panicking
        let resolved = resolve_external_type("A", dep, &alloc).unwrap();
        // Should have at least A's own prop
        assert!(
            !resolved.props.is_empty(),
            "Should resolve at least some props without crashing"
        );
    }
}
