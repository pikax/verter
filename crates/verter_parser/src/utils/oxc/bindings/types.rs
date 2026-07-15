//! Type definitions for binding extraction.
//!
//! This module contains all the types used by the binding extraction system.

use crate::common::RelativeSpan;
use rustc_hash::FxHashSet;
use smallvec::SmallVec;
use std::collections::HashSet;

use super::keywords::{is_global, is_keyword};

// ======================== Dynamism ========================

/// Three-state dynamism classification for template expressions.
///
/// Tells codegen whether a script-binding lookup is needed to determine
/// if an expression is truly static or dynamic:
///
/// - [`Static`](Dynamism::Static) — no identifiers at all → skip lookup.
/// - [`MaybeDynamic`](Dynamism::MaybeDynamic) — has script-level identifiers →
///   codegen checks if they are `const` (static) or `ref`/`reactive` (dynamic).
/// - [`Dynamic`](Dynamism::Dynamic) — has injected locals (v-for/v-slot) →
///   definitely per-iteration, skip lookup.
///
/// Computed incrementally during binding extraction — no separate iteration needed.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Dynamism {
    /// No identifier references — pure literals/operators. Definitely constant.
    /// Codegen can skip script binding lookup.
    Static,

    /// Has script-level identifier references that could be `const` (static)
    /// or `ref`/`reactive`/`computed` (dynamic). Codegen must resolve via
    /// script binding analysis.
    MaybeDynamic,

    /// Has at least one injected local (v-for/v-slot variable). Definitely
    /// per-iteration/per-slot — the value changes at runtime. Codegen can
    /// skip script binding lookup.
    Dynamic,
}

/// Type alias for parameter byte slices - most functions have ≤8 params
pub type ParamBytes<'a> = SmallVec<[&'a str; 8]>;

/// Represents a binding extracted from an expression (byte-optimized version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding<'a> {
    /// The name of the identifier
    pub name: &'a str,
    /// The span of the identifier relative to the parsed expression start.
    /// OXC produces expression-relative offsets; this preserves that semantic.
    pub span: RelativeSpan,
    /// The absolute position (span.start + base_offset)
    pub pos: u32,
    /// Whether this binding should be ignored (is a keyword, parameter, or local variable)
    pub ignore: bool,
    /// Whether this identifier is the value of a shorthand property (`{ foo }`).
    /// When true and a prefix is applied (e.g., `_ctx.`), the shorthand must be
    /// expanded to key: value form (`{ foo: _ctx.foo }`).
    pub is_shorthand: bool,
}

/// Represents a function found in an expression (byte-optimized version).
#[derive(Debug, Clone)]
pub struct FunctionBinding {
    /// The span of the function relative to the parsed expression start.
    pub span: RelativeSpan,
    /// The span of the function body relative to the parsed expression start.
    pub body_span: RelativeSpan,
    /// The absolute position (span.start + base_offset)
    pub pos: u32,
    /// The absolute position of the body
    pub body_pos: u32,
}

/// Represents a literal found in an expression (byte-optimized version).
#[derive(Debug, Clone)]
pub struct LiteralBinding<'a> {
    /// The span of the literal relative to the parsed expression start.
    pub span: RelativeSpan,
    /// The absolute position (span.start + base_offset)
    pub pos: u32,
    /// The string representation of the literal value
    pub content: &'a str,
}

/// The result of extracting bindings from an expression (byte-optimized version).
#[derive(Debug, Clone)]
pub struct BindingExtractionResult<'a> {
    /// All identifier bindings found
    pub bindings: Vec<Binding<'a>>,
    /// All function expressions found
    pub functions: Vec<FunctionBinding>,
    /// All literals found
    pub literals: Vec<LiteralBinding<'a>>,
    /// Whether the expression had parse errors
    pub has_errors: bool,
    /// Three-state dynamism classification, computed incrementally during extraction.
    /// No separate iteration over bindings needed.
    pub dynamism: Dynamism,
}

impl Default for BindingExtractionResult<'_> {
    fn default() -> Self {
        Self {
            bindings: Vec::new(),
            functions: Vec::new(),
            literals: Vec::new(),
            has_errors: false,
            dynamism: Dynamism::Static,
        }
    }
}

impl<'a> BindingExtractionResult<'a> {
    /// Get all non-ignored binding names (unique)
    pub fn non_ignored_binding_names(&self) -> Vec<&'a str> {
        let mut seen = HashSet::new();
        self.bindings
            .iter()
            .filter(|b| !b.ignore)
            .filter_map(|b| {
                if seen.insert(b.name) {
                    Some(b.name)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if any functions were found
    pub fn has_functions(&self) -> bool {
        !self.functions.is_empty()
    }

    /// Extend this result with bindings from another result.
    /// Used for propagating parent bindings to child scopes.
    pub fn extend(&mut self, other: &BindingExtractionResult<'a>) {
        self.bindings.extend(other.bindings.iter().cloned());
        self.functions.extend(other.functions.iter().cloned());
        self.literals.extend(other.literals.iter().cloned());
        if other.has_errors {
            self.has_errors = true;
        }
        // Dynamic trumps MaybeDynamic trumps Static
        if self.dynamism != Dynamism::Dynamic {
            match other.dynamism {
                Dynamism::Dynamic => self.dynamism = Dynamism::Dynamic,
                Dynamism::MaybeDynamic => self.dynamism = Dynamism::MaybeDynamic,
                Dynamism::Static => {}
            }
        }
    }
}

/// Context for binding extraction, tracking ignored identifiers in scope (byte-optimized version).
#[derive(Debug, Clone)]
pub struct BindingContext<'a> {
    /// Identifiers that should be ignored (parameters, local variables) as str slices
    ignored_identifiers: FxHashSet<&'a str>,
    /// Base offset to add to all positions
    pub base_offset: u32,
    /// When set, an identifier that is a *prefix* of an in-scope local is also
    /// treated as ignored. This supports IDE completion inside v-for / v-slot
    /// scopes, where partial identifiers (`it`, `slotI`) arrive mid-keystroke and
    /// must stay bare so the type provider can offer scoped completions.
    ///
    /// Runtime (VDOM / Vapor) codegen leaves this off: there, a partial identifier
    /// is a genuine reference to a script binding and must resolve normally, not be
    /// silently swallowed as a scoped local.
    completion_prefixes: bool,
}

impl Default for BindingContext<'_> {
    fn default() -> Self {
        Self::new(0)
    }
}

impl<'a> BindingContext<'a> {
    /// Create a new binding context with a base offset
    pub fn new(base_offset: u32) -> Self {
        Self {
            ignored_identifiers: FxHashSet::default(),
            base_offset,
            completion_prefixes: false,
        }
    }

    /// Create a context with pre-existing ignored identifiers
    pub fn with_ignored(base_offset: u32, ignored: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            ignored_identifiers: ignored.into_iter().collect(),
            base_offset,
            completion_prefixes: false,
        }
    }

    /// Enable or disable completion-prefix matching (see [`completion_prefixes`]).
    ///
    /// [`completion_prefixes`]: BindingContext::completion_prefixes
    #[inline]
    pub fn completion_aware(mut self, enabled: bool) -> Self {
        self.completion_prefixes = enabled;
        self
    }

    /// Check if an identifier should be ignored.
    ///
    /// `$event` is a Vue template built-in: the codegen wraps inline event
    /// handlers in `$event => (...)`, so `$event` inside the expression is
    /// the arrow parameter and must NOT be prefixed with `_ctx.`.
    #[inline]
    pub fn should_ignore(&self, name: &str) -> bool {
        let bytes = name.as_bytes();
        is_keyword(bytes)
            || is_global(bytes)
            || name == "$event"
            || self.ignored_identifiers.contains(name)
            // In IDE completion mode, partial completions inside v-for / v-slot scopes
            // arrive as unfinished identifiers (`it`, `slotI`, etc.). Treat prefixes of
            // ignored locals as ignored too so the template codegen keeps them bare and
            // the type provider can offer scoped completions instead of instance members.
            // Gated off for runtime codegen, where a partial identifier is a real
            // reference and the per-binding scan is wasted work.
            || (self.completion_prefixes
                && self
                    .ignored_identifiers
                    .iter()
                    .any(|ignored| ignored.starts_with(name)))
    }

    /// Add an identifier to the ignore list
    #[inline]
    pub fn add_ignored(&mut self, name: &'a str) {
        self.ignored_identifiers.insert(name);
    }

    /// Create a child context with additional ignored identifiers
    pub fn child_with_ignored(&self, additional: SmallVec<[&'a str; 8]>) -> Self {
        let mut ignored = self.ignored_identifiers.clone();
        ignored.extend(additional);
        Self {
            ignored_identifiers: ignored,
            base_offset: self.base_offset,
            completion_prefixes: self.completion_prefixes,
        }
    }
}

/// Result of extracting bindings from function parameters (slots, v-for, etc.)
///
/// This distinguishes between:
/// - **locals**: Bindings that are declared by the pattern (parameter names)
/// - **references**: External identifiers referenced in the expression (type annotations, default values)
#[derive(Debug, Default)]
pub struct ParameterBindingsResult {
    /// Bindings declared by the pattern (e.g., `role` in `{ rowData: role }`)
    pub locals: Vec<String>,
    /// External identifiers referenced (e.g., `ProjectRole` in `: { rowData: ProjectRole }`)
    pub references: Vec<String>,
    /// Whether parsing failed
    pub has_errors: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binding_context_new() {
        let ctx = BindingContext::new(100);
        assert_eq!(ctx.base_offset, 100);
        assert!(!ctx.should_ignore("foo"));
    }

    #[test]
    fn test_binding_context_keywords() {
        let ctx = BindingContext::new(0);
        assert!(ctx.should_ignore("true"));
        assert!(ctx.should_ignore("false"));
        assert!(ctx.should_ignore("null"));
        assert!(ctx.should_ignore("undefined"));
        assert!(!ctx.should_ignore("myVar"));
    }

    #[test]
    fn test_binding_context_globals() {
        let ctx = BindingContext::new(0);
        assert!(ctx.should_ignore("String"));
        assert!(ctx.should_ignore("Array"));
        assert!(ctx.should_ignore("Object"));
        assert!(ctx.should_ignore("Math"));
        assert!(ctx.should_ignore("Number"));
        assert!(ctx.should_ignore("Boolean"));
        assert!(ctx.should_ignore("Date"));
        assert!(ctx.should_ignore("JSON"));
        assert!(ctx.should_ignore("Map"));
        assert!(ctx.should_ignore("Set"));
        assert!(ctx.should_ignore("console"));
        assert!(ctx.should_ignore("Infinity"));
        assert!(ctx.should_ignore("parseInt"));
        assert!(ctx.should_ignore("parseFloat"));
        assert!(ctx.should_ignore("Promise"));
        assert!(ctx.should_ignore("RegExp"));
        assert!(ctx.should_ignore("Error"));
        assert!(ctx.should_ignore("Symbol"));
        assert!(ctx.should_ignore("globalThis"));
        assert!(ctx.should_ignore("require"));
        assert!(!ctx.should_ignore("myVar"));
    }

    #[test]
    fn test_binding_context_add_ignored() {
        let mut ctx = BindingContext::new(0);
        assert!(!ctx.should_ignore("foo"));
        ctx.add_ignored("foo");
        assert!(ctx.should_ignore("foo"));
    }

    /// Runtime codegen must NOT treat a partial identifier as a scoped local just
    /// because it is a prefix of one — `it` is a real reference even when `item` is
    /// in scope. The completion-prefix scan is gated to IDE completion contexts.
    #[test]
    fn completion_prefix_scan_is_gated_to_ide_contexts() {
        // Runtime context (default): only exact locals are ignored.
        let mut runtime = BindingContext::new(0);
        runtime.add_ignored("item");
        assert!(
            runtime.should_ignore("item"),
            "exact local stays ignored in every context"
        );
        assert!(
            !runtime.should_ignore("it"),
            "runtime must treat a prefix of a local as a real reference"
        );

        // IDE completion context: prefixes of locals are also ignored.
        let mut ide = BindingContext::new(0).completion_aware(true);
        ide.add_ignored("item");
        assert!(ide.should_ignore("item"));
        assert!(
            ide.should_ignore("it"),
            "IDE completion keeps partial identifiers bare for scoped completion"
        );
    }

    /// The completion-prefix flag propagates to child (arrow-function) scopes so a
    /// nested expression keeps the same gating as its enclosing context.
    #[test]
    fn completion_prefix_flag_propagates_to_child_scope() {
        let runtime_child = BindingContext::new(0).child_with_ignored(smallvec::smallvec!["item"]);
        assert!(!runtime_child.should_ignore("it"));

        let ide_child = BindingContext::new(0)
            .completion_aware(true)
            .child_with_ignored(smallvec::smallvec!["item"]);
        assert!(ide_child.should_ignore("it"));
    }

    #[test]
    fn test_binding_context_child() {
        let ctx = BindingContext::new(50);
        let child = ctx.child_with_ignored(smallvec::smallvec!["x", "y"]);

        assert_eq!(child.base_offset, 50);
        assert!(child.should_ignore("x"));
        assert!(child.should_ignore("y"));
        assert!(!child.should_ignore("z"));
    }

    #[test]
    fn test_binding_extraction_result() {
        let mut result = BindingExtractionResult::default();
        result.bindings.push(Binding {
            name: "foo",
            span: RelativeSpan::new(0, 3),
            pos: 0,
            ignore: false,
            is_shorthand: false,
        });
        result.bindings.push(Binding {
            name: "bar",
            span: RelativeSpan::new(6, 9),
            pos: 6,
            ignore: false,
            is_shorthand: false,
        });
        result.bindings.push(Binding {
            name: "foo",
            span: RelativeSpan::new(12, 15),
            pos: 12,
            ignore: false,
            is_shorthand: false,
        }); // duplicate
        result.bindings.push(Binding {
            name: "ignored",
            span: RelativeSpan::new(18, 25),
            pos: 18,
            ignore: true,
            is_shorthand: false,
        });

        let names = result.non_ignored_binding_names();
        assert_eq!(names, vec!["foo", "bar"]);
    }

    #[test]
    fn test_parameter_bindings_result_default() {
        let result = ParameterBindingsResult::default();
        assert!(result.locals.is_empty());
        assert!(result.references.is_empty());
        assert!(!result.has_errors);
    }
}
