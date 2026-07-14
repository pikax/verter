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

use rustc_hash::FxHashSet;

use crate::ast::types::{AstNodeKind, TagType, TemplateAst};
use crate::template::code_gen::vdom::element::to_pascal_case;
use crate::template::oxc::{
    parse_template_expressions,
    types::{OxcNodeData, OxcParsedAst},
};
use crate::utils::oxc::bindings::collect_expression_free_ref_spans;

/// Collect the set of identifier names a `<template>` references.
///
/// Reads ONLY the typed template IR — expression-binding facts, v-for source
/// references, and component tag names — never a raw-source string scan. The
/// returned set is the AST-driven "used in template" inventory consumed by two
/// callers: script import-elision (runtime lane) and unused-binding liveness
/// (IDE lane). Both must agree on what counts as a template use, so the logic
/// lives here once rather than being re-derived per lane.
///
/// `oxc_ast` is the parsed template-expression overlay; `template_ast` is the
/// structural template AST (for component tag names); `source` is the SFC text
/// (only sliced for already-located tag-name / v-for spans, never parsed).
///
/// Returns `(used, complete)`. `complete` is `false` when ANY template
/// expression failed to parse — its referenced identifiers are then UNKNOWN, so
/// the set cannot be trusted for the unused-binding gate. The caller treats an
/// incomplete result as the conservative `None` (fail open: no demotion).
pub fn collect_template_used_vars(
    oxc_ast: &OxcParsedAst<'_>,
    template_ast: &TemplateAst,
    source: &str,
) -> (FxHashSet<String>, bool) {
    let mut vars = FxHashSet::default();
    let mut complete = true;

    // 1. Identifiers from every expression binding (interpolations, v-if
    //    conditions, directive values, dynamic args). A template-EXPRESSION parse
    //    error (`{{ count + }}`, `{{ @@@ }}`) makes that expression's references
    //    unknowable — mark the whole set INCOMPLETE so the gate fails open. This
    //    is distinct from an SFC-level tokenizer error (which the caller already
    //    gates on); a structurally-valid template can still carry an individually
    //    unparseable interpolation.
    for expr in oxc_ast.iter_expressions() {
        // A parse FAILURE (`errors: Some`) makes this expression's references
        // unknowable — fail open. An EMPTY interpolation (`{{ }}`) parses cleanly
        // with no expression and `errors: None`, and references nothing, so it is
        // NOT incomplete. Distinguish the two by `errors`, never by `bindings`
        // being `None` (which an empty interpolation also produces).
        if expr.errors.is_some() {
            complete = false;
        }
        // LIVENESS routes through the COMPLETE `Visit` span collector over the
        // parsed expression — NOT the runtime `BindingVisitor` binding facts, whose
        // hand-rolled walker drops whole statement/expression families (a `switch`
        // inside an IIFE, etc.) behind a `_ => {}` arm. The default `walk::*`
        // traversal visits every node, so a setup binding referenced only inside a
        // nested construct in an interpolation is never missed.
        //
        // Global-named references are retained (a setup binding may shadow a JS
        // global). The expression AST carries substring-relative spans, so each
        // span is shifted by `expr.offset` to slice the file-relative `source`.
        // No per-expression scope-local `ignored` set is threaded here: a v-for /
        // v-slot scope local that leaks into this set is harmless for the gate —
        // over-inclusion only SUPPRESSES a diagnostic (the conservative-safe
        // direction), it never demotes a used binding.
        if let Some(ref expression) = expr.expression {
            let mut spans = FxHashSet::default();
            collect_expression_free_ref_spans(expression, &FxHashSet::default(), &mut spans);
            for span in spans {
                let start = (span.start + expr.offset) as usize;
                let end = (span.end + expr.offset) as usize;
                vars.insert(source[start..end].to_string());
            }
        }
    }

    // 2. Identifiers from v-for AND v-slot source/reference expressions. Both
    //    carry the free-identifier NAMES their expressions use (a v-slot default-
    //    slot binding's defaults/type refs, a v-for source). Counting a v-slot
    //    reference as a use is safe for this gate: it can only SUPPRESS a
    //    diagnostic, never fabricate one.
    //
    //    Liveness reads `liveness_reference_names`, NOT the runtime `references`
    //    spans. That set is collected by the COMPLETE `Visit` walker, so a setup
    //    binding referenced ONLY inside a nested callback in the source / default
    //    (`v-for="x in rows.map(r => fmt(r))"`, `#default="{ row = list.map(r => fmt(r)) }"`)
    //    is recorded; it RETAINS global-named identifiers (a binding may SHADOW a
    //    JS global, so `v-for="x in Date"` over a `const Date` binding is a real
    //    use); and it carries NAMES, so liveness never depends on the partial
    //    wrapped→file-relative span shift.
    for node_data in &oxc_ast.data {
        if let OxcNodeData::Element(el) = node_data {
            if let Some(ref v_for) = el.v_for {
                // A v-for whose source expression failed to parse has unknowable
                // references — fail open.
                if v_for.parsed.has_errors() {
                    complete = false;
                }
                for name in &v_for.parsed.liveness_reference_names {
                    vars.insert(name.clone());
                }
            }
            if let Some(ref v_slot) = el.v_slot {
                if v_slot.parsed.has_errors() {
                    complete = false;
                }
                for name in &v_slot.parsed.liveness_reference_names {
                    vars.insert(name.clone());
                }
            }
        }
    }

    // 3. Component tag names from the structural template AST. A component tag
    //    can resolve to a setup binding under ANY of Vue's casing forms, so each
    //    tag contributes every candidate name (see `component_tag_candidates`).
    for node in &template_ast.nodes {
        if let AstNodeKind::Element(el) = &node.kind {
            if el.tag_type == TagType::Component {
                let tag_name =
                    &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];
                for candidate in component_tag_candidates(tag_name) {
                    vars.insert(candidate);
                }
            }
        }
    }

    (vars, complete)
}

/// The set of setup-binding NAMES a component tag may resolve to, covering every
/// casing form Vue/JSX component resolution accepts.
///
/// Vue resolves `<my-comp/>` against a setup binding registered as `myComp`
/// (camelCase) OR `MyComp` (PascalCase), and a literal `<myComp/>` / `<MyComp/>`
/// against the same-cased binding. A MEMBER-style tag (`<Foo.Bar/>`) resolves
/// against the ROOT binding (`Foo`). Returning every candidate is the
/// conservative direction: an over-broad match only suppresses a diagnostic, it
/// never fabricates one, while missing the real binding's casing would
/// false-positive a used component as unused.
pub fn component_tag_candidates(tag_name: &str) -> Vec<String> {
    let mut out = Vec::new();
    // A member-style tag resolves against its ROOT segment.
    let root = tag_name.split('.').next().unwrap_or(tag_name);
    let mut push = |s: String| {
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    // The raw tag (and its member root) verbatim.
    push(tag_name.to_string());
    push(root.to_string());
    // PascalCase and camelCase variants of the root (covers kebab/camel/Pascal
    // registration forms).
    let pascal = to_pascal_case(root);
    let camel = to_camel_case(root);
    push(pascal);
    push(camel);
    out
}

/// Lower-camelCase a tag name: PascalCase the kebab/snake segments, then
/// lowercase the leading character. `my-comp` → `myComp`, `MyComp` → `myComp`.
fn to_camel_case(s: &str) -> String {
    let pascal = to_pascal_case(s);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(first) => {
            let mut out: String = first.to_lowercase().collect();
            out.push_str(chars.as_str());
            out
        }
        None => pascal,
    }
}

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

    #[test]
    fn kebab_component_tag_yields_camel_and_pascal_candidates() {
        let c = component_tag_candidates("my-comp");
        assert!(c.contains(&"my-comp".to_string()), "raw kebab tag");
        assert!(c.contains(&"MyComp".to_string()), "PascalCase candidate");
        assert!(
            c.contains(&"myComp".to_string()),
            "camelCase candidate (the binding form a `const myComp` would register)"
        );
    }

    #[test]
    fn camel_component_tag_yields_self_and_pascal() {
        let c = component_tag_candidates("myComp");
        assert!(c.contains(&"myComp".to_string()));
        assert!(c.contains(&"MyComp".to_string()));
    }

    #[test]
    fn member_style_tag_counts_root() {
        let c = component_tag_candidates("Foo.Bar");
        assert!(
            c.contains(&"Foo".to_string()),
            "member-style tag counts its root"
        );
    }
}
