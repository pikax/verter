//! Structural inventory of the MODULE SPECIFIERS in a generated TypeScript
//! carrier.
//!
//! Consumers that need to rewrite a carrier's specifiers (verter-tsc lowers
//! them onto its in-memory stub carriers) must edit exactly the specifier
//! string literals and nothing else. Locating them by scanning for `import(`
//! or `from ` is not that: the same bytes occur inside authored string
//! literals, template literals, comments, and regular expressions, and the
//! Options-API stub passes the user's authored script body through verbatim.
//! A scan therefore rewrites
//!
//! ```ts
//! const marker = 'import("@/Child.vue")' as const
//! ```
//!
//! into a different literal — silently changing the user's type and the
//! diagnostics they see.
//!
//! This module answers the same question from the PARSED program: every span
//! below is the source range of a real specifier node, so a caller splicing by
//! span cannot reach anything else. It is the Typed-IR-Only rule applied to the
//! one place a consumer was tempted to text-scan.

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::SourceType;

/// One module specifier: the byte range of its whole string literal, the
/// DECODED specifier it denotes, and the quote character it was written with.
///
/// The range covers the QUOTES, and `text` is the literal's VALUE rather than
/// its bytes, so a caller that wants to replace a specifier re-emits a whole
/// literal instead of splicing into the middle of one. That is what makes an
/// escaped specifier — `'..\\x'` on Windows, or `'./Ch\u0069ld.vue'` —
/// rewritable at all: splicing over the interior would corrupt the escape,
/// and skipping it would silently leave a real import un-rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSpecifierSpan {
    /// Byte offset of the opening quote.
    pub start: usize,
    /// Byte offset one past the closing quote.
    pub end: usize,
    /// The specifier the literal DENOTES, with escapes decoded.
    pub text: String,
    /// The quote character the literal was written with, so a replacement can
    /// keep the carrier's existing style.
    pub quote: char,
}

/// Render `specifier` as a string literal delimited by `quote`, escaping what
/// the delimiter requires. The inverse of [`ModuleSpecifierSpan::text`], so a
/// round-trip through it is always a valid literal denoting exactly
/// `specifier` — including a Windows path full of backslashes.
///
/// Escapes everything a JS/TS string literal cannot carry raw: the delimiter,
/// the backslash, and the FOUR characters the grammar treats as line
/// terminators — `\n`, `\r`, and the Unicode separators U+2028 / U+2029.
/// A specifier containing any of the latter is exotic but reachable (a POSIX
/// path component may legally contain a newline), and emitting one raw does
/// not produce a wrong specifier — it produces an UNTERMINATED literal and a
/// cascade of syntax errors through the rest of the carrier.
#[must_use]
pub fn quote_module_specifier(specifier: &str, quote: char) -> String {
    let mut out = String::with_capacity(specifier.len() + 2);
    out.push(quote);
    for ch in specifier.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => {
                if ch == quote || ch == '\\' {
                    out.push('\\');
                }
                out.push(ch);
            }
        }
    }
    out.push(quote);
    out
}

/// Every module specifier in `source`, in ascending source order.
///
/// Covers the four positions a carrier can name a module from:
///
/// * `import … from "x"` / `export … from "x"` / `export * from "x"` —
///   declaration sources;
/// * `import("x")` in VALUE position — a dynamic import expression;
/// * `import("x")` in TYPE position — a `TSImportType`, which is what the
///   fallthrough widening emits (`typeof import("./Child.vue")["default"]`);
/// * `import x = require("y")` — a TS import-equals declaration.
///
/// The source is parsed as TSX so both the generated `.tsx` validation carriers
/// and the `.ts` public-API stubs are handled by one entry point.
///
/// Returns `None` when the parse was not CLEAN — either the parser gave up
/// (`panicked`) or it recovered but reported diagnostics. Both mean "this
/// inventory has no answer", which is not the same as "there are no
/// specifiers"; a caller that conflated them would silently leave every
/// specifier in a broken carrier un-rewritten while believing it had done its
/// job, so a caller receiving `None` must fail closed VISIBLY.
///
/// Recovered errors are excluded deliberately, not incidentally: `panicked` is
/// false for a good many malformed sources (a duplicate `public public`
/// modifier, for one), and a recovering parser is free to synthesise nodes at
/// positions the author never wrote. Splicing on a span recovered from a source
/// we could not fully parse risks corrupting the carrier; declining to splice
/// at worst leaves a specifier un-absolutized, which surfaces as a
/// module-resolution diagnostic beside the syntax error that caused it.
#[must_use]
pub fn collect_module_specifier_spans(source: &str) -> Option<Vec<ModuleSpecifierSpan>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return None;
    }
    let mut collector = SpecifierCollector {
        source,
        spans: Vec::new(),
    };
    collector.visit_program(&parsed.program);
    collector.spans.sort_by_key(|span| span.start);
    collector.spans.dedup_by_key(|span| span.start);
    Some(collector.spans)
}

struct SpecifierCollector<'s> {
    source: &'s str,
    spans: Vec<ModuleSpecifierSpan>,
}

impl SpecifierCollector<'_> {
    /// Record a string-literal node as a specifier.
    ///
    /// Record a string-literal node as a specifier, taking its DECODED value
    /// and its quote character from the parsed node rather than from the bytes.
    fn record(&mut self, literal: &StringLiteral<'_>) {
        let start = literal.span.start as usize;
        let end = literal.span.end as usize;
        let quote = self
            .source
            .get(start..end)
            .and_then(|raw| raw.chars().next())
            .filter(|ch| *ch == '\'' || *ch == '"');
        let Some(quote) = quote else {
            return;
        };
        self.spans.push(ModuleSpecifierSpan {
            start,
            end,
            text: literal.value.as_str().to_string(),
            quote,
        });
    }

    /// Record a NO-SUBSTITUTION template literal as a specifier.
    ///
    /// The recorded quote is a double quote, not a backtick: a replacement is
    /// emitted as an ordinary string literal, which denotes the same specifier
    /// and keeps [`quote_module_specifier`] from having to reason about `${`
    /// interpolation syntax. The literal KIND therefore changes when — and only
    /// when — the specifier is actually rewritten.
    fn record_template(&mut self, template: &TemplateLiteral<'_>) {
        if !template.expressions.is_empty() {
            return;
        }
        let [quasi] = template.quasis.as_slice() else {
            return;
        };
        let Some(cooked) = quasi.value.cooked.as_ref() else {
            return;
        };
        self.spans.push(ModuleSpecifierSpan {
            start: template.span.start as usize,
            end: template.span.end as usize,
            text: cooked.as_str().to_string(),
            quote: '"',
        });
    }
}

impl<'a> Visit<'a> for SpecifierCollector<'_> {
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        self.record(&decl.source);
        oxc_ast_visit::walk::walk_import_declaration(self, decl);
    }

    fn visit_export_named_declaration(&mut self, decl: &ExportNamedDeclaration<'a>) {
        if let Some(source) = decl.source.as_ref() {
            self.record(source);
        }
        oxc_ast_visit::walk::walk_export_named_declaration(self, decl);
    }

    fn visit_export_all_declaration(&mut self, decl: &ExportAllDeclaration<'a>) {
        self.record(&decl.source);
        oxc_ast_visit::walk::walk_export_all_declaration(self, decl);
    }

    fn visit_import_expression(&mut self, expr: &ImportExpression<'a>) {
        match &expr.source {
            Expression::StringLiteral(literal) => self.record(literal),
            // A NO-SUBSTITUTION template literal is an ordinary static
            // specifier that happens to be written with backticks
            // (import(`./Child.vue`)), and a carrier that moves to another
            // directory needs it rewritten exactly as much as the quoted form.
            //
            // A template WITH substitutions is not statically knowable — its
            // specifier depends on runtime values — so it is deliberately left
            // alone rather than guessed at.
            Expression::TemplateLiteral(template) => self.record_template(template),
            _ => {}
        }
        oxc_ast_visit::walk::walk_import_expression(self, expr);
    }

    fn visit_ts_import_type(&mut self, ty: &TSImportType<'a>) {
        self.record(&ty.source);
        oxc_ast_visit::walk::walk_ts_import_type(self, ty);
    }

    fn visit_ts_import_equals_declaration(&mut self, decl: &TSImportEqualsDeclaration<'a>) {
        if let TSModuleReference::ExternalModuleReference(reference) = &decl.module_reference {
            self.record(&reference.expression);
        }
        oxc_ast_visit::walk::walk_ts_import_equals_declaration(self, decl);
    }
}

#[cfg(test)]
#[path = "module_specifiers_tests.rs"]
mod module_specifiers_tests;
