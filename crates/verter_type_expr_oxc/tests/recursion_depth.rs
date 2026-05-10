//! Recursion-depth tests: programmatically build very deep TS-type
//! source strings and assert `lower_ts_type` returns without panic /
//! stack overflow on the default stack budget.
//!
//! Real codebases (Pinia, Vue Router, strict tuple builders) produce
//! deep types; the lowering layer must handle them without
//! wall-clock timeouts and without recursing past the default thread
//! stack. The architecture rule "high budgets OK but detect real
//! recursion; no wall-clock timeouts" applies here: each fixture
//! drives `lower_ts_type` to a 100-level structural depth and the
//! gate is "did the call return at all".
//!
//! Each fixture is a discriminating test: it FAILS (stack overflow /
//! panic) on a tree where `lower_ts_type` recurses unguarded past the
//! default stack budget, and PASSES on the post-cutover tree. If a
//! later change re-introduces unbounded recursion, these fixtures
//! catch it.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;

use verter_type_expr_oxc::lower_ts_type;

const DEPTH: usize = 100;

fn build_deep_conditional() -> String {
    // type __T = A0 extends 0 ? A1 extends 0 ? ... ? never : never : never;
    // Open all DEPTH conditional headers, then close them all.
    let mut s = String::from("type __T = ");
    for i in 0..DEPTH {
        s.push_str(&format!("A{i} extends 0 ? "));
    }
    s.push_str("never");
    for _ in 0..DEPTH {
        s.push_str(" : never");
    }
    s.push(';');
    s
}

fn build_deep_template_literal() -> String {
    // type __T = `${`${`${ ... `${"x"}` ... }`}`}`;
    let mut s = String::from("type __T = ");
    for _ in 0..DEPTH {
        s.push_str("`${");
    }
    s.push_str(r#""x""#);
    for _ in 0..DEPTH {
        s.push('}');
        s.push('`');
    }
    s.push(';');
    s
}

fn build_deep_mapped() -> String {
    // type __T = { [K0 in keyof T]: { [K1 in keyof T]: ... string ... } };
    let mut s = String::from("type __T = ");
    for i in 0..DEPTH {
        s.push_str(&format!("{{ [K{i} in keyof T]: "));
    }
    s.push_str("string");
    for _ in 0..DEPTH {
        s.push_str(" }");
    }
    s.push(';');
    s
}

fn build_deep_generic() -> String {
    // type __T = Array<Array<Array< ... <string> ... >>>;
    let mut s = String::from("type __T = ");
    for _ in 0..DEPTH {
        s.push_str("Array<");
    }
    s.push_str("string");
    for _ in 0..DEPTH {
        s.push('>');
    }
    s.push(';');
    s
}

/// Parse `source` (which must declare `type __T = ...;`), locate the
/// `__T` alias, and call `lower_ts_type` on its annotation.
///
/// The test passes if this function returns at all — `lower_ts_type`
/// is allowed to produce `TypeExpr::Unknown` on the deep input. What
/// it must NOT do is panic or stack-overflow.
fn lower_or_panic(source: String) {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &source, SourceType::ts()).parse();

    assert!(
        !ret.panicked,
        "OXC parser panicked on fixture (fixture builder bug, not lowering bug)"
    );

    let alias = ret
        .program
        .body
        .iter()
        .find_map(|stmt| match stmt {
            Statement::TSTypeAliasDeclaration(alias) if alias.id.name == "__T" => {
                Some(&alias.type_annotation)
            }
            _ => None,
        })
        .expect("test wrapper missing __T alias — fixture builder bug");

    // The actual exercise. If `lower_ts_type` recurses unguarded past
    // the default stack budget, this overflows. The test passing
    // means lowering returned (regardless of what TypeExpr it
    // produced).
    let _expr = lower_ts_type(alias, &source);
}

#[test]
fn deep_conditional_terminates() {
    lower_or_panic(build_deep_conditional());
}

#[test]
fn deep_template_literal_terminates() {
    lower_or_panic(build_deep_template_literal());
}

#[test]
fn deep_mapped_terminates() {
    lower_or_panic(build_deep_mapped());
}

#[test]
fn deep_generic_terminates() {
    lower_or_panic(build_deep_generic());
}
