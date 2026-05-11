//! TS-checker display-text input boundary adapter.
//!
//! TypeScript's `checker.typeToString()` (used by both tsserver's
//! `quickinfo.displayString` and TSGO's hover contents) emits TS source-form
//! type text. This module is the single permitted bridge between that text
//! and Verter's typed [`TypeExpr`] IR.
//!
//! The resolver / projector / registry / policy / materialiser pipeline never
//! parses TS type text directly. Every other producer-side caller in that
//! pipeline operates on a `TSType<'_>` AST node and lowers via
//! [`verter_type_expr_oxc::lower_ts_type`]. Checker-text parsing exists only
//! here, at the input boundary, where the source AST is gone and only the
//! display form remains. See the "Typed-IR-Only Resolver Rule" in CLAUDE.md.
//!
//! # Architecture
//!
//! The adapter wraps the input in `type __T = <input>`, parses it as TS via
//! OXC, walks the resulting type-alias declaration, and lowers the right-hand
//! side `TSType<'_>` AST node via `lower_ts_type`. Unrecognised or invalid
//! input produces [`TypeExpr::Unknown { raw }`].
//!
//! # Performance
//!
//! The OXC `Allocator` is bumpalo-backed and grows monotonically until reset.
//! Each call resets a thread-local pooled allocator so its capacity is reused
//! without leaking across invocations. Failing to reset would amplify into a
//! real memory growth on the hot path (background indexing, "Go to
//! Definition" chains).
//!
//! The architecture guard `no_checker_display_text_parsing_outside_adapter`
//! (in `crates/verter_session/tests/architecture_guards.rs`) blocks any other
//! production module from referring to `parse_checker_text_to_type_expr`.

use std::cell::RefCell;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_type_expr::TypeExpr;

thread_local! {
    /// Pooled OXC allocator. Reused across invocations on the same thread to
    /// keep arena allocations off the hot path; reset at the start of each
    /// call so prior parses do not retain memory.
    static OXC_POOL: RefCell<Allocator> = RefCell::new(Allocator::default());
}

/// Parse TS checker display text into a [`TypeExpr`].
///
/// Returns [`TypeExpr::Unknown`] when the input is empty or the wrap-and-lower
/// parse does not produce a `TSTypeAliasDeclaration`.
///
/// This is the **single permitted bridge** between TS-checker display text and
/// the typed-IR resolver pipeline. Any module that needs to consume checker
/// text must call this function. The architecture guard
/// `no_checker_display_text_parsing_outside_adapter` enforces that property.
pub fn parse_checker_text_to_type_expr(text: &str) -> TypeExpr {
    if text.trim().is_empty() {
        return TypeExpr::Unknown {
            raw: text.to_string(),
        };
    }

    OXC_POOL.with(|alloc| {
        let mut alloc_ref = alloc.borrow_mut();
        // CRITICAL: reset the arena between calls. OXC's bumpalo-backed
        // Allocator grows monotonically until reset; without this line the
        // thread-local pool would leak on every invocation.
        alloc_ref.reset();

        let wrapper = format!("type __T = {text}");
        let ret = Parser::new(&alloc_ref, &wrapper, SourceType::ts()).parse();

        for stmt in &ret.program.body {
            if let oxc_ast::ast::Statement::TSTypeAliasDeclaration(alias) = stmt {
                return verter_type_expr_oxc::lower_ts_type(&alias.type_annotation, &wrapper);
            }
        }

        TypeExpr::Unknown {
            raw: text.to_string(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_type_expr::{LiteralValue, ObjectMember, PrimitiveName};

    #[test]
    fn lowers_primitive_string() {
        match parse_checker_text_to_type_expr("string") {
            TypeExpr::Primitive(p) => assert_eq!(p, PrimitiveName::String),
            other => panic!("expected Primitive(String), got: {other:?}"),
        }
    }

    #[test]
    fn lowers_primitive_number() {
        match parse_checker_text_to_type_expr("number") {
            TypeExpr::Primitive(p) => assert_eq!(p, PrimitiveName::Number),
            other => panic!("expected Primitive(Number), got: {other:?}"),
        }
    }

    #[test]
    fn lowers_union() {
        match parse_checker_text_to_type_expr("string | number | undefined") {
            TypeExpr::Union(arms) => {
                assert_eq!(arms.len(), 3, "union should have 3 arms, got: {arms:?}");
            }
            other => panic!("expected Union, got: {other:?}"),
        }
    }

    #[test]
    fn lowers_object_literal() {
        match parse_checker_text_to_type_expr("{ x: number; y: string }") {
            TypeExpr::Object(obj) => {
                assert_eq!(obj.properties.len(), 2);
                if let ObjectMember::Property(p) = &obj.properties[0] {
                    assert_eq!(p.name, "x");
                    assert!(matches!(p.ty, TypeExpr::Primitive(PrimitiveName::Number)));
                }
                if let ObjectMember::Property(p) = &obj.properties[1] {
                    assert_eq!(p.name, "y");
                    assert!(matches!(p.ty, TypeExpr::Primitive(PrimitiveName::String)));
                }
            }
            other => panic!("expected Object, got: {other:?}"),
        }
    }

    #[test]
    fn lowers_indexed_access() {
        match parse_checker_text_to_type_expr(r#"Foo["bar"]"#) {
            TypeExpr::IndexedAccess { object, index } => {
                assert!(
                    matches!(&*object, TypeExpr::Ref { name, .. } if name.as_ref() == "Foo"),
                    "unexpected object: {object:?}"
                );
                assert!(
                    matches!(&*index, TypeExpr::Literal(LiteralValue::String(s)) if s == "bar"),
                    "unexpected index: {index:?}"
                );
            }
            other => panic!("expected IndexedAccess, got: {other:?}"),
        }
    }

    #[test]
    fn empty_input_is_unknown() {
        match parse_checker_text_to_type_expr("") {
            TypeExpr::Unknown { ref raw } => assert!(raw.is_empty()),
            other => panic!("expected Unknown for empty input, got: {other:?}"),
        }
    }

    #[test]
    fn whitespace_only_is_unknown() {
        match parse_checker_text_to_type_expr("   ") {
            TypeExpr::Unknown { .. } => {}
            other => panic!("expected Unknown for whitespace input, got: {other:?}"),
        }
    }

    // Pooled-allocator reset discriminator: hammers the adapter in a tight
    // loop and asserts the thread-local arena's reported capacity stays
    // bounded.
    //
    // Without the `alloc_ref.reset()` call in `parse_checker_text_to_type_expr`
    // the arena would grow monotonically with every iteration. We check
    // boundedness by comparing the arena's used bytes after a warm-up call
    // against the bytes used after 10,000 iterations — the ratio must stay
    // small (we allow a generous 4x slack to absorb cross-platform allocator
    // alignment quirks; a real leak grows by 4 orders of magnitude over the
    // same loop size).
    #[test]
    fn pooled_allocator_resets_between_calls_bounded_memory() {
        // Warm up: prime the allocator with one parse so the post-reset
        // capacity is a stable baseline rather than fresh-default.
        let _ = parse_checker_text_to_type_expr("string | number | { a: boolean }");

        let baseline_used = OXC_POOL.with(|alloc| {
            // After parse + drop of alloc_ref, the alloc still holds onto
            // its arena pages. We probe used bytes to capture a stable
            // single-call baseline.
            let alloc_ref = alloc.borrow();
            alloc_ref.used_bytes()
        });

        // Hammer the adapter 10,000 times with a non-trivial type.
        for _ in 0..10_000 {
            let expr = parse_checker_text_to_type_expr(
                "string | number | { a: boolean; b: { c: string }[] } | undefined",
            );
            // Use the result so the optimiser does not elide the parse.
            std::hint::black_box(expr);
        }

        let after_used = OXC_POOL.with(|alloc| {
            let alloc_ref = alloc.borrow();
            alloc_ref.used_bytes()
        });

        // The arena must not have grown unboundedly. We allow a generous 4x
        // slack over the single-call baseline; a real leak would balloon
        // into the megabyte range across 10,000 iterations. The reset
        // invariant keeps the post-loop figure within the same arena page
        // as the post-warm-up baseline.
        //
        // Note: `used_bytes()` on bumpalo reports the current chunk's
        // residual; after a reset + a single parse, that equals the
        // peak-of-one-parse, which is bounded.
        let slack_limit = baseline_used.saturating_mul(4).max(64 * 1024);
        assert!(
            after_used <= slack_limit,
            "pooled allocator leaked: baseline used = {baseline_used} bytes, \
             after 10k iterations = {after_used} bytes (slack limit = {slack_limit}). \
             This indicates a missing `alloc_ref.reset()` call.",
        );
    }
}
