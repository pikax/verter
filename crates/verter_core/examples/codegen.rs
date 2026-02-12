//! Codegen Example
//!
//! This example reads .vue files from examples/codegen/source/ directory,
//! runs them through the codegen pipeline, and outputs the generated JavaScript
//! (with inline source maps) to examples/codegen/generated/{filename}.js
//!
//! Also compiles with vize_atelier_sfc for comparison, outputting to {filename}.vize.js
//!
//! Run with: cargo run --example codegen

use std::fs;
use std::path::Path;
use verter_core::builder::codegen::{compile, CodegenOptions};
use vize_atelier_sfc::{compile_sfc, parse_sfc, SfcCompileOptions, SfcParseOptions};

fn main() {
    let example_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("codegen");
    let source_dir = example_dir.join("source");
    let generated_dir = example_dir.join("generated");

    // Ensure directories exist
    fs::create_dir_all(&source_dir).expect("Failed to create source directory");
    fs::create_dir_all(&generated_dir).expect("Failed to create generated directory");

    // Get all .vue files from source directory
    let vue_files: Vec<_> = match fs::read_dir(&source_dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map(|ext| ext == "vue")
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => {
            println!("No source directory found. Creating sample files...");
            create_sample_files(&source_dir);
            fs::read_dir(&source_dir)
                .expect("Failed to read source directory after creation")
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .map(|ext| ext == "vue")
                        .unwrap_or(false)
                })
                .collect()
        }
    };

    if vue_files.is_empty() {
        println!("No .vue files found in source/ directory. Creating sample files...");
        create_sample_files(&source_dir);
        // Re-read after creating samples
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
            println!("Still no .vue files found. Exiting.");
            return;
        }

        process_files(&vue_files, &generated_dir);
    } else {
        process_files(&vue_files, &generated_dir);
    }
}

fn process_files(vue_files: &[std::fs::DirEntry], generated_dir: &Path) {
    println!("Found {} .vue file(s) to process\n", vue_files.len());

    for entry in vue_files {
        let file_path = entry.path();
        let file_name = file_path.file_name().unwrap().to_string_lossy();
        let base_name = file_path.file_stem().unwrap().to_string_lossy();
        let output_path = generated_dir.join(format!("{}.js", base_name));
        let vize_output_path = generated_dir.join(format!("{}.vize.js", base_name));

        println!("Processing: {}", file_name);

        match fs::read_to_string(&file_path) {
            Ok(source) => {
                // Verter codegen
                let allocator = oxc_allocator::Allocator::new();
                let options = CodegenOptions::new().with_filename(file_name.to_string());

                let result = compile(&source, &options, &allocator);

                // Write the output with inline source map
                fs::write(&output_path, &result.code_with_source_map)
                    .expect("Failed to write output file");

                println!("  -> {}", output_path.display());
                println!("     Code length: {} bytes", result.code.len());
                println!("     Source map length: {} bytes", result.source_map.len());

                // // Vize codegen for comparison
                // let parse_options = SfcParseOptions {
                //     filename: file_name.to_string(),
                //     source_map: false,
                //     ..Default::default()
                // };
                // match parse_sfc(&source, parse_options) {
                //     Ok(descriptor) => {
                //         let compile_options = SfcCompileOptions::default();

                //         match compile_sfc(&descriptor, compile_options) {
                //             Ok(vize_result) => {
                //                 fs::write(&vize_output_path, &vize_result.code)
                //                     .expect("Failed to write vize output file");
                //                 println!("  -> {}", vize_output_path.display());
                //                 println!("     Vize code length: {} bytes", vize_result.code.len());
                //             }
                //             Err(err) => {
                //                 eprintln!("  Vize compile error: {:?}", err);
                //             }
                //         }
                //     }
                //     Err(err) => {
                //         eprintln!("  Vize parse error: {:?}", err);
                //     }
                // }
            }
            Err(err) => {
                eprintln!("  Error reading file: {}", err);
            }
        }
    }

    println!("\nDone!");
    println!("\nTo compare with Vue's official compiler, run:");
    println!("  node examples/codegen.js");
}

fn create_sample_files(source_dir: &Path) {
    fs::create_dir_all(source_dir).expect("Failed to create source directory");

    // Simple component
    let simple = r#"<template>
  <div class="hello">
    <h1>{{ msg }}</h1>
  </div>
</template>

<script setup>
const msg = 'Hello World'
</script>
"#;
    fs::write(source_dir.join("simple.vue"), simple).expect("Failed to write simple.vue");

    // Component with v-if/v-else
    let conditional = r#"<template>
  <div>
    <span v-if="show">Visible</span>
    <span v-else>Hidden</span>
  </div>
</template>

<script setup>
import { ref } from 'vue'
const show = ref(true)
</script>
"#;
    fs::write(source_dir.join("conditional.vue"), conditional)
        .expect("Failed to write conditional.vue");

    // Component with v-for
    let list = r#"<template>
  <ul>
    <li v-for="item in items" :key="item.id">
      {{ item.name }}
    </li>
  </ul>
</template>

<script setup>
import { ref } from 'vue'
const items = ref([
  { id: 1, name: 'Apple' },
  { id: 2, name: 'Banana' },
  { id: 3, name: 'Cherry' }
])
</script>
"#;
    fs::write(source_dir.join("list.vue"), list).expect("Failed to write list.vue");

    println!("Created sample .vue files in {:?}", source_dir);
}
