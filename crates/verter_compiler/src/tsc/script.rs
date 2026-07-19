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
use oxc_parser::Parser;
use oxc_sourcemap::SourceMapBuilder;
use oxc_span::{GetSpan, SourceType};
use rustc_hash::{FxHashMap, FxHashSet};
use verter_macro_dto::{MacroTscBundle, MacroTscOutcome, MacroTscProjection};

use crate::common::Span;
use crate::cursor::position::PositionResolver;
use crate::diagnostics::{SyntaxPluginContext, SyntaxPluginOptions};
use crate::parser::Syntax;
use crate::template::code_gen::binding::BindingType;
use crate::tokenizer::byte::tokenize_sfc;
use crate::utils::oxc::vue::{
    extract_options_component_macro_args, parse_script, DefaultExportType, ImportSpecifierKind,
    MacroArrayArg, MacroObjectArg, MacroTypeParams, OptionsComponentMacroArgs,
    RuntimeConstructorSyntax, ScriptItem, ScriptMacro, ScriptMode, ScriptParseContext,
};

/// Macro stub declarations shared between `generate_code` (when expose entries
/// need the setup body) and `generate_testing_code`.
const MACRO_STUBS: &str = "\
declare function defineProps<TypeProps>(): TypeProps\n\
declare function defineProps<RuntimeProps extends Record<string, any>>(props: RuntimeProps): import(\"vue\").ExtractPropTypes<RuntimeProps>\n\
declare function defineProps<PropName extends string>(props: readonly PropName[]): Record<PropName, unknown>\n\
declare function defineEmits<TypeEmits extends ((...args: any[]) => any) | Record<string, any>>(): __Verter_EmitFn<TypeEmits>\n\
declare function defineEmits<Named extends string>(names: readonly Named[]): __Verter_EmitFn<Record<Named, unknown[]>>\n\
declare function defineEmits<ObjectEmits extends Record<string, any>>(options: ObjectEmits): __Verter_EmitFn<ObjectEmits>\n\
declare function defineExpose<Exposed extends Record<string, any> = Record<string, never>>(exposed?: Exposed): void\n\
declare function defineOptions(options: Record<string, unknown>): void\n\
declare function defineSlots<Slots extends Record<string, any>>(): Slots\n\
declare function withDefaults<Props, Defaults extends Partial<Props>>(props: Props, defaults: Defaults): Omit<Props, keyof Defaults> & { [K in keyof Defaults]-?: K extends keyof Props ? Exclude<Props[K], undefined> : never }\n\
declare function defineModel<Model = unknown>(nameOrOptions?: string | unknown, options?: unknown): import(\"vue\").Ref<Model | undefined>\n";

const TERMINAL_EMITS_TO_PROPS_TYPE: &str = "type __Verter_EmitsToProps<T> = T extends (...args: infer A) => any ? A extends [infer E extends string, ...infer P] ? { [K in E as `on${Capitalize<K>}`]?: (...args: P) => void } : {} : T extends Record<string, any> ? { [K in keyof T as K extends string ? `on${Capitalize<K>}` : never]?: (...args: T[K] extends any[] ? T[K] : T[K] extends (...args: infer P) => any ? P : unknown[]) => void } : {}\n";

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
    /// The declaration-only public surface (`.d.<ext>.ts`): a strictly valid
    /// `.d.ts` — pure declarations, NO runtime / value code — that a bare
    /// framework-carrier import (`import B from "./B.vue"`) resolves to. It
    /// renders the SAME public surface [`Self::Public`] computes, but as an
    /// explicit `declare const … export default …` instead of `typeof` over a
    /// runtime `defineComponent` value.
    Declaration,
}

/// Options for tsc codegen.
#[derive(Debug, Default)]
pub struct TscGenOptions {
    /// Experimental: Enable conditional root generic narrowing.
    pub conditional_root_narrowing: bool,
    /// Source filename used in source maps and cross-file type resolution.
    pub filename: Option<String>,
    /// Public or testing/debug output mode.
    pub mode: TscMode,
}

// ── Internal state ───────────────────────────────────────────────────────────

/// TypeScript type representation for props (from `defineProps`).
#[derive(Clone)]
enum PropsTs {
    /// Terminal TypeInfo splice for a typed macro surface.
    TypeText(String),
    /// Syntax-derived property list for runtime object/array forms.
    Inline(Vec<InlinePropEntry>),
}

#[derive(Clone)]
struct InlinePropEntry {
    name: String,
    optional: bool,
    ts_type: String,
    comment: Option<String>,
    map_span: Option<Span>,
}

#[derive(Clone)]
struct EmitEntry {
    name: String,
    payload: EmitPayload,
    map_span: Option<Span>,
}

#[derive(Clone)]
enum EmitPayload {
    Unknown,
    Call { params_text: String },
}

#[derive(Clone)]
struct ModelEntry {
    name: String,
    ts_type: String,
    map_span: Option<Span>,
}

#[derive(Clone)]
struct TestingPropBinding {
    name: String,
    ts_type: String,
    optional: bool,
    map_span: Option<Span>,
}

#[derive(Clone)]
struct TestBindingEntry {
    name: String,
    binding_type: BindingType,
    map_span: Option<Span>,
}

/// An exposed property from `defineExpose({ ... })`.
#[derive(Clone)]
struct ExposeEntry {
    /// The property name (key in the object literal).
    name: String,
    /// When `Some(ident)`, the codegen emits `name: typeof ident`.
    /// When `None`, falls back to `name: any` (methods, complex expressions).
    typeof_target: Option<String>,
}

struct LocalTypeDecl<'a> {
    name: &'a str,
    text: &'a str,
}

/// One generated-code byte offset mapped to its authored source byte span.
///
/// Shared with framework declaration carriers rendered outside this module
/// (the Svelte public-API projector maps its generated prop-name tokens back
/// to the authored `$props()` annotation members through the same
/// [`build_tsc_source_map`] V3 JSON the store/plugin consume for `.vue`).
#[derive(Clone, Copy)]
pub struct GeneratedMapping {
    /// Byte offset of the mapped token's start in the generated code.
    pub generated_offset: usize,
    /// The authored byte span the token maps to (only `start` is encoded).
    pub source_span: Span,
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

#[derive(Default, Clone)]
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
    /// Authoritative terminal emit projection from TypeInfo.
    terminal_emits_ts: Option<String>,
    // defineModel — each model binding
    models: Vec<ModelEntry>,

    // defineSlots — TypeScript type for $slots
    slots_ts: Option<String>,

    // defineExpose — individual property entries (from object arg)
    expose_entries: Vec<ExposeEntry>,
    // defineExpose — TypeScript type text (from type param)
    expose_type_text: Option<String>,

    // Local type declarations (interfaces, type aliases)
    local_types: Vec<String>,

    // Type import statements to emit (e.g. `import type { Props } from './types'`)
    type_import_stmts: Vec<String>,

    // Declaration-only type import statements: VALUE imports (no `type`
    // modifier) that are USED IN A TYPE POSITION (e.g. `import { Props }` used by
    // `defineProps<Props>()`). The `Public`/`Testing` paths bring these into
    // scope via the emitted `<script setup>` body; the `Declaration` path omits
    // that body, so it emits these as declaration-legal `import type { … }`
    // statements instead. Kept SEPARATE from `type_import_stmts` so emitting
    // them does NOT duplicate the value import the setup body already carries in
    // the non-declaration paths.
    declaration_promoted_type_imports: Vec<String>,

    /// Content-free compiler syntax slots awaiting authoritative TSC splices.
    semantic_slots: Vec<TscSemanticSlot>,
}

#[derive(Clone)]
struct TscSemanticSlot {
    syntax_index: u32,
    role: TscSemanticRole,
    type_span: Option<Span>,
}

#[derive(Clone)]
enum TscSemanticRole {
    Props,
    Emits,
    Model { name: String },
}

// ── Extract + Cache API ──────────────────────────────────────────────────────

/// Options for the extract-only path (steps 1–7 without external types).
#[derive(Debug, Default)]
pub struct TscExtractOptions {
    /// Source filename used in source maps.
    pub filename: Option<String>,
}

/// Cached intermediate state from SFC macro extraction.
///
/// Captures everything that depends on the SFC source text alone (steps 1–7)
/// so code generation can be repeated with terminal semantic bundles and
/// different modes without re-parsing.
pub struct ExtractedTscState {
    // Note: Debug is manually implemented below (fields contain non-Debug internal types).
    macro_state: TscMacroState,
    generic_params: Option<String>,
    attrs_type: Option<String>,
    root_element_tag: Option<String>,
    test_bindings: Vec<TestBindingEntry>,
    /// Script setup content string (owned).
    content_str: String,
    /// Filename for source maps.
    filename: Option<String>,
}

impl std::fmt::Debug for ExtractedTscState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtractedTscState").finish_non_exhaustive()
    }
}

/// Extract intermediate TSC state from an SFC without external types.
///
/// Runs steps 1–7 of the TSC pipeline (SFC tokenization, OXC parsing, macro
/// extraction, type tracking) using only syntax-owned facts. Type-based macro
/// surfaces remain empty until [`generate_tsc_from_state`] applies a bundle.
///
/// Returns `None` if the SFC has no `<script setup>` block.
pub fn extract_tsc_state(
    sfc_source: &str,
    component_name: &str,
    options: &TscExtractOptions,
) -> Option<ExtractedTscState> {
    let _ = component_name; // used by callers to name the component, not needed during extract

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

    let setup = syntax.script_setup()?;
    let content_span = setup.content?;

    let content_str = &sfc_source[content_span.start as usize..content_span.end as usize];

    // ── 2. OXC-parse script content ───────────────────────────────────
    let alloc = Allocator::default();
    let parse_result = Parser::new(&alloc, content_str, SourceType::ts()).parse();
    let program = parse_result.program;

    // ── 3. Extract script items (macros + imports) — NO external types ─
    let parsed = parse_script(&program, ScriptMode::Setup, 0, content_str);
    let test_bindings = collect_test_bindings(&parsed.bindings, content_str, content_span.start);

    // ── 4. Collect type-only imports ──────────────────────────────────
    let type_imports = collect_type_imports(&parsed.items);
    let mut type_usage_tracker = TypeUsageTracker::new(&parsed.items, content_str, &type_imports);

    // ── 5. Build macro state ──────────────────────────────────────────
    let mut state = build_macro_state(
        &parsed.items,
        content_str,
        content_span.start,
        &type_imports,
        &mut type_usage_tracker,
    );

    // ── 6. Extract generic params ────────────────────────────────────
    let generic_params = setup.generic.map(|g| {
        sfc_source[g.start as usize..g.end as usize]
            .trim()
            .to_string()
    });

    // ── 6b. Extract attrs type ──────────────────────────────────────
    let explicit_attrs = setup
        .attrs
        .map(|a| sfc_source[a.start as usize..a.end as usize].trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let use_attrs_fallback;
    let attrs_type = if explicit_attrs.is_some() {
        explicit_attrs
    } else {
        use_attrs_fallback = detect_use_attrs_type_arg_tsc(&program.body, content_str);
        use_attrs_fallback
    };

    // ── 7. Extract root element tag for attrs fallthrough ──────────
    if let Some(attrs) = attrs_type.as_deref() {
        type_usage_tracker.mark_type_text(attrs);
    }
    type_usage_tracker.finalize(&mut state);

    let root_element_tag = if attrs_type.is_none() && !state.has_inherit_attrs_false {
        extract_root_element_tag(syntax.template_ast(), sfc_source)
    } else {
        None
    };

    Some(ExtractedTscState {
        macro_state: state,
        generic_params,
        attrs_type,
        root_element_tag,
        test_bindings,
        content_str: content_str.to_string(),
        filename: options.filename.clone(),
    })
}

/// Generate TSC output from a previously extracted state.
///
/// Clones the cached syntax state, applies terminal macro splices, and calls
/// the appropriate code generation function.
pub fn generate_tsc_from_state(
    state: &ExtractedTscState,
    sfc_source: &str,
    component_name: &str,
    mode: TscMode,
    macro_tsc: Option<&MacroTscBundle>,
) -> TscOutput {
    let component_name = &sanitize_tsc_component_name(component_name);
    let mut macro_state = state.macro_state.clone();
    apply_tsc_bundle(&mut macro_state, macro_tsc);

    let generic_params = state.generic_params.as_deref();
    let attrs_type = state.attrs_type.as_deref();
    let root_element_tag = state.root_element_tag.as_deref();
    let filename = state.filename.as_deref();

    match mode {
        TscMode::Testing => generate_testing_code(
            component_name,
            &macro_state,
            sfc_source,
            filename,
            generic_params,
            attrs_type,
            root_element_tag,
            &state.content_str,
            &state.test_bindings,
        ),
        TscMode::Declaration => generate_declaration_code(
            component_name,
            &macro_state,
            sfc_source,
            filename,
            generic_params,
            attrs_type,
            root_element_tag,
        ),
        TscMode::Public => generate_code(
            component_name,
            &macro_state,
            sfc_source,
            filename,
            generic_params,
            attrs_type,
            None, // narrowing not used in cache path
            root_element_tag,
            &state.content_str,
        ),
    }
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
    generate_tsc_output_with_options(sfc_source, component_name, &TscGenOptions::default(), None)
}

/// Sanitize a component name to be a valid TypeScript identifier.
///
/// Replaces non-alphanumeric, non-underscore, non-`$` characters with `_`.
/// This handles dotted file stems like `Drawer.draggable` → `Drawer_draggable`.
fn sanitize_tsc_component_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();

    let result = if sanitized.is_empty() {
        "_Component".to_string()
    } else if sanitized.chars().next().unwrap().is_ascii_digit() {
        format!("_{sanitized}")
    } else {
        sanitized
    };

    // Prefix reserved words so they are valid TS identifiers
    match result.as_str() {
        "default" | "export" | "import" | "class" | "function" | "return" | "var" | "let"
        | "const" | "if" | "else" | "for" | "while" | "do" | "switch" | "case" | "break"
        | "continue" | "new" | "delete" | "typeof" | "void" | "this" | "with" | "throw" | "try"
        | "catch" | "finally" | "in" | "of" | "yield" | "await" | "async" | "extends" | "super"
        | "static" | "enum" | "implements" | "interface" | "package" | "private" | "protected"
        | "public" => format!("_{result}"),
        _ => result,
    }
}

/// Like [`generate_tsc_output`] but with explicit options.
pub fn generate_tsc_output_with_options(
    sfc_source: &str,
    component_name: &str,
    tsc_options: &TscGenOptions,
    macro_tsc: Option<&MacroTscBundle>,
) -> TscOutput {
    let component_name = &sanitize_tsc_component_name(component_name);
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
        // No <script setup>. In Declaration mode every no-setup case must stay
        // declaration-safe: the Options-API runtime stub wraps the default
        // export in a runtime `defineComponent(...)` and the empty stub creates
        // a runtime `__comp` — neither is `.d.ts`-legal. A no-setup Declaration
        // request projects the FULL options-object public surface (props/emits
        // from the `defineComponent({ props, emits })` options object) into a
        // declaration-safe component declaration, falling back to the minimal
        // empty stub only for a genuinely-empty surface.
        if matches!(tsc_options.mode, TscMode::Declaration) {
            if let Some(script) = syntax.script() {
                if let Some(content) = script.content {
                    let content_str = &sfc_source[content.start as usize..content.end as usize];
                    if let Some(output) = generate_options_api_declaration(
                        component_name,
                        sfc_source,
                        content_str,
                        tsc_options.filename.as_deref(),
                    ) {
                        return output;
                    }
                }
            }
            return generate_declaration_empty_stub(component_name);
        }
        // No <script setup> — check for Options API <script> block.
        // If present, pass through its content so defineComponent() props
        // are preserved for cross-component type checking.
        if let Some(script) = syntax.script() {
            if let Some(content) = script.content {
                let content_str = &sfc_source[content.start as usize..content.end as usize];
                return generate_options_api_stub(component_name, content_str);
            }
        }
        return generate_empty_stub(component_name);
    };
    let Some(content_span) = setup.content else {
        if matches!(tsc_options.mode, TscMode::Declaration) {
            return generate_declaration_empty_stub(component_name);
        }
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
        &mut type_usage_tracker,
    );
    apply_tsc_bundle(&mut state, macro_tsc);

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
    let narrowing =
        if tsc_options.conditional_root_narrowing && matches!(tsc_options.mode, TscMode::Public) {
            extract_tsc_narrowing(syntax.template_ast(), &state, sfc_source)
        } else {
            None
        };

    // ── 9. Generate code + source map ────────────────────────────────
    match tsc_options.mode {
        TscMode::Testing => generate_testing_code(
            component_name,
            &state,
            sfc_source,
            tsc_options.filename.as_deref(),
            generic_params,
            attrs_type,
            root_element_tag.as_deref(),
            content_str,
            &test_bindings,
        ),
        TscMode::Declaration => generate_declaration_code(
            component_name,
            &state,
            sfc_source,
            tsc_options.filename.as_deref(),
            generic_params,
            attrs_type,
            root_element_tag.as_deref(),
        ),
        TscMode::Public => generate_code(
            component_name,
            &state,
            sfc_source,
            tsc_options.filename.as_deref(),
            generic_params,
            attrs_type,
            narrowing.as_ref(),
            root_element_tag.as_deref(),
            content_str,
        ),
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

/// A type-position-referenced import whose declaration-legal `import type …`
/// statement must be reconstructed. Carries the structured import shape so the
/// reconstruction is driven entirely by typed data — never re-parsing or
/// string-slicing the source.
#[derive(Clone, Copy)]
struct ReconstructedTypeImport<'a> {
    /// The local binding name as referenced in the type position.
    local: &'a str,
    /// The module-exported name for a NAMED import (`Some` for named, possibly
    /// equal to `local`); `None` for default/namespace imports. For a
    /// string-literal export name this is the UNQUOTED value (`x-y`) — see
    /// [`Self::imported_is_string_literal`].
    imported: Option<&'a str>,
    /// Whether the NAMED imported name is a string literal
    /// (`import { "x-y" as Local }`) rather than a plain identifier. Carried
    /// from the typed import binding so the declaration reconstruction re-quotes
    /// a string-literal export name into a declaration-legal form; never a
    /// downstream string sniff for quotes.
    imported_is_string_literal: bool,
    /// The module specifier (e.g. `./types`).
    source: &'a str,
    /// Named / default / namespace — selects the type-only statement form.
    kind: ImportSpecifierKind,
}

impl<'a> ReconstructedTypeImport<'a> {
    /// Render the declaration-legal `import type …` statement for this import
    /// form, preserving the imported name for aliased named imports and the
    /// quoting for a string-literal export name.
    ///
    /// - Named, `local == imported`:  `import type { Local } from '…'`
    /// - Named, aliased:              `import type { Imported as Local } from '…'`
    /// - Named, string-literal name:  `import type { "x-y" as Local } from '…'`
    /// - Namespace:                   `import type * as Local from '…'`
    /// - Default:                     `import type Local from '…'`
    fn render_stmt(&self) -> String {
        match self.kind {
            ImportSpecifierKind::Named => match self.imported {
                // A string-literal export name (`import { "x-y" as Local }`) is
                // not a valid identifier, so it can NEVER equal the identifier
                // local and is ALWAYS aliased — re-quote it into the
                // declaration-legal `import type { "x-y" as Local }` form.
                Some(imported) if self.imported_is_string_literal => {
                    format!(
                        "import type {{ {} as {} }} from '{}'",
                        quote_module_export_name(imported),
                        self.local,
                        self.source
                    )
                }
                Some(imported) if imported != self.local => {
                    format!(
                        "import type {{ {} as {} }} from '{}'",
                        imported, self.local, self.source
                    )
                }
                // Named non-aliased (or the imported name is unexpectedly
                // absent): the bare-local form resolves because local ==
                // imported.
                _ => format!("import type {{ {} }} from '{}'", self.local, self.source),
            },
            ImportSpecifierKind::Namespace => {
                format!("import type * as {} from '{}'", self.local, self.source)
            }
            ImportSpecifierKind::Default => {
                format!("import type {} from '{}'", self.local, self.source)
            }
        }
    }
}

/// Re-quote a string-literal module export name (`import { "x-y" as Local }`)
/// into a declaration-legal double-quoted TS string literal. The captured
/// `imported` value is the UNQUOTED cooked name (`x-y`), so every character a
/// double-quoted TS/JS string literal cannot carry raw is re-escaped when
/// re-wrapping. The result is a VALID single-line double-quoted literal for ANY
/// input — printable characters (including the hyphen in `"vue-props"`) pass
/// through unchanged, so the encoder never over-escapes.
pub(super) fn quote_module_export_name(unquoted: &str) -> String {
    let mut out = String::with_capacity(unquoted.len() + 2);
    out.push('"');
    for ch in unquoted.chars() {
        match ch {
            // Backslash and the closing delimiter must always be escaped.
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            // Line terminators are illegal raw inside a string literal.
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            // Remaining ASCII control characters (U+0000..U+001F) are illegal
            // raw: use the conventional short escapes where they exist, and a
            // zero-padded 4-hex `\uXXXX` fallback for the rest.
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\u{000B}' => out.push_str("\\v"),
            '\u{000C}' => out.push_str("\\f"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct TypeUsageTracker<'a> {
    /// Type-only imports (`import type …` / `import { type Foo }`), in source
    /// order, with their full reconstructed shape (named/aliased/default/
    /// namespace). Reconstruction preserves the imported name so an aliased
    /// type-only import round-trips as `import type { Imported as Local }`.
    imports: Vec<ReconstructedTypeImport<'a>>,
    import_lookup: FxHashMap<&'a str, &'a str>,
    /// VALUE imports (no `type` modifier), in source order, with their full
    /// reconstructed shape. A value import used in a type position is PROMOTED
    /// to a declaration-legal `import type` for the `Declaration` path — every
    /// import form (named, aliased-named, default, namespace) promotes to its
    /// own correct type-only statement so the referenced symbol resolves.
    value_imports: Vec<ReconstructedTypeImport<'a>>,
    value_import_lookup: FxHashMap<&'a str, &'a str>,
    locals: Vec<LocalTypeDecl<'a>>,
    local_lookup: FxHashMap<&'a str, usize>,
    needed_imports: FxHashSet<&'a str>,
    /// Value imports observed in a TYPE position (the promotion set), keyed by
    /// local name.
    needed_value_imports: FxHashSet<&'a str>,
    needed_locals: FxHashSet<&'a str>,
}

impl<'a> TypeUsageTracker<'a> {
    fn new(
        items: &[ScriptItem<'a>],
        content_str: &'a str,
        type_imports: &FxHashMap<&'a str, TypeImportInfo<'a>>,
    ) -> Self {
        let mut imports = Vec::new();
        let mut value_imports = Vec::new();
        for item in items {
            if let ScriptItem::Import(imp) = item {
                for binding in &imp.bindings {
                    // Reconstruct the import's full shape from the typed binding
                    // — never re-parsed. A binding with no `import_kind` is not
                    // an import specifier (it is an export/decl binding) and is
                    // skipped.
                    let Some(kind) = binding.import_kind else {
                        continue;
                    };
                    let reconstructed = ReconstructedTypeImport {
                        local: binding.name,
                        imported: binding.imported,
                        imported_is_string_literal: binding.imported_is_string_literal,
                        source: imp.source,
                        kind,
                    };
                    if imp.is_type_only || binding.is_type_only {
                        imports.push(reconstructed);
                    } else {
                        // A VALUE import (no `type` modifier) of ANY form. Every
                        // form has a declaration-legal type-only promotion, so
                        // it is eligible for promotion when used in a type
                        // position (named → `import type { … }`, namespace →
                        // `import type * as …`, default → `import type …`).
                        value_imports.push(reconstructed);
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

        let import_lookup: FxHashMap<&'a str, &'a str> = type_imports
            .iter()
            .map(|(name, info)| (*name, info.source))
            .collect();

        // A name that is BOTH a type-only import and (separately) a value
        // import resolves as type-only — the type-only entry already brings it
        // into scope, so it must not also be tracked for value promotion. The
        // lookup is keyed by the LOCAL name (what a type position references).
        let value_import_lookup: FxHashMap<&'a str, &'a str> = value_imports
            .iter()
            .filter(|imp| !import_lookup.contains_key(imp.local))
            .map(|imp| (imp.local, imp.source))
            .collect();

        Self {
            imports,
            import_lookup,
            value_imports,
            value_import_lookup,
            locals,
            local_lookup,
            needed_imports: FxHashSet::default(),
            needed_value_imports: FxHashSet::default(),
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
        } else if self.value_import_lookup.contains_key(name) {
            // A value import (of any form) used in a type position — record its
            // local name for promotion to a declaration-legal `import type`
            // (Declaration path). For `import * as NS`, `NS` is recorded here
            // when `NS.Props` is referenced.
            self.needed_value_imports.insert(name);
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
                } else if let Some((name, _)) = self.value_import_lookup.get_key_value(token) {
                    // A value import referenced from within a type position
                    // (e.g. a local type alias body referencing it).
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
        // Type-only imports referenced in a type position. The statement is
        // reconstructed from the typed import shape so an aliased type-only
        // import keeps its imported name (`import type { Imported as Local }`).
        let mut emitted_imports = FxHashSet::default();
        for imp in self.imports {
            if self.needed_imports.contains(imp.local) {
                let stmt = imp.render_stmt();
                if emitted_imports.insert(stmt.clone()) {
                    state.type_import_stmts.push(stmt);
                }
            }
        }

        // Value imports used in a type position → declaration-only `import type`
        // promotions. Kept separate so the non-declaration paths (which emit the
        // setup body verbatim) do NOT also emit a duplicate `import type`. Each
        // form promotes to its own correct type-only statement.
        let mut emitted_promotions = FxHashSet::default();
        for imp in self.value_imports {
            if self.needed_value_imports.contains(imp.local) {
                let stmt = imp.render_stmt();
                if emitted_promotions.insert(stmt.clone()) {
                    state.declaration_promoted_type_imports.push(stmt);
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

fn absolute_type_span(type_params: &MacroTypeParams, content_offset: u32) -> Option<Span> {
    Some(Span::new(
        type_params.type_span.start.saturating_add(content_offset),
        type_params.type_span.end.saturating_add(content_offset),
    ))
}

fn model_name_from_span(name_span: Option<Span>, content_str: &str) -> String {
    name_span
        .map(|span| {
            content_str[span.start as usize..span.end as usize]
                .trim_matches(['\'', '"'])
                .to_string()
        })
        .unwrap_or_else(|| "modelValue".to_string())
}

fn build_macro_state<'a>(
    items: &[ScriptItem<'a>],
    content_str: &'a str,
    content_offset: u32,
    type_imports: &FxHashMap<&'a str, TypeImportInfo<'a>>,
    type_usage_tracker: &mut TypeUsageTracker<'a>,
) -> TscMacroState {
    let mut state = TscMacroState::default();
    let mut syntax_index = 0_u32;

    for item in items {
        let ScriptItem::Macro(m) = item else {
            continue;
        };
        let current_syntax_index = syntax_index;
        syntax_index = syntax_index.saturating_add(1);
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
                if let Some(type_params) = type_params {
                    state.semantic_slots.push(TscSemanticSlot {
                        syntax_index: current_syntax_index,
                        role: TscSemanticRole::Props,
                        type_span: absolute_type_span(type_params, content_offset),
                    });
                } else {
                    process_props(
                        object_arg.as_ref(),
                        array_arg.as_ref(),
                        content_str,
                        content_offset,
                        type_usage_tracker,
                        &mut state,
                    );
                }
            }
            ScriptMacro::DefineEmits {
                type_params,
                object_arg,
                array_arg,
                ..
            } => {
                if let Some(type_params) = type_params {
                    state.semantic_slots.push(TscSemanticSlot {
                        syntax_index: current_syntax_index,
                        role: TscSemanticRole::Emits,
                        type_span: absolute_type_span(type_params, content_offset),
                    });
                } else {
                    process_emits(
                        object_arg.as_ref(),
                        array_arg.as_ref(),
                        content_str,
                        content_offset,
                        type_usage_tracker,
                        &mut state,
                    );
                }
            }
            ScriptMacro::DefineModel {
                span,
                type_params,
                name_span,
                ..
            } => {
                if let Some(type_params) = type_params {
                    state.semantic_slots.push(TscSemanticSlot {
                        syntax_index: current_syntax_index,
                        role: TscSemanticRole::Model {
                            name: model_name_from_span(*name_span, content_str),
                        },
                        type_span: absolute_type_span(type_params, content_offset),
                    });
                } else {
                    process_model(*span, *name_span, content_str, content_offset, &mut state);
                }
            }
            ScriptMacro::WithDefaults {
                define_props_type_params,
                defaults,
                ..
            } => {
                if let Some(type_params) = define_props_type_params {
                    state.semantic_slots.push(TscSemanticSlot {
                        syntax_index: current_syntax_index,
                        role: TscSemanticRole::Props,
                        type_span: absolute_type_span(type_params, content_offset),
                    });
                }
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
            ScriptMacro::DefineExpose {
                type_params,
                object_arg,
                ..
            } => {
                process_expose(
                    type_params.as_ref(),
                    object_arg.as_ref(),
                    content_str,
                    type_usage_tracker,
                    &mut state,
                );
            }
        }
    }

    state
}

fn apply_tsc_bundle(state: &mut TscMacroState, bundle: Option<&MacroTscBundle>) {
    let Some(bundle) = bundle else {
        return;
    };

    for slot in state.semantic_slots.clone() {
        let Some(entry) = bundle
            .entries
            .iter()
            .find(|entry| entry.syntax_index == slot.syntax_index)
        else {
            continue;
        };
        let MacroTscOutcome::Complete(projection) = &entry.outcome else {
            continue;
        };

        match (&slot.role, projection) {
            (TscSemanticRole::Props, MacroTscProjection::Props { splice }) => {
                state.props_ts = Some(PropsTs::TypeText(splice.as_str().to_string()));
            }
            (TscSemanticRole::Emits, MacroTscProjection::Emits { splice }) => {
                state.terminal_emits_ts = Some(splice.as_str().to_string());
            }
            (TscSemanticRole::Model { name }, MacroTscProjection::Model { splice }) => {
                state.models.push(ModelEntry {
                    name: name.clone(),
                    ts_type: splice.as_str().to_string(),
                    map_span: slot.type_span,
                });
            }
            _ => {}
        }
    }
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

fn process_props<'a>(
    object_arg: Option<&MacroObjectArg<'a>>,
    array_arg: Option<&MacroArrayArg<'a>>,
    content_str: &'a str,
    content_offset: u32,
    type_usage_tracker: &mut TypeUsageTracker<'a>,
    state: &mut TscMacroState,
) {
    if let Some(obj) = object_arg {
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
        // Array syntax names props by string literal; the name is read from the
        // AST (unwrapping any TS wrapper), never sliced off the element span.
        state.testing_props = arr
            .elements
            .iter()
            .filter_map(|elem| {
                elem.name.map(|name| TestingPropBinding {
                    name: name.to_string(),
                    ts_type: "unknown".to_string(),
                    optional: true,
                    map_span: Some(local_to_sfc_span(elem.span, content_offset)),
                })
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

/// Map authored constructor syntax to the runtime props section.
fn runtime_type_to_constructor(rt: &RuntimeConstructorSyntax) -> &'static str {
    match rt {
        RuntimeConstructorSyntax::String => "String",
        RuntimeConstructorSyntax::Number => "Number",
        RuntimeConstructorSyntax::Boolean => "Boolean",
        RuntimeConstructorSyntax::Object => "Object",
        RuntimeConstructorSyntax::Array => "Array",
        RuntimeConstructorSyntax::Function => "Function",
        RuntimeConstructorSyntax::Symbol => "Symbol",
    }
}

fn process_emits<'a>(
    object_arg: Option<&MacroObjectArg<'a>>,
    array_arg: Option<&MacroArrayArg<'a>>,
    content_str: &str,
    content_offset: u32,
    type_usage_tracker: &mut TypeUsageTracker<'a>,
    state: &mut TscMacroState,
) {
    if let Some(arr) = array_arg {
        // Array syntax names events by string literal; the name is read from the
        // AST (unwrapping any TS wrapper), never sliced off the element span.
        for elem in &arr.elements {
            if let Some(name) = elem.name {
                state.emits_names.push(name.to_string());
                state.emits_ts.push(EmitEntry {
                    name: name.to_string(),
                    payload: EmitPayload::Unknown,
                    map_span: Some(local_to_sfc_span(elem.span, content_offset)),
                });
            }
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

fn mark_emit_payload_types(type_usage_tracker: &mut TypeUsageTracker<'_>, payload: &EmitPayload) {
    match payload {
        EmitPayload::Unknown => {}
        EmitPayload::Call { params_text } => {
            if !params_text.is_empty() {
                type_usage_tracker.mark_type_text(params_text);
            }
        }
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
    macro_span: Span,
    name_span: Option<Span>,
    content_str: &str,
    content_offset: u32,
    state: &mut TscMacroState,
) {
    let model_name = match name_span {
        Some(ns) => {
            let s = content_str[ns.start as usize..ns.end as usize].trim();
            s.trim_matches(|c: char| c == '\'' || c == '"').to_string()
        }
        None => "modelValue".to_string(),
    };

    state.models.push(ModelEntry {
        name: model_name,
        ts_type: "unknown".to_string(),
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

fn process_expose(
    type_params: Option<&MacroTypeParams>,
    object_arg: Option<&MacroObjectArg<'_>>,
    content_str: &str,
    type_usage_tracker: &mut TypeUsageTracker<'_>,
    state: &mut TscMacroState,
) {
    // Type param wins over object arg
    if let Some(tp) = type_params {
        let type_text = content_str[tp.type_span.start as usize..tp.type_span.end as usize].trim();
        state.expose_type_text = Some(type_text.to_string());
        type_usage_tracker.mark_type_text(type_text);
        return;
    }
    // Fall back to extracting property entries from object arg
    if let Some(obj) = object_arg {
        for prop in &obj.properties {
            let typeof_target = if prop.is_method {
                // Method shorthand: `focus() {}` — can't typeof
                None
            } else if prop.value_span.is_none() {
                // Shorthand: `{ foo }` → typeof foo
                Some(prop.name.to_string())
            } else {
                // Non-shorthand: `{ myVal: val }` — check if value is a simple identifier
                let val_span = prop.value_span.unwrap();
                let val_text = content_str[val_span.start as usize..val_span.end as usize].trim();
                if is_simple_ident(val_text) {
                    Some(val_text.to_string())
                } else {
                    None
                }
            };
            state.expose_entries.push(ExposeEntry {
                name: prop.name.to_string(),
                typeof_target,
            });
        }
    }
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

/// Returns true if the text is a simple JS identifier (no dots, calls, etc.).
fn is_simple_ident(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let mut chars = text.chars();
    let first = chars.next().unwrap();
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
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
                if let Some(name) = segment.split([' ', ':', '=']).find(|part| !part.is_empty()) {
                    names.push(name.to_string());
                }
                segment_start = idx + 1;
            }
            _ => {}
        }
    }

    let trailing = generic_params[segment_start..].trim();
    if let Some(name) = trailing
        .split([' ', ':', '='])
        .find(|part| !part.is_empty())
    {
        names.push(name.to_string());
    }

    names
}

// ── Step 6: generate code ─────────────────────────────────────────────────────

/// Generate a stub for Options API `<script>` blocks that preserves the
/// original script content (including `defineComponent()` props/emits/etc.)
/// so that cross-component type checking works.
///
/// When the default export is a plain object (no `defineComponent` wrapper),
/// we insert `defineComponent()` around it so that Vue's type overloads
/// infer data/methods/computed on the instance type. This makes
/// `InstanceType<typeof import('./Foo.vue.verter.ts')['default']>` resolve to
/// the full component instance rather than `never` (the public-API carrier is
/// the `.verter.ts` surface).
fn generate_options_api_stub(_component_name: &str, script_content: &str) -> TscOutput {
    let source_map = minimal_source_map();
    let encoded = BASE64_STANDARD.encode(source_map.as_bytes());

    // Parse to detect if the default export is a plain object (needs wrapping)
    let alloc = Allocator::default();
    let parser_ret = Parser::new(&alloc, script_content, SourceType::tsx()).parse();
    let parse_result = parse_script(&parser_ret.program, ScriptMode::Options, 0, script_content);

    // Find a plain object default export that needs defineComponent wrapping
    let obj_span = parse_result.items.iter().find_map(|item| {
        if let ScriptItem::DefaultExport(de) = item {
            if de.export_type == DefaultExportType::Object {
                return de.object_span;
            }
        }
        None
    });

    let code = if let Some(span) = obj_span {
        // Check if defineComponent is already imported
        let has_dc_import = parse_result.items.iter().any(|item| {
            if let ScriptItem::Import(imp) = item {
                imp.source == "vue"
                    && imp.bindings.iter().any(|b| {
                        b.name == "defineComponent"
                            && b.import_kind == Some(ImportSpecifierKind::Named)
                    })
            } else {
                false
            }
        });

        let mut result = String::with_capacity(script_content.len() + 80);
        if !has_dc_import {
            result.push_str("import { defineComponent } from \"vue\"\n");
        }
        result.push_str(&script_content[..span.start as usize]);
        result.push_str("defineComponent(");
        result.push_str(&script_content[span.start as usize..span.end as usize]);
        result.push(')');
        result.push_str(&script_content[span.end as usize..]);
        let trimmed = result.trim();
        format!("{trimmed}\n//# sourceMappingURL=data:application/json;base64,{encoded}\n")
    } else {
        // Already has defineComponent or different export type — pass through
        format!(
            "{content}\n//# sourceMappingURL=data:application/json;base64,{map}\n",
            content = script_content.trim(),
            map = encoded,
        )
    };

    TscOutput { code, source_map }
}

fn generate_empty_stub(component_name: &str) -> TscOutput {
    let name = sanitize_tsc_component_name(component_name);
    let source_map = minimal_source_map();
    let encoded = BASE64_STANDARD.encode(source_map.as_bytes());
    let code = format!(
        "import {{ defineComponent }} from \"vue\"\nconst __comp = defineComponent({{}})\ndeclare const {name}: typeof __comp\nexport default {name}\n//# sourceMappingURL=data:application/json;base64,{map}\n",
        name = name,
        map = encoded,
    );
    TscOutput { code, source_map }
}

/// Project an Options-API component's full public surface into the
/// declaration-only (`.d.<ext>.ts`) form, or `None` for a genuinely-empty
/// surface (no `props`/`emits` on the options object) so the caller falls back
/// to [`generate_declaration_empty_stub`].
///
/// The Options-API runtime `props: { … }` / `props: [ … ]` and `emits: { … }` /
/// `emits: [ … ]` are extracted (typed-IR only) through the SHARED parser
/// macro-argument extractor and fed into the SAME `process_props` /
/// `process_emits` normalization the `<script setup>` macros use — there is no
/// second prop/emit engine. The populated [`TscMacroState`] then renders through
/// [`generate_declaration_code`], the mode-agnostic declaration-SAFE renderer
/// (no runtime `defineComponent(...)` call, no `__comp`, no `typeof __comp`).
fn generate_options_api_declaration(
    component_name: &str,
    sfc_source: &str,
    options_script_content: &str,
    filename: Option<&str>,
) -> Option<TscOutput> {
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, options_script_content, SourceType::ts()).parse();
    let ctx = ScriptParseContext::new(0, options_script_content.as_bytes());
    let macro_args: OptionsComponentMacroArgs =
        extract_options_component_macro_args(&parsed.program, &ctx);

    let has_props = macro_args.props_object.is_some() || macro_args.props_array.is_some();
    let has_emits = macro_args.emits_object.is_some() || macro_args.emits_array.is_some();
    if !has_props && !has_emits {
        // Genuinely-empty surface: let the caller emit the minimal empty stub.
        return None;
    }

    // Thread the component's REAL type-import context through the SAME machinery
    // the `<script setup>` declaration path uses, so an Options-API prop typed via
    // an imported type (`type: Object as PropType<Foo>` with `Foo` imported) keeps
    // `Foo`'s import in the emitted declaration. The import first pass of
    // `parse_script` is mode-independent, so it yields the script's `ScriptItem`s
    // (including its imports) for `collect_type_imports` + `TypeUsageTracker`; the
    // `process_props` / `process_emits` calls below already register used types via
    // `mark_type_text`, and `type_usage_tracker.finalize` emits exactly the imports
    // those types reference (type-only directly, value imports promoted to `import
    // type`) — no separate import engine.
    let script = parse_script(
        &parsed.program,
        ScriptMode::Options,
        0,
        options_script_content,
    );
    let type_imports = collect_type_imports(&script.items);
    let mut type_usage_tracker =
        TypeUsageTracker::new(&script.items, options_script_content, &type_imports);
    let mut state = TscMacroState::default();

    // Props: object form populates the full `props_ts` inline surface via the
    // shared `process_props`. The array (string-name) form names props without a
    // type; surface each as an `unknown`-typed inline entry so a `props: ['msg']`
    // component still exposes `msg` in `$props` (parity with object form, which
    // `process_props` renders directly).
    if let Some(props_obj) = macro_args.props_object.as_ref() {
        process_props(
            Some(props_obj),
            None,
            options_script_content,
            0,
            &mut type_usage_tracker,
            &mut state,
        );
    } else if let Some(props_arr) = macro_args.props_array.as_ref() {
        let entries: Vec<InlinePropEntry> = props_arr
            .elements
            .iter()
            .filter_map(|elem| {
                elem.name.map(|name| InlinePropEntry {
                    name: name.to_string(),
                    optional: true,
                    ts_type: "unknown".to_string(),
                    comment: None,
                    map_span: Some(local_to_sfc_span(elem.span, 0)),
                })
            })
            .collect();
        if !entries.is_empty() {
            state.props_ts = Some(PropsTs::Inline(entries));
        }
    }

    // Emits: both object and array forms populate `emits_ts` via `process_emits`.
    if macro_args.emits_object.is_some() || macro_args.emits_array.is_some() {
        process_emits(
            macro_args.emits_object.as_ref(),
            macro_args.emits_array.as_ref(),
            options_script_content,
            0,
            &mut type_usage_tracker,
            &mut state,
        );
    }

    type_usage_tracker.finalize(&mut state);

    Some(generate_declaration_code(
        component_name,
        &state,
        sfc_source,
        filename,
        None,
        None,
        None,
    ))
}

/// The declaration-safe minimal component declaration for a no-`<script setup>`
/// Declaration request with a genuinely-empty public surface (an empty SFC, a
/// scriptless SFC, or an Options-API component declaring no props/emits).
///
/// Pure declarations only: a `DefineComponent`-typed `declare const` value with
/// an empty props/emits surface and a default export. NO `import { defineComponent }`
/// value import, NO `const __comp = …` runtime value, NO `typeof __comp` — the
/// runtime [`generate_empty_stub`] emits all three; this is the `.d.ts`-legal
/// counterpart. An Options-API component that DOES declare props/emits projects
/// its full surface via [`generate_options_api_declaration`] instead.
fn generate_declaration_empty_stub(component_name: &str) -> TscOutput {
    let name = sanitize_tsc_component_name(component_name);
    let source_map = minimal_source_map();
    let encoded = BASE64_STANDARD.encode(source_map.as_bytes());
    let code = format!(
        "declare const {name}: import(\"vue\").DefineComponent<{{}}, {{}}, any>\nexport default {name}\n//# sourceMappingURL=data:application/json;base64,{map}\n",
        name = name,
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

#[allow(clippy::too_many_arguments)]
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
    if state.terminal_emits_ts.is_some() {
        out.push_str(TERMINAL_EMITS_TO_PROPS_TYPE);
    }
    out.push_str(MACRO_STUBS);

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
        state.terminal_emits_ts.as_deref(),
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
        state.terminal_emits_ts.as_deref(),
        &state.models,
        &state.defaulted_prop_names,
        None,
    ));
    out.push_str(",\n");
    out.push_str("    $emit: ");
    out.append_rendered(render_emit_fn_type(
        &state.emits_ts,
        state.terminal_emits_ts.as_deref(),
        &state.models,
    ));
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

#[allow(clippy::too_many_arguments)]
fn generate_code(
    component_name: &str,
    state: &TscMacroState,
    sfc_source: &str,
    filename: Option<&str>,
    generic_params: Option<&str>,
    attrs_type: Option<&str>,
    narrowing: Option<&TscNarrowingInfo>,
    root_element_tag: Option<&str>,
    setup_content: &str,
) -> TscOutput {
    let needs_setup_body = !state.expose_entries.is_empty();
    let mut out = TscWriter::new(if needs_setup_body { 2048 } else { 512 });

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
    if state.terminal_emits_ts.is_some() && !needs_setup_body {
        out.push_str(
            "type __Verter_UnionToIntersection<U> = (U extends any ? (value: U) => void : never) extends ((value: infer I) => void) ? I : never\n",
        );
        out.push_str(
            "type __Verter_EmitFn<T> = T extends (...args: any[]) => any ? T : T extends Record<string, any> ? __Verter_UnionToIntersection<{ [K in keyof T]: T[K] extends any[] ? (event: K, ...args: T[K]) => void : T[K] extends (...args: infer A) => any ? (event: K, ...args: A) => void : (event: K, ...args: unknown[]) => void }[keyof T]> : (event: string, ...args: unknown[]) => void\n",
        );
    }
    if state.terminal_emits_ts.is_some() {
        out.push_str(TERMINAL_EMITS_TO_PROPS_TYPE);
    }

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

    // ── Setup body (when expose_entries is non-empty) ────────────────
    // Include macro stubs + script setup body so `typeof` can resolve
    // exposed bindings to their inferred types.
    if needs_setup_body {
        // Emit utility types needed by macro stubs
        out.push_str(
            "type __Verter_UnionToIntersection<U> = (U extends any ? (value: U) => void : never) extends ((value: infer I) => void) ? I : never\n",
        );
        out.push_str(
            "type __Verter_EmitFn<T> = T extends (...args: any[]) => any ? T : T extends Record<string, any> ? __Verter_UnionToIntersection<{ [K in keyof T]: T[K] extends any[] ? (event: K, ...args: T[K]) => void : T[K] extends (...args: infer A) => any ? (event: K, ...args: A) => void : (event: K, ...args: unknown[]) => void }[keyof T]> : (event: string, ...args: unknown[]) => void\n",
        );
        out.push_str(MACRO_STUBS);
        if let Some(gp) = generic_params {
            for name in extract_generic_param_names(gp) {
                if is_testing_decl_ident(&name) {
                    out.push_str(&format!("type {} = any\n", name));
                }
            }
        }
        if !setup_content.trim().is_empty() {
            out.push_str(setup_content);
            if !setup_content.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push('\n');
    }

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
    // Render the explicit instance shape (`new(...)` + `$props`/`$emit`/… +
    // the expose tail + closing `}`). This is the SAME public surface the
    // declaration path renders — the only `Public`-vs-`Declaration` difference
    // is the value-bearing `__OmitNew<typeof __comp> &` prefix above, which the
    // declaration path omits.
    render_instance_shape_body(
        &mut out,
        state,
        attrs_type,
        root_element_tag,
        narrowing,
        full_gp.as_deref(),
        // `Public` emits the setup body, so `typeof <exposed-binding>` resolves.
        true,
    );
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

/// Render the explicit instance-shape body of the `declare const Component`
/// declaration: the `new(...)` construct signature, the `$props`/`$emit`/
/// `$slots`/`$data`/`$attrs`/`$refs`/`$root` instance members, and the expose
/// tail, followed by the object's closing `}`.
///
/// This is the SINGLE renderer for the public instance surface — BOTH the
/// runtime `Public` path ([`generate_code`]) and the declaration-only path
/// ([`generate_declaration_code`]) call it, so the two cannot drift. The
/// caller is responsible for opening the `declare const Name: …{` line (the
/// `Public` path prefixes the value-bearing `__OmitNew<typeof __comp> &`; the
/// declaration path opens a bare `{`).
fn render_instance_shape_body(
    out: &mut TscWriter,
    state: &TscMacroState,
    attrs_type: Option<&str>,
    root_element_tag: Option<&str>,
    narrowing: Option<&TscNarrowingInfo>,
    full_gp: Option<&str>,
    expose_typeof_resolvable: bool,
) {
    // Generate simplified constructor: `new(props?: PublicProps & Props): { $props, $emit, ... }`
    // Does NOT include ComponentPublicInstance in the return type — CPI has many
    // generic params that TypeScript expands, causing "Type instantiation is
    // excessively deep" with self-referential prop types (e.g. Action → callback(Action)).
    // The explicit $props/$emit/$slots/$data/$attrs/$refs fields cover instance access.
    match full_gp {
        Some(gp) => {
            out.push_str(&format!(
                "  new<{gp}>(props?: import(\"vue\").PublicProps & "
            ));
        }
        None => {
            out.push_str("  new(props?: import(\"vue\").PublicProps & ");
        }
    }
    out.append_rendered(render_full_props_type(
        &state.props_ts,
        &state.emits_ts,
        state.terminal_emits_ts.as_deref(),
        &state.models,
        &state.defaulted_prop_names,
        narrowing,
    ));
    out.push_str("): {\n");

    out.push_str("    $props: import(\"vue\").PublicProps & ");
    out.append_rendered(render_full_props_type(
        &state.props_ts,
        &state.emits_ts,
        state.terminal_emits_ts.as_deref(),
        &state.models,
        &state.defaulted_prop_names,
        narrowing,
    ));
    out.push_str(",\n");

    out.push_str("    $emit: ");
    out.append_rendered(render_emit_fn_type(
        &state.emits_ts,
        state.terminal_emits_ts.as_deref(),
        &state.models,
    ));
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

    if let Some(ref expose_type) = state.expose_type_text {
        // Type-param form: `defineExpose<{ foo: number }>()`
        out.push_str(&format!("  }} & {}\n", expose_type));
    } else if !state.expose_entries.is_empty() {
        // Runtime object form: `defineExpose({ foo, bar: val })`
        // Build ShallowUnwrapRef intersection over each entry.
        out.push_str("  }\n    & import(\"vue\").ShallowUnwrapRef<{ ");
        for (i, entry) in state.expose_entries.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            if expose_typeof_resolvable {
                // `Public` path: the setup body is emitted, so `typeof <ident>`
                // resolves the binding's inferred type. A method/complex entry
                // (`typeof_target == None`) falls back to `any`.
                match &entry.typeof_target {
                    Some(target) => {
                        out.push_str(&format!("{}: typeof {}", entry.name, target));
                    }
                    None => {
                        out.push_str(&format!("{}: any", entry.name));
                    }
                }
            } else {
                // Declaration path: the setup body is OMITTED, so the exposed
                // binding is NOT in scope — `typeof <ident>` would be an unbound
                // value reference (an erroring declaration). Render a
                // declaration-legal placeholder instead. `unknown` (not `any`)
                // preserves the public member shape without inventing a type or
                // silently widening to an unsound `any`.
                //
                // TODO(follow-up): this is a PRECISION placeholder, not the final
                // declaration strategy. A runtime-object `defineExpose({ x })`
                // entry's exact type is the inferred type of the setup binding
                // `x`, which is not yet captured in the typed macro/codegen state
                // (only the identifier name is). Capturing resolved setup-binding
                // types — at the point setup bindings are already classified — is
                // required so the declaration can render each exposed member's
                // exact type; this MUST land before the declaration carrier is
                // wired to a consuming engine. The type-PARAMETER form
                // (`defineExpose<{ x: T }>()`) already renders its exact type via
                // `expose_type_text` above and is unaffected.
                out.push_str(&format!("{}: unknown", entry.name));
            }
        }
        out.push_str(" }>\n");
    } else {
        out.push_str("  }\n");
    }
    out.push_str("}\n");
}

/// Generate the declaration-only public surface (`.d.<ext>.ts`).
///
/// A declaration-safe surface: type-only imports, local type declarations, the
/// props/emits/slots-derived interfaces, and an explicit
/// `declare const Component: { new(...): { … } } …; export default Component;`.
/// It renders the SAME public instance surface [`generate_code`] computes (via
/// the shared [`render_instance_shape_body`]), but as an EXPLICIT declaration
/// instead of `typeof` over a runtime `defineComponent` value.
///
/// It emits NO runtime / value code: NO `import { defineComponent }`, NO macro
/// stubs, NO `<script setup>` executable body, NO `const __comp = …`, NO
/// `typeof __comp`.
///
/// # Declaration contract: the instance/public-props surface
///
/// The declaration carries the component's INSTANCE surface — the
/// props/emits/slots/expose a consumer needs to type-check `<Component />`,
/// `createApp(Component)`, and instance/prop access — projected through the
/// explicit `new(...)` construct signature. It deliberately does NOT reproduce
/// the `Public` path's `__OmitNew<typeof __comp> &` prefix: that prefix is a
/// value-bearing `typeof` over the runtime `defineComponent` const and is not
/// declaration-legal, so non-constructor component/static members it carried are
/// NOT projected here. The instance/public-props surface is the load-bearing
/// contract for importing a component and using it as a component value.
///
/// # Expose surface
///
/// The `defineExpose` surface the `Public` path resolves via the setup body +
/// `typeof` is rendered here from the typed expose state: the type-parameter
/// form (`defineExpose<{ … }>()` → `expose_type_text`) projects its exact type;
/// the runtime-object form (`defineExpose({ x })` → `expose_entries`) renders
/// each member with a declaration-legal placeholder (`unknown`) rather than the
/// `typeof <setup-binding>` the omitted setup body would require (see
/// [`render_instance_shape_body`]'s `expose_typeof_resolvable`). Driven from the
/// typed state, never a re-parse of source text.
fn generate_declaration_code(
    component_name: &str,
    state: &TscMacroState,
    sfc_source: &str,
    filename: Option<&str>,
    generic_params: Option<&str>,
    attrs_type: Option<&str>,
    root_element_tag: Option<&str>,
) -> TscOutput {
    let mut out = TscWriter::new(512);

    if state.terminal_emits_ts.is_some() {
        out.push_str(
            "type __Verter_UnionToIntersection<U> = (U extends any ? (value: U) => void : never) extends ((value: infer I) => void) ? I : never\n",
        );
        out.push_str(
            "type __Verter_EmitFn<T> = T extends (...args: any[]) => any ? T : T extends Record<string, any> ? __Verter_UnionToIntersection<{ [K in keyof T]: T[K] extends any[] ? (event: K, ...args: T[K]) => void : T[K] extends (...args: infer A) => any ? (event: K, ...args: A) => void : (event: K, ...args: unknown[]) => void }[keyof T]> : (event: string, ...args: unknown[]) => void\n",
        );
        out.push_str(TERMINAL_EMITS_TO_PROPS_TYPE);
    }

    // ── Type import statements (declaration-legal `import type …`) ────
    for stmt in &state.type_import_stmts {
        out.push_str(stmt);
        out.push('\n');
    }
    // Value imports used in a type position, promoted to declaration-legal
    // `import type` (the setup body that brought them into scope in the runtime
    // path is omitted here).
    for stmt in &state.declaration_promoted_type_imports {
        out.push_str(stmt);
        out.push('\n');
    }

    // ── Local type declarations ───────────────────────────────────────
    for lt in &state.local_types {
        out.push_str(lt);
        out.push('\n');
    }
    out.push('\n');

    // ── declare const ComponentName ───────────────────────────────────
    // The declaration form is the explicit instance shape WITHOUT the
    // value-bearing `__OmitNew<typeof __comp> &` prefix the runtime path uses:
    // a `.d.ts` has no runtime `__comp` value to take `typeof` of, so the
    // declaration opens a bare object type whose `new(...)` carries the full
    // public `$props`/`$emit`/`$slots`/… surface.
    out.push_str(&format!("declare const {component_name}: {{\n"));

    // Declaration mode does not narrow on root conditions (narrowing is a
    // `Public`-only template-driven projection); the instance shape is rendered
    // without narrowing generics.
    let full_gp = generic_params.map(str::to_string);
    render_instance_shape_body(
        &mut out,
        state,
        attrs_type,
        root_element_tag,
        None,
        full_gp.as_deref(),
        // Declaration OMITS the setup body, so `typeof <exposed-binding>` is
        // unbound — render exposed members with a declaration-legal placeholder.
        false,
    );
    out.push_str(&format!("export default {component_name}\n"));

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

fn render_emit_fn_type(
    emits: &[EmitEntry],
    terminal_emits: Option<&str>,
    models: &[ModelEntry],
) -> RenderedText {
    let mut rendered = RenderedText::default();
    if emits.is_empty() && terminal_emits.is_none() && models.is_empty() {
        rendered.push_str("(event: string, ...args: unknown[]) => void");
        return rendered;
    }

    let mut needs_join = false;
    if let Some(splice) = terminal_emits {
        rendered.push_str("__Verter_EmitFn<");
        rendered.push_str(splice);
        rendered.push_str(">");
        needs_join = true;
    }
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
    terminal_emits: Option<&str>,
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
    if let Some(splice) = terminal_emits {
        let mut terminal = RenderedText::default();
        terminal.push_str("__Verter_EmitsToProps<__Verter_EmitFn<");
        terminal.push_str(splice);
        terminal.push_str(">>");
        parts.push(terminal);
    }
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

/// Convert runtime-constructor syntax to a TypeScript type string.
///
/// When a union contains `Function`, the arrow type is wrapped in parentheses
/// to avoid TS1385 ("Function type notation must be parenthesized when used
/// in a union or intersection type").
fn runtime_types_to_ts(types: &[RuntimeConstructorSyntax]) -> String {
    if types.is_empty() {
        return "unknown".to_string();
    }
    let needs_parens = types.len() > 1;
    let ts: Vec<&str> = types
        .iter()
        .map(|t| match t {
            RuntimeConstructorSyntax::String => "string",
            RuntimeConstructorSyntax::Number => "number",
            RuntimeConstructorSyntax::Boolean => "boolean",
            RuntimeConstructorSyntax::Object => "Record<string, unknown>",
            RuntimeConstructorSyntax::Array => "unknown[]",
            RuntimeConstructorSyntax::Function => {
                if needs_parens {
                    "((...args: unknown[]) => unknown)"
                } else {
                    "(...args: unknown[]) => unknown"
                }
            }
            RuntimeConstructorSyntax::Symbol => "symbol",
        })
        .collect();
    ts.join(" | ")
}

fn minimal_source_map() -> String {
    r#"{"version":3,"file":"","sourceRoot":"","sources":[],"names":[],"mappings":""}"#.to_string()
}

/// Build the V3 source-map JSON (oxc_sourcemap) for a generated TypeScript
/// carrier against its authored SFC source: one source (the component file,
/// content embedded), one token per mapping. This is the exact JSON shape the
/// carrier store publishes and the editor plugin consumes for `.vue` /
/// `.svelte` carriers alike.
pub fn build_tsc_source_map(
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
