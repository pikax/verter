//! IDE code generation for the Vue SFC compiler.
//!
//! Generates valid `.tsx` or `.jsx` output from a parsed SFC for type checking.
//! Unlike the VDOM/Vapor codegen backends (which produce render functions), this
//! module produces a single file with:
//!
//! - **Script block**: preserves types, macros, and imports (TS or JSDoc)
//! - **Template block**: converts Vue template syntax to valid JSX
//!
//! The output mode depends on the SFC's script language:
//! - **TypeScript** (`<script lang="ts">` or `<script setup lang="ts">`): `.tsx`
//!   with TypeScript annotations and generics.
//! - **JavaScript** (`<script>` or `<script lang="js">`): `.jsx` with JSDoc
//!   annotations instead of TypeScript syntax.
//!
//! The IDE output is used by the LSP for hover, completions, go-to-definition,
//! and diagnostics, and by the playground's "Types" tab.
//!
//! ## Architecture
//!
//! This is a **top-level module**, not a template codegen variant. It receives
//! the full `Syntax` AST (script blocks + template AST) and generates two
//! independent output blocks with source maps. It does NOT implement
//! `TemplateCodeGen` or use the shared walker.
//!
//! ```text
//! compile() orchestrator
//!   ├── generate_script()      → JS/TS script block (existing)
//!   ├── generate_template()    → render function (existing)
//!   ├── ide::script::generate_ide_script()    → IDE script block
//!   └── ide::template::generate_ide_template() → IDE template JSX
//! ```

pub mod condition;
pub mod condition_narrowing;
pub mod script;
pub mod script_recover;
pub mod template;

/// The reserved public-API carrier virtual-file suffix (`.verter.ts`).
///
/// A component's PUBLIC type lives in the REDIRECT-reached public-API carrier
/// (`Foo.vue.verter.ts` / `Foo.svelte.verter.ts`) — the cross-package /
/// project-reference redirect target. It is NEVER the in-project bare-import
/// target (a bare `import X from "./Comp.vue"` resolves natively to the
/// `.d.vue.ts` declaration carrier through the engine's basename-append probe).
///
/// The IDE carrier self-imports this API surface to type the component
/// instance and re-exports its public default, so the declaration carrier
/// surfaces the component's public type.
///
/// This MIRRORS `verter_workspace::CARRIER_API_VIRTUAL_SUFFIX` (the single
/// naming authority). `verter_compiler` is a lower crate than `verter_workspace`
/// and cannot import it, so the two derivations are kept byte-for-byte in sync
/// by [`tests::carrier_api_suffix_matches_workspace_naming`] + the
/// `virtual_file_naming_*` guards rather than a shared import.
pub const CARRIER_API_VIRTUAL_SUFFIX: &str = ".verter.ts";

/// Options for IDE script generation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IdeScriptOptions<'a> {
    /// Component name extracted from filename.
    pub component_name: &'a str,
    /// Sanitized JS identifier for the component (e.g., `_123Widget` for `123-widget.vue`).
    pub js_component_name: &'a str,
    /// Original filename (e.g., `"App.vue"`) used for self-import.
    pub filename: &'a str,
    /// Scoped style ID (e.g., `"data-v-abc123"`).
    pub scope_id: &'a str,
    /// Whether any `<style scoped>` block exists.
    pub has_scoped_style: bool,
    /// Runtime module name (e.g., `"vue"`).
    pub runtime_module_name: &'a str,
    /// Types module name (e.g., `"$verter/types"` or `"@verter/types"`).
    pub types_module_name: &'a str,
    /// Whether the SFC uses Vapor mode.
    pub is_vapor: bool,
    /// Embed `declare module "@verter/types"` in TSX output.
    /// When `false`, the ambient block is omitted (requires real package).
    pub embed_ambient_types: bool,
    /// When `true`, emit JavaScript + JSDoc instead of TypeScript.
    /// `false` → `.tsx` with type annotations and generics.
    /// `true` → `.jsx` with JSDoc annotations, no TS syntax.
    pub is_jsx: bool,
    /// Experimental: Enable conditional root generic narrowing.
    /// When a root v-if references a prop, the component's type signature
    /// narrows based on the prop value passed by the parent.
    pub conditional_root_narrowing: bool,
    /// Binding names referenced in style `v-bind()` expressions, derived SOUNDLY
    /// from the typed AST (each `v-bind()` expression parsed and its identifier
    /// roots collected — never a `.split('.')` string heuristic). Used to emit
    /// `void(name)` and to keep a style-used binding's value-read unwrap entry.
    pub style_v_bind_vars: Vec<String>,
    /// Whether [`Self::style_v_bind_vars`] is a COMPLETE, sound usage set. `false`
    /// when any discovered `v-bind()` expression failed to parse (or no style
    /// scan ran), in which case unused-binding liveness fails open (every binding
    /// is treated as used — no TS6133 demotion).
    pub style_usage_complete: bool,
    /// CSS module info: `(module_name, class_names)` pairs.
    /// `module_name` is `"$style"` for `<style module>` or the custom name
    /// from `<style module="classes">`.
    pub css_modules: Vec<CssModuleInfo>,
    /// AST-driven set of identifier names referenced by the `<template>`.
    ///
    /// Built from the typed template IR (expression bindings ∪ v-for sources ∪
    /// component tag names), never a raw-source scan. A setup binding present
    /// here is "used in template" and keeps its value-read unwrap entry so it is
    /// not falsely flagged unused; a binding absent here AND absent from the
    /// script body / style `v-bind` is genuinely unused and emits a type-only
    /// unwrap entry so TypeScript surfaces TS6133 at its source declaration.
    ///
    /// `None` when no template parse was available (parse errors, template-less
    /// SFC) — in that case the lowering conservatively keeps every value read.
    ///
    /// Built from the `ide_completion = false` template-expression overlay (the
    /// COMPLETE liveness overlay), NOT the `ide_completion = true` IDE/completion
    /// overlay, which intentionally suppresses real references for completion and
    /// would mis-report a genuinely-used binding as unused.
    pub template_used_vars: Option<rustc_hash::FxHashSet<String>>,
}

/// CSS module information for IDE codegen.
#[derive(Debug, Clone)]
pub struct CssModuleInfo {
    /// Binding name (e.g., `"$style"` or `"classes"`)
    pub binding_name: String,
    /// CSS class names found in the module block.
    pub class_names: Vec<String>,
}

/// Options for IDE template generation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IdeTemplateOptions<'a> {
    /// Self-referencing component name (PascalCase).
    pub self_name: &'a str,
    /// Whether to preserve HTML comments in JSX output.
    pub comments: bool,
    /// When `true`, emit JavaScript (no TS annotations in template).
    pub is_jsx: bool,
    /// Experimental: strict slot children type checking.
    /// Emits `strictRenderSlot` calls to enforce typed slot children.
    pub strict_slots: bool,
}

// ── Generic info ─────────────────────────────────────────────────

/// Prefix for sanitised generic type parameter names.
const GENERIC_SANITISE_PREFIX: &str = "__VERTER__TS__";

/// Parsed and processed generic type parameters for IDE emission.
///
/// Built from the raw `generic="..."` attribute, this struct holds the
/// original source, extracted names, sanitised names (prefixed to avoid
/// collisions), and a full declaration string for the public API types.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IdeGenericInfo {
    /// Original generic source (e.g., `"T extends object"`).
    pub source: String,
    /// Extracted parameter names (e.g., `["T"]`).
    pub names: Vec<String>,
    /// Sanitised parameter names with `__VERTER__TS__` prefix (e.g., `["__VERTER__TS__T"]`).
    pub sanitised_names: Vec<String>,
    /// Full declaration string with sanitised names, constraints, and defaults.
    /// (e.g., `"__VERTER__TS__T extends object = any"`).
    pub declaration: String,
}

#[allow(dead_code)]
impl IdeGenericInfo {
    /// Build generic info from the raw generic attribute source string.
    ///
    /// Returns `None` if the source is empty or parsing fails.
    pub fn from_source(generic_str: &str) -> Option<Self> {
        let trimmed = generic_str.trim();
        if trimmed.is_empty() {
            return None;
        }

        let alloc = oxc_allocator::Allocator::default();
        let result = crate::utils::oxc::vue::parse_generic(&alloc, trimmed, 0);

        if !result.is_ok() {
            return None;
        }

        let source_bytes = trimmed.as_bytes();

        let names: Vec<String> = result
            .params
            .iter()
            .map(|p| String::from_utf8_lossy(p.name(source_bytes)).into_owned())
            .collect();

        let sanitised_names: Vec<String> = names
            .iter()
            .map(|n| format!("{}{}", GENERIC_SANITISE_PREFIX, n))
            .collect();

        // Build declaration: for each param, emit `{sanitised_name} [extends {sanitised_constraint}] [= {sanitised_default} | = any]`
        let mut declaration_parts: Vec<String> = Vec::with_capacity(result.params.len());
        for (i, param) in result.params.iter().enumerate() {
            let mut part = sanitised_names[i].clone();

            if let Some(constraint_bytes) = param.constraint(source_bytes) {
                let constraint_str = std::str::from_utf8(constraint_bytes).unwrap_or("");
                let sanitised_constraint =
                    sanitise_type_references(constraint_str, &names, &sanitised_names);
                part.push_str(" extends ");
                part.push_str(&sanitised_constraint);
            }

            if let Some(default_bytes) = param.default_type(source_bytes) {
                let default_str = std::str::from_utf8(default_bytes).unwrap_or("");
                let sanitised_default =
                    sanitise_type_references(default_str, &names, &sanitised_names);
                part.push_str(" = ");
                part.push_str(&sanitised_default);
            } else {
                part.push_str(" = any");
            }

            declaration_parts.push(part);
        }

        let declaration = declaration_parts.join(", ");

        Some(IdeGenericInfo {
            source: trimmed.to_string(),
            names,
            sanitised_names,
            declaration,
        })
    }

    /// Returns `<{source}>` or empty string.
    pub fn source_bracket(&self) -> String {
        format!("<{}>", self.source)
    }

    /// Returns `<{names}>` (comma-separated) or empty string.
    pub fn names_bracket(&self) -> String {
        format!("<{}>", self.names.join(", "))
    }

    /// Returns `<{sanitised_names}>` (comma-separated) or empty string.
    pub fn sanitised_names_bracket(&self) -> String {
        format!("<{}>", self.sanitised_names.join(", "))
    }

    /// Returns `<{declaration}>` or empty string.
    pub fn declaration_bracket(&self) -> String {
        format!("<{}>", self.declaration)
    }
}

/// Replace generic name references in a type string with sanitised names.
fn sanitise_type_references(
    type_str: &str,
    names: &[String],
    sanitised_names: &[String],
) -> String {
    let mut result = type_str.to_string();
    for (name, sanitised) in names.iter().zip(sanitised_names.iter()) {
        result = replace_word_boundary(&result, name, sanitised);
    }
    result
}

/// Replace all occurrences of `needle` in `haystack` where the needle is at a
/// word boundary (not preceded/followed by `[a-zA-Z0-9_]`).
fn replace_word_boundary(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || haystack.is_empty() {
        return haystack.to_string();
    }

    let haystack_bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    let needle_len = needle_bytes.len();
    let mut result = String::with_capacity(haystack.len());
    let mut pos = 0;

    while pos + needle_len <= haystack_bytes.len() {
        if &haystack_bytes[pos..pos + needle_len] == needle_bytes {
            // Check word boundary before
            let before_ok = pos == 0 || !is_ident_char(haystack_bytes[pos - 1]);
            // Check word boundary after
            let after_ok = pos + needle_len >= haystack_bytes.len()
                || !is_ident_char(haystack_bytes[pos + needle_len]);

            if before_ok && after_ok {
                result.push_str(replacement);
                pos += needle_len;
                continue;
            }
        }
        result.push(haystack_bytes[pos] as char);
        pos += 1;
    }

    // Append remaining bytes
    while pos < haystack_bytes.len() {
        result.push(haystack_bytes[pos] as char);
        pos += 1;
    }

    result
}

#[inline]
fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ── Shared IDE Helpers ──────────────────────────────────────────

use crate::types::NodeProp;

/// Extract the directive name from a prop.
///
/// Handles shorthand (`:`→`"bind"`, `@`→`"on"`, `#`→`"slot"`) and
/// full `v-*` prefix.
pub(crate) fn get_directive_name<'a>(prop: &NodeProp, source: &'a str) -> &'a str {
    let name = &source[prop.start as usize..prop.name_end as usize];

    if name.starts_with(':') || name.starts_with('.') {
        return "bind";
    }
    if name.starts_with('@') {
        return "on";
    }
    if name.starts_with('#') {
        return "slot";
    }

    name.strip_prefix("v-").unwrap_or(name)
}

/// Convert a Vue event name to JSX event prop name (PascalCase segments).
///
/// - `click` → `onClick`
/// - `update:modelValue` → `onUpdate:modelValue`
/// - `custom-event` → `onCustomEvent`
pub(crate) fn event_to_jsx_name(event_name: &str) -> String {
    if let Some(rest) = event_name.strip_prefix("update:") {
        return format!("onUpdate:{}", rest);
    }

    // Prepend "on" and capitalize first letter, preserving the rest as-is.
    // Hyphens are NOT removed — kebab-case events stay kebab-case.
    // Names with hyphens (e.g. "onCustom-event") are not valid JSX identifiers
    // and will be emitted using spread syntax by the caller.
    let mut result = String::with_capacity(event_name.len() + 2);
    result.push_str("on");
    let mut chars = event_name.chars();
    if let Some(first) = chars.next() {
        for upper in first.to_uppercase() {
            result.push(upper);
        }
        result.extend(chars);
    }
    result
}

/// Build the event-handler PARAMETER TUPLE type for a template event binding.
///
/// Derives the parameter-tuple shape from the element's event surface:
/// - Native element: `Parameters<NonNullable<import('vue').IntrinsicElementAttributes["tag"]["onEvent"]>>`
/// - Component: `Parameters<NonNullable<Required<InstanceType<typeof Binding>["$props"]>["onEvent"]>>`
///
/// Used by the script-handler inference path (`event_inference`), which annotates a
/// function declaration's params as `...[e]: <this type>`, and — for the COMPONENT
/// case only — by the template `$event` spread path (`template::von`, indexed
/// `<this type>[0]`). The native spread `$event` instead uses the ambient
/// [`native_dom_event_payload_type`], because the `import('vue')` indexed formula
/// here resolves only under a full `tsserver`, not the native preview (`tsgo`).
///
/// `event_prop` is the JSX prop name produced by [`event_to_jsx_name`] (`onClick`).
/// Returns `None` for a component whose binding could not be resolved, or for a tag
/// kind that has no typed event surface.
pub(crate) fn event_handler_params_type(
    tag_name: &str,
    tag_type: crate::ast::types::TagType,
    component_binding: Option<&str>,
    event_prop: &str,
) -> Option<String> {
    use crate::ast::types::TagType;
    match tag_type {
        TagType::Element => Some(format!(
            "Parameters<NonNullable<import('vue').IntrinsicElementAttributes[\"{tag_name}\"][\"{event_prop}\"]>>"
        )),
        TagType::Component => Some(format!(
            "Parameters<NonNullable<Required<InstanceType<typeof {}>[\"$props\"]>[\"{event_prop}\"]>>",
            component_binding?
        )),
        _ => None,
    }
}

/// Ambient DOM event-payload type for a NATIVE-element `$event`, keyed by the Vue
/// event name (`click`, `mousedown`, `click-overlay`).
///
/// Used by the template `$event` spread path (`template::von`) where JSX contextual
/// typing cannot flow and an explicit annotation is required. Unlike the
/// `import('vue').IntrinsicElementAttributes[...]` formula, this references ONLY
/// ambient lib.dom types, so it resolves under every TypeProvider — including the
/// native preview (`tsgo`), which does not resolve `import('vue')` *type queries*
/// from the generated virtual `.vue.tsx` and treats `JSX.IntrinsicElements[...]`
/// indexed access as `any` (the arbitrary-element index signature on Vue's global
/// JSX namespace). A real `<button onClick={…}>` still type-checks under `tsgo`
/// because JSX element checking resolves the named member — only *manual* indexed
/// access collapses; the spread object literal cannot use JSX element checking.
///
/// `GlobalEventHandlersEventMap[name]` is the canonical DOM event type for a known
/// event (`"click"` → `PointerEvent`). The intersected `{ [k: string]: Event }`
/// fallback keeps a non-standard / custom native event name (e.g. `click-overlay`)
/// typed as the base `Event` instead of producing a `ts(2339)` "property does not
/// exist" error — a precise type where one exists, a safe base type otherwise.
pub(crate) fn native_dom_event_payload_type(event_name: &str) -> String {
    format!("(GlobalEventHandlersEventMap & {{ [___VERTER___EventKey: string]: Event }})[\"{event_name}\"]")
}

/// Shared inventory of the component bindings visible to the generated IDE TSX template.
///
/// A component referenced in a template types through an in-scope const: either a local
/// script binding (an import or setup const, known to the `BindingResolver` /
/// `available_bindings`) OR a GlobalComponents fallback const synthesized by
/// [`crate::ide::script::wrapper::emit_global_component_fallbacks`] for a globally-registered
/// component that the script never imports.
///
/// Before this inventory existed, template event typing consulted only the resolver, so a
/// globally-registered component's `@event` payload could not see its fallback const and
/// fell back to `$event: any`. This inventory carries the fallback const names — collected
/// ONCE via [`crate::ide::script::wrapper::collect_global_component_fallbacks`] and shared by
/// the emission, the template `$event`/arrow spread path ([`crate::ide::template::von`]), and
/// the simple-handler param inference ([`crate::ide::script::event_inference`]) — so every
/// surface resolves the same component to the same typed const.
#[derive(Debug, Clone, Default)]
pub(crate) struct TemplateComponentBindings {
    /// PascalCase GlobalComponents fallback const names, in first-seen source order.
    fallback_consts: Vec<String>,
}

impl TemplateComponentBindings {
    /// Build from a collected fallback list (see `collect_global_component_fallbacks`).
    pub(crate) fn new(fallback_consts: Vec<String>) -> Self {
        Self { fallback_consts }
    }

    /// Whether `name` is a GlobalComponents fallback const emitted for this template.
    pub(crate) fn is_fallback_const(&self, name: &str) -> bool {
        self.fallback_consts.iter().any(|c| c == name)
    }

    /// Resolve a template tag to the in-scope const that types it, consulting local script
    /// bindings (`local_is_known`) first and the GlobalComponents fallback consts second.
    ///
    /// Returns `None` for non-component tags and for components with neither a local binding
    /// nor a fallback const (builtins, member-expression tags, or unregistered components —
    /// the cases that fall through to explicit `any`).
    pub(crate) fn resolve(
        &self,
        tag_name: &str,
        tag_type: crate::ast::types::TagType,
        local_is_known: impl Fn(&str) -> bool,
    ) -> Option<String> {
        if !matches!(tag_type, crate::ast::types::TagType::Component) {
            return None;
        }
        crate::ide::script::event_inference::resolve_component_binding_name(tag_name, |n| {
            local_is_known(n) || self.is_fallback_const(n)
        })
    }
}

// ── Utilities ──────────────────────────────────────────────────────

/// Sanitize a filename into a valid JavaScript identifier.
///
/// Rules:
/// - Strip file extension (`.vue`, `.setup.vue`)
/// - Convert kebab-case and dot-separated to PascalCase
/// - Strip non-alphanumeric characters
/// - Prefix with `_` if starts with a digit
/// - Fallback to `"Component"` if empty
pub fn sanitize_js_identifier(filename: &str) -> String {
    // Extract basename (strip directory separators)
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or(filename);

    // Strip extensions: .setup.vue, .vue, or just last extension
    let stem = basename
        .strip_suffix(".setup.vue")
        .or_else(|| basename.strip_suffix(".vue"))
        .or_else(|| basename.rsplit_once('.').map(|(stem, _)| stem))
        .unwrap_or(basename);

    if stem.is_empty() {
        return "Component".to_string();
    }

    // Convert to PascalCase: split on non-alphanumeric, capitalize each segment
    let mut result = String::with_capacity(stem.len());
    let mut capitalize_next = true;

    for ch in stem.chars() {
        if ch.is_alphanumeric() {
            if capitalize_next {
                for upper in ch.to_uppercase() {
                    result.push(upper);
                }
                capitalize_next = false;
            } else {
                result.push(ch);
            }
        } else {
            // Non-alphanumeric characters act as separators
            capitalize_next = true;
        }
    }

    if result.is_empty() {
        return "Component".to_string();
    }

    // Prefix with `_` if starts with a digit
    if result.as_bytes()[0].is_ascii_digit() {
        result.insert(0, '_');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The API-carrier suffix must hold its documented shape: it carries the
    /// reserved `.verter.` infix (the REDIRECT-reached cross-package target).
    /// `verter_compiler` cannot depend on `verter_workspace`, so the literal
    /// byte-equality with `verter_workspace::CARRIER_API_VIRTUAL_SUFFIX` is
    /// guarded cross-crate by `virtual_file_naming_characterization` in
    /// `verter_session`; this local test pins the shape so a drift here is
    /// caught at the source crate too.
    #[test]
    fn carrier_api_suffix_matches_workspace_naming() {
        assert_eq!(CARRIER_API_VIRTUAL_SUFFIX, ".verter.ts");
        // The reserved `.verter.` infix marks the API surface — the
        // redirect-reached cross-package target, never a bare-import probe
        // target (a bare `./Comp.vue` resolves natively to the `.d.vue.ts`
        // declaration carrier, which is bare-probed, not `.verter.`-infixed).
        assert!(CARRIER_API_VIRTUAL_SUFFIX.contains(".verter."));
    }

    #[test]
    fn sanitize_basic() {
        assert_eq!(sanitize_js_identifier("my-component.vue"), "MyComponent");
    }

    #[test]
    fn sanitize_numeric_start() {
        assert_eq!(sanitize_js_identifier("123-widget.vue"), "_123Widget");
    }

    #[test]
    fn sanitize_index() {
        assert_eq!(sanitize_js_identifier("index.vue"), "Index");
    }

    #[test]
    fn sanitize_setup_extension() {
        assert_eq!(sanitize_js_identifier("my-comp.setup.vue"), "MyComp");
    }

    #[test]
    fn sanitize_special_chars() {
        assert_eq!(sanitize_js_identifier("special@chars.vue"), "SpecialChars");
    }

    #[test]
    fn sanitize_empty_stem() {
        assert_eq!(sanitize_js_identifier(".vue"), "Component");
    }

    #[test]
    fn sanitize_no_extension() {
        assert_eq!(sanitize_js_identifier("App"), "App");
    }

    #[test]
    fn sanitize_with_path() {
        assert_eq!(
            sanitize_js_identifier("src/components/my-button.vue"),
            "MyButton"
        );
    }

    #[test]
    fn sanitize_dot_separated() {
        assert_eq!(sanitize_js_identifier("my.comp.vue"), "MyComp");
    }

    #[test]
    fn sanitize_already_pascal() {
        assert_eq!(sanitize_js_identifier("MyComponent.vue"), "MyComponent");
    }

    #[test]
    fn sanitize_all_special_chars() {
        assert_eq!(sanitize_js_identifier("----.vue"), "Component");
    }

    // ── replace_word_boundary tests ──────────────────────────────

    #[test]
    fn word_boundary_simple_replace() {
        assert_eq!(
            replace_word_boundary("T extends T", "T", "X"),
            "X extends X"
        );
    }

    #[test]
    fn word_boundary_no_replace_in_prefix() {
        assert_eq!(
            replace_word_boundary("T extends T | TFoo", "T", "X"),
            "X extends X | TFoo"
        );
    }

    #[test]
    fn word_boundary_no_replace_in_suffix() {
        assert_eq!(
            replace_word_boundary("FooT extends T", "T", "X"),
            "FooT extends X"
        );
    }

    #[test]
    fn word_boundary_underscore_is_ident() {
        assert_eq!(
            replace_word_boundary("_T extends T", "T", "X"),
            "_T extends X"
        );
    }

    #[test]
    fn word_boundary_generic_angle_brackets() {
        assert_eq!(
            replace_word_boundary("Array<T>", "T", "__X__"),
            "Array<__X__>"
        );
    }

    #[test]
    fn word_boundary_empty_needle() {
        assert_eq!(replace_word_boundary("hello", "", "X"), "hello");
    }

    #[test]
    fn word_boundary_empty_haystack() {
        assert_eq!(replace_word_boundary("", "T", "X"), "");
    }

    #[test]
    fn word_boundary_multiple_occurrences() {
        assert_eq!(replace_word_boundary("T | T & T", "T", "Y"), "Y | Y & Y");
    }

    // ── IdeGenericInfo tests ─────────────────────────────────────

    #[test]
    fn generic_info_simple_param() {
        let info = IdeGenericInfo::from_source("T").unwrap();
        assert_eq!(info.names, vec!["T"]);
        assert_eq!(info.sanitised_names, vec!["__VERTER__TS__T"]);
        assert_eq!(info.declaration, "__VERTER__TS__T = any");
    }

    #[test]
    fn generic_info_constraint() {
        let info = IdeGenericInfo::from_source("T extends string").unwrap();
        assert_eq!(info.names, vec!["T"]);
        assert_eq!(info.declaration, "__VERTER__TS__T extends string = any");
    }

    #[test]
    fn generic_info_constraint_and_default() {
        let info = IdeGenericInfo::from_source("T extends object = {}").unwrap();
        assert_eq!(info.declaration, "__VERTER__TS__T extends object = {}");
    }

    #[test]
    fn generic_info_cross_reference_sanitisation() {
        let info = IdeGenericInfo::from_source("T, U extends Array<T>").unwrap();
        assert_eq!(info.names, vec!["T", "U"]);
        assert_eq!(
            info.declaration,
            "__VERTER__TS__T = any, __VERTER__TS__U extends Array<__VERTER__TS__T> = any"
        );
    }

    #[test]
    fn generic_info_multiple_mixed() {
        let info = IdeGenericInfo::from_source("K extends string, V").unwrap();
        assert_eq!(info.names, vec!["K", "V"]);
        assert_eq!(
            info.declaration,
            "__VERTER__TS__K extends string = any, __VERTER__TS__V = any"
        );
    }

    #[test]
    fn generic_info_default_type() {
        let info = IdeGenericInfo::from_source("T = string").unwrap();
        assert_eq!(info.declaration, "__VERTER__TS__T = string");
    }

    #[test]
    fn generic_info_empty_returns_none() {
        assert!(IdeGenericInfo::from_source("").is_none());
        assert!(IdeGenericInfo::from_source("  ").is_none());
    }

    #[test]
    fn generic_info_brackets() {
        let info = IdeGenericInfo::from_source("T extends string").unwrap();
        assert_eq!(info.source_bracket(), "<T extends string>");
        assert_eq!(info.names_bracket(), "<T>");
        assert_eq!(info.sanitised_names_bracket(), "<__VERTER__TS__T>");
        assert_eq!(
            info.declaration_bracket(),
            "<__VERTER__TS__T extends string = any>"
        );
    }

    #[test]
    fn generic_info_keyof_cross_ref() {
        let info = IdeGenericInfo::from_source("T extends object, K extends keyof T").unwrap();
        assert_eq!(
            info.declaration,
            "__VERTER__TS__T extends object = any, __VERTER__TS__K extends keyof __VERTER__TS__T = any"
        );
    }
}
