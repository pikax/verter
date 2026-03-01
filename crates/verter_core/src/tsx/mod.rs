//! TSX code generation for the Vue SFC compiler.
//!
//! Generates valid `.tsx` output from a parsed SFC for TypeScript type checking.
//! Unlike the VDOM/Vapor codegen backends (which produce render functions), this
//! module produces a single file with:
//!
//! - **Script block**: preserves TypeScript types, macros, and imports
//! - **Template block**: converts Vue template syntax to valid JSX
//!
//! The TSX output is used by the LSP for hover, completions, go-to-definition,
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
//!   ├── tsx::script::generate_tsx_script()    → TSX script block (NEW)
//!   └── tsx::template::generate_tsx_template() → TSX template JSX (NEW)
//! ```

pub mod condition;
pub mod script;
pub mod template;

/// Options for TSX script generation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TsxScriptOptions<'a> {
    /// Component name extracted from filename.
    pub component_name: &'a str,
    /// Sanitized JS identifier for the component (e.g., `_123Widget` for `123-widget.vue`).
    pub js_component_name: &'a str,
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
}

/// Options for TSX template generation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TsxTemplateOptions<'a> {
    /// Self-referencing component name (PascalCase).
    pub self_name: &'a str,
    /// Whether to preserve HTML comments in JSX output.
    pub comments: bool,
}

// ── Generic info ─────────────────────────────────────────────────

/// Prefix for sanitised generic type parameter names.
const GENERIC_SANITISE_PREFIX: &str = "__VERTER__TS__";

/// Parsed and processed generic type parameters for TSX emission.
///
/// Built from the raw `generic="..."` attribute, this struct holds the
/// original source, extracted names, sanitised names (prefixed to avoid
/// collisions), and a full declaration string for the public API types.
#[derive(Debug, Clone)]
pub struct TsxGenericInfo {
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

impl TsxGenericInfo {
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

        Some(TsxGenericInfo {
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

// ── Shared TSX Helpers ───────────────────────────────────────────

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

    let mut result = String::with_capacity(event_name.len() + 2);
    result.push_str("on");
    let mut capitalize_next = true;
    for ch in event_name.chars() {
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

    // ── TsxGenericInfo tests ─────────────────────────────────────

    #[test]
    fn generic_info_simple_param() {
        let info = TsxGenericInfo::from_source("T").unwrap();
        assert_eq!(info.names, vec!["T"]);
        assert_eq!(info.sanitised_names, vec!["__VERTER__TS__T"]);
        assert_eq!(info.declaration, "__VERTER__TS__T = any");
    }

    #[test]
    fn generic_info_constraint() {
        let info = TsxGenericInfo::from_source("T extends string").unwrap();
        assert_eq!(info.names, vec!["T"]);
        assert_eq!(info.declaration, "__VERTER__TS__T extends string = any");
    }

    #[test]
    fn generic_info_constraint_and_default() {
        let info = TsxGenericInfo::from_source("T extends object = {}").unwrap();
        assert_eq!(info.declaration, "__VERTER__TS__T extends object = {}");
    }

    #[test]
    fn generic_info_cross_reference_sanitisation() {
        let info = TsxGenericInfo::from_source("T, U extends Array<T>").unwrap();
        assert_eq!(info.names, vec!["T", "U"]);
        assert_eq!(
            info.declaration,
            "__VERTER__TS__T = any, __VERTER__TS__U extends Array<__VERTER__TS__T> = any"
        );
    }

    #[test]
    fn generic_info_multiple_mixed() {
        let info = TsxGenericInfo::from_source("K extends string, V").unwrap();
        assert_eq!(info.names, vec!["K", "V"]);
        assert_eq!(
            info.declaration,
            "__VERTER__TS__K extends string = any, __VERTER__TS__V = any"
        );
    }

    #[test]
    fn generic_info_default_type() {
        let info = TsxGenericInfo::from_source("T = string").unwrap();
        assert_eq!(info.declaration, "__VERTER__TS__T = string");
    }

    #[test]
    fn generic_info_empty_returns_none() {
        assert!(TsxGenericInfo::from_source("").is_none());
        assert!(TsxGenericInfo::from_source("  ").is_none());
    }

    #[test]
    fn generic_info_brackets() {
        let info = TsxGenericInfo::from_source("T extends string").unwrap();
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
        let info = TsxGenericInfo::from_source("T extends object, K extends keyof T").unwrap();
        assert_eq!(
            info.declaration,
            "__VERTER__TS__T extends object = any, __VERTER__TS__K extends keyof __VERTER__TS__T = any"
        );
    }
}
