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
    /// Pre-resolved type annotation text for cross-file types.
    /// Set by `finalize_external_resolution` when spans reference an external
    /// source file. Consumers prefer this over extracting from `type_span`.
    pub type_text: Option<String>,
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
#[derive(Debug, Clone)]
pub struct TypeResolutionContext<'ctx, 'a: 'ctx> {
    /// Source bytes for name comparisons
    pub source: &'ctx [u8],
    /// Local type alias declarations: (name_span, type_node, type_params)
    pub type_aliases: Vec<(
        Span,
        &'ctx TSType<'a>,
        Option<&'ctx TSTypeParameterDeclaration<'a>>,
    )>,
    /// Local interface declarations: (name_span, interface_body_members, extends_type_names, heritage_refs)
    /// The extends_type_names are extracted from heritage clauses as String names,
    /// since we need to look them up recursively. Heritage refs are preserved for
    /// utility types like `Pick<T, K>` that need type argument resolution.
    pub interfaces: Vec<(
        Span,
        &'ctx oxc_allocator::Vec<'a, TSSignature<'a>>,
        Vec<String>,
        &'ctx [TSInterfaceHeritage<'a>],
        Option<&'ctx TSTypeParameterDeclaration<'a>>,
    )>,
    /// Generic type parameters with constraints: (name_span, constraint_type)
    pub type_params: Vec<(Span, Option<&'ctx TSType<'a>>)>,
    /// Bound generic type parameters for the current instantiation.
    pub type_param_bindings: Vec<(Span, &'ctx TSType<'a>)>,
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
            type_param_bindings: Vec::new(),
            diagnostics: Vec::new(),
            companion_types: rustc_hash::FxHashMap::default(),
        }
    }

    /// Look up a type alias by comparing spans against source bytes
    pub fn find_type_alias(
        &self,
        name: &[u8],
    ) -> Option<(
        &'ctx TSType<'a>,
        Option<&'ctx TSTypeParameterDeclaration<'a>>,
    )> {
        self.type_aliases
            .iter()
            .find(|(span, _, _)| &self.source[span.start as usize..span.end as usize] == name)
            .map(|(_, ty, params)| (*ty, *params))
    }

    /// Look up an interface by comparing spans against source bytes.
    /// Returns (body_members, extends_type_names).
    pub fn find_interface(
        &self,
        name: &[u8],
    ) -> Option<(
        &'ctx oxc_allocator::Vec<'a, TSSignature<'a>>,
        &[String],
        &'ctx [TSInterfaceHeritage<'a>],
        Option<&'ctx TSTypeParameterDeclaration<'a>>,
    )> {
        self.interfaces
            .iter()
            .find(|(span, _, _, _, _)| &self.source[span.start as usize..span.end as usize] == name)
            .map(|(_, members, extends, heritage, params)| {
                (*members, extends.as_slice(), *heritage, *params)
            })
    }

    /// Look up a type parameter constraint by comparing spans against source bytes
    pub fn find_type_param(&self, name: &[u8]) -> Option<&'ctx TSType<'a>> {
        if let Some(bound) = self
            .type_param_bindings
            .iter()
            .find(|(span, _)| &self.source[span.start as usize..span.end as usize] == name)
        {
            return Some(bound.1);
        }
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
                ctx.type_aliases.push((
                    name_span,
                    &alias.type_annotation,
                    alias.type_parameters.as_deref(),
                ));
            }
            // Collect interfaces: `interface Foo { bar: string }`
            Statement::TSInterfaceDeclaration(interface) => {
                // interface.id.span is already adjusted by adjust_program_spans() to SFC coordinates.
                let name_span = Span::from(interface.id.span);
                let extends = extract_heritage_type_names(&interface.extends);
                ctx.interfaces.push((
                    name_span,
                    &interface.body.body,
                    extends,
                    &interface.extends,
                    interface.type_parameters.as_deref(),
                ));
            }
            // Collect exported type aliases and interfaces:
            // `export type Foo = { bar: string }` / `export interface Foo { bar: string }`
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    match decl {
                        Declaration::TSTypeAliasDeclaration(alias) => {
                            let name_span = Span::from(alias.id.span);
                            ctx.type_aliases.push((
                                name_span,
                                &alias.type_annotation,
                                alias.type_parameters.as_deref(),
                            ));
                        }
                        Declaration::TSInterfaceDeclaration(interface) => {
                            let name_span = Span::from(interface.id.span);
                            let extends = extract_heritage_type_names(&interface.extends);
                            ctx.interfaces.push((
                                name_span,
                                &interface.body.body,
                                extends,
                                &interface.extends,
                                interface.type_parameters.as_deref(),
                            ));
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

fn instantiate_type_params_ctx<'ctx, 'a: 'ctx>(
    ctx: &TypeResolutionContext<'ctx, 'a>,
    decl_params: Option<&'ctx TSTypeParameterDeclaration<'a>>,
    type_args: Option<&'ctx TSTypeParameterInstantiation<'a>>,
) -> TypeResolutionContext<'ctx, 'a> {
    let mut child = ctx.clone();
    child.diagnostics.clear();
    let Some(decl_params) = decl_params else {
        return child;
    };

    for (index, param) in decl_params.params.iter().enumerate() {
        let bound = type_args
            .and_then(|args| args.params.get(index))
            .or(param.default.as_ref())
            .or(param.constraint.as_ref());
        if let Some(bound) = bound {
            child
                .type_param_bindings
                .push((Span::from(param.name.span), bound));
        }
    }

    child
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
                    &interface.extends,
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
                                &interface.extends,
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

            if let Some((aliased_type, _)) = ctx.find_type_alias(type_name_bytes) {
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

            if let Some((aliased_type, _)) = ctx.find_type_alias(type_name_bytes) {
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
    heritage: &'ctx [TSInterfaceHeritage<'a>],
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &mut TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    // Resolve own members
    resolve_type_literal_members(members, base_offset, result, ctx.source);

    // Resolve extends — try heritage AST first for utility type support,
    // then fall back to string-based lookup (matches _ctx_ref variant).
    for (i, base_name) in extends.iter().enumerate() {
        if recursion_guard.contains(base_name) {
            continue; // Avoid infinite recursion
        }
        recursion_guard.push(base_name.clone());

        let base_bytes = base_name.as_bytes();

        // When a heritage clause has type arguments (e.g., `extends Pick<T, 'k'>`),
        // resolve through the utility type dispatch inline.
        if let Some(h) = heritage.get(i) {
            if let Some(type_args) = &h.type_arguments {
                if !type_args.params.is_empty() {
                    let name_str = base_name.as_str();
                    let handled = match name_str {
                        "Pick" if type_args.params.len() >= 2 => {
                            let mut inner = ResolvedElements::default();
                            resolve_type_elements_inner_with_ctx(
                                &type_args.params[0],
                                base_offset,
                                &mut inner,
                                ctx,
                            );
                            let keys = extract_string_literal_keys(&type_args.params[1]);
                            inner
                                .props
                                .retain(|p| p.key_name.as_ref().is_some_and(|n| keys.contains(n)));
                            inner.emits.retain(|e| keys.contains(&e.name));
                            result.props.extend(inner.props);
                            result.emits.extend(inner.emits);
                            true
                        }
                        "Omit" if type_args.params.len() >= 2 => {
                            let mut inner = ResolvedElements::default();
                            resolve_type_elements_inner_with_ctx(
                                &type_args.params[0],
                                base_offset,
                                &mut inner,
                                ctx,
                            );
                            let keys = extract_string_literal_keys(&type_args.params[1]);
                            inner
                                .props
                                .retain(|p| p.key_name.as_ref().is_none_or(|n| !keys.contains(n)));
                            inner.emits.retain(|e| !keys.contains(&e.name));
                            result.props.extend(inner.props);
                            result.emits.extend(inner.emits);
                            true
                        }
                        "Partial" | "Required" | "Readonly" if !type_args.params.is_empty() => {
                            let mut inner = ResolvedElements::default();
                            resolve_type_elements_inner_with_ctx(
                                &type_args.params[0],
                                base_offset,
                                &mut inner,
                                ctx,
                            );
                            result.props.extend(inner.props);
                            result.emits.extend(inner.emits);
                            true
                        }
                        _ => false,
                    };
                    if handled {
                        recursion_guard.pop();
                        continue;
                    }
                }
            }
        }

        // Check local type aliases
        if let Some((aliased_type, _)) = ctx.find_type_alias(base_bytes) {
            resolve_type_elements_inner_with_ctx(aliased_type, base_offset, result, ctx);
        }
        // Check local interfaces (need to clone extends to avoid borrow conflict)
        else if let Some((iface_members, iface_extends, iface_heritage, _)) =
            ctx.find_interface(base_bytes)
        {
            let iface_extends_owned: Vec<String> = iface_extends.to_vec();
            resolve_interface_with_extends_ctx(
                iface_members,
                &iface_extends_owned,
                iface_heritage,
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
    heritage: &'ctx [TSInterfaceHeritage<'a>],
    base_offset: u32,
    result: &mut ResolvedElements,
    ctx: &TypeResolutionContext<'ctx, 'a>,
    recursion_guard: &mut Vec<String>,
) {
    // Resolve own members
    resolve_type_literal_members(members, base_offset, result, ctx.source);

    // Resolve extends — try heritage AST first for utility type support,
    // then fall back to string-based lookup.
    for (i, base_name) in extends.iter().enumerate() {
        if recursion_guard.contains(base_name) {
            continue;
        }
        recursion_guard.push(base_name.clone());

        let base_bytes = base_name.as_bytes();

        // When a heritage clause has type arguments (e.g., `extends Pick<T, 'k'>`),
        // the name-based lookup below won't find it since "Pick" isn't a local type.
        // Resolve the full heritage expression through the type system which handles
        // all TypeScript utility types (Pick, Omit, Partial, Required, Readonly,
        // Record, Extract, Exclude, etc.) in a single code path.
        if let Some(h) = heritage.get(i) {
            if let Some(type_args) = &h.type_arguments {
                if !type_args.params.is_empty() {
                    // Resolve each type_argument through the normal pipeline.
                    // For utility types like Pick<T, K>, the first param is the
                    // source type; filtering/transformation is handled by the
                    // utility type branch in resolve_type_elements_inner_with_ctx_ref
                    // when it encounters the corresponding TSTypeReference node.
                    //
                    // We can't construct a synthetic TSTypeReference here (needs arena),
                    // so we replicate the utility type dispatch inline. This covers the
                    // most common cases; truly complex types may need the JS-side
                    // type registry fallback.
                    let name_str = base_name.as_str();
                    let handled = match name_str {
                        "Pick" if type_args.params.len() >= 2 => {
                            let mut inner = ResolvedElements::default();
                            resolve_type_elements_inner_with_ctx_ref(
                                &type_args.params[0],
                                base_offset,
                                &mut inner,
                                ctx,
                            );
                            let keys = extract_string_literal_keys(&type_args.params[1]);
                            inner
                                .props
                                .retain(|p| p.key_name.as_ref().is_some_and(|n| keys.contains(n)));
                            inner.emits.retain(|e| keys.contains(&e.name));
                            result.props.extend(inner.props);
                            result.emits.extend(inner.emits);
                            true
                        }
                        "Omit" if type_args.params.len() >= 2 => {
                            let mut inner = ResolvedElements::default();
                            resolve_type_elements_inner_with_ctx_ref(
                                &type_args.params[0],
                                base_offset,
                                &mut inner,
                                ctx,
                            );
                            let keys = extract_string_literal_keys(&type_args.params[1]);
                            inner
                                .props
                                .retain(|p| p.key_name.as_ref().is_none_or(|n| !keys.contains(n)));
                            inner.emits.retain(|e| !keys.contains(&e.name));
                            result.props.extend(inner.props);
                            result.emits.extend(inner.emits);
                            true
                        }
                        "Partial" | "Required" | "Readonly" if !type_args.params.is_empty() => {
                            let mut inner = ResolvedElements::default();
                            resolve_type_elements_inner_with_ctx_ref(
                                &type_args.params[0],
                                base_offset,
                                &mut inner,
                                ctx,
                            );
                            if name_str == "Partial" {
                                for p in &mut inner.props {
                                    p.optional = true;
                                }
                            } else if name_str == "Required" {
                                for p in &mut inner.props {
                                    p.optional = false;
                                }
                            }
                            result.props.extend(inner.props);
                            result.emits.extend(inner.emits);
                            true
                        }
                        "Record" if type_args.params.len() >= 2 => {
                            // Record<K, V> — resolve as object type
                            result.root_runtime_types.push(RuntimeType::Object);
                            true
                        }
                        _ => false,
                    };
                    if handled {
                        if result.has_call_signature {
                            result.has_call_signature = true;
                        }
                        recursion_guard.pop();
                        continue;
                    }
                    // Not a recognized utility type — fall through to name-based lookup
                }
            }
        }

        if let Some((aliased_type, _)) = ctx.find_type_alias(base_bytes) {
            resolve_type_elements_inner_with_ctx_ref(aliased_type, base_offset, result, ctx);
        } else if let Some((iface_members, iface_extends, iface_heritage, _)) =
            ctx.find_interface(base_bytes)
        {
            let iface_extends_owned: Vec<String> = iface_extends.to_vec();
            resolve_interface_with_extends_ctx_ref(
                iface_members,
                &iface_extends_owned,
                iface_heritage,
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
            if let Some((aliased_type, type_params)) = ctx.find_type_alias(type_name_bytes) {
                let mut child = instantiate_type_params_ctx(
                    ctx,
                    type_params,
                    type_ref.type_arguments.as_deref(),
                );
                resolve_type_elements_inner_with_ctx(aliased_type, base_offset, result, &mut child);
                ctx.diagnostics.append(&mut child.diagnostics);
                return;
            }

            // 2. Check local interfaces (with extends support)
            if let Some((interface_members, iface_extends, iface_heritage, iface_type_params)) =
                ctx.find_interface(type_name_bytes)
            {
                let extends_owned: Vec<String> = iface_extends.to_vec();
                let mut guard = vec![type_name.clone()];
                let mut child = instantiate_type_params_ctx(
                    ctx,
                    iface_type_params,
                    type_ref.type_arguments.as_deref(),
                );
                resolve_interface_with_extends_ctx(
                    interface_members,
                    &extends_owned,
                    iface_heritage,
                    base_offset,
                    result,
                    &mut child,
                    &mut guard,
                );
                ctx.diagnostics.append(&mut child.diagnostics);
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

            // 5. Handle built-in TypeScript utility types (Omit, Pick, Partial, etc.)
            if let Some(args) = &type_ref.type_arguments {
                match type_name.as_str() {
                    "Omit" if args.params.len() >= 2 => {
                        // Omit<T, K>: resolve T, then remove keys in K
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                        );
                        let keys = extract_string_literal_keys(&args.params[1]);
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_none_or(|n| !keys.contains(n)));
                        inner.emits.retain(|e| !keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.emits.extend(inner.emits);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                        return;
                    }
                    "Pick" if args.params.len() >= 2 => {
                        // Pick<T, K>: resolve T, then keep only keys in K
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                        );
                        let keys = extract_string_literal_keys(&args.params[1]);
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_some_and(|n| keys.contains(n)));
                        inner.emits.retain(|e| keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.emits.extend(inner.emits);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                        return;
                    }
                    "Partial" | "Required" | "Readonly" if !args.params.is_empty() => {
                        // These preserve structure, just change modifiers
                        resolve_type_elements_inner_with_ctx(
                            &args.params[0],
                            base_offset,
                            result,
                            ctx,
                        );
                        return;
                    }
                    _ => {}
                }
            }

            // 6. Couldn't resolve - add diagnostic
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
            if let Some((aliased_type, type_params)) = ctx.find_type_alias(type_name_bytes) {
                let child = instantiate_type_params_ctx(
                    ctx,
                    type_params,
                    type_ref.type_arguments.as_deref(),
                );
                resolve_type_elements_inner_with_ctx_ref(aliased_type, base_offset, result, &child);
                return;
            }

            // 2. Check local interfaces (with extends support)
            if let Some((interface_members, iface_extends, iface_heritage, iface_type_params)) =
                ctx.find_interface(type_name_bytes)
            {
                let extends_owned: Vec<String> = iface_extends.to_vec();
                let mut guard = vec![type_name.clone()];
                let child = instantiate_type_params_ctx(
                    ctx,
                    iface_type_params,
                    type_ref.type_arguments.as_deref(),
                );
                resolve_interface_with_extends_ctx_ref(
                    interface_members,
                    &extends_owned,
                    iface_heritage,
                    base_offset,
                    result,
                    &child,
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
                return;
            }

            // 5. Handle built-in TypeScript utility types (Omit, Pick, Partial, etc.)
            if let Some(args) = &type_ref.type_arguments {
                match type_name.as_str() {
                    "Omit" if args.params.len() >= 2 => {
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx_ref(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                        );
                        let keys = extract_string_literal_keys(&args.params[1]);
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_none_or(|n| !keys.contains(n)));
                        inner.emits.retain(|e| !keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.emits.extend(inner.emits);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                    }
                    "Pick" if args.params.len() >= 2 => {
                        let mut inner = ResolvedElements::default();
                        resolve_type_elements_inner_with_ctx_ref(
                            &args.params[0],
                            base_offset,
                            &mut inner,
                            ctx,
                        );
                        let keys = extract_string_literal_keys(&args.params[1]);
                        inner
                            .props
                            .retain(|p| p.key_name.as_ref().is_some_and(|n| keys.contains(n)));
                        inner.emits.retain(|e| keys.contains(&e.name));
                        result.props.extend(inner.props);
                        result.emits.extend(inner.emits);
                        if inner.has_call_signature {
                            result.has_call_signature = true;
                        }
                    }
                    "Partial" | "Required" | "Readonly" if !args.params.is_empty() => {
                        resolve_type_elements_inner_with_ctx_ref(
                            &args.params[0],
                            base_offset,
                            result,
                            ctx,
                        );
                    }
                    _ => {}
                }
            }

            // 6. Couldn't resolve - skip silently (no diagnostics in immutable version)
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

/// Extract string literal keys from a type argument (supports single literal and unions).
/// Used for `Omit<T, 'a' | 'b'>` and `Pick<T, 'a' | 'b'>`.
fn extract_string_literal_keys(ty: &TSType) -> Vec<String> {
    match ty {
        TSType::TSLiteralType(lit) => {
            if let TSLiteral::StringLiteral(s) = &lit.literal {
                vec![s.value.to_string()]
            } else {
                vec![]
            }
        }
        TSType::TSUnionType(union) => union
            .types
            .iter()
            .flat_map(|t| extract_string_literal_keys(t))
            .collect(),
        _ => vec![],
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
        type_text: None,
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
        type_text: None,
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
            type_text: None,
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

/// Result of extracting type bindings from a dependency file.
/// Includes both named bindings (from `import` and `export {} from`) and
/// wildcard re-export sources (from `export * from`).
#[derive(Debug, Clone, Default)]
pub struct ExtractedTypeBindings {
    pub bindings: Vec<ImportedTypeBinding>,
    pub wildcard_reexport_sources: Vec<String>,
}

pub fn extract_imported_type_bindings(
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
) -> ExtractedTypeBindings {
    let source_type = oxc_span::SourceType::ts();
    let parsed = oxc_parser::Parser::new(allocator, dep_source, source_type).parse();

    if parsed.panicked {
        return ExtractedTypeBindings::default();
    }

    let mut result = ExtractedTypeBindings::default();
    for stmt in &parsed.program.body {
        match stmt {
            Statement::ImportDeclaration(import_decl) => {
                let Some(specifiers) = &import_decl.specifiers else {
                    continue;
                };
                for specifier in specifiers {
                    let ImportDeclarationSpecifier::ImportSpecifier(import_spec) = specifier else {
                        continue;
                    };
                    result.bindings.push(ImportedTypeBinding {
                        local_name: import_spec.local.name.to_string(),
                        imported_name: import_spec.imported.name().to_string(),
                        source: import_decl.source.value.to_string(),
                    });
                }
            }
            Statement::ExportNamedDeclaration(export_decl) => {
                // `export { X } from './Y'` — named re-export with source
                let Some(source) = &export_decl.source else {
                    continue;
                };
                for specifier in &export_decl.specifiers {
                    let local_name = specifier.exported.name().to_string();
                    let imported_name = specifier.local.name().to_string();
                    result.bindings.push(ImportedTypeBinding {
                        local_name,
                        imported_name,
                        source: source.value.to_string(),
                    });
                }
            }
            Statement::ExportAllDeclaration(export_all) => {
                // `export * from './Drawer'` — wildcard re-export
                result
                    .wildcard_reexport_sources
                    .push(export_all.source.value.to_string());
            }
            _ => {}
        }
    }

    result
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
    if let Some((ts_type, _)) = ctx.find_type_alias(name_bytes) {
        result = Some(resolve_type_elements_with_ctx_ref(ts_type, 0, &ctx));
    }

    // Try interface (with extends support)
    if result.is_none() {
        if let Some((members, extends, heritage, _)) = ctx.find_interface(name_bytes) {
            let mut r = ResolvedElements::default();
            let extends_owned: Vec<String> = extends.to_vec();
            let mut guard = vec![type_name.to_string()];
            resolve_interface_with_extends_ctx_ref(
                members,
                &extends_owned,
                heritage,
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

    // Try companion types directly — handles `export { X } from './y'` re-exports
    // where X is not defined in this file but was resolved from the import source.
    if result.is_none() {
        if let Some(companion) = companion_types.get(type_name) {
            result = Some(companion.clone());
        }
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
        // Set type_text from type_span for cross-file props (spans reference
        // this external source, not the consuming SFC). Skip if already set
        // by a previous resolution step (e.g., companion-derived props).
        if prop.type_text.is_none() {
            if let Some(type_span) = prop.type_span {
                let ts = type_span.start as usize;
                let te = type_span.end as usize;
                if ts < te && te <= source_bytes.len() {
                    if let Ok(text) = std::str::from_utf8(&source_bytes[ts..te]) {
                        prop.type_text = Some(text.to_string());
                    }
                }
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
#[path = "resolve_type_tests.rs"]
mod resolve_type_tests;
