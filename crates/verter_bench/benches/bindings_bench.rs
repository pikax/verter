use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use std::hint::black_box;
use verter_core::utils::oxc::{
    extract_bindings_from_expression, extract_bindings_from_program, BindingContext,
};

/// Generate a simple expression with N identifiers
fn generate_simple_expr(n: usize) -> String {
    (0..n)
        .map(|i| format!("var{}", i))
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Generate a chained member expression: a.b.c.d...
fn generate_member_expr(depth: usize) -> String {
    let parts: Vec<String> = (0..depth).map(|i| format!("prop{}", i)).collect();
    format!("obj.{}", parts.join("."))
}

/// Generate nested function calls: fn1(fn2(fn3(...)))
fn generate_nested_calls(depth: usize) -> String {
    let mut result = "innerValue".to_string();
    for i in (0..depth).rev() {
        result = format!("func{}({})", i, result);
    }
    result
}

/// Generate array with N elements
fn generate_array_expr(n: usize) -> String {
    let items: Vec<String> = (0..n).map(|i| format!("item{}", i)).collect();
    format!("[{}]", items.join(", "))
}

/// Generate object with N properties
fn generate_object_expr(n: usize) -> String {
    let props: Vec<String> = (0..n).map(|i| format!("key{}: val{}", i, i)).collect();
    format!("{{ {} }}", props.join(", "))
}

/// Generate a complex expression with mixed constructs
fn generate_complex_expr(complexity: usize) -> String {
    let mut parts = Vec::new();

    // Add some identifiers
    for i in 0..complexity {
        parts.push(format!("var{}", i));
    }

    // Add some member expressions
    for i in 0..complexity / 2 {
        parts.push(format!("obj{}.prop{}.nested", i, i));
    }

    // Add some function calls
    for i in 0..complexity / 2 {
        parts.push(format!("fn{}(arg{})", i, i));
    }

    // Add some template literals
    for i in 0..complexity / 4 {
        parts.push(format!("`template ${{interp{}}} text`", i));
    }

    parts.join(" + ")
}

/// Generate an arrow function with N parameters and body references
fn generate_arrow_function(params: usize, external_refs: usize) -> String {
    let param_list: Vec<String> = (0..params).map(|i| format!("p{}", i)).collect();
    let body_refs: Vec<String> = (0..external_refs).map(|i| format!("ext{}", i)).collect();
    let param_uses: Vec<String> = (0..params).map(|i| format!("p{}", i)).collect();

    let all_refs = [param_uses, body_refs].concat().join(" + ");
    format!("({}) => {}", param_list.join(", "), all_refs)
}

/// Generate a program with N variable declarations
fn generate_program_vars(n: usize) -> String {
    (0..n)
        .map(|i| format!("const var{} = value{} + external{};", i, i, i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate a program with nested functions
fn generate_program_functions(n: usize) -> String {
    (0..n)
        .map(|i| {
            format!(
                "function func{}(param{}) {{ return param{} + external{}; }}",
                i, i, i, i
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate a complex program simulating real Vue template expressions
fn generate_vue_like_program(complexity: usize) -> String {
    let mut statements = Vec::new();

    // Import-like statements (variable declarations)
    for i in 0..complexity / 2 {
        statements.push(format!("const component{} = imported{};", i, i));
    }

    // Computed-like expressions
    for i in 0..complexity / 2 {
        statements.push(format!(
            "const computed{} = () => state{}.value + props{}.data;",
            i, i, i
        ));
    }

    // Method-like functions
    for i in 0..complexity / 4 {
        statements.push(format!(
            "function handle{}(event) {{ return process{}(event.target.value, config{}); }}",
            i, i, i
        ));
    }

    // Watch-like expressions
    for i in 0..complexity / 4 {
        statements.push(format!(
            "const effect{} = (newVal, oldVal) => {{ if (newVal !== oldVal) emit{}(newVal); }};",
            i, i
        ));
    }

    statements.join("\n")
}

fn bench_expression_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("expression_extraction");

    // Benchmark simple expressions with varying identifier counts
    for size in [5, 10, 25, 50, 100] {
        let expr = generate_simple_expr(size);
        group.throughput(Throughput::Bytes(expr.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("simple_identifiers", size),
            &expr,
            |b, expr| {
                b.iter(|| {
                    let allocator = Allocator::default();
                    let source_type = SourceType::tsx();
                    let parser = Parser::new(&allocator, expr, source_type);
                    let result = parser.parse_expression();
                    if let Ok(ast) = result {
                        let ctx = BindingContext::new(0);
                        let extraction =
                            extract_bindings_from_expression(black_box(&ast), expr, ctx);
                        black_box(extraction);
                    }
                });
            },
        );
    }

    // Benchmark member expression chains
    for depth in [5, 10, 20, 50] {
        let expr = generate_member_expr(depth);
        group.throughput(Throughput::Bytes(expr.len() as u64));
        group.bench_with_input(BenchmarkId::new("member_chain", depth), &expr, |b, expr| {
            b.iter(|| {
                let allocator = Allocator::default();
                let source_type = SourceType::tsx();
                let parser = Parser::new(&allocator, expr, source_type);
                let result = parser.parse_expression();
                if let Ok(ast) = result {
                    let ctx = BindingContext::new(0);
                    let extraction = extract_bindings_from_expression(black_box(&ast), expr, ctx);
                    black_box(extraction);
                }
            });
        });
    }

    // Benchmark nested function calls
    for depth in [5, 10, 20, 50] {
        let expr = generate_nested_calls(depth);
        group.throughput(Throughput::Bytes(expr.len() as u64));
        group.bench_with_input(BenchmarkId::new("nested_calls", depth), &expr, |b, expr| {
            b.iter(|| {
                let allocator = Allocator::default();
                let source_type = SourceType::tsx();
                let parser = Parser::new(&allocator, expr, source_type);
                let result = parser.parse_expression();
                if let Ok(ast) = result {
                    let ctx = BindingContext::new(0);
                    let extraction = extract_bindings_from_expression(black_box(&ast), expr, ctx);
                    black_box(extraction);
                }
            });
        });
    }

    // Benchmark array expressions
    for size in [10, 25, 50, 100] {
        let expr = generate_array_expr(size);
        group.throughput(Throughput::Bytes(expr.len() as u64));
        group.bench_with_input(BenchmarkId::new("array_expr", size), &expr, |b, expr| {
            b.iter(|| {
                let allocator = Allocator::default();
                let source_type = SourceType::tsx();
                let parser = Parser::new(&allocator, expr, source_type);
                let result = parser.parse_expression();
                if let Ok(ast) = result {
                    let ctx = BindingContext::new(0);
                    let extraction = extract_bindings_from_expression(black_box(&ast), expr, ctx);
                    black_box(extraction);
                }
            });
        });
    }

    // Benchmark object expressions
    for size in [10, 25, 50, 100] {
        let expr = generate_object_expr(size);
        group.throughput(Throughput::Bytes(expr.len() as u64));
        group.bench_with_input(BenchmarkId::new("object_expr", size), &expr, |b, expr| {
            b.iter(|| {
                let allocator = Allocator::default();
                let source_type = SourceType::tsx();
                let parser = Parser::new(&allocator, expr, source_type);
                let result = parser.parse_expression();
                if let Ok(ast) = result {
                    let ctx = BindingContext::new(0);
                    let extraction = extract_bindings_from_expression(black_box(&ast), expr, ctx);
                    black_box(extraction);
                }
            });
        });
    }

    // Benchmark complex mixed expressions
    for complexity in [5, 10, 25, 50] {
        let expr = generate_complex_expr(complexity);
        group.throughput(Throughput::Bytes(expr.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("complex_mixed", complexity),
            &expr,
            |b, expr| {
                b.iter(|| {
                    let allocator = Allocator::default();
                    let source_type = SourceType::tsx();
                    let parser = Parser::new(&allocator, expr, source_type);
                    let result = parser.parse_expression();
                    if let Ok(ast) = result {
                        let ctx = BindingContext::new(0);
                        let extraction =
                            extract_bindings_from_expression(black_box(&ast), expr, ctx);
                        black_box(extraction);
                    }
                });
            },
        );
    }

    // Benchmark arrow functions
    for (params, ext_refs) in [(2, 5), (5, 10), (10, 20), (20, 50)] {
        let expr = generate_arrow_function(params, ext_refs);
        let label = format!("{}p_{}ext", params, ext_refs);
        group.throughput(Throughput::Bytes(expr.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("arrow_function", label),
            &expr,
            |b, expr| {
                b.iter(|| {
                    let allocator = Allocator::default();
                    let source_type = SourceType::tsx();
                    let parser = Parser::new(&allocator, expr, source_type);
                    let result = parser.parse_expression();
                    if let Ok(ast) = result {
                        let ctx = BindingContext::new(0);
                        let extraction =
                            extract_bindings_from_expression(black_box(&ast), expr, ctx);
                        black_box(extraction);
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_program_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("program_extraction");

    // Benchmark programs with variable declarations
    for n in [10, 25, 50, 100, 200] {
        let program = generate_program_vars(n);
        group.throughput(Throughput::Bytes(program.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("variable_decls", n),
            &program,
            |b, program| {
                b.iter(|| {
                    let allocator = Allocator::default();
                    let source_type = SourceType::tsx();
                    let parser = Parser::new(&allocator, program, source_type);
                    let result = parser.parse();
                    if result.errors.is_empty() {
                        let ctx = BindingContext::new(0);
                        let extraction = extract_bindings_from_program(
                            black_box(&result.program),
                            program,
                            &ctx,
                        );
                        black_box(extraction);
                    }
                });
            },
        );
    }

    // Benchmark programs with function declarations
    for n in [5, 10, 25, 50, 100] {
        let program = generate_program_functions(n);
        group.throughput(Throughput::Bytes(program.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("function_decls", n),
            &program,
            |b, program| {
                b.iter(|| {
                    let allocator = Allocator::default();
                    let source_type = SourceType::tsx();
                    let parser = Parser::new(&allocator, program, source_type);
                    let result = parser.parse();
                    if result.errors.is_empty() {
                        let ctx = BindingContext::new(0);
                        let extraction = extract_bindings_from_program(
                            black_box(&result.program),
                            program,
                            &ctx,
                        );
                        black_box(extraction);
                    }
                });
            },
        );
    }

    // Benchmark Vue-like programs
    for complexity in [10, 25, 50, 100, 200] {
        let program = generate_vue_like_program(complexity);
        group.throughput(Throughput::Bytes(program.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("vue_like", complexity),
            &program,
            |b, program| {
                b.iter(|| {
                    let allocator = Allocator::default();
                    let source_type = SourceType::tsx();
                    let parser = Parser::new(&allocator, program, source_type);
                    let result = parser.parse();
                    if result.errors.is_empty() {
                        let ctx = BindingContext::new(0);
                        let extraction = extract_bindings_from_program(
                            black_box(&result.program),
                            program,
                            &ctx,
                        );
                        black_box(extraction);
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark with pre-populated context (simulating real usage where some bindings are pre-ignored)
fn bench_with_context(c: &mut Criterion) {
    let mut group = c.benchmark_group("with_context");

    // Test how ignored identifiers affect performance
    for ignored_count in [0, 10, 50, 100] {
        let expr = generate_complex_expr(50);
        let label = format!("{}_ignored", ignored_count);

        group.throughput(Throughput::Bytes(expr.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("complex_expr", label),
            &(expr.clone(), ignored_count),
            |b, (expr, ignored_count)| {
                // Pre-create context with ignored identifiers
                let ignored: Vec<String> = (0..*ignored_count)
                    .map(|i| format!("ignored{}", i))
                    .collect();

                b.iter(|| {
                    let allocator = Allocator::default();
                    let source_type = SourceType::tsx();
                    let parser = Parser::new(&allocator, expr, source_type);
                    let result = parser.parse_expression();
                    if let Ok(ast) = result {
                        let ctx =
                            BindingContext::with_ignored(0, ignored.iter().map(|s| s.as_str()));
                        let extraction =
                            extract_bindings_from_expression(black_box(&ast), expr, ctx);
                        black_box(extraction);
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark parsing + extraction vs extraction only
fn bench_parsing_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing_overhead");

    let expr = generate_complex_expr(50);
    group.throughput(Throughput::Bytes(expr.len() as u64));

    // Benchmark parsing only
    group.bench_function("parse_only", |b| {
        b.iter(|| {
            let allocator = Allocator::default();
            let source_type = SourceType::tsx();
            let parser = Parser::new(&allocator, &expr, source_type);
            let result = parser.parse_expression();
            let _ = black_box(result);
        });
    });

    // Benchmark extraction only (pre-parsed)
    group.bench_function("extract_only", |b| {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let parser = Parser::new(&allocator, &expr, source_type);
        let ast = parser.parse_expression().unwrap();

        b.iter(|| {
            let ctx = BindingContext::new(0);
            let extraction = extract_bindings_from_expression(black_box(&ast), &expr, ctx);
            black_box(extraction);
        });
    });

    // Benchmark parse + extraction combined
    group.bench_function("parse_and_extract", |b| {
        b.iter(|| {
            let allocator = Allocator::default();
            let source_type = SourceType::tsx();
            let parser = Parser::new(&allocator, &expr, source_type);
            let result = parser.parse_expression();
            if let Ok(ast) = result {
                let ctx = BindingContext::new(0);
                let extraction = extract_bindings_from_expression(black_box(&ast), &expr, ctx);
                black_box(extraction);
            }
        });
    });

    group.finish();
}

/// Benchmark keyword detection performance
fn bench_keyword_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("keyword_detection");

    // Expression with many keywords (properly parenthesized to avoid parser errors)
    let expr_with_keywords = "(true && false) || (null) || (undefined && this.foo) + super.bar";
    group.throughput(Throughput::Bytes(expr_with_keywords.len() as u64));

    group.bench_function("with_keywords", |b| {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let parser = Parser::new(&allocator, expr_with_keywords, source_type);
        let ast = parser.parse_expression().unwrap();

        b.iter(|| {
            let ctx = BindingContext::new(0);
            let extraction =
                extract_bindings_from_expression(black_box(&ast), expr_with_keywords, ctx);
            black_box(extraction);
        });
    });

    // Expression with no keywords (all identifiers)
    let expr_no_keywords = generate_simple_expr(50);
    group.throughput(Throughput::Bytes(expr_no_keywords.len() as u64));

    group.bench_function("identifiers_only", |b| {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let parser = Parser::new(&allocator, &expr_no_keywords, source_type);
        let ast = parser.parse_expression().unwrap();

        b.iter(|| {
            let ctx = BindingContext::new(0);
            let extraction =
                extract_bindings_from_expression(black_box(&ast), &expr_no_keywords, ctx);
            black_box(extraction);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_expression_extraction,
    bench_program_extraction,
    bench_with_context,
    bench_parsing_overhead,
    bench_keyword_detection
);
criterion_main!(benches);
