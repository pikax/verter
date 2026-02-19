//! Compare Vapor vs Vapor2 output on the same input to verify correctness.
//!
//! Run with:
//!   cargo run --example compare_vapor --package verter_bench

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use verter_core::builder::codegen::{compile, CodegenOptions};
use verter_core::code_transform::CodeTransform;
use verter_core::new_impl::script::{generate_script, ScriptCodeGenOptions};
use verter_core::new_impl::syntax::Syntax as NewSyntax;
use verter_core::new_impl::template::code_gen::{
    generate_template, CodeGenMode, TemplateCodeGenOptions,
};
use verter_core::new_impl::template::oxc::parse_template_expressions;
use verter_core::syntax::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
use verter_core::tokenizer::byte::tokenize;

fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/benches/fixtures/{}.vue",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e))
}

/// Run the new_impl template codegen in a specific mode and return the full output string.
fn run_template_codegen(source: &str, mode: CodeGenMode) -> String {
    let alloc = Allocator::default();

    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &opts,
        diagnostics: Vec::new(),
    };
    let mut syntax = NewSyntax::new(false);
    tokenize(source.as_bytes(), |e| syntax.handle(&e, &ctx));

    // Script codegen (to get bindings)
    let mut ct = CodeTransform::new(source, &alloc);
    let script_opts = ScriptCodeGenOptions {
        component_name: "Anonymous",
        scope_id: "a4f2eed6",
        has_scoped_style: syntax.has_style_scope(),
        ..Default::default()
    };
    let script_result = generate_script(
        syntax.script(),
        syntax.script_setup(),
        source,
        &mut ct,
        &alloc,
        &script_opts,
    );

    let template_ast = syntax.take_template_ast();
    if let Some(ast) = &template_ast {
        let oxc_ast = parse_template_expressions(ast, source, &alloc, SourceType::tsx());
        let mut ct2 = CodeTransform::new(source, &alloc);
        generate_template(
            ast,
            &oxc_ast,
            source,
            &mut ct2,
            &alloc,
            script_result.bindings,
            &TemplateCodeGenOptions {
                mode,
                ..Default::default()
            },
        );
        ct2.build_string()
    } else {
        "(no template)".to_string()
    }
}

/// Run old pipeline in vapor mode.
fn run_old_vapor(source: &str) -> String {
    let vapor_source = source.replacen("<template>", "<template vapor>", 1);
    let allocator = Allocator::new();
    let mut options = CodegenOptions::new().with_filename("test.vue");
    options.skip_source_map = true;
    compile(&vapor_source, &options, &allocator).code
}

fn main() {
    let fixtures = [
        "simple",
        "medium",
        "large",
        "kitchen-sink",
        "template-heavy",
        "composition-heavy",
    ];

    for name in &fixtures {
        let source = load_fixture(name);

        println!("============================================================");
        println!("FIXTURE: {}", name);
        println!("============================================================");

        // Old pipeline vapor
        let old_vapor = run_old_vapor(&source);

        // New pipeline: Vapor v1
        let vapor1 = run_template_codegen(&source, CodeGenMode::Vapor);

        // New pipeline: Vapor2
        let vapor2 = run_template_codegen(&source, CodeGenMode::Vapor2);

        println!("\n--- OLD PIPELINE VAPOR (first 2000 chars) ---");
        println!("{}", &old_vapor[..old_vapor.len().min(2000)]);

        println!("\n--- NEW PIPELINE VAPOR v1 (first 2000 chars) ---");
        println!("{}", &vapor1[..vapor1.len().min(2000)]);

        println!("\n--- NEW PIPELINE VAPOR v2 (first 2000 chars) ---");
        println!("{}", &vapor2[..vapor2.len().min(2000)]);

        // Structural checks
        println!("\n--- STRUCTURAL COMPARISON ---");

        let checks = [
            ("_template(", "template declaration"),
            ("function render(", "render function"),
            ("return ", "return statement"),
            ("_renderEffect", "render effect"),
            ("_setText", "setText calls"),
            ("_toDisplayString", "toDisplayString"),
            ("_setClass", "setClass"),
            ("_setProp", "setProp"),
            ("_delegateEvents", "delegateEvents"),
            ("_createInvoker", "createInvoker"),
            ("_on(", "on() calls"),
            ("_child(", "child navigation"),
            ("_next(", "next navigation"),
            ("_txt(", "text node creation"),
        ];

        println!(
            "{:<25} {:>12} {:>12} {:>12}",
            "Feature", "Old Vapor", "Vapor v1", "Vapor v2"
        );
        println!("{:-<65}", "");

        for (pattern, label) in &checks {
            let old_count = old_vapor.matches(pattern).count();
            let v1_count = vapor1.matches(pattern).count();
            let v2_count = vapor2.matches(pattern).count();
            let marker = if v1_count != v2_count { " !!!" } else { "" };
            println!(
                "{:<25} {:>12} {:>12} {:>12}{}",
                label, old_count, v1_count, v2_count, marker
            );
        }

        println!();
    }
}
