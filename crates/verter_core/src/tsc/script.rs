//! TSC codegen — generates minimal TypeScript declaration files for Vue SFCs.
//!
//! Unlike the full compile pipeline, this module performs **macro extraction only**:
//! it OXC-parses the `<script setup>` block to extract `defineProps`, `defineEmits`,
//! `defineModel`, and `defineOptions` calls, then generates a `.tsc.tsx` file that
//! TypeScript can use for type checking.
//!
//! # Output structure
//!
//! ```typescript
//! import { defineComponent } from "vue"
//!
//! // JS runtime options (from macros)
//! const __comp = defineComponent({
//!   name: 'MyComp' as const,
//!   props: { title: String },
//!   emits: ['update:title'],
//! })
//!
//! // TypeScript types (with inline import() syntax — no intermediate aliases)
//! declare const MyComp: typeof __comp & import("vue").ComponentPublicInstance<...> & {
//!   new(): {
//!     $props: import('./types').Props & { ... },
//!     $emit: ((event: 'change', ...args: unknown[]) => void) & ...,
//!   }
//! }
//! export default MyComp
//! //# sourceMappingURL=data:application/json;base64,...
//! ```
//!
//! No template compilation, no setup body, no intermediate type aliases.

use base64::prelude::*;
use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, Statement};
use oxc_ast::{Comment, CommentContent};
use oxc_parser::Parser;
use oxc_sourcemap::SourceMapBuilder;
use oxc_span::{GetSpan, SourceType};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::common::Span;
use crate::cursor::position::PositionResolver;
use crate::diagnostics::{SyntaxPluginContext, SyntaxPluginOptions};
use crate::parser::Syntax;
use crate::template::code_gen::binding::BindingType;
use crate::tokenizer::byte::tokenize_sfc;
use crate::utils::oxc::vue::{
    extract_companion_types, parse_script_with_companion, MacroArrayArg, MacroObjectArg,
    MacroTypeParams, ResolvedElements, ResolvedEmitSignature, RuntimeType, ScriptItem, ScriptMacro,
    ScriptMode,
};

/// Output from the tsc codegen.
pub struct TscOutput {
    /// The generated `.tsc.tsx` source with inline source map.
    pub code: String,
    /// The JSON source map string (without base64 encoding).
    pub source_map: String,
}

/// Output mode for generated TypeScript declaration files.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TscMode {
    /// Public component API only.
    #[default]
    Public,
    /// Testing/debug API that exposes script-setup bindings on the instance.
    Testing,
}

/// Options for tsc codegen.
#[derive(Debug, Default)]
pub struct TscGenOptions {
    /// Experimental: Enable conditional root generic narrowing.
    pub conditional_root_narrowing: bool,
    /// Source filename used in source maps and cross-file type resolution.
    pub filename: Option<String>,
    /// Pre-resolved external macro types, keyed by imported type name.
    pub external_types: Option<rustc_hash::FxHashMap<String, ResolvedElements>>,
    /// Public or testing/debug output mode.
    pub mode: TscMode,
}

// ── Internal state ───────────────────────────────────────────────────────────

/// TypeScript type representation for props (from `defineProps`).
enum PropsTs {
    /// Type reference (name only) — from `defineProps<ImportedType>()`.
    /// The corresponding `import type { ... }` is in `TscMacroState::type_import_stmts`.
    TypeRef(String),
    /// Raw TypeScript type text — for unresolved or inline complex types
    TypeText(String),
    /// Resolved property list — from object-syntax or inline type literal
    Inline(Vec<InlinePropEntry>),
}

struct InlinePropEntry {
    name: String,
    optional: bool,
    ts_type: String,
    comment: Option<String>,
    map_span: Option<Span>,
}

struct EmitEntry {
    name: String,
    payload: EmitPayload,
    map_span: Option<Span>,
}

enum EmitPayload {
    Unknown,
    Call { params_text: String },
    Tuple { tuple_text: String },
}

struct ModelEntry {
    name: String,
    ts_type: String,
    map_span: Option<Span>,
}

struct TestingPropBinding {
    name: String,
    ts_type: String,
    optional: bool,
    map_span: Option<Span>,
}

struct TestBindingEntry {
    name: String,
    binding_type: BindingType,
    map_span: Option<Span>,
}

struct LocalTypeDecl<'a> {
    name: &'a str,
    text: &'a str,
}

#[derive(Clone, Copy)]
struct GeneratedMapping {
    generated_offset: usize,
    source_span: Span,
}

#[derive(Default)]
struct RenderedText {
    text: String,
    mappings: Vec<GeneratedMapping>,
}

impl RenderedText {
    fn push_str(&mut self, text: &str) {
        self.text.push_str(text);
    }

    fn push_mapped(&mut self, text: &str, source_span: Span) {
        self.mappings.push(GeneratedMapping {
            generated_offset: self.text.len(),
            source_span,
        });
        self.text.push_str(text);
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn append_rendered(&mut self, rendered: RenderedText) {
        let base = self.text.len();
        self.text.push_str(&rendered.text);
        self.mappings.extend(
            rendered
                .mappings
                .into_iter()
                .map(|mapping| GeneratedMapping {
                    generated_offset: mapping.generated_offset + base,
                    source_span: mapping.source_span,
                }),
        );
    }
}

struct TscWriter {
    text: String,
    mappings: Vec<GeneratedMapping>,
}

impl TscWriter {
    fn new(capacity: usize) -> Self {
        Self {
            text: String::with_capacity(capacity),
            mappings: Vec::new(),
        }
    }

    fn push_str(&mut self, text: &str) {
        self.text.push_str(text);
    }

    fn push(&mut self, ch: char) {
        self.text.push(ch);
    }

    fn push_mapped(&mut self, text: &str, source_span: Span) {
        self.mappings.push(GeneratedMapping {
            generated_offset: self.text.len(),
            source_span,
        });
        self.text.push_str(text);
    }

    fn append_rendered(&mut self, rendered: RenderedText) {
        let base = self.text.len();
        self.text.push_str(&rendered.text);
        self.mappings.extend(
            rendered
                .mappings
                .into_iter()
                .map(|mapping| GeneratedMapping {
                    generated_offset: mapping.generated_offset + base,
                    source_span: mapping.source_span,
                }),
        );
    }

    fn into_parts(self) -> (String, Vec<GeneratedMapping>) {
        (self.text, self.mappings)
    }
}

#[derive(Default)]
struct TscMacroState {
    // defineOptions
    options_name: Option<String>,
    options_extras: Vec<(String, String)>, // (key, raw value text)
    /// Whether `defineOptions({ inheritAttrs: false })` was detected.
    has_inherit_attrs_false: bool,

    // defineProps — runtime (object-syntax only, as (name, raw_value_text) pairs)
    props_runtime: Vec<(String, String)>,
    // defineProps — TypeScript type info
    props_ts: Option<PropsTs>,
    defaulted_prop_names: Vec<String>,
    // defineProps — internal bare-prop bindings used by testing mode
    testing_props: Vec<TestingPropBinding>,

    // defineEmits — runtime emit names (for array output)
    emits_names: Vec<String>,
    // defineEmits — TypeScript emit entries
    emits_ts: Vec<EmitEntry>,
    // defineModel — each model binding
    models: Vec<ModelEntry>,

    // defineSlots — TypeScript type for $slots
    slots_ts: Option<String>,

    // Local type declarations (interfaces, type aliases)
    local_types: Vec<String>,

    // Type import statements to emit (e.g. `import type { Props } from './types'`)
    type_import_stmts: Vec<String>,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Generate a minimal TypeScript declaration file for a Vue SFC.
///
/// Only `<script setup>` is parsed (OXC macro extraction only).
/// Template compilation is entirely skipped (unless narrowing is enabled).
///
/// # Arguments
/// * `sfc_source` — full SFC source text
/// * `component_name` — component name used in the `declare const` statement
pub fn generate_tsc_output(sfc_source: &str, component_name: &str) -> TscOutput {
    generate_tsc_output_with_options(sfc_source, component_name, &TscGenOptions::default())
}

/// Like [`generate_tsc_output`] but with explicit options.
pub fn generate_tsc_output_with_options(
    sfc_source: &str,
    component_name: &str,
    tsc_options: &TscGenOptions,
) -> TscOutput {
    // ── 1. Tokenize SFC to locate <script setup> ──────────────────────
    let bytes = sfc_source.as_bytes();
    let ctx = SyntaxPluginContext {
        input: sfc_source,
        bytes,
        options: &SyntaxPluginOptions::default(),
        diagnostics: Vec::new(),
    };
    let mut syntax = Syntax::new(false);
    tokenize_sfc(bytes, |e| syntax.handle(&e, &ctx));

    let Some(setup) = syntax.script_setup() else {
        return generate_empty_stub(component_name);
    };
    let Some(content_span) = setup.content else {
        return generate_empty_stub(component_name);
    };

    let content_str = &sfc_source[content_span.start as usize..content_span.end as usize];

    // Collect companion <script> types for same-SFC cross-block resolution.
    let companion_types = if let Some(script) = syntax.script() {
        if let Some(script_content) = script.content {
            let script_source =
                &sfc_source[script_content.start as usize..script_content.end as usize];
            let alloc = Allocator::default();
            let parse_result = Parser::new(&alloc, script_source, SourceType::ts()).parse();
            Some(extract_companion_types(
                &parse_result.program,
                script_source.as_bytes(),
                script_content.start,
            ))
        } else {
            None
        }
    } else {
        None
    };

    // ── 2. OXC-parse script content ───────────────────────────────────
    let alloc = Allocator::default();
    let parse_result = Parser::new(&alloc, content_str, SourceType::ts()).parse();
    let program = parse_result.program;

    // ── 3. Extract script items (macros + imports) ────────────────────
    // content_offset = 0: all spans are relative to content_str
    let companion_types = match (companion_types, tsc_options.external_types.as_ref()) {
        (Some(mut ct), Some(ext)) => {
            for (k, v) in ext {
                ct.entry(k.clone()).or_insert_with(|| v.clone());
            }
            Some(ct)
        }
        (Some(ct), None) => Some(ct),
        (None, Some(ext)) => Some(ext.clone()),
        (None, None) => None,
    };
    let parsed =
        parse_script_with_companion(&program, ScriptMode::Setup, 0, content_str, companion_types);
    let test_bindings = if matches!(tsc_options.mode, TscMode::Testing) {
        collect_test_bindings(&parsed.bindings, content_str, content_span.start)
    } else {
        Vec::new()
    };

    // ── 4. Collect type-only imports ──────────────────────────────────
    let type_imports = collect_type_imports(&parsed.items);
    let mut type_usage_tracker = TypeUsageTracker::new(&parsed.items, content_str, &type_imports);

    // ── 5. Build macro state ──────────────────────────────────────────
    let mut state = build_macro_state(
        &parsed.items,
        content_str,
        content_span.start,
        &type_imports,
        &program.comments,
        &mut type_usage_tracker,
    );

    // ── 6. Extract generic params ────────────────────────────────────
    let generic_params = setup
        .generic
        .map(|g| sfc_source[g.start as usize..g.end as usize].trim());

    // ── 6b. Extract attrs type ──────────────────────────────────────
    // Priority: `attrs` attribute > `useAttrs<T>()` > `{}` (default)
    let explicit_attrs = setup
        .attrs
        .map(|a| sfc_source[a.start as usize..a.end as usize].trim())
        .filter(|s| !s.is_empty());
    let use_attrs_fallback;
    let attrs_type = if explicit_attrs.is_some() {
        explicit_attrs
    } else {
        use_attrs_fallback = detect_use_attrs_type_arg_tsc(&program.body, content_str);
        use_attrs_fallback.as_deref()
    };

    // ── 7. Extract root element tag for attrs fallthrough ──────────
    if let Some(attrs) = attrs_type {
        type_usage_tracker.mark_type_text(attrs);
    }
    type_usage_tracker.finalize(&mut state);

    let root_element_tag = if attrs_type.is_none() && !state.has_inherit_attrs_false {
        extract_root_element_tag(syntax.template_ast(), sfc_source)
    } else {
        None
    };

    // ── 8. Extract root conditions for narrowing ────────────────────
    let narrowing = if tsc_options.conditional_root_narrowing
        && matches!(tsc_options.mode, TscMode::Public)
    {
        extract_tsc_narrowing(syntax.template_ast(), &state, sfc_source)
    } else {
        None
    };

    // ── 9. Generate code + source map ────────────────────────────────
    if matches!(tsc_options.mode, TscMode::Testing) {
        generate_testing_code(
            component_name,
            &state,
            sfc_source,
            tsc_options.filename.as_deref(),
            generic_params,
            attrs_type,
            root_element_tag.as_deref(),
            content_str,
            &test_bindings,
        )
    } else {
        generate_code(
            component_name,
            &state,
            sfc_source,
            tsc_options.filename.as_deref(),
            generic_params,
            attrs_type,
            narrowing.as_ref(),
            root_element_tag.as_deref(),
        )
    }
}

// ── Step 4: collect type imports ─────────────────────────────────────────────

/// Info about a type import for code generation.
struct TypeImportInfo<'a> {
    /// The module source path (e.g. `./types`).
    source: &'a str,
    /// Whether the entire import statement is `import type { ... }`.
    #[allow(dead_code)]
    is_stmt_type_only: bool,
}

/// Collect type-only imports.
///
/// Returns a map: `type_name → TypeImportInfo` for all bindings that are type-only
/// (either the whole import is `import type { ... }` or the individual specifier
/// has the `type` modifier: `import { type Foo }`).
fn collect_type_imports<'a>(items: &[ScriptItem<'a>]) -> FxHashMap<&'a str, TypeImportInfo<'a>> {
    let mut map = FxHashMap::default();
    for item in items {
        if let ScriptItem::Import(imp) = item {
            for binding in &imp.bindings {
                if imp.is_type_only || binding.is_type_only {
                    map.insert(
                        binding.name,
                        TypeImportInfo {
                            source: imp.source,
                            is_stmt_type_only: imp.is_type_only,
                        },
                    );
                }
            }
        }
    }
    map
}

struct TypeUsageTracker<'a> {
    imports: Vec<(&'a str, &'a str)>,
    import_lookup: FxHashMap<&'a str, &'a str>,
    locals: Vec<LocalTypeDecl<'a>>,
    local_lookup: FxHashMap<&'a str, usize>,
    needed_imports: FxHashSet<&'a str>,
    needed_locals: FxHashSet<&'a str>,
}

impl<'a> TypeUsageTracker<'a> {
    fn new(
        items: &[ScriptItem<'a>],
        content_str: &'a str,
        type_imports: &FxHashMap<&'a str, TypeImportInfo<'a>>,
    ) -> Self {
        let mut imports = Vec::new();
        for item in items {
            if let ScriptItem::Import(imp) = item {
                for binding in &imp.bindings {
                    if imp.is_type_only || binding.is_type_only {
                        imports.push((binding.name, imp.source));
                    }
                }
            }
        }

        let mut locals = Vec::new();
        let mut local_lookup = FxHashMap::default();
        for item in items {
            if let ScriptItem::TypeDeclaration(td) = item {
                if let Some(name) = td.name {
                    let idx = locals.len();
                    locals.push(LocalTypeDecl {
                        name,
                        text: &content_str[td.span.start as usize..td.span.end as usize],
                    });
                    local_lookup.insert(name, idx);
                }
            }
        }

        let import_lookup = type_imports
            .iter()
            .map(|(name, info)| (*name, info.source))
            .collect();

        Self {
            imports,
            import_lookup,
            locals,
            local_lookup,
            needed_imports: FxHashSet::default(),
            needed_locals: FxHashSet::default(),
        }
    }

    fn mark_type_text(&mut self, type_text: &str) {
        let refs = self.collect_references(type_text, None);
        for name in refs {
            self.mark_name(name);
        }
    }

    fn mark_name(&mut self, name: &'a str) {
        if self.local_lookup.contains_key(name) {
            if self.needed_locals.insert(name) {
                let refs = {
                    let decl = &self.locals[self.local_lookup[name]];
                    self.collect_references(decl.text, Some(name))
                };
                for dep in refs {
                    self.mark_name(dep);
                }
            }
        } else if self.import_lookup.contains_key(name) {
            self.needed_imports.insert(name);
        }
    }

    fn collect_references(&self, text: &str, skip_name: Option<&str>) -> Vec<&'a str> {
        let mut refs = Vec::new();
        let mut seen = FxHashSet::default();
        let bytes = text.as_bytes();
        let mut idx = 0;

        while idx < bytes.len() {
            if is_ident_start(bytes[idx]) {
                let start = idx;
                idx += 1;
                while idx < bytes.len() && is_ident_continue(bytes[idx]) {
                    idx += 1;
                }
                let token = &text[start..idx];
                if skip_name.is_some_and(|skip| skip == token) {
                    continue;
                }

                if let Some((name, _)) = self.local_lookup.get_key_value(token) {
                    let name = *name;
                    if seen.insert(name) {
                        refs.push(name);
                    }
                } else if let Some((name, _)) = self.import_lookup.get_key_value(token) {
                    let name = *name;
                    if seen.insert(name) {
                        refs.push(name);
                    }
                }
            } else {
                idx += 1;
            }
        }

        refs
    }

    fn finalize(self, state: &mut TscMacroState) {
        let mut emitted_imports = FxHashSet::default();
        for (name, source) in self.imports {
            if self.needed_imports.contains(name) {
                let stmt = format!("import type {{ {} }} from '{}'", name, source);
                if emitted_imports.insert(stmt.clone()) {
                    state.type_import_stmts.push(stmt);
                }
            }
        }

        for local in self.locals {
            if self.needed_locals.contains(local.name) {
                state.local_types.push(local.text.to_string());
            }
        }
    }
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$')
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}

// ── Step 5: build macro state ─────────────────────────────────────────────────

fn build_macro_state<'a>(
    items: &[ScriptItem<'a>],
    content_str: &'a str,
    content_offset: u32,
    type_imports: &FxHashMap<&'a str, TypeImportInfo<'a>>,
    comments: &[Comment],
    type_usage_tracker: &mut TypeUsageTracker<'a>,
) -> TscMacroState {
    let mut state = TscMacroState::default();

    for item in items {
        let ScriptItem::Macro(m) = item else {
            continue;
        };
        match m {
            ScriptMacro::DefineOptions {
                object_arg: Some(obj),
                ..
            } => {
                process_options(obj, content_str, &mut state);
            }
            ScriptMacro::DefineOptions { .. } => {}
            ScriptMacro::DefineProps {
                type_params,
                object_arg,
                array_arg,
                ..
            } => {
                process_props(
                    type_params.as_ref(),
                    object_arg.as_ref(),
                    array_arg.as_ref(),
                    content_str,
                    content_offset,
                    type_imports,
                    comments,
                    type_usage_tracker,
                    &mut state,
                );
            }
            ScriptMacro::DefineEmits {
                type_params,
                object_arg,
                array_arg,
                ..
            } => {
                process_emits(
                    type_params.as_ref(),
                    object_arg.as_ref(),
                    array_arg.as_ref(),
                    content_str,
                    content_offset,
                    type_usage_tracker,
                    &mut state,
                );
            }
            ScriptMacro::DefineModel {
                span,
                type_params,
                name_span,
                ..
            } => {
                process_model(
                    type_params.as_ref(),
                    *span,
                    *name_span,
                    content_str,
                    content_offset,
                    type_usage_tracker,
                    &mut state,
                );
            }
            ScriptMacro::WithDefaults {
                define_props_type_params,
                defaults,
                ..
            } => {
                // First, process the inner defineProps (type-only)
                process_props(
                    define_props_type_params.as_ref(),
                    None,
                    None,
                    content_str,
                    content_offset,
                    type_imports,
                    comments,
                    type_usage_tracker,
                    &mut state,
                );
                // Then mark props with defaults as optional
                if let Some(defaults_obj) = defaults {
                    process_props_with_defaults(defaults_obj, &mut state);
                }
            }
            ScriptMacro::DefineSlots { type_params, .. } => {
                process_slots(
                    type_params.as_ref(),
                    content_str,
                    type_imports,
                    type_usage_tracker,
                    &mut state,
                );
            }
            _ => {}
        }
    }

    state
}

/// Given a `withDefaults(defineProps<{...}>(), { key1: ..., key2: ... })` defaults
/// object, mark those props as optional in the already-built props_ts.
fn process_props_with_defaults(defaults: &MacroObjectArg<'_>, state: &mut TscMacroState) {
    let default_names: Vec<&str> = defaults.properties.iter().map(|p| p.name).collect();
    for name in &default_names {
        if !state
            .defaulted_prop_names
            .iter()
            .any(|existing| existing == name)
        {
            state.defaulted_prop_names.push((*name).to_string());
        }
    }

    if let Some(PropsTs::Inline(ref mut entries)) = state.props_ts {
        for entry in entries.iter_mut() {
            if default_names.contains(&entry.name.as_str()) {
                entry.optional = true;
            }
        }
    }

    for prop in &mut state.testing_props {
        if default_names.contains(&prop.name.as_str()) {
            prop.optional = false;
        }
    }
}

fn process_options(obj: &MacroObjectArg<'_>, content_str: &str, state: &mut TscMacroState) {
    for prop in &obj.properties {
        let value_text = prop
            .value_span
            .map(|vs| content_str[vs.start as usize..vs.end as usize].trim());

        if prop.name == "name" {
            if let Some(v) = value_text {
                let stripped = v.trim_matches(|c: char| c == '\'' || c == '"');
                state.options_name = Some(stripped.to_string());
            }
        } else if prop.name == "inheritAttrs" {
            if let Some(v) = value_text {
                if v == "false" {
                    state.has_inherit_attrs_false = true;
                }
            }
            // Still add to options_extras for the runtime defineComponent output
            if let Some(v) = value_text {
                state
                    .options_extras
                    .push((prop.name.to_string(), v.to_string()));
            }
        } else if let Some(v) = value_text {
            state
                .options_extras
                .push((prop.name.to_string(), v.to_string()));
        }
    }
}

fn local_to_sfc_span(span: Span, content_offset: u32) -> Span {
    Span::new(
        span.start.saturating_add(content_offset),
        span.end.saturating_add(content_offset),
    )
}

fn normalize_resolved_span(span: Span, span_is_absolute: bool, content_offset: u32) -> Span {
    if span_is_absolute {
        span
    } else {
        local_to_sfc_span(span, content_offset)
    }
}

fn process_props<'a>(
    type_params: Option<&MacroTypeParams>,
    object_arg: Option<&MacroObjectArg<'a>>,
    array_arg: Option<&MacroArrayArg>,
    content_str: &'a str,
    content_offset: u32,
    type_imports: &FxHashMap<&'a str, TypeImportInfo<'a>>,
    comments: &[Comment],
    type_usage_tracker: &mut TypeUsageTracker<'a>,
    state: &mut TscMacroState,
) {
    if let Some(tp) = type_params {
        state.testing_props = tp
            .resolved
            .props
            .iter()
            .map(|prop| {
                let name = prop.key_name.clone().unwrap_or_else(|| {
                    content_str[prop.key.start as usize..prop.key.end as usize].to_string()
                });
                let ts_type = if let Some(ts) = prop.type_span {
                    content_str[ts.start as usize..ts.end as usize]
                        .trim()
                        .to_string()
                } else {
                    runtime_types_to_ts(&prop.types)
                };

                TestingPropBinding {
                    name,
                    optional: prop.optional,
                    ts_type,
                    map_span: Some(if prop.map_local {
                        normalize_resolved_span(prop.key, prop.span_is_absolute, content_offset)
                    } else {
                        local_to_sfc_span(tp.type_span, content_offset)
                    }),
                }
            })
            .collect();

        let type_text = content_str[tp.type_span.start as usize..tp.type_span.end as usize].trim();

        if looks_like_named_type_reference(type_text) {
            if let Some(info) = type_imports.get(type_text) {
                let _ = info;
                state.props_ts = Some(PropsTs::TypeRef(type_text.to_string()));
            } else {
                state.props_ts = Some(PropsTs::TypeText(type_text.to_string()));
            }
            type_usage_tracker.mark_type_text(type_text);
        } else if !tp.resolved.props.is_empty() {
            let entries = tp
                .resolved
                .props
                .iter()
                .map(|prop| {
                    let name = prop.key_name.clone().unwrap_or_else(|| {
                        content_str[prop.key.start as usize..prop.key.end as usize].to_string()
                    });
                    // Prefer the original type annotation span (preserves parenthesized
                    // function types in unions) over the lossy runtime type mapping.
                    let ts_type = if let Some(ts) = prop.type_span {
                        content_str[ts.start as usize..ts.end as usize]
                            .trim()
                            .to_string()
                    } else {
                        runtime_types_to_ts(&prop.types)
                    };
                    let comment = find_leading_jsdoc(comments, prop.key.start, content_str);
                    type_usage_tracker.mark_type_text(&ts_type);
                    InlinePropEntry {
                        name,
                        optional: prop.optional,
                        ts_type,
                        comment,
                        map_span: Some(if prop.map_local {
                            normalize_resolved_span(prop.key, prop.span_is_absolute, content_offset)
                        } else {
                            local_to_sfc_span(tp.type_span, content_offset)
                        }),
                    }
                })
                .collect();
            state.props_ts = Some(PropsTs::Inline(entries));
        } else {
            state.props_ts = Some(PropsTs::TypeText(type_text.to_string()));
            type_usage_tracker.mark_type_text(type_text);
        }
    } else if let Some(obj) = object_arg {
        // Object-syntax: uses AST-extracted MacroProperty fields exclusively.
        // No string parsing of prop values — all type info comes from the AST.
        let mut entries = Vec::new();
        state.testing_props.clear();
        for prop in &obj.properties {
            // Runtime: reconstruct from AST-extracted fields.
            // We can't use raw value text because `as PropType<X>` may appear
            // inside nested objects (e.g. `{ type: Object as PropType<{name: string}>, required: true }`),
            // and naive string stripping would truncate the object literal.
            let runtime_value = build_runtime_prop_value(prop);
            state
                .props_runtime
                .push((prop.name.to_string(), runtime_value));

            // TypeScript type: prefer PropType<T> annotation, else map runtime constructors
            let ts_type = if let Some(ann_span) = prop.prop_type_annotation {
                content_str[ann_span.start as usize..ann_span.end as usize]
                    .trim()
                    .to_string()
            } else if prop.runtime_types.is_empty() {
                "unknown".to_string()
            } else {
                runtime_types_to_ts(&prop.runtime_types)
            };
            type_usage_tracker.mark_type_text(&ts_type);

            let optional = !prop.required || prop.has_default;
            entries.push(InlinePropEntry {
                name: prop.name.to_string(),
                optional,
                ts_type: ts_type.clone(),
                comment: None,
                map_span: Some(local_to_sfc_span(prop.name_span, content_offset)),
            });
            state.testing_props.push(TestingPropBinding {
                name: prop.name.to_string(),
                optional: !prop.required && !prop.has_default,
                ts_type,
                map_span: Some(local_to_sfc_span(prop.name_span, content_offset)),
            });
        }
        state.props_ts = Some(PropsTs::Inline(entries));
    } else if let Some(arr) = array_arg {
        state.testing_props = arr
            .element_spans
            .iter()
            .map(|elem_span| {
                let elem = content_str[elem_span.start as usize..elem_span.end as usize].trim();
                TestingPropBinding {
                    name: elem.trim_matches(|c: char| c == '\'' || c == '"').to_string(),
                    ts_type: "unknown".to_string(),
                    optional: true,
                    map_span: Some(local_to_sfc_span(*elem_span, content_offset)),
                }
            })
            .collect();
    }
}

/// Reconstruct a clean runtime prop value from AST-extracted fields.
///
/// For flat constructors like `String`, emits just `String`.
/// For object form like `{ type: Array as PropType<X>, required: true }`,
/// reconstructs `{ type: Array, required: true }` from AST data — no string manipulation.
fn build_runtime_prop_value(prop: &crate::utils::oxc::vue::MacroProperty<'_>) -> String {
    let constructors: Vec<&str> = prop
        .runtime_types
        .iter()
        .map(|t| runtime_type_to_constructor(t))
        .collect();

    // Simple shorthand: `title: String` or `value: [String, Number]`
    if !prop.required && !prop.has_default && prop.value_span.is_none() {
        return if constructors.len() == 1 {
            constructors[0].to_string()
        } else if constructors.is_empty() {
            "null".to_string()
        } else {
            format!("[{}]", constructors.join(", "))
        };
    }

    // Flat constructor (no object form): `title: String` with value_span
    if prop.value_span.is_some() && !prop.is_method && !prop.required && !prop.has_default {
        return if constructors.len() == 1 {
            constructors[0].to_string()
        } else if constructors.is_empty() {
            "null".to_string()
        } else {
            format!("[{}]", constructors.join(", "))
        };
    }

    // Object form: reconstruct { type: ..., required: ..., default: ... }
    let mut parts = Vec::new();
    if !constructors.is_empty() {
        if constructors.len() == 1 {
            parts.push(format!("type: {}", constructors[0]));
        } else {
            parts.push(format!("type: [{}]", constructors.join(", ")));
        }
    }
    if prop.required {
        parts.push("required: true".to_string());
    }

    if parts.is_empty() {
        "null".to_string()
    } else {
        format!("{{ {} }}", parts.join(", "))
    }
}

/// Map RuntimeType to a JavaScript constructor name for the runtime props section.
fn runtime_type_to_constructor(rt: &RuntimeType) -> &'static str {
    match rt {
        RuntimeType::String => "String",
        RuntimeType::Number => "Number",
        RuntimeType::Boolean => "Boolean",
        RuntimeType::Object => "Object",
        RuntimeType::Array => "Array",
        RuntimeType::Function => "Function",
        RuntimeType::Symbol => "Symbol",
        RuntimeType::Null => "null",
        RuntimeType::BuiltIn(_) | RuntimeType::Unknown => "Object",
    }
}

fn process_emits<'a>(
    type_params: Option<&MacroTypeParams>,
    object_arg: Option<&MacroObjectArg<'a>>,
    array_arg: Option<&MacroArrayArg>,
    content_str: &str,
    content_offset: u32,
    type_usage_tracker: &mut TypeUsageTracker<'a>,
    state: &mut TscMacroState,
) {
    if let Some(tp) = type_params {
        let type_text = content_str[tp.type_span.start as usize..tp.type_span.end as usize].trim();
        type_usage_tracker.mark_type_text(type_text);

        for emit in &tp.resolved.emits {
            state.emits_names.push(emit.name.clone());
            let payload = resolved_emit_payload(&emit.signature);
            mark_emit_payload_types(type_usage_tracker, &payload);
            state.emits_ts.push(EmitEntry {
                name: emit.name.clone(),
                payload,
                map_span: Some(if emit.map_local {
                    normalize_resolved_span(
                        emit.name_span.unwrap_or(emit.span),
                        emit.span_is_absolute,
                        content_offset,
                    )
                } else {
                    local_to_sfc_span(tp.type_span, content_offset)
                }),
            });
        }
    } else if let Some(arr) = array_arg {
        for elem_span in &arr.element_spans {
            let elem = content_str[elem_span.start as usize..elem_span.end as usize].trim();
            let name = elem
                .trim_matches(|c: char| c == '\'' || c == '"')
                .to_string();
            state.emits_names.push(name.clone());
            state.emits_ts.push(EmitEntry {
                name,
                payload: EmitPayload::Unknown,
                map_span: Some(local_to_sfc_span(*elem_span, content_offset)),
            });
        }
    } else if let Some(obj) = object_arg {
        for prop in &obj.properties {
            let name = prop
                .name
                .trim_matches(|c: char| c == '\'' || c == '"')
                .to_string();
            let payload = prop
                .value_span
                .map(|span| content_str[span.start as usize..span.end as usize].trim())
                .map(extract_object_emit_payload)
                .unwrap_or(EmitPayload::Unknown);
            mark_emit_payload_types(type_usage_tracker, &payload);
            state.emits_names.push(name.clone());
            state.emits_ts.push(EmitEntry {
                name,
                payload,
                map_span: Some(local_to_sfc_span(prop.name_span, content_offset)),
            });
        }
    }
}

fn resolved_emit_payload(signature: &ResolvedEmitSignature) -> EmitPayload {
    match signature {
        ResolvedEmitSignature::Call { params_text } => EmitPayload::Call {
            params_text: params_text.clone(),
        },
        ResolvedEmitSignature::Tuple { tuple_text } => EmitPayload::Tuple {
            tuple_text: tuple_text.clone(),
        },
    }
}

fn mark_emit_payload_types(type_usage_tracker: &mut TypeUsageTracker<'_>, payload: &EmitPayload) {
    match payload {
        EmitPayload::Unknown => {}
        EmitPayload::Call { params_text } => {
            if !params_text.is_empty() {
                type_usage_tracker.mark_type_text(params_text);
            }
        }
        EmitPayload::Tuple { tuple_text } => type_usage_tracker.mark_type_text(tuple_text),
    }
}

fn extract_object_emit_payload(value_text: &str) -> EmitPayload {
    let trimmed = value_text.trim();
    if trimmed == "null" {
        return EmitPayload::Unknown;
    }

    if let Some(params_text) = extract_callable_params_text(trimmed) {
        return EmitPayload::Call { params_text };
    }

    EmitPayload::Unknown
}

fn extract_callable_params_text(value_text: &str) -> Option<String> {
    let trimmed = value_text.trim();
    if let Some(open_idx) = trimmed.find('(') {
        let close_idx = find_matching_paren(trimmed, open_idx)?;
        return Some(trimmed[open_idx + 1..close_idx].trim().to_string());
    }

    let arrow_idx = trimmed.find("=>")?;
    Some(trimmed[..arrow_idx].trim().to_string())
}

fn find_matching_paren(text: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0u32;
    for (idx, ch) in text.char_indices().skip(open_idx) {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn process_model(
    type_params: Option<&MacroTypeParams>,
    macro_span: Span,
    name_span: Option<Span>,
    content_str: &str,
    content_offset: u32,
    type_usage_tracker: &mut TypeUsageTracker<'_>,
    state: &mut TscMacroState,
) {
    let model_name = match name_span {
        Some(ns) => {
            let s = content_str[ns.start as usize..ns.end as usize].trim();
            s.trim_matches(|c: char| c == '\'' || c == '"').to_string()
        }
        None => "modelValue".to_string(),
    };

    let ts_type = match type_params {
        Some(tp) => content_str[tp.type_span.start as usize..tp.type_span.end as usize]
            .trim()
            .to_string(),
        None => "unknown".to_string(),
    };
    type_usage_tracker.mark_type_text(&ts_type);

    state.models.push(ModelEntry {
        name: model_name,
        ts_type,
        map_span: Some(local_to_sfc_span(
            name_span.unwrap_or(macro_span),
            content_offset,
        )),
    });
}

fn process_slots(
    type_params: Option<&MacroTypeParams>,
    content_str: &str,
    _type_imports: &FxHashMap<&str, TypeImportInfo<'_>>,
    type_usage_tracker: &mut TypeUsageTracker<'_>,
    state: &mut TscMacroState,
) {
    let Some(tp) = type_params else {
        return;
    };
    let type_text = content_str[tp.type_span.start as usize..tp.type_span.end as usize].trim();
    state.slots_ts = Some(type_text.to_string());
    type_usage_tracker.mark_type_text(type_text);
}

fn collect_test_bindings(
    bindings: &[(Span, BindingType)],
    content_str: &str,
    content_offset: u32,
) -> Vec<TestBindingEntry> {
    let mut seen_order = Vec::new();
    let mut latest_by_name = FxHashMap::default();

    for (span, binding_type) in bindings {
        if matches!(
            binding_type,
            BindingType::SetupImport | BindingType::Data | BindingType::Options
        ) {
            continue;
        }
        if span.start >= span.end || span.end as usize > content_str.len() {
            continue;
        }
        let name = content_str[span.start as usize..span.end as usize].to_string();
        if name.trim().is_empty() {
            continue;
        }
        if !latest_by_name.contains_key(&name) {
            seen_order.push(name.clone());
        }
        latest_by_name.insert(
            name.clone(),
            TestBindingEntry {
                name,
                binding_type: *binding_type,
                map_span: Some(local_to_sfc_span(*span, content_offset)),
            },
        );
    }

    seen_order
        .into_iter()
        .filter_map(|name| latest_by_name.remove(&name))
        .collect()
}

fn is_testing_decl_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return false;
    }
    if crate::utils::oxc::bindings::keywords::is_keyword(name.as_bytes()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

fn render_testing_prop_type(prop: &TestingPropBinding) -> String {
    if prop.optional {
        format!("({}) | undefined", prop.ts_type)
    } else {
        prop.ts_type.clone()
    }
}

fn render_testing_binding_key(name: &str) -> String {
    if is_testing_decl_ident(name) {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

fn extract_generic_param_names(generic_params: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth = 0u32;
    let mut segment_start = 0usize;

    for (idx, ch) in generic_params.char_indices() {
        match ch {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let segment = generic_params[segment_start..idx].trim();
                if let Some(name) = segment
                    .split(|c: char| c == ' ' || c == ':' || c == '=')
                    .find(|part| !part.is_empty())
                {
                    names.push(name.to_string());
                }
                segment_start = idx + 1;
            }
            _ => {}
        }
    }

    let trailing = generic_params[segment_start..].trim();
    if let Some(name) = trailing
        .split(|c: char| c == ' ' || c == ':' || c == '=')
        .find(|part| !part.is_empty())
    {
        names.push(name.to_string());
    }

    names
}

// ── Step 6: generate code ─────────────────────────────────────────────────────

fn generate_empty_stub(component_name: &str) -> TscOutput {
    let source_map = minimal_source_map();
    let encoded = BASE64_STANDARD.encode(source_map.as_bytes());
    let code = format!(
        "import {{ defineComponent }} from \"vue\"\nconst __comp = defineComponent({{}})\ndeclare const {name}: typeof __comp\nexport default {name}\n//# sourceMappingURL=data:application/json;base64,{map}\n",
        name = component_name,
        map = encoded,
    );
    TscOutput { code, source_map }
}

// ── Narrowing types for TSC path ──────────────────────────────────────────

/// Narrowing info extracted from template AST for TSC codegen.
struct TscNarrowingInfo {
    /// Narrowing analysis result from condition_narrowing module.
    narrowing: crate::ide::condition_narrowing::ConditionalRootNarrowing,
    /// Root element tag names for each branch (for $root type).
    branch_tags: Vec<TscBranchTag>,
}

/// Extract the root element tag name for attrs fallthrough.
///
/// Returns `Some(tag_name)` when there is a single native HTML root element
/// (possibly with v-if/v-else-if/v-else branches that are all the same native tag,
/// though for simplicity we only check the first branch).
/// Returns `None` for component roots, fragments, or no template.
fn extract_root_element_tag(
    template_ast: Option<&crate::ast::types::TemplateAst>,
    sfc_source: &str,
) -> Option<String> {
    use crate::ast::types::{AstNodeKind, ElementNodeConditionKind, TagType};

    let tpl = template_ast?;
    let root_children = tpl
        .root
        .content
        .as_ref()
        .map(|c| c.children.as_slice())
        .unwrap_or(&[]);

    // Count independent root elements (excluding v-else/v-else-if branches)
    let mut root_element: Option<&crate::ast::types::ElementNode> = None;
    let mut independent_count = 0u32;
    for &child_id in root_children {
        let node = &tpl.nodes[child_id.0];
        if let AstNodeKind::Element(el) = &node.kind {
            let is_branch = matches!(
                el.v_condition.as_ref().map(|c| &c.kind),
                Some(ElementNodeConditionKind::Else | ElementNodeConditionKind::ElseIf)
            );
            if !is_branch {
                independent_count += 1;
                root_element = Some(el);
            }
        }
    }

    // Only single root (possibly with v-if chain)
    if independent_count != 1 {
        return None;
    }

    let el = root_element?;
    // Skip component roots — we can't resolve their type without import resolution
    if el.tag_type == TagType::Component {
        return None;
    }

    let tag_name =
        sfc_source.get((el.tag_open.start + 1) as usize..el.tag_open.name_end as usize)?;
    Some(tag_name.to_string())
}

struct TscBranchTag {
    tag_name: String,
    is_component: bool,
}

/// Extract narrowing info from the template AST for TSC codegen.
fn extract_tsc_narrowing(
    template_ast: Option<&crate::ast::types::TemplateAst>,
    state: &TscMacroState,
    sfc_source: &str,
) -> Option<TscNarrowingInfo> {
    use crate::ast::types::{AstNodeKind, ElementNodeConditionKind, TagType};
    use rustc_hash::FxHashSet;

    let tpl = template_ast?;
    let root_children = tpl
        .root
        .content
        .as_ref()
        .map(|c| c.children.as_slice())
        .unwrap_or(&[]);

    // Collect root element conditions and tag names
    let mut conditions: Vec<(Option<String>, u32)> = Vec::new();
    let mut branch_tags: Vec<TscBranchTag> = Vec::new();

    for &child_id in root_children {
        let node = &tpl.nodes[child_id.0];
        if let AstNodeKind::Element(el) = &node.kind {
            // Extract tag name from source: between `<` (start+1) and name_end
            let tag_name = sfc_source
                .get((el.tag_open.start + 1) as usize..el.tag_open.name_end as usize)
                .unwrap_or("div")
                .to_string();
            let is_component = el.tag_type == TagType::Component;

            let condition_text = el.v_condition.as_ref().and_then(|cond| match cond.kind {
                ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                    let (Some(vs), Some(ve)) = (cond.prop.value_start, cond.prop.value_end) else {
                        return None;
                    };
                    sfc_source
                        .get(vs as usize..ve as usize)
                        .map(|s| s.to_string())
                }
                ElementNodeConditionKind::Else => None,
            });

            conditions.push((condition_text, el.tag_open.start));
            branch_tags.push(TscBranchTag {
                tag_name,
                is_component,
            });
        }
    }

    if conditions.len() <= 1 {
        return None; // Single root or no root — no narrowing needed
    }

    // Collect prop names from state
    let prop_names: FxHashSet<&str> = match &state.props_ts {
        Some(PropsTs::Inline(entries)) => entries.iter().map(|e| e.name.as_str()).collect(),
        _ => FxHashSet::default(),
    };

    if prop_names.is_empty() {
        return None;
    }

    let conditions_ref: Vec<(Option<&str>, u32)> =
        conditions.iter().map(|(c, o)| (c.as_deref(), *o)).collect();

    let narrowing =
        crate::ide::condition_narrowing::analyze_conditional_chain(&conditions_ref, &prop_names)
            .ok()?;

    Some(TscNarrowingInfo {
        narrowing,
        branch_tags,
    })
}

fn generate_testing_code(
    component_name: &str,
    state: &TscMacroState,
    sfc_source: &str,
    filename: Option<&str>,
    generic_params: Option<&str>,
    attrs_type: Option<&str>,
    root_element_tag: Option<&str>,
    setup_content: &str,
    test_bindings: &[TestBindingEntry],
) -> TscOutput {
    let mut out = TscWriter::new(setup_content.len().saturating_add(2048));

    out.push_str("import { defineComponent } from \"vue\"\n");
    out.push_str("type __OmitNew<T> = { [K in keyof T]: T[K] }\n");
    out.push_str(
        "type __Verter_UnionToIntersection<U> = (U extends any ? (value: U) => void : never) extends ((value: infer I) => void) ? I : never\n",
    );
    out.push_str(
        "type __Verter_EmitFn<T> = T extends (...args: any[]) => any ? T : T extends Record<string, any> ? __Verter_UnionToIntersection<{ [K in keyof T]: T[K] extends any[] ? (event: K, ...args: T[K]) => void : T[K] extends (...args: infer A) => any ? (event: K, ...args: A) => void : (event: K, ...args: unknown[]) => void }[keyof T]> : (event: string, ...args: unknown[]) => void\n",
    );
    out.push_str(
        "declare function defineProps<TypeProps>(): TypeProps\ndeclare function defineProps<RuntimeProps extends Record<string, any>>(props: RuntimeProps): import(\"vue\").ExtractPropTypes<RuntimeProps>\ndeclare function defineProps<PropName extends string>(props: readonly PropName[]): Record<PropName, unknown>\n",
    );
    out.push_str(
        "declare function defineEmits<TypeEmits extends ((...args: any[]) => any) | Record<string, any>>(): __Verter_EmitFn<TypeEmits>\ndeclare function defineEmits<Named extends string>(names: readonly Named[]): __Verter_EmitFn<Record<Named, unknown[]>>\ndeclare function defineEmits<ObjectEmits extends Record<string, any>>(options: ObjectEmits): __Verter_EmitFn<ObjectEmits>\n",
    );
    out.push_str(
        "declare function defineExpose<Exposed extends Record<string, any> = Record<string, never>>(exposed?: Exposed): void\ndeclare function defineOptions(options: Record<string, unknown>): void\ndeclare function defineSlots<Slots extends Record<string, any>>(): Slots\ndeclare function withDefaults<Props, Defaults extends Partial<Props>>(props: Props, defaults: Defaults): Omit<Props, keyof Defaults> & { [K in keyof Defaults]-?: K extends keyof Props ? Exclude<Props[K], undefined> : never }\ndeclare function defineModel<Model = unknown>(nameOrOptions?: string | unknown, options?: unknown): import(\"vue\").Ref<Model | undefined>\n",
    );

    if let Some(gp) = generic_params {
        for name in extract_generic_param_names(gp) {
            if is_testing_decl_ident(&name) {
                out.push_str(&format!("type {} = any\n", name));
            }
        }
    }

    let declared_names: FxHashSet<String> = test_bindings
        .iter()
        .filter(|binding| !matches!(binding.binding_type, BindingType::Props))
        .map(|binding| binding.name.clone())
        .collect();
    for prop in &state.testing_props {
        if declared_names.contains(&prop.name) || !is_testing_decl_ident(&prop.name) {
            continue;
        }
        out.push_str("declare const ");
        if let Some(map_span) = prop.map_span {
            out.push_mapped(&prop.name, map_span);
        } else {
            out.push_str(&prop.name);
        }
        out.push_str(": ");
        out.push_str(&render_testing_prop_type(prop));
        out.push_str("\n");
    }

    if !setup_content.trim().is_empty() {
        out.push_str(setup_content);
        if !setup_content.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push('\n');

    if test_bindings.is_empty() {
        out.push_str("type __Verter_TestBindings = {}\n\n");
    } else {
        out.push_str("type __Verter_TestBindings = import(\"vue\").ShallowUnwrapRef<{\n");
        for binding in test_bindings {
            out.push_str("  ");
            let rendered_name = render_testing_binding_key(&binding.name);
            if let Some(map_span) = binding.map_span {
                out.push_mapped(&rendered_name, map_span);
            } else {
                out.push_str(&rendered_name);
            }
            out.push_str(": typeof ");
            out.push_str(&binding.name);
            out.push_str(";\n");
        }
        out.push_str("}>\n\n");
    }

    out.push_str("const __comp = defineComponent({\n");

    if let Some(ref name) = state.options_name {
        out.push_str(&format!("  name: '{}' as const,\n", name));
    }
    for (key, val) in &state.options_extras {
        out.push_str(&format!("  {}: {},\n", key, val));
    }

    let has_props = !state.props_runtime.is_empty() || !state.models.is_empty();
    if has_props {
        out.push_str("  props: {\n");
        for (name, val) in &state.props_runtime {
            out.push_str(&format!("    {}: {},\n", name, val));
        }
        for model in &state.models {
            let ctor = ts_to_constructor(&model.ts_type);
            out.push_str(&format!("    {}: {},\n", model.name, ctor));
        }
        out.push_str("  },\n");
    }

    let has_emits = !state.emits_names.is_empty() || !state.models.is_empty();
    if has_emits {
        let mut names: Vec<String> = state
            .emits_names
            .iter()
            .map(|n| format!("'{}'", n))
            .collect();
        for model in &state.models {
            names.push(format!("'update:{}'", model.name));
        }
        out.push_str(&format!("  emits: [{}],\n", names.join(", ")));
    }

    out.push_str("})\n\n");
    out.push_str(&format!(
        "declare const {name}: __OmitNew<typeof __comp> & {{\n",
        name = component_name,
    ));

    let full_props = render_full_props_type(
        &state.props_ts,
        &state.emits_ts,
        &state.models,
        &state.defaulted_prop_names,
        None,
    );

    match generic_params {
        Some(gp) => {
            out.push_str(&format!(
                "  new<{gp}>(props?: import(\"vue\").PublicProps & "
            ));
            out.append_rendered(full_props);
            out.push_str("): {\n");
        }
        None => {
            out.push_str("  new(props?: import(\"vue\").PublicProps & ");
            out.append_rendered(full_props);
            out.push_str("): {\n");
        }
    }

    out.push_str("    $props: import(\"vue\").PublicProps & ");
    out.append_rendered(render_full_props_type(
        &state.props_ts,
        &state.emits_ts,
        &state.models,
        &state.defaulted_prop_names,
        None,
    ));
    out.push_str(",\n");
    out.push_str("    $emit: ");
    out.append_rendered(render_emit_fn_type(&state.emits_ts, &state.models));
    out.push_str(",\n");
    if let Some(ref slots) = state.slots_ts {
        out.push_str(&format!("    $slots: {},\n", slots));
    }
    out.push_str("    $data: {},\n");
    if let Some(attrs) = attrs_type {
        out.push_str(&format!("    $attrs: {},\n", attrs));
    } else if root_element_tag.is_some() {
        out.push_str("    $attrs: import(\"vue\").HTMLAttributes,\n");
    } else {
        out.push_str("    $attrs: {},\n");
    }
    out.push_str("    $refs: {},\n");
    out.push_str("  } & __Verter_TestBindings\n");
    out.push_str("}\n");
    out.push_str(&format!("export default {}\n", component_name));

    let (mut code, mappings) = out.into_parts();
    let source_map = build_tsc_source_map(&code, sfc_source, filename, &mappings);
    let encoded = BASE64_STANDARD.encode(source_map.as_bytes());
    code.push_str(&format!(
        "//# sourceMappingURL=data:application/json;base64,{}\n",
        encoded
    ));

    TscOutput { code, source_map }
}

fn generate_code(
    component_name: &str,
    state: &TscMacroState,
    sfc_source: &str,
    filename: Option<&str>,
    generic_params: Option<&str>,
    attrs_type: Option<&str>,
    narrowing: Option<&TscNarrowingInfo>,
    root_element_tag: Option<&str>,
) -> TscOutput {
    let mut out = TscWriter::new(512);

    // ── Import ────────────────────────────────────────────────────────
    out.push_str("import { defineComponent } from \"vue\"\n");

    // ── Utility type: strip construct signature from typeof __comp ────
    // `typeof __comp` carries DefineComponent's `new()` which returns
    // `ComponentPublicInstance<{}>` (empty props). When barrel-re-exported
    // (`export { default as X } from './X.vue'`), TypeScript picks this
    // empty `new()` over our explicit typed one. Stripping construct sigs
    // via a mapped type leaves only the static members (props, emits options)
    // so there is exactly one `new()` — ours — with the correct $props.
    out.push_str("type __OmitNew<T> = { [K in keyof T]: T[K] }\n");

    // ── Type import statements ────────────────────────────────────────
    for stmt in &state.type_import_stmts {
        out.push_str(stmt);
        out.push('\n');
    }

    // ── Local type declarations ───────────────────────────────────────
    for lt in &state.local_types {
        out.push_str(lt);
        out.push('\n');
    }
    out.push('\n');

    // ── const __comp = defineComponent({...}) ────────────────────────
    out.push_str("const __comp = defineComponent({\n");

    if let Some(ref name) = state.options_name {
        out.push_str(&format!("  name: '{}' as const,\n", name));
    }
    for (key, val) in &state.options_extras {
        out.push_str(&format!("  {}: {},\n", key, val));
    }

    let has_props = !state.props_runtime.is_empty() || !state.models.is_empty();
    if has_props {
        out.push_str("  props: {\n");
        for (name, val) in &state.props_runtime {
            out.push_str(&format!("    {}: {},\n", name, val));
        }
        for model in &state.models {
            let ctor = ts_to_constructor(&model.ts_type);
            out.push_str(&format!("    {}: {},\n", model.name, ctor));
        }
        out.push_str("  },\n");
    }

    let has_emits = !state.emits_names.is_empty() || !state.models.is_empty();
    if has_emits {
        let mut names: Vec<String> = state
            .emits_names
            .iter()
            .map(|n| format!("'{}'", n))
            .collect();
        for model in &state.models {
            names.push(format!("'update:{}'", model.name));
        }
        out.push_str(&format!("  emits: [{}],\n", names.join(", ")));
    }

    out.push_str("})\n\n");

    // ── declare const ComponentName ───────────────────────────────────
    // Uses `__OmitNew<typeof __comp>` to strip the construct signature from
    // DefineComponent, then provides a single `new()` that returns the
    // intersection of ComponentPublicInstance and the typed instance shape.
    // This ensures barrel re-exports preserve the correct $props/$emit types.
    out.push_str(&format!(
        "declare const {name}: __OmitNew<typeof __comp> & {{\n",
        name = component_name,
    ));

    // Build generic params for new(), appending narrowing generics if present
    let full_gp = if let Some(nr) = narrowing {
        let mut narrowing_parts: Vec<String> = Vec::new();
        for g in &nr.narrowing.generics {
            // Find the prop type from inline entries
            let prop_type = match &state.props_ts {
                Some(PropsTs::Inline(entries)) => entries
                    .iter()
                    .find(|e| e.name == g.prop_name)
                    .map(|e| e.ts_type.as_str())
                    .unwrap_or("unknown"),
                _ => "unknown",
            };
            narrowing_parts.push(format!(
                "T_{prop} extends {pt} = {pt}",
                prop = g.prop_name,
                pt = prop_type,
            ));
        }
        let nr_str = narrowing_parts.join(", ");
        match generic_params {
            Some(gp) => Some(format!("{gp}, {nr_str}")),
            None => Some(nr_str),
        }
    } else {
        generic_params.map(|s| s.to_string())
    };
    let full_props = render_full_props_type(
        &state.props_ts,
        &state.emits_ts,
        &state.models,
        &state.defaulted_prop_names,
        narrowing,
    );

    // Generate simplified constructor: `new(props?: PublicProps & Props): { $props, $emit, ... }`
    // Does NOT include ComponentPublicInstance in the return type — CPI has many
    // generic params that TypeScript expands, causing "Type instantiation is
    // excessively deep" with self-referential prop types (e.g. Action → callback(Action)).
    // The explicit $props/$emit/$slots/$data/$attrs/$refs fields cover instance access.
    match &full_gp {
        Some(gp) => {
            out.push_str(&format!(
                "  new<{gp}>(props?: import(\"vue\").PublicProps & "
            ));
            out.append_rendered(render_full_props_type(
                &state.props_ts,
                &state.emits_ts,
                &state.models,
                &state.defaulted_prop_names,
                narrowing,
            ));
            out.push_str("): {\n");
        }
        None => {
            out.push_str("  new(props?: import(\"vue\").PublicProps & ");
            out.append_rendered(full_props);
            out.push_str("): {\n");
        }
    }

    out.push_str("    $props: import(\"vue\").PublicProps & ");
    out.append_rendered(render_full_props_type(
        &state.props_ts,
        &state.emits_ts,
        &state.models,
        &state.defaulted_prop_names,
        narrowing,
    ));
    out.push_str(",\n");

    out.push_str("    $emit: ");
    out.append_rendered(render_emit_fn_type(&state.emits_ts, &state.models));
    out.push_str(",\n");
    if let Some(ref slots) = state.slots_ts {
        out.push_str(&format!("    $slots: {},\n", slots));
    }
    out.push_str("    $data: {},\n");
    if let Some(attrs) = attrs_type {
        // Explicit attrs type from attrs= attribute or useAttrs<T>()
        out.push_str(&format!("    $attrs: {},\n", attrs));
    } else if root_element_tag.is_some() {
        // Native HTML root + inheritAttrs: true → HTMLAttributes for fallthrough
        out.push_str("    $attrs: import(\"vue\").HTMLAttributes,\n");
    } else {
        // inheritAttrs: false, component root, fragment, or no template
        out.push_str("    $attrs: {},\n");
    }
    out.push_str("    $refs: {},\n");

    // $root — conditional type for narrowing
    if let Some(nr) = narrowing {
        out.push_str("    $root: ");
        for (i, branch) in nr.narrowing.branches.iter().enumerate() {
            let tag_type = if i < nr.branch_tags.len() {
                let bt = &nr.branch_tags[i];
                if bt.is_component {
                    format!("InstanceType<typeof {}>", bt.tag_name)
                } else {
                    html_tag_to_element_type(&bt.tag_name)
                }
            } else {
                "unknown".to_string()
            };

            if let Some(ref cond) = branch.narrowing {
                let extends_rhs = if let Some(ref lit) = cond.literal {
                    lit.clone()
                } else if cond.negated {
                    "false".to_string()
                } else {
                    "true".to_string()
                };
                out.push_str(&format!(
                    "T_{prop} extends {rhs} ? {tag_type} : ",
                    prop = cond.prop_name,
                    rhs = extends_rhs,
                ));
            } else {
                // v-else: terminal
                out.push_str(&tag_type);
            }
            if i == nr.narrowing.branches.len() - 1 && branch.narrowing.is_some() {
                out.push_str("never");
            }
        }
        out.push_str(",\n");
    }

    out.push_str("  }\n");
    out.push_str("}\n");
    out.push_str(&format!("export default {}\n", component_name));

    // ── Inline source map ─────────────────────────────────────────────
    let (mut code, mappings) = out.into_parts();
    let source_map = build_tsc_source_map(&code, sfc_source, filename, &mappings);
    let encoded = BASE64_STANDARD.encode(source_map.as_bytes());
    code.push_str(&format!(
        "//# sourceMappingURL=data:application/json;base64,{}\n",
        encoded
    ));

    TscOutput { code, source_map }
}

// ── Build helpers ─────────────────────────────────────────────────────────────

/// Build an inline emit function type (overloaded function signatures).
///
/// Instead of relying on `EmitFn` (which is internal to Vue and not exported),
/// we build the overloaded emit function type directly:
/// ```text
/// ((event: 'change') => void) & ((event: 'update:model', v: string) => void)
/// ```
fn build_emit_fn_type(emits: &[EmitEntry], models: &[ModelEntry]) -> String {
    if emits.is_empty() && models.is_empty() {
        return "(event: string, ...args: unknown[]) => void".to_string();
    }
    let mut overloads: Vec<String> = emits.iter().map(emit_overload_type).collect();
    for model in models {
        overloads.push(format!(
            "((event: 'update:{}', v: {}) => void)",
            model.name, model.ts_type
        ));
    }
    overloads.join(" & ")
}

fn render_emit_fn_type(emits: &[EmitEntry], models: &[ModelEntry]) -> RenderedText {
    let mut rendered = RenderedText::default();
    if emits.is_empty() && models.is_empty() {
        rendered.push_str("(event: string, ...args: unknown[]) => void");
        return rendered;
    }

    let mut needs_join = false;
    for emit in emits {
        if needs_join {
            rendered.push_str(" & ");
        }
        rendered.push_str("((event: ");
        if let Some(map_span) = emit.map_span {
            rendered.push_mapped(&format!("'{}'", emit.name), map_span);
        } else {
            rendered.push_str(&format!("'{}'", emit.name));
        }
        match &emit.payload {
            EmitPayload::Unknown => rendered.push_str(", ...args: unknown[]) => void)"),
            EmitPayload::Call { params_text } => {
                if params_text.is_empty() {
                    rendered.push_str(") => void)");
                } else {
                    rendered.push_str(", ");
                    rendered.push_str(params_text);
                    rendered.push_str(") => void)");
                }
            }
            EmitPayload::Tuple { tuple_text } => {
                rendered.push_str(", ...args: ");
                rendered.push_str(tuple_text);
                rendered.push_str(") => void)");
            }
        }
        needs_join = true;
    }

    for model in models {
        if needs_join {
            rendered.push_str(" & ");
        }
        rendered.push_str("((event: ");
        if let Some(map_span) = model.map_span {
            rendered.push_mapped(&format!("'update:{}'", model.name), map_span);
        } else {
            rendered.push_str(&format!("'update:{}'", model.name));
        }
        rendered.push_str(", v: ");
        rendered.push_str(&model.ts_type);
        rendered.push_str(") => void)");
        needs_join = true;
    }

    rendered
}

/// Convert emit entries to event handler props for the `$props` type.
///
/// Each emit becomes one or two optional `onEventName` props:
/// ```text
/// EmitEntry { name: "clickOverlay", payload: Call { params_text: "event: MouseEvent" } }
///   → "onClickOverlay"?: (event: MouseEvent) => void
/// ```
fn build_emits_to_props_type(emits: &[EmitEntry]) -> Option<String> {
    if emits.is_empty() {
        return None;
    }
    let mut fields: Vec<String> = Vec::new();
    for e in emits {
        let handler = emit_handler_type(e);
        for key in emit_handler_keys(&e.name) {
            fields.push(format!("\"{}\"?: {}", key, handler));
        }
    }
    Some(format!("{{ {} }}", fields.join("; ")))
}

fn render_emits_to_props_type(emits: &[EmitEntry]) -> RenderedText {
    let mut rendered = RenderedText::default();
    if emits.is_empty() {
        return rendered;
    }

    rendered.push_str("{ ");
    let mut first = true;
    for emit in emits {
        let handler = emit_handler_type(emit);
        for key in emit_handler_keys(&emit.name) {
            if !first {
                rendered.push_str("; ");
            }
            if let Some(map_span) = emit.map_span {
                rendered.push_mapped(&format!("\"{}\"", key), map_span);
            } else {
                rendered.push_str(&format!("\"{}\"", key));
            }
            rendered.push_str("?: ");
            rendered.push_str(&handler);
            first = false;
        }
    }
    rendered.push_str(" }");
    rendered
}

fn render_props_shape_type(
    props_ts: &Option<PropsTs>,
    defaulted_prop_names: &[String],
    generic_props: Option<&FxHashSet<&str>>,
) -> RenderedText {
    let mut rendered = RenderedText::default();

    match props_ts {
        Some(PropsTs::TypeRef(name)) => {
            rendered.push_str(&wrap_defaulted_props(name, defaulted_prop_names))
        }
        Some(PropsTs::TypeText(text)) => {
            rendered.push_str(&wrap_defaulted_props(text, defaulted_prop_names))
        }
        Some(PropsTs::Inline(entries)) if !entries.is_empty() => {
            rendered.push_str("{ ");
            let mut first = true;
            for entry in entries {
                if !first {
                    rendered.push_str("; ");
                }
                if let Some(comment) = &entry.comment {
                    rendered.push_str(comment);
                    rendered.push_str(" ");
                }
                if let Some(map_span) = entry.map_span {
                    rendered.push_mapped(&entry.name, map_span);
                } else {
                    rendered.push_str(&entry.name);
                }
                rendered.push_str(if entry.optional { "?: " } else { ": " });
                if generic_props.is_some_and(|set| set.contains(entry.name.as_str())) {
                    rendered.push_str(&format!("T_{}", entry.name));
                } else {
                    rendered.push_str(&entry.ts_type);
                }
                first = false;
            }
            rendered.push_str(" }");
        }
        _ => {}
    }

    rendered
}

fn render_model_props_type(models: &[ModelEntry]) -> Vec<RenderedText> {
    models
        .iter()
        .map(|model| {
            let mut rendered = RenderedText::default();
            rendered.push_str("{ ");
            if let Some(map_span) = model.map_span {
                rendered.push_mapped(&model.name, map_span);
            } else {
                rendered.push_str(&model.name);
            }
            rendered.push_str("?: ");
            rendered.push_str(&model.ts_type);
            rendered.push_str("; ");
            if let Some(map_span) = model.map_span {
                rendered.push_mapped(&format!("\"onUpdate:{}\"", model.name), map_span);
            } else {
                rendered.push_str(&format!("\"onUpdate:{}\"", model.name));
            }
            rendered.push_str("?: (v: ");
            rendered.push_str(&model.ts_type);
            rendered.push_str(") => void }");
            rendered
        })
        .collect()
}

fn render_full_props_type(
    props_ts: &Option<PropsTs>,
    emits: &[EmitEntry],
    models: &[ModelEntry],
    defaulted_prop_names: &[String],
    narrowing: Option<&TscNarrowingInfo>,
) -> RenderedText {
    let generic_props = narrowing.map(|nr| {
        nr.narrowing
            .generics
            .iter()
            .map(|g| g.prop_name.as_str())
            .collect::<FxHashSet<_>>()
    });
    let mut parts = Vec::new();

    let props_part =
        render_props_shape_type(props_ts, defaulted_prop_names, generic_props.as_ref());
    if !props_part.is_empty() {
        parts.push(props_part);
    }
    parts.extend(render_model_props_type(models));
    let emits_part = render_emits_to_props_type(emits);
    if !emits_part.is_empty() {
        parts.push(emits_part);
    }

    if parts.is_empty() {
        let mut rendered = RenderedText::default();
        rendered.push_str("{}");
        return rendered;
    }

    let mut rendered = RenderedText::default();
    let mut first = true;
    for part in parts {
        if !first {
            rendered.push_str(" & ");
        }
        rendered.append_rendered(part);
        first = false;
    }
    rendered
}

fn emit_handler_type(emit: &EmitEntry) -> String {
    match &emit.payload {
        EmitPayload::Unknown => "(...args: unknown[]) => void".to_string(),
        EmitPayload::Call { params_text } => {
            if params_text.is_empty() {
                "() => void".to_string()
            } else {
                format!("({}) => void", params_text)
            }
        }
        EmitPayload::Tuple { tuple_text } => format!("(...args: {}) => void", tuple_text),
    }
}

fn emit_overload_type(emit: &EmitEntry) -> String {
    match &emit.payload {
        EmitPayload::Unknown => format!("((event: '{}', ...args: unknown[]) => void)", emit.name),
        EmitPayload::Call { params_text } => {
            if params_text.is_empty() {
                format!("((event: '{}') => void)", emit.name)
            } else {
                format!("((event: '{}', {}) => void)", emit.name, params_text)
            }
        }
        EmitPayload::Tuple { tuple_text } => {
            format!(
                "((event: '{}', ...args: {}) => void)",
                emit.name, tuple_text
            )
        }
    }
}

fn emit_handler_keys(name: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let canonical = format!("on{}", capitalize_first(name));
    keys.push(canonical.clone());

    if !name.contains(':') {
        let camel = format!("on{}", capitalize_first(&camelize_event_name(name)));
        if camel != canonical {
            keys.push(camel);
        }

        let kebab = format!("on{}", capitalize_first(&hyphenate_event_name(name)));
        if kebab != canonical && !keys.iter().any(|key| key == &kebab) {
            keys.push(kebab);
        }
    }

    keys
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Camelize a kebab-case string: `"my-custom-event"` → `"myCustomEvent"`.
fn camelize_event_name(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for ch in s.chars() {
        if ch == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            for upper in ch.to_uppercase() {
                result.push(upper);
            }
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

fn hyphenate_event_name(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for (idx, ch) in s.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 {
                result.push('-');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

fn looks_like_named_type_reference(type_text: &str) -> bool {
    let trimmed = type_text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if matches!(trimmed.as_bytes()[0], b'{' | b'[' | b'(' | b'"' | b'\'') {
        return false;
    }
    if trimmed.contains(['|', '&', ';', ':', '=']) {
        return false;
    }

    !matches!(
        trimmed,
        "string"
            | "number"
            | "boolean"
            | "symbol"
            | "bigint"
            | "any"
            | "unknown"
            | "never"
            | "void"
            | "null"
            | "undefined"
            | "true"
            | "false"
    )
}

fn wrap_defaulted_props(base: &str, defaulted_prop_names: &[String]) -> String {
    if defaulted_prop_names.is_empty() {
        return base.to_string();
    }

    let quoted_names = defaulted_prop_names
        .iter()
        .map(|name| format!("'{}'", name))
        .collect::<Vec<_>>()
        .join(" | ");

    format!(
        "Omit<{base}, {quoted_names}> & Partial<Pick<{base}, {quoted_names}>>",
        base = base,
        quoted_names = quoted_names,
    )
}

fn build_props_type(
    props_ts: &Option<PropsTs>,
    models: &[ModelEntry],
    defaulted_prop_names: &[String],
) -> String {
    let mut parts: Vec<String> = Vec::new();

    match props_ts {
        Some(PropsTs::TypeRef(name)) => {
            parts.push(wrap_defaulted_props(name, defaulted_prop_names));
        }
        Some(PropsTs::TypeText(text)) => {
            parts.push(wrap_defaulted_props(text, defaulted_prop_names));
        }
        Some(PropsTs::Inline(entries)) if !entries.is_empty() => {
            let fields: Vec<String> = entries
                .iter()
                .map(|e| {
                    let field = if e.optional {
                        format!("{}?: {}", e.name, e.ts_type)
                    } else {
                        format!("{}: {}", e.name, e.ts_type)
                    };
                    match &e.comment {
                        Some(comment) => format!("{} {}", comment, field),
                        None => field,
                    }
                })
                .collect();
            parts.push(format!("{{ {} }}", fields.join("; ")));
        }
        _ => {}
    }

    for model in models {
        parts.push(format!(
            "{{ {}?: {}; \"onUpdate:{}\"?: (v: {}) => void }}",
            model.name, model.ts_type, model.name, model.ts_type,
        ));
    }

    if parts.is_empty() {
        "{}".to_string()
    } else {
        parts.join(" & ")
    }
}

/// Build props type with narrowing generic substitutions.
/// For inline props, replaces the type of narrowing props with T_{prop}.
fn build_narrowing_props_type(
    props_ts: &Option<PropsTs>,
    models: &[ModelEntry],
    narrowing: &crate::ide::condition_narrowing::ConditionalRootNarrowing,
) -> Option<String> {
    let entries = match props_ts {
        Some(PropsTs::Inline(entries)) if !entries.is_empty() => entries,
        _ => return None, // Can't substitute for TypeRef/TypeText
    };

    let generic_props: rustc_hash::FxHashSet<&str> = narrowing
        .generics
        .iter()
        .map(|g| g.prop_name.as_str())
        .collect();

    let fields: Vec<String> = entries
        .iter()
        .map(|e| {
            let ts_type = if generic_props.contains(e.name.as_str()) {
                format!("T_{}", e.name)
            } else {
                e.ts_type.clone()
            };
            let field = if e.optional {
                format!("{}?: {}", e.name, ts_type)
            } else {
                format!("{}: {}", e.name, ts_type)
            };
            match &e.comment {
                Some(comment) => format!("{} {}", comment, field),
                None => field,
            }
        })
        .collect();

    let mut parts = vec![format!("{{ {} }}", fields.join("; "))];

    for model in models {
        parts.push(format!(
            "{{ {}?: {}; \"onUpdate:{}\"?: (v: {}) => void }}",
            model.name, model.ts_type, model.name, model.ts_type,
        ));
    }

    Some(parts.join(" & "))
}

/// Map HTML tag name to TypeScript DOM element type.
fn html_tag_to_element_type(tag: &str) -> String {
    let element = match tag {
        "a" => "HTMLAnchorElement",
        "button" => "HTMLButtonElement",
        "canvas" => "HTMLCanvasElement",
        "div" => "HTMLDivElement",
        "form" => "HTMLFormElement",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "HTMLHeadingElement",
        "img" => "HTMLImageElement",
        "input" => "HTMLInputElement",
        "label" => "HTMLLabelElement",
        "li" => "HTMLLIElement",
        "nav" => "HTMLElement",
        "ol" => "HTMLOListElement",
        "p" => "HTMLParagraphElement",
        "pre" => "HTMLPreElement",
        "section" | "article" | "aside" | "footer" | "header" | "main" => "HTMLElement",
        "select" => "HTMLSelectElement",
        "span" => "HTMLSpanElement",
        "table" => "HTMLTableElement",
        "textarea" => "HTMLTextAreaElement",
        "ul" => "HTMLUListElement",
        "video" => "HTMLVideoElement",
        _ => "HTMLElement",
    };
    element.to_string()
}

// ── Value conversion helpers ──────────────────────────────────────────────────

fn ts_to_constructor(ts: &str) -> &'static str {
    match ts.trim() {
        "string" => "String",
        "number" => "Number",
        "boolean" => "Boolean",
        "symbol" => "Symbol",
        "bigint" => "BigInt",
        _ => "Object",
    }
}

/// Convert `RuntimeType` list to a TypeScript type string.
///
/// When a union contains `Function`, the arrow type is wrapped in parentheses
/// to avoid TS1385 ("Function type notation must be parenthesized when used
/// in a union or intersection type").
fn runtime_types_to_ts(types: &[RuntimeType]) -> String {
    if types.is_empty() {
        return "unknown".to_string();
    }
    let needs_parens = types.len() > 1;
    let ts: Vec<&str> = types
        .iter()
        .map(|t| match t {
            RuntimeType::String => "string",
            RuntimeType::Number => "number",
            RuntimeType::Boolean => "boolean",
            RuntimeType::Object => "Record<string, unknown>",
            RuntimeType::Array => "unknown[]",
            RuntimeType::Function => {
                if needs_parens {
                    "((...args: unknown[]) => unknown)"
                } else {
                    "(...args: unknown[]) => unknown"
                }
            }
            RuntimeType::Symbol => "symbol",
            RuntimeType::BuiltIn(_) => "unknown",
            RuntimeType::Null => "null",
            RuntimeType::Unknown => "unknown",
        })
        .collect();
    ts.join(" | ")
}

/// Find a leading JSDoc comment for a property at the given position.
///
/// OXC's `Comment.attached_to` is the byte offset of the token the comment precedes.
/// We match comments where `attached_to == target_start` and the comment is a JSDoc
/// block comment (starts with `/**`).
fn find_leading_jsdoc(
    comments: &[Comment],
    target_start: u32,
    content_str: &str,
) -> Option<String> {
    for comment in comments {
        if comment.attached_to == target_start
            && comment.is_block()
            && matches!(
                comment.content,
                CommentContent::Jsdoc | CommentContent::JsdocLegal
            )
        {
            let text = &content_str[comment.span.start as usize..comment.span.end as usize];
            return Some(text.to_string());
        }
    }
    None
}

fn minimal_source_map() -> String {
    r#"{"version":3,"file":"","sourceRoot":"","sources":[],"names":[],"mappings":""}"#.to_string()
}

fn build_tsc_source_map(
    code: &str,
    sfc_source: &str,
    filename: Option<&str>,
    mappings: &[GeneratedMapping],
) -> String {
    let mut builder = SourceMapBuilder::default();
    let source_id = builder.set_source_and_content(filename.unwrap_or("source.vue"), sfc_source);

    let generated_resolver = PositionResolver::new_for_sourcemap(code);
    let source_resolver = PositionResolver::new_for_sourcemap(sfc_source);

    for mapping in mappings {
        if mapping.generated_offset > code.len()
            || mapping.source_span.start as usize > sfc_source.len()
        {
            continue;
        }
        let (generated_line, generated_col) =
            generated_resolver.offset_to_line_and_col(mapping.generated_offset);
        let (source_line, source_col) =
            source_resolver.offset_to_line_and_col(mapping.source_span.start as usize);
        builder.add_token(
            (generated_line - 1) as u32,
            (generated_col - 1) as u32,
            (source_line - 1) as u32,
            (source_col - 1) as u32,
            Some(source_id),
            None,
        );
    }

    builder.into_sourcemap().to_json_string()
}

/// Detect `useAttrs<T>()` calls in the script setup body and return the type parameter text.
///
/// Used as a fallback for `attrs_type` when no `attrs` attribute is present on the script tag.
fn detect_use_attrs_type_arg_tsc<'a>(body: &[Statement<'a>], source: &'a str) -> Option<String> {
    for stmt in body {
        let call = match stmt {
            Statement::VariableDeclaration(var_decl) => var_decl
                .declarations
                .iter()
                .find_map(|d| d.init.as_ref())
                .and_then(|e| match e {
                    Expression::CallExpression(c) => Some(c.as_ref()),
                    _ => None,
                }),
            Statement::ExpressionStatement(expr_stmt) => match &expr_stmt.expression {
                Expression::CallExpression(c) => Some(c.as_ref()),
                _ => None,
            },
            _ => None,
        };
        if let Some(call) = call {
            if let Expression::Identifier(ident) = &call.callee {
                if ident.name == "useAttrs" {
                    if let Some(tp) = &call.type_arguments {
                        if let Some(param) = tp.params.first() {
                            let span: oxc_span::Span = param.span();
                            let text = &source[span.start as usize..span.end as usize];
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                return Some(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}
