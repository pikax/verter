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
use oxc_ast::{Comment, CommentContent};
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashMap;

use crate::common::Span;
use crate::diagnostics::{SyntaxPluginContext, SyntaxPluginOptions};
use crate::parser::Syntax;
use crate::tokenizer::byte::tokenize_sfc;
use crate::utils::oxc::vue::{
    parse_script, MacroArrayArg, MacroObjectArg, MacroTypeParams, RuntimeType, ScriptItem,
    ScriptMacro, ScriptMode,
};

/// Output from the tsc codegen.
pub struct TscOutput {
    /// The generated `.tsc.tsx` source with inline source map.
    pub code: String,
    /// The JSON source map string (without base64 encoding).
    pub source_map: String,
}

/// Options for tsc codegen (reserved for future use).
#[derive(Debug, Default)]
pub struct TscGenOptions {}

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
}

struct EmitEntry {
    name: String,
    params_ts: String,
}

struct ModelEntry {
    name: String,
    ts_type: String,
}

#[derive(Default)]
struct TscMacroState {
    // defineOptions
    options_name: Option<String>,
    options_extras: Vec<(String, String)>, // (key, raw value text)

    // defineProps — runtime (object-syntax only, as (name, raw_value_text) pairs)
    props_runtime: Vec<(String, String)>,
    // defineProps — TypeScript type info
    props_ts: Option<PropsTs>,

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
/// Template compilation is entirely skipped.
///
/// # Arguments
/// * `sfc_source` — full SFC source text
/// * `component_name` — component name used in the `declare const` statement
pub fn generate_tsc_output(sfc_source: &str, component_name: &str) -> TscOutput {
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

    // ── 2. OXC-parse script content ───────────────────────────────────
    let alloc = Allocator::default();
    let parse_result = Parser::new(&alloc, content_str, SourceType::ts()).parse();
    let program = parse_result.program;

    // ── 3. Extract script items (macros + imports) ────────────────────
    // content_offset = 0: all spans are relative to content_str
    let parsed = parse_script(&program, ScriptMode::Setup, 0, content_str);

    // ── 4. Collect type-only imports ──────────────────────────────────
    let type_imports = collect_type_imports(&parsed.items);

    // ── 5. Build macro state ──────────────────────────────────────────
    let state = build_macro_state(&parsed.items, content_str, &type_imports, &program.comments);

    // ── 6. Extract generic params ────────────────────────────────────
    let generic_params = setup
        .generic
        .map(|g| sfc_source[g.start as usize..g.end as usize].trim());

    // ── 7. Generate code + source map ────────────────────────────────
    generate_code(component_name, &state, generic_params)
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

// ── Step 5: build macro state ─────────────────────────────────────────────────

fn build_macro_state<'a>(
    items: &[ScriptItem<'a>],
    content_str: &'a str,
    type_imports: &FxHashMap<&'a str, TypeImportInfo<'a>>,
    comments: &[Comment],
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
                    type_imports,
                    comments,
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
                    &mut state,
                );
            }
            ScriptMacro::DefineModel {
                type_params,
                name_span,
                ..
            } => {
                process_model(type_params.as_ref(), *name_span, content_str, &mut state);
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
                    type_imports,
                    comments,
                    &mut state,
                );
                // Then mark props with defaults as optional
                if let Some(defaults_obj) = defaults {
                    process_props_with_defaults(defaults_obj, &mut state);
                }
            }
            ScriptMacro::DefineSlots { type_params, .. } => {
                process_slots(type_params.as_ref(), content_str, type_imports, &mut state);
            }
            _ => {}
        }
    }

    // Only collect local type declarations that are referenced by props or slots.
    // This avoids TS6196 (unused) and TS2304 (unresolved transitive deps) errors
    // from type declarations that our output never references.
    collect_referenced_local_types(items, content_str, &mut state);

    state
}

/// Collect local type declarations only if they're referenced by props or slots.
///
/// When `defineProps<LocalType>()` resolves to `PropsTs::TypeText("LocalType")`,
/// or `defineSlots<LocalSlots>()` references a local interface, we need the local
/// type declaration so tsc can resolve the name.
fn collect_referenced_local_types(
    items: &[ScriptItem<'_>],
    content_str: &str,
    state: &mut TscMacroState,
) {
    // Determine which type names are actually needed.
    let mut needed_names: Vec<&str> = Vec::new();
    if let Some(PropsTs::TypeText(text)) = &state.props_ts {
        needed_names.push(text.as_str());
    }
    if let Some(slots) = &state.slots_ts {
        needed_names.push(slots.as_str());
    }

    if needed_names.is_empty() {
        return;
    }

    for item in items {
        if let ScriptItem::TypeDeclaration(td) = item {
            if let Some(name) = td.name {
                if needed_names.contains(&name) {
                    let text = &content_str[td.span.start as usize..td.span.end as usize];
                    state.local_types.push(text.to_string());
                }
            }
        }
    }
}

/// Given a `withDefaults(defineProps<{...}>(), { key1: ..., key2: ... })` defaults
/// object, mark those props as optional in the already-built props_ts.
fn process_props_with_defaults(defaults: &MacroObjectArg<'_>, state: &mut TscMacroState) {
    let default_names: Vec<&str> = defaults.properties.iter().map(|p| p.name).collect();

    if let Some(PropsTs::Inline(ref mut entries)) = state.props_ts {
        for entry in entries.iter_mut() {
            if default_names.contains(&entry.name.as_str()) {
                entry.optional = true;
            }
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
        } else if let Some(v) = value_text {
            state
                .options_extras
                .push((prop.name.to_string(), v.to_string()));
        }
    }
}

fn process_props<'a>(
    type_params: Option<&MacroTypeParams>,
    object_arg: Option<&MacroObjectArg<'a>>,
    _array_arg: Option<&MacroArrayArg>,
    content_str: &'a str,
    type_imports: &FxHashMap<&'a str, TypeImportInfo<'a>>,
    comments: &[Comment],
    state: &mut TscMacroState,
) {
    if let Some(tp) = type_params {
        let type_text = content_str[tp.type_span.start as usize..tp.type_span.end as usize].trim();

        if tp.unresolved_type_ref {
            if let Some(info) = type_imports.get(type_text) {
                // Emit a proper import type statement and use the type name directly
                state.type_import_stmts.push(format!(
                    "import type {{ {} }} from '{}'",
                    type_text, info.source
                ));
                state.props_ts = Some(PropsTs::TypeRef(type_text.to_string()));
            } else {
                state.props_ts = Some(PropsTs::TypeText(type_text.to_string()));
            }
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
                    InlinePropEntry {
                        name,
                        optional: prop.optional,
                        ts_type,
                        comment,
                    }
                })
                .collect();
            state.props_ts = Some(PropsTs::Inline(entries));
        } else {
            state.props_ts = Some(PropsTs::TypeText(type_text.to_string()));
        }
    } else if let Some(obj) = object_arg {
        // Object-syntax: uses AST-extracted MacroProperty fields exclusively.
        // No string parsing of prop values — all type info comes from the AST.
        let mut entries = Vec::new();
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

            let optional = !prop.required || prop.has_default;
            entries.push(InlinePropEntry {
                name: prop.name.to_string(),
                optional,
                ts_type,
                comment: None,
            });
        }
        state.props_ts = Some(PropsTs::Inline(entries));
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
    state: &mut TscMacroState,
) {
    if let Some(tp) = type_params {
        for emit in &tp.resolved.emits {
            state.emits_names.push(emit.name.clone());
            state.emits_ts.push(EmitEntry {
                name: emit.name.clone(),
                params_ts: String::new(),
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
                params_ts: String::new(),
            });
        }
    } else if let Some(obj) = object_arg {
        for prop in &obj.properties {
            let name = prop
                .name
                .trim_matches(|c: char| c == '\'' || c == '"')
                .to_string();
            state.emits_names.push(name.clone());
            state.emits_ts.push(EmitEntry {
                name,
                params_ts: String::new(),
            });
        }
    }
}

fn process_model(
    type_params: Option<&MacroTypeParams>,
    name_span: Option<Span>,
    content_str: &str,
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

    state.models.push(ModelEntry {
        name: model_name,
        ts_type,
    });
}

fn process_slots(
    type_params: Option<&MacroTypeParams>,
    content_str: &str,
    type_imports: &FxHashMap<&str, TypeImportInfo<'_>>,
    state: &mut TscMacroState,
) {
    let Some(tp) = type_params else {
        return;
    };
    let type_text = content_str[tp.type_span.start as usize..tp.type_span.end as usize].trim();

    if tp.unresolved_type_ref {
        if let Some(info) = type_imports.get(type_text) {
            state.type_import_stmts.push(format!(
                "import type {{ {} }} from '{}'",
                type_text, info.source
            ));
            state.slots_ts = Some(type_text.to_string());
        } else {
            // Local type reference (e.g. interface MySlots { ... })
            state.slots_ts = Some(type_text.to_string());
        }
    } else {
        // Inline type literal
        state.slots_ts = Some(type_text.to_string());
    }
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

fn generate_code(
    component_name: &str,
    state: &TscMacroState,
    generic_params: Option<&str>,
) -> TscOutput {
    let mut out = String::with_capacity(512);

    // ── Import ────────────────────────────────────────────────────────
    out.push_str("import { defineComponent } from \"vue\"\n");

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
    let emits_type = build_emits_type(&state.emits_ts, &state.models);
    let props_type = build_props_type(&state.props_ts, &state.models);

    out.push_str(&format!(
        "declare const {name}: typeof __comp & import(\"vue\").ComponentPublicInstance<{{}}, {{}}, {{}}, {{}}, {{}}, {{}}, {{}}, {emits}> & {{\n",
        name = component_name,
        emits = emits_type,
    ));
    let emit_fn_type = build_emit_fn_type(&state.emits_ts, &state.models);
    match generic_params {
        Some(gp) => out.push_str(&format!("  new<{gp}>(): {{\n")),
        None => out.push_str("  new(): {\n"),
    }
    out.push_str(&format!("    $props: {},\n", props_type));
    out.push_str(&format!("    $emit: {},\n", emit_fn_type));
    if let Some(ref slots) = state.slots_ts {
        out.push_str(&format!("    $slots: {},\n", slots));
    }
    out.push_str("    $data: {},\n");
    out.push_str("    $attrs: {},\n");
    out.push_str("    $refs: {},\n");
    out.push_str("  }\n");
    out.push_str("}\n");
    out.push_str(&format!("export default {}\n", component_name));

    // ── Inline source map ─────────────────────────────────────────────
    let source_map = minimal_source_map();
    let encoded = BASE64_STANDARD.encode(source_map.as_bytes());
    out.push_str(&format!(
        "//# sourceMappingURL=data:application/json;base64,{}\n",
        encoded
    ));

    TscOutput {
        code: out,
        source_map,
    }
}

// ── Build helpers ─────────────────────────────────────────────────────────────

fn build_emits_type(emits: &[EmitEntry], models: &[ModelEntry]) -> String {
    if emits.is_empty() && models.is_empty() {
        return "{}".to_string();
    }
    let mut parts: Vec<String> = emits
        .iter()
        .map(|e| {
            let key = ts_property_key(&e.name);
            if e.params_ts.is_empty() {
                format!("{}: []", key)
            } else {
                format!("{}: [{}]", key, e.params_ts)
            }
        })
        .collect();
    for model in models {
        parts.push(format!("'update:{}': [v: {}]", model.name, model.ts_type));
    }
    format!("{{ {} }}", parts.join("; "))
}

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
    let mut overloads: Vec<String> = emits
        .iter()
        .map(|e| {
            if e.params_ts.is_empty() {
                format!("((event: '{}', ...args: unknown[]) => void)", e.name)
            } else {
                format!("((event: '{}', {}) => void)", e.name, e.params_ts)
            }
        })
        .collect();
    for model in models {
        overloads.push(format!(
            "((event: 'update:{}', v: {}) => void)",
            model.name, model.ts_type
        ));
    }
    overloads.join(" & ")
}

fn build_props_type(props_ts: &Option<PropsTs>, models: &[ModelEntry]) -> String {
    let mut parts: Vec<String> = Vec::new();

    match props_ts {
        Some(PropsTs::TypeRef(name)) => {
            parts.push(name.clone());
        }
        Some(PropsTs::TypeText(text)) => {
            parts.push(text.clone());
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

fn ts_property_key(name: &str) -> String {
    if is_valid_identifier(name) {
        name.to_string()
    } else {
        format!("'{}'", name)
    }
}

fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn minimal_source_map() -> String {
    r#"{"version":3,"file":"","sourceRoot":"","sources":[],"names":[],"mappings":""}"#.to_string()
}
