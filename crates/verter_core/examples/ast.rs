//! AST Example
//!
//! This example reads .vue files from examples/ast/source/ directory,
//! parses them using verter's parser, and outputs the AST to
//! examples/ast/generated/{filename}.verter.json
//!
//! Run with: cargo run --example ast

use bumpalo::Bump;
use std::fs;
use std::path::Path;
use verter_core::syntax::plugin::SyntaxPluginOptions;
use vize_armature::parser as vize_parser;

fn main() {
    let example_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("ast");
    let source_dir = example_dir.join("source");
    let generated_dir = example_dir.join("generated");

    // Ensure generated directory exists
    fs::create_dir_all(&generated_dir).expect("Failed to create generated directory");

    // Get all .vue files from source directory
    let vue_files: Vec<_> = fs::read_dir(&source_dir)
        .expect("Failed to read source directory")
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "vue")
                .unwrap_or(false)
        })
        .collect();

    if vue_files.is_empty() {
        println!("No .vue files found in source/ directory");
        return;
    }

    println!("Found {} .vue file(s) to process\n", vue_files.len());

    let options = SyntaxPluginOptions::default();

    for entry in vue_files {
        let file_path = entry.path();
        let file_name = file_path.file_name().unwrap().to_string_lossy();
        let base_name = file_path.file_stem().unwrap().to_string_lossy();
        let output_path = generated_dir.join(format!("{}.verter.json", base_name));

        println!("Processing: {}", file_name);

        match fs::read_to_string(&file_path) {
            Ok(source) => {
                let allocator = oxc_allocator::Allocator::new();
                let root =
                    verter_core::builder::parser_syntax::parse(&source, &options, &allocator);

                // Serialize directly to pretty JSON
                let json_output =
                    serde_json::to_string_pretty(&root).expect("Failed to serialize JSON");
                fs::write(&output_path, &json_output).expect("Failed to write output file");
                println!("  -> {}", output_path.display());
            }
            Err(err) => {
                eprintln!("  Error reading file: {}", err);
            }
        }
    }

    println!("\nDone!");
    println!("\nTo compare with Vue's official parser, run:");
    println!("  cd examples/ast && npm install @vue/compiler-sfc && node script.js");
}
