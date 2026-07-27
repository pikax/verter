use super::super::types::Dynamism;
use super::*;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

/// Helper to parse and extract bindings from an expression
fn extract(source: &str) -> (Vec<String>, bool, Vec<u32>) {
    extract_with_offset(source, 0)
}

fn extract_with_offset(source: &str, offset: u32) -> (Vec<String>, bool, Vec<u32>) {
    let allocator = Allocator::default();
    let parser = Parser::new(&allocator, source, SourceType::tsx());
    match parser.parse_expression() {
        Ok(expr) => {
            let ctx = BindingContext::new(offset);
            let result = extract_bindings_from_expression(&expr, source, ctx);
            let names: Vec<String> = result
                .non_ignored_binding_names()
                .into_iter()
                .map(String::from)
                .collect();
            let positions: Vec<u32> = result
                .bindings
                .iter()
                .filter(|b| !b.ignore)
                .map(|b| b.pos)
                .collect();
            (names, result.has_functions(), positions)
        }
        Err(_) => (vec![], false, vec![]),
    }
}

// ===========================================
// Simple identifiers
// ===========================================

#[test]
fn test_single_identifier() {
    let (names, _, positions) = extract("foo");
    assert_eq!(names, vec!["foo"]);
    assert_eq!(positions, vec![0]);
}

#[test]
fn test_single_identifier_with_offset() {
    let (names, _, positions) = extract_with_offset("foo", 100);
    assert_eq!(names, vec!["foo"]);
    assert_eq!(positions, vec![100]);
}

#[test]
fn test_multiple_identifiers() {
    let (names, _, positions) = extract("foo + bar");
    assert_eq!(names, vec!["foo", "bar"]);
    assert_eq!(positions, vec![0, 6]);
}

#[test]
fn test_keywords_ignored() {
    let (names, _, _) = extract("true && false || null");
    assert!(names.is_empty());
}

#[test]
fn test_undefined_ignored() {
    let (names, _, _) = extract("foo ?? undefined");
    assert_eq!(names, vec!["foo"]);
}

// ===========================================
// Member expressions
// ===========================================

#[test]
fn test_simple_member() {
    let (names, _, _) = extract("foo.bar");
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn test_computed_member() {
    let (names, _, _) = extract("foo[bar]");
    assert_eq!(names, vec!["foo", "bar"]);
}

#[test]
fn test_chained_member() {
    let (names, _, _) = extract("foo.bar.baz");
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn test_mixed_member() {
    let (names, _, _) = extract("foo.bar[baz].qux");
    assert_eq!(names, vec!["foo", "baz"]);
}

// ===========================================
// Function calls
// ===========================================

#[test]
fn test_function_call() {
    let (names, _, _) = extract("foo()");
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn test_function_call_with_args() {
    let (names, _, _) = extract("foo(bar, baz)");
    assert_eq!(names, vec!["foo", "bar", "baz"]);
}

#[test]
fn test_method_call() {
    let (names, _, _) = extract("foo.bar(baz)");
    assert_eq!(names, vec!["foo", "baz"]);
}

// ===========================================
// Arrow functions
// ===========================================

#[test]
fn test_arrow_function_simple() {
    let (names, has_funcs, _) = extract("() => foo");
    assert!(has_funcs);
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn test_arrow_function_with_param() {
    let (names, has_funcs, _) = extract("(x) => x + foo");
    assert!(has_funcs);
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn test_arrow_function_with_default() {
    let (names, has_funcs, _) = extract("(x = defaultVal) => x + foo");
    assert!(has_funcs);
    assert_eq!(names, vec!["defaultVal", "foo"]);
}

#[test]
fn test_arrow_function_with_destructuring() {
    let (names, has_funcs, _) = extract("({ a, b }) => a + b + foo");
    assert!(has_funcs);
    assert_eq!(names, vec!["foo"]);
}

// ===========================================
// Object literals
// ===========================================

#[test]
fn test_object_property() {
    let (names, _, _) = extract("{ foo: bar }");
    assert_eq!(names, vec!["bar"]);
}

#[test]
fn test_object_shorthand() {
    let (names, _, _) = extract("{ foo }");
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn test_object_spread() {
    let (names, _, _) = extract("{ ...obj, foo: bar }");
    assert_eq!(names, vec!["obj", "bar"]);
}

// ===========================================
// Array literals
// ===========================================

#[test]
fn test_array() {
    let (names, _, _) = extract("[foo, bar, baz]");
    assert_eq!(names, vec!["foo", "bar", "baz"]);
}

#[test]
fn test_array_spread() {
    let (names, _, _) = extract("[...arr, foo]");
    assert_eq!(names, vec!["arr", "foo"]);
}

// ===========================================
// Ternary
// ===========================================

#[test]
fn test_ternary() {
    let (names, _, _) = extract("cond ? foo : bar");
    assert_eq!(names, vec!["cond", "foo", "bar"]);
}

// ===========================================
// Template literals
// ===========================================

#[test]
fn test_template_literal() {
    let (names, _, _) = extract("`hello ${name}`");
    assert_eq!(names, vec!["name"]);
}

#[test]
fn test_tagged_template() {
    let (names, _, _) = extract("tag`hello ${name}`");
    assert_eq!(names, vec!["tag", "name"]);
}

// ===========================================
// TypeScript
// ===========================================

#[test]
fn test_as_assertion() {
    let (names, _, _) = extract("foo as string");
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn test_non_null_assertion() {
    let (names, _, _) = extract("foo!");
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn test_satisfies() {
    let (names, _, _) = extract("foo satisfies MyType");
    assert_eq!(names, vec!["foo"]);
}

// ===========================================
// Edge cases
// ===========================================

#[test]
fn test_deeply_nested() {
    let (names, _, _) = extract("a.b.c.d.e.f.g.h(i, j, k).l.m[n].o");
    assert_eq!(names, vec!["a", "i", "j", "k", "n"]);
}

#[test]
fn test_multiple_arrow_functions() {
    let (names, has_funcs, _) = extract("(x) => (y) => (z) => x + y + z + foo");
    assert!(has_funcs);
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn test_iife() {
    let (names, has_funcs, _) = extract("((x) => x + foo)(bar)");
    assert!(has_funcs);
    assert_eq!(names, vec!["foo", "bar"]);
}

#[test]
fn test_optional_chaining() {
    let (names, _, _) = extract("foo?.bar?.baz");
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn test_nullish_coalescing() {
    let (names, _, _) = extract("foo ?? bar");
    assert_eq!(names, vec!["foo", "bar"]);
}

// ===========================================
// Dynamism (computed incrementally during extraction)
// ===========================================

/// Helper that returns the full BindingExtractionResult for dynamism tests
fn extract_result<'a>(
    source: &'a str,
    alloc: &'a Allocator,
    ignored: &[&'a str],
) -> BindingExtractionResult<'a> {
    let parser = Parser::new(alloc, source, SourceType::tsx());
    let expr = parser.parse_expression().unwrap();
    let ctx = BindingContext::with_ignored(0, ignored.iter().copied());
    extract_bindings_from_expression(&expr, source, ctx)
}

/// Pure literal → Static (no identifiers at all)
#[test]
fn dynamism_literal_static() {
    let alloc = Allocator::default();
    let result = extract_result("42", &alloc, &[]);
    assert_eq!(result.dynamism, Dynamism::Static);
}

/// String literal → Static
#[test]
fn dynamism_string_literal_static() {
    let alloc = Allocator::default();
    let result = extract_result("'hello'", &alloc, &[]);
    assert_eq!(result.dynamism, Dynamism::Static);
}

/// Binary expression of pure literals → Static
#[test]
fn dynamism_binary_literals_static() {
    let alloc = Allocator::default();
    let result = extract_result("1 + 2", &alloc, &[]);
    assert_eq!(result.dynamism, Dynamism::Static);
}

/// Keywords only (true && false) → Static
#[test]
fn dynamism_keywords_only_static() {
    let alloc = Allocator::default();
    let result = extract_result("true && false", &alloc, &[]);
    assert_eq!(result.dynamism, Dynamism::Static);
}

/// Script-level identifier → MaybeDynamic
#[test]
fn dynamism_script_identifier_maybe_dynamic() {
    let alloc = Allocator::default();
    let result = extract_result("foo", &alloc, &[]);
    assert_eq!(result.dynamism, Dynamism::MaybeDynamic);
}

/// Multiple script-level identifiers → MaybeDynamic
#[test]
fn dynamism_multiple_script_identifiers_maybe_dynamic() {
    let alloc = Allocator::default();
    let result = extract_result("foo + bar", &alloc, &[]);
    assert_eq!(result.dynamism, Dynamism::MaybeDynamic);
}

/// Ignored identifier (v-for local) → Dynamic
#[test]
fn dynamism_ignored_identifier_dynamic() {
    let alloc = Allocator::default();
    let result = extract_result("item", &alloc, &["item"]);
    assert_eq!(result.dynamism, Dynamism::Dynamic);
}

/// Member expression with ignored root → Dynamic
#[test]
fn dynamism_ignored_member_expr_dynamic() {
    let alloc = Allocator::default();
    let result = extract_result("item.name", &alloc, &["item"]);
    assert_eq!(result.dynamism, Dynamism::Dynamic);
}

/// Mixed: ignored local + script-level → Dynamic (injected trumps)
#[test]
fn dynamism_mixed_injected_trumps() {
    let alloc = Allocator::default();
    let result = extract_result("item.name + cls", &alloc, &["item"]);
    assert_eq!(result.dynamism, Dynamism::Dynamic);
}

/// Keyword-ignored (e.g. `undefined`) is NOT an injected local → stays MaybeDynamic
#[test]
fn dynamism_keyword_ignored_not_injected() {
    let alloc = Allocator::default();
    let result = extract_result("foo ?? undefined", &alloc, &[]);
    // `undefined` is keyword-ignored, not injected → MaybeDynamic (from `foo`)
    assert_eq!(result.dynamism, Dynamism::MaybeDynamic);
}

/// Arrow function body references script-level → MaybeDynamic
#[test]
fn dynamism_arrow_function_maybe_dynamic() {
    let alloc = Allocator::default();
    let result = extract_result("() => foo", &alloc, &[]);
    assert_eq!(result.dynamism, Dynamism::MaybeDynamic);
}

/// Arrow function with ignored in body → Dynamic
#[test]
fn dynamism_arrow_function_with_ignored_dynamic() {
    let alloc = Allocator::default();
    let result = extract_result("() => item.name", &alloc, &["item"]);
    assert_eq!(result.dynamism, Dynamism::Dynamic);
}

// ===========================================
// JS globals should be ignored (not prefixed with _ctx.)
// ===========================================

#[test]
fn test_global_string_method() {
    // String.fromCharCode(65) should NOT produce "String" as a non-ignored binding
    let (names, _, _) = extract("String.fromCharCode(65)");
    assert!(
        names.is_empty(),
        "String should be ignored as a global, got: {:?}",
        names
    );
}

#[test]
fn test_global_math_method() {
    let (names, _, _) = extract("Math.max(a, b)");
    assert_eq!(names, vec!["a", "b"], "Math should be ignored as a global");
}

#[test]
fn test_global_array_method() {
    let (names, _, _) = extract("Array.isArray(items)");
    assert_eq!(names, vec!["items"], "Array should be ignored as a global");
}

#[test]
fn test_global_json() {
    let (names, _, _) = extract("JSON.stringify(data)");
    assert_eq!(names, vec!["data"], "JSON should be ignored as a global");
}

#[test]
fn test_global_object_keys() {
    let (names, _, _) = extract("Object.keys(obj)");
    assert_eq!(names, vec!["obj"], "Object should be ignored as a global");
}

#[test]
fn test_global_number_is_finite() {
    let (names, _, _) = extract("Number.isFinite(val)");
    assert_eq!(names, vec!["val"], "Number should be ignored as a global");
}

#[test]
fn test_global_console_log() {
    let (names, _, _) = extract("console.log(msg)");
    assert_eq!(names, vec!["msg"], "console should be ignored as a global");
}

#[test]
fn test_global_parseint() {
    let (names, _, _) = extract("parseInt(str, 10)");
    assert_eq!(names, vec!["str"], "parseInt should be ignored as a global");
}

#[test]
fn test_global_promise() {
    let (names, _, _) = extract("Promise.resolve(val)");
    assert_eq!(names, vec!["val"], "Promise should be ignored as a global");
}

#[test]
fn test_global_dynamism_is_static() {
    // Pure global call with literal args should be Static
    let alloc = Allocator::default();
    let result = extract_result("String.fromCharCode(65)", &alloc, &[]);
    assert_eq!(
        result.dynamism,
        Dynamism::Static,
        "Global with literal args should be Static"
    );
}

// ===========================================
// Source-order invariant
// ===========================================

/// Assert the ordering contract `BindingExtractionResult` publishes: the
/// non-ignored bindings are in strictly increasing, non-overlapping source
/// order.
///
/// This is the exact precondition the template emitter consumes. It walks the
/// bindings in vector order and, before each one, copies the verbatim source
/// run that ends where that binding starts; a binding that arrives at or before
/// the previous binding's end silently loses its preceding run and lands after
/// text that already contains it.
fn assert_source_ordered(source: &str, result: &BindingExtractionResult<'_>) {
    let mut prev_end = 0u32;
    let mut prev_name = "";
    for binding in result.bindings.iter().filter(|b| !b.ignore) {
        assert!(
            binding.pos >= prev_end,
            "bindings must arrive in source order: {:?} at {} follows {:?} ending at {}, in {source:?}\nbindings: {:?}",
            binding.name,
            binding.pos,
            prev_name,
            prev_end,
            result
                .bindings
                .iter()
                .map(|b| (b.name, b.pos, b.ignore))
                .collect::<Vec<_>>(),
        );
        prev_end = binding.pos + binding.name.len() as u32;
        prev_name = binding.name;
    }
}

fn extract_program_result<'a>(
    source: &'a str,
    alloc: &'a Allocator,
) -> BindingExtractionResult<'a> {
    let parsed = Parser::new(alloc, source, SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "fixture must parse: {source:?} → {:?}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
    let program = alloc.alloc(parsed.program);
    extract_bindings_from_program(program, source, BindingContext::new(0))
}

/// Source order is an INVARIANT of `BindingExtractionResult`, not a rule each
/// visitor arm has to remember.
///
/// The emitter interleaves each binding with the verbatim source run in front
/// of it, so it is only correct when the bindings arrive in source order. That
/// used to be an unwritten obligation on every arm of the descent, and an arm
/// only has to visit two sub-positions in evaluation order rather than source
/// order to break it — which no exhaustiveness check can catch, because the arm
/// itself is present and the variant is covered. The single mutator now places
/// each binding in source order as it is recorded, so the obligation is gone.
///
/// This walks the descent surface — every statement form and every compound
/// expression form with more than one identifier position — and pins the
/// property. `for (x of xs)` is the row that fails if the ordering is dropped:
/// nothing else in the grammar today visits sub-positions out of source order.
#[test]
fn extracted_bindings_are_in_source_order_for_every_form() {
    let sources = [
        // Loop heads: the target is a real reference whose position precedes
        // the iterated expression it is evaluated after.
        "for (x of xs) log(x)",
        "for (x in xs) log(x)",
        "for ([a, b] of xs) log(a)",
        "for ({ k: v } of xs) log(v)",
        "for (obj.k of xs) log(obj.k)",
        "for (obj[key] of xs) log(key)",
        "for (const y of xs) log(y)",
        "for (i = 0; i < n; i++) log(i)",
        "for (;;) log(a)",
        // Other statement forms.
        "if (a) b(); else c()",
        "while (a) b()",
        "do b(); while (a)",
        "switch (a) { case b: c(); break; default: d() }",
        "try { a() } catch (e) { b(e) } finally { c() }",
        "with (a) { b() }",
        "label: { a(); break label }",
        "throw a",
        "const { p = q } = r",
        "let [s = t] = u",
        "class C extends Base { static { a() } [k]() { return b } accessor v = c }",
        "function f(p = a, { q = b } = c) { return p + q + d }",
        // Compound expressions.
        "a ? b : c",
        "a + b * c",
        "a && b || c",
        "a?.[b]?.(c)",
        "obj[key].fn(arg)",
        "`${a} and ${b}`",
        "tag`${a}${b}`",
        "({ ...a, [b]: c, d })",
        "[a, ...b, c]",
        "new Ctor(a, b)",
        "(a = b), (c = d)",
        "a = b ?? c",
        "({ a, b } = c)",
        "[a, b] = c",
        "a as B",
        "a satisfies B",
        "a!.b",
        "import(a, b)",
        "(a, b) => a + b + c",
        "async () => { await a; yield_(b) }",
        "function* g() { yield a; yield* b }",
        "typeof a === 'x' ? b : c",
        "delete a.b",
        "void a",
        "-a + +b",
        "a instanceof B",
        "#p in obj",
    ];

    for source in sources {
        let alloc = Allocator::default();
        // `#p in obj` is only legal inside a class body.
        let wrapped = if source.starts_with('#') {
            format!("class C {{ #p; m(o) {{ return {source} }} }}")
        } else {
            source.to_string()
        };
        let result = extract_program_result(&wrapped, &alloc);
        assert_source_ordered(&wrapped, &result);
    }
}
