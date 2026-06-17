//! Read-only shared template-expression overlay.
//!
//! A single compile drives several consumers that each need an [`OxcParsedAst`]
//! over the same template: the early script-import-elision scan, runtime
//! VDOM/Vapor codegen, raw template-data extraction, and IDE TSX codegen. This
//! store builds those parsed facts in the top compile allocator and hands them
//! out read-only to every consumer whose parse inputs are identical, parsing
//! exactly once per distinct key.
//!
//! The codegen paths (runtime VDOM/Vapor vs IDE TSX) remain SEPARATE output
//! owners. They share ONLY these immutable parsed facts; the overlay carries no
//! emitter-specific codegen state.
//!
//! The key holds the EXACT parse inputs — the source text, the template span,
//! the parse-affecting options, the full `SourceType` value, and the
//! `ide_completion` flag — compared by value, so a warm hit is an exact-content
//! match, never a hash guess that could collide. Two dimensions in particular
//! keep lanes apart:
//!
//! - `SourceType`: a JS SFC's TSX lane parses with `SourceType::jsx()` while its
//!   runtime lane parses with `SourceType::tsx()`, so the two never share.
//! - `ide_completion`: even a TS SFC, whose runtime and TSX lanes both use
//!   `tsx()`, parses each lane separately because completion-prefix matching
//!   changes the stored binding facts — the runtime lane keys `false` and the
//!   IDE/TSX lane keys `true`. The sharing that remains is intra-lane: the early
//!   elision scan and runtime codegen both reuse the single `false` entry.

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::ast::types::TemplateAst;
use crate::template::oxc::{parse_template_expressions, types::OxcParsedAst};

/// Parse-affecting codegen options that shape the template AST the expressions
/// cover. Held EXACTLY (not hashed) so two requests that differ here are
/// compared by value and never share a parsed overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOptionsKey {
    /// Custom interpolation delimiters, exactly as configured (`None` is the
    /// default `("{{", "}}")`).
    delimiters: Option<(String, String)>,
    /// Custom-element tag prefixes, exactly as configured.
    custom_elements: Option<Vec<String>>,
}

impl ParseOptionsKey {
    /// Capture the exact parse-affecting options for one compile.
    pub fn new(delimiters: Option<(String, String)>, custom_elements: Option<Vec<String>>) -> Self {
        Self {
            delimiters,
            custom_elements,
        }
    }
}

/// Identity of a parsed template-expression overlay.
///
/// Two requests with an equal key produce identical parsed facts, so the
/// overlay built for the first is reused for the second. The key holds the
/// EXACT parse inputs — the source text, the parsed `<template>` span, the
/// parse-affecting codegen options, and the full [`SourceType`] value — and
/// `PartialEq` compares them by value, so a warm hit is an exact-content match
/// rather than a lossy hash equality that could collide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateExprKey<'alloc> {
    /// The exact SFC source text the expressions were parsed from. Compared
    /// byte-for-byte via `&str` equality, never via a hash.
    source: &'alloc str,
    /// Byte span `[start, end)` of the parsed `<template>` region.
    template_span: (u32, u32),
    /// The exact parse-affecting codegen options that shape the template AST
    /// these expressions cover.
    parse_options: ParseOptionsKey,
    /// The exact [`SourceType`] the expressions were parsed with. Compared by
    /// full value: two source types that differ in any field — `language`,
    /// `module_kind`, `variant`, or `extension` — never share an entry.
    source_type: SourceType,
    /// Whether completion-prefix matching was enabled for the parse. This is NOT
    /// transient: it flips `Binding.ignore` and the stored `dynamism` for v-for
    /// / v-slot scope locals, so the runtime lane (`false`, partial identifiers
    /// stay real references) and the IDE/TSX lane (`true`, partial identifiers
    /// stay bare for completion) parse DIFFERENT facts and must never share an
    /// overlay entry.
    ide_completion: bool,
}

impl<'alloc> TemplateExprKey<'alloc> {
    /// Build a key from the exact parse inputs. The full `SourceType` and the
    /// `ide_completion` flag enter the key by value, so a request differing in
    /// any parse-affecting field — a `jsx()` lane, a different module kind, a
    /// different completion mode, and so on — can never collide with an existing
    /// entry.
    pub fn new(
        source: &'alloc str,
        template_span: (u32, u32),
        parse_options: &ParseOptionsKey,
        source_type: SourceType,
        ide_completion: bool,
    ) -> Self {
        Self {
            source,
            template_span,
            parse_options: parse_options.clone(),
            source_type,
            ide_completion,
        }
    }
}

/// One immutable parsed overlay plus its identity.
struct TemplateExprOverlay<'alloc> {
    key: TemplateExprKey<'alloc>,
    ast: OxcParsedAst<'alloc>,
}

/// Per-compile store of read-only template-expression overlays.
///
/// Built lazily on first demand: the first lane that needs a given
/// `(source, span, options, source_type, ide_completion)` parses it into the
/// top compile allocator; every later lane with an equal key reuses the same
/// parsed facts without touching OXC. Within one compile this collapses the
/// runtime + TSX double parse of a TS SFC to one parse per `ide_completion`
/// value — runtime (`false`) and IDE/TSX (`true`) keep distinct entries because
/// completion mode changes the stored binding facts.
#[derive(Default)]
pub struct TemplateExprStore<'alloc> {
    entries: Vec<TemplateExprOverlay<'alloc>>,
}

impl<'alloc> TemplateExprStore<'alloc> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Return the parsed overlay for the given parse inputs, parsing it exactly
    /// once on first demand and reusing the cached facts thereafter.
    ///
    /// `template_ast`, `source`, and `alloc` are consulted only on a cold key;
    /// a warm key returns the already-parsed facts read-only.
    // Every parameter is a distinct parse input: the cold-build inputs
    // (`template_ast`, `source`, `alloc`) plus the five overlay-key dimensions
    // (`source`, `template_span`, `parse_options`, `source_type`,
    // `ide_completion`). Collapsing them into a struct would only hide the key
    // composition the caller must supply, so the flat signature is intentional.
    #[allow(clippy::too_many_arguments)]
    pub fn get_or_build(
        &mut self,
        template_ast: &TemplateAst,
        source: &'alloc str,
        alloc: &'alloc Allocator,
        template_span: (u32, u32),
        parse_options: &ParseOptionsKey,
        source_type: SourceType,
        ide_completion: bool,
    ) -> &OxcParsedAst<'alloc> {
        let key = TemplateExprKey::new(
            source,
            template_span,
            parse_options,
            source_type,
            ide_completion,
        );
        if let Some(idx) = self.entries.iter().position(|e| e.key == key) {
            return &self.entries[idx].ast;
        }
        let ast =
            parse_template_expressions(template_ast, source, alloc, source_type, ide_completion);
        self.entries.push(TemplateExprOverlay { key, ast });
        &self.entries.last().expect("just pushed").ast
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_options() -> ParseOptionsKey {
        ParseOptionsKey::new(None, None)
    }

    #[test]
    fn key_discriminates_tsx_from_jsx() {
        let opts = default_options();
        let tsx = TemplateExprKey::new("source", (0, 10), &opts, SourceType::tsx(), false);
        let jsx = TemplateExprKey::new("source", (0, 10), &opts, SourceType::jsx(), false);
        assert_ne!(
            tsx, jsx,
            "a jsx() lane must never key-collide with a tsx() entry"
        );
    }

    #[test]
    fn key_discriminates_ide_completion() {
        let opts = default_options();
        let runtime = TemplateExprKey::new("source", (0, 10), &opts, SourceType::tsx(), false);
        let ide = TemplateExprKey::new("source", (0, 10), &opts, SourceType::tsx(), true);
        assert_ne!(
            runtime, ide,
            "runtime and IDE completion modes store different binding facts"
        );
    }

    #[test]
    fn key_discriminates_source_type_beyond_ts_jsx() {
        let opts = default_options();
        // Two TS+JSX source types that AGREE on `is_typescript()` and `is_jsx()`
        // yet differ in `module_kind`. The module kind changes how the template
        // expressions parse, so the two must never share a parsed overlay — and
        // a key that compared only the ts/jsx pair would wrongly equate them.
        let unambiguous = SourceType::tsx();
        let module = SourceType::tsx().with_module(true);
        assert!(
            unambiguous.is_typescript() && module.is_typescript(),
            "both inputs must be TypeScript to exercise the ts/jsx collision"
        );
        assert!(
            unambiguous.is_jsx() && module.is_jsx(),
            "both inputs must be JSX to exercise the ts/jsx collision"
        );
        assert_ne!(
            unambiguous, module,
            "the two source types differ in a parse-affecting field beyond ts/jsx"
        );

        let a = TemplateExprKey::new("source", (0, 10), &opts, unambiguous, false);
        let b = TemplateExprKey::new("source", (0, 10), &opts, module, false);
        assert_ne!(
            a, b,
            "source types differing in any parse-affecting field must never share an overlay key"
        );
    }

    #[test]
    fn key_equal_for_identical_inputs() {
        let opts = ParseOptionsKey::new(Some(("[[".into(), "]]".into())), None);
        let a = TemplateExprKey::new("source", (0, 10), &opts, SourceType::tsx(), false);
        let b = TemplateExprKey::new("source", (0, 10), &opts, SourceType::tsx(), false);
        assert_eq!(a, b);
    }

    #[test]
    fn key_compares_source_text_by_exact_bytes() {
        let opts = default_options();
        let a = TemplateExprKey::new("alpha", (0, 5), &opts, SourceType::tsx(), false);
        let b = TemplateExprKey::new("bravo", (0, 5), &opts, SourceType::tsx(), false);
        assert_ne!(a, b);
        // A one-byte difference must discriminate — equality is exact-content,
        // not a truncated/lossy fingerprint.
        let c = TemplateExprKey::new("alpha", (0, 5), &opts, SourceType::tsx(), false);
        let d = TemplateExprKey::new("alphb", (0, 5), &opts, SourceType::tsx(), false);
        assert_ne!(c, d);
        assert_eq!(a, c);
    }

    #[test]
    fn key_discriminates_delimiters() {
        let a = TemplateExprKey::new(
            "source",
            (0, 5),
            &ParseOptionsKey::new(Some(("[[".into(), "]]".into())), None),
            SourceType::tsx(),
            false,
        );
        let b = TemplateExprKey::new(
            "source",
            (0, 5),
            &ParseOptionsKey::new(Some(("{%".into(), "%}".into())), None),
            SourceType::tsx(),
            false,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn key_discriminates_custom_elements() {
        let a = TemplateExprKey::new(
            "source",
            (0, 5),
            &ParseOptionsKey::new(None, Some(vec!["ion-".into()])),
            SourceType::tsx(),
            false,
        );
        let b = TemplateExprKey::new(
            "source",
            (0, 5),
            &ParseOptionsKey::new(None, Some(vec!["my-".into()])),
            SourceType::tsx(),
            false,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn key_discriminates_template_span() {
        let opts = default_options();
        let a = TemplateExprKey::new("source", (0, 5), &opts, SourceType::tsx(), false);
        let b = TemplateExprKey::new("source", (0, 6), &opts, SourceType::tsx(), false);
        assert_ne!(a, b);
    }
}
