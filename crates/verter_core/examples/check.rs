use oxc_parser::Parser;
use oxc_span::SourceType;
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use verter_core::builder::codegen::{
    generate, generate_for_vite, get_hash, CodegenOptions, ViteCodegenOptions,
};
use walkdir::WalkDir;

fn find_vue_files(dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .into_iter()
        // IMPORTANT: prune before par_bridge so we don't descend into node_modules/dist/.* dirs
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != "node_modules" && name != "dist" && !name.starts_with('.')
        })
        .par_bridge()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension() == Some("vue".as_ref()))
        .map(|e| e.into_path())
        .collect()
}

fn main() {
    let example_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("check");
    let source_dir = example_dir.join("source");
    let generated_dir = example_dir.join("generated");

    // Ensure directories exist
    fs::create_dir_all(&source_dir).expect("Failed to create source directory");
    fs::create_dir_all(&generated_dir).expect("Failed to create generated directory");

    // Get all .vue files from source directory
    let vue_files: Vec<PathBuf> = match fs::read_dir(&source_dir) {
        Ok(entries) => {
            let allentries: Vec<_> = entries.collect();

            if allentries.is_empty() {
                resolve_sample_files(&source_dir);
            }

            println!("source directory found.");
            allentries
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .map(|ext| ext == "vue")
                        .unwrap_or(false)
                })
                .map(|entry| entry.path())
                .collect()
        }
        Err(_) => {
            println!("No source directory found. Creating sample files...");
            resolve_sample_files(&source_dir);
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
                .map(|entry| entry.path())
                .collect()
        }
    };

    if vue_files.is_empty() {
        println!("No .vue files found in source/ directory.");
        return;
    }

    println!("Found {} .vue file(s) to process\n", vue_files.len());

    // Clean up existing .verter.js files
    if let Ok(entries) = fs::read_dir(&generated_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map(|e| e == "js").unwrap_or(false)
                && path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.ends_with(".verter"))
                    .unwrap_or(false)
            {
                let _ = fs::remove_file(&path);
            }
        }
    }

    // Track statistics using atomics for thread-safe updates
    let total_size_bytes = Arc::new(AtomicU64::new(0));
    let errored_files = Arc::new(AtomicUsize::new(0));
    let total_codegen_ms = Arc::new(AtomicU64::new(0));

    let start_time = Instant::now();

    // Process files in parallel
    vue_files.par_iter().for_each(|file_path| {
        let file_name = file_path.file_name().unwrap().to_string_lossy();
        let base_name = file_path.file_stem().unwrap().to_string_lossy();
        let output_path = generated_dir.join(format!("{}.verter.js", base_name));

        match fs::read_to_string(file_path) {
            Ok(source) => {
                let allocator = oxc_allocator::Allocator::new();
                let options = CodegenOptions::new()
                    .with_filename(file_name.to_string())
                    .include_source_content(true);

                let codegen_start = Instant::now();
                let result = generate(&source, &options, &allocator);
                total_codegen_ms.fetch_add(
                    codegen_start.elapsed().as_millis() as u64,
                    Ordering::Relaxed,
                );

                if let Err(e) = fs::write(&output_path, &result.code) {
                    eprintln!("  Error writing {}: {}", output_path.display(), e);
                    errored_files.fetch_add(1, Ordering::Relaxed);
                } else {
                    total_size_bytes.fetch_add(result.code.len() as u64, Ordering::Relaxed);
                }
            }
            Err(err) => {
                eprintln!("  Error reading file {}: {}", file_path.display(), err);
                errored_files.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // ========================================================================
    // Per-block generation (Vite-style: dev / prod / ssr) - parallel
    // ========================================================================
    let modes: &[(&str, bool, bool)] = &[
        ("dev", false, false), // (label, is_production, ssr)
        ("prod", true, false),
        ("ssr", false, true),
    ];

    vue_files.par_iter().for_each(|file_path| {
        let file_name = file_path.file_name().unwrap().to_string_lossy();
        let base_name = file_path.file_stem().unwrap().to_string_lossy();
        // Use the source-dir-relative filepath to match check.js's filePath
        let filepath_str = file_path.to_string_lossy().replace('\\', "/");

        let source = match fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => return,
        };

        for &(mode, is_production, ssr) in modes {
            let component_id = if is_production {
                get_hash(&filepath_str)
            } else {
                get_hash(&format!("{}{}", filepath_str, source))
            };

            let allocator = oxc_allocator::Allocator::new();
            let options = ViteCodegenOptions {
                filename: Some(file_name.to_string()),
                is_production,
                ssr,
                component_id: Some(component_id),
                sourcemap: false,
            };

            let codegen_start = Instant::now();
            let result = generate_for_vite(&source, &options, &allocator);
            total_codegen_ms.fetch_add(
                codegen_start.elapsed().as_millis() as u64,
                Ordering::Relaxed,
            );

            // script block
            if let Some(ref block) = result.script {
                let out = generated_dir.join(format!("{}.script.{}.verter.js", base_name, mode));
                let _ = fs::write(&out, &block.code);
                total_size_bytes.fetch_add(block.code.len() as u64, Ordering::Relaxed);
            }

            // template / render block
            if let Some(ref block) = result.template {
                let out = generated_dir.join(format!("{}.render.{}.verter.js", base_name, mode));
                let _ = fs::write(&out, &block.code);
                total_size_bytes.fetch_add(block.code.len() as u64, Ordering::Relaxed);
            }

            // style blocks
            for (i, style) in result.styles.iter().enumerate() {
                let out =
                    generated_dir.join(format!("{}.style{}.{}.verter.js", base_name, i, mode));
                let _ = fs::write(&out, &style.code);
                total_size_bytes.fetch_add(style.code.len() as u64, Ordering::Relaxed);
            }

            // custom blocks
            for custom in &result.custom {
                let out =
                    generated_dir.join(format!("{}.{}.{}.verter.js", base_name, custom.tag, mode));
                let _ = fs::write(&out, &custom.content);
                total_size_bytes.fetch_add(custom.content.len() as u64, Ordering::Relaxed);
            }
        }
    });

    // ========================================================================
    // AST Comparison Phase
    // ========================================================================
    println!("\nComparing Vue vs Verter output...");

    // Check if .vue.js files exist
    let vue_js_files: Vec<_> = fs::read_dir(&generated_dir)
        .unwrap_or_else(|_| std::fs::read_dir(&generated_dir).unwrap())
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .to_str()
                .map(|s| s.ends_with(".vue.js"))
                .unwrap_or(false)
        })
        .collect();

    if vue_js_files.is_empty() {
        println!("  ⚠️  No .vue.js files found in generated directory.");
        println!("  To enable AST comparison, generate Vue compiler output first:");
        println!("  1. Build Vue components with @vitejs/plugin-vue");
        println!(
            "  2. Place .vue.js output files in: {}",
            generated_dir.display()
        );
        println!("  3. Re-run this example");
    }

    let comparison = compare_all_blocks(&generated_dir);

    println!("  Total pairs: {}", comparison.total_pairs);
    if comparison.total_pairs > 0 {
        println!("  Matched (auto): {}", comparison.matched);
        println!("  Mismatched: {}", comparison.mismatched);
        println!("  Previously matched: {}", comparison.previously_matched);
    }
    if comparison.verter_missing > 0 {
        println!("  Verter missing: {}", comparison.verter_missing);
    }
    if comparison.vue_parse_error > 0 {
        println!("  Vue parse errors: {}", comparison.vue_parse_error);
    }
    if comparison.verter_parse_error > 0 {
        println!("  Verter parse errors: {}", comparison.verter_parse_error);
    }
    println!("\n  Tier breakdown:");
    println!(
        "    render.dev:  {}/{} matched",
        comparison.render_dev.matched, comparison.render_dev.total
    );
    println!(
        "    script.dev:  {}/{} matched",
        comparison.script_dev.matched, comparison.script_dev.total
    );
    println!(
        "    render.ssr:  {}/{} matched",
        comparison.render_ssr.matched, comparison.render_ssr.total
    );
    println!(
        "    script.prod: {}/{} matched",
        comparison.script_prod.matched, comparison.script_prod.total
    );
    println!(
        "    script.ssr:  {}/{} matched",
        comparison.script_ssr.matched, comparison.script_ssr.total
    );
    println!(
        "    styles:      {}/{} matched",
        comparison.styles.matched, comparison.styles.total
    );
    println!(
        "    custom:      {}/{} matched",
        comparison.custom.matched, comparison.custom.total
    );

    let elapsed_ms = start_time.elapsed().as_millis() as u64;
    let codegen_ms = total_codegen_ms.load(Ordering::SeqCst);
    let total_size_bytes_val = total_size_bytes.load(Ordering::SeqCst);
    let errored_files_val = errored_files.load(Ordering::SeqCst);

    // Write summary
    let summary = Summary {
        count: vue_files.len(),
        ms: elapsed_ms,
        codegen_ms,
        size_bytes: total_size_bytes_val,
        errored_files: errored_files_val,
        comparison,
    };

    let summary_path = example_dir.join("summary.verter.json");
    let json = serde_json::to_string_pretty(&summary).expect("Failed to serialize summary");
    fs::write(&summary_path, json).expect("Failed to write summary.verter.json");

    println!("\nDone!");
    println!("  Files processed: {} (multi-threaded)", summary.count);
    println!("  Total time: {} ms", summary.ms);
    println!("  Codegen time: {} ms", summary.codegen_ms);
    println!("  Total output size: {} bytes", summary.size_bytes);
    println!("  Errored files: {}", summary.errored_files);
    println!("  Summary written to: {}", summary_path.display());
}

fn resolve_sample_files(source_dir: &Path) {
    // let home = std::env::var("HOME").unwrap_or_else(|_| String::from("~"));
    // let folder = Path::new(&home).join("Documents/dev");
    let folder = Path::new(r"D:\dev\");
    let vue_files = find_vue_files(&folder);
    if vue_files.is_empty() {
        println!("No .vue files found in source/ directory. Please add some .vue files to check.");
    } else {
        let mut files = Vec::new();

        println!("Found {} .vue files to check:", vue_files.len());
        let mut i = 0;
        for file in &vue_files {
            i += 1;

            let content = fs::read_to_string(file);
            if let Err(e) = content {
                println!("Failed to read file {}: {}", file.display(), e);
                continue;
            }

            let content = content.unwrap();
            let filename = file.file_name().unwrap().to_string_lossy();
            let final_filename = format!("{}_{}", i, filename);
            files.push(FileStat {
                name: final_filename.clone(),
                path: file.to_string_lossy().to_string(),
                size: content.len() as u64,
            });

            fs::write(source_dir.join(final_filename), content)
                .expect(format!("Failed to write {}", filename).as_str());
        }

        // store files.json
        let json = serde_json::to_string_pretty(&files).expect("Failed to serialize file stats");
        fs::write(source_dir.join("files.json"), json).expect("Failed to write files.json");
    }
}

#[derive(Debug, serde::Serialize)]
pub struct FileStat {
    pub path: String,
    pub size: u64,
    pub name: String,
}

#[derive(Debug, serde::Serialize)]
pub struct Summary {
    pub count: usize,
    pub ms: u64,
    pub codegen_ms: u64,
    pub size_bytes: u64,
    pub errored_files: usize,
    pub comparison: ComparisonSummary,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ComparisonSummary {
    pub total_pairs: usize,
    pub matched: usize,
    pub mismatched: usize,
    pub verter_missing: usize,
    pub vue_parse_error: usize,
    pub verter_parse_error: usize,
    pub previously_matched: usize,
    pub render_dev: TierStats,
    pub script_dev: TierStats,
    pub render_ssr: TierStats,
    pub script_prod: TierStats,
    pub script_ssr: TierStats,
    pub styles: TierStats,
    pub custom: TierStats,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct TierStats {
    pub total: usize,
    pub matched: usize,
    pub mismatched: usize,
}

#[allow(dead_code)]
enum CompareResult {
    Match,
    Mismatch { diffs: Vec<String> },
    ParseError { side: &'static str },
}

/// Classify a `.vue.js` filename into a tier key: (block, mode)
/// e.g. "1000_Foo.render.dev.vue.js" → Some(("render", "dev"))
fn classify_block_file(filename: &str) -> Option<(&str, &str)> {
    // Strip .vue.js suffix
    let stem = filename.strip_suffix(".vue.js")?;
    // Find the mode (last dot-segment): dev | prod | ssr
    let dot_mode = stem.rfind('.')?;
    let mode = &stem[dot_mode + 1..];
    if mode != "dev" && mode != "prod" && mode != "ssr" {
        return None;
    }
    // Find the block (second-to-last dot-segment)
    let before_mode = &stem[..dot_mode];
    let dot_block = before_mode.rfind('.')?;
    let block = &before_mode[dot_block + 1..];
    Some((block, mode))
}

/// Compare two JS blocks by parsing with OXC and comparing AST structure.
fn compare_js_blocks(vue_code: &str, verter_code: &str) -> CompareResult {
    let alloc_v = oxc_allocator::Allocator::default();
    let alloc_o = oxc_allocator::Allocator::default();
    let source_type = SourceType::mjs();

    let vue_parsed = Parser::new(&alloc_v, vue_code, source_type).parse();
    let verter_parsed = Parser::new(&alloc_o, verter_code, source_type).parse();

    if !vue_parsed.errors.is_empty() {
        return CompareResult::ParseError { side: "vue" };
    }
    if !verter_parsed.errors.is_empty() {
        return CompareResult::ParseError { side: "verter" };
    }

    let mut diffs = Vec::new();

    // --- Compare import sources (sorted sets, order-independent) ---
    let vue_imports: BTreeSet<&str> = vue_parsed
        .program
        .body
        .iter()
        .filter_map(|s| {
            if let oxc_ast::ast::Statement::ImportDeclaration(decl) = s {
                Some(decl.source.value.as_str())
            } else {
                None
            }
        })
        .collect();

    let verter_imports: BTreeSet<&str> = verter_parsed
        .program
        .body
        .iter()
        .filter_map(|s| {
            if let oxc_ast::ast::Statement::ImportDeclaration(decl) = s {
                Some(decl.source.value.as_str())
            } else {
                None
            }
        })
        .collect();

    if vue_imports != verter_imports {
        diffs.push(format!(
            "import sources differ: vue={:?}, verter={:?}",
            vue_imports, verter_imports
        ));
    }

    // --- Compare import specifiers per source ---
    for source_mod in vue_imports.intersection(&verter_imports) {
        let vue_specs: BTreeSet<String> = vue_parsed
            .program
            .body
            .iter()
            .filter_map(|s| {
                if let oxc_ast::ast::Statement::ImportDeclaration(decl) = s {
                    if decl.source.value.as_str() == *source_mod {
                        decl.specifiers.as_ref().map(|specs| {
                            specs
                                .iter()
                                .map(|s| match s {
                                    oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(
                                        spec,
                                    ) => spec.local.name.to_string(),
                                    oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(spec) => {
                                        spec.local.name.to_string()
                                    }
                                    oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(spec) => {
                                        spec.local.name.to_string()
                                    }
                                })
                                .collect::<BTreeSet<_>>()
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        let verter_specs: BTreeSet<String> = verter_parsed
            .program
            .body
            .iter()
            .filter_map(|s| {
                if let oxc_ast::ast::Statement::ImportDeclaration(decl) = s {
                    if decl.source.value.as_str() == *source_mod {
                        decl.specifiers.as_ref().map(|specs| {
                            specs
                                .iter()
                                .map(|s| match s {
                                    oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(
                                        spec,
                                    ) => spec.local.name.to_string(),
                                    oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(spec) => {
                                        spec.local.name.to_string()
                                    }
                                    oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(spec) => {
                                        spec.local.name.to_string()
                                    }
                                })
                                .collect::<BTreeSet<_>>()
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        if vue_specs != verter_specs {
            diffs.push(format!(
                "specifiers for '{}' differ: vue={:?}, verter={:?}",
                source_mod, vue_specs, verter_specs
            ));
        }
    }

    // --- Compare non-import statement count ---
    let vue_non_import = vue_parsed
        .program
        .body
        .iter()
        .filter(|s| !matches!(s, oxc_ast::ast::Statement::ImportDeclaration(_)))
        .count();
    let verter_non_import = verter_parsed
        .program
        .body
        .iter()
        .filter(|s| !matches!(s, oxc_ast::ast::Statement::ImportDeclaration(_)))
        .count();

    if vue_non_import != verter_non_import {
        diffs.push(format!(
            "non-import statement count differs: vue={}, verter={}",
            vue_non_import, verter_non_import
        ));
    }

    // --- Compare hoisted constant count (const _hoisted_N) ---
    fn count_hoisted(code: &str) -> usize {
        code.lines()
            .filter(|l| {
                let t = l.trim();
                t.starts_with("const _hoisted_")
            })
            .count()
    }

    let vue_hoisted = count_hoisted(vue_code);
    let verter_hoisted = count_hoisted(verter_code);
    if vue_hoisted != verter_hoisted {
        diffs.push(format!(
            "hoisted constant count differs: vue={}, verter={}",
            vue_hoisted, verter_hoisted
        ));
    }

    if diffs.is_empty() {
        CompareResult::Match
    } else {
        CompareResult::Mismatch { diffs }
    }
}

/// Compare two style blocks by normalizing whitespace and comparing strings.
fn compare_style_blocks(vue_css: &str, verter_css: &str) -> CompareResult {
    let normalize = |s: &str| -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") };
    if normalize(vue_css) == normalize(verter_css) {
        CompareResult::Match
    } else {
        CompareResult::Mismatch {
            diffs: vec!["style content differs after normalization".to_string()],
        }
    }
}

/// Compare two custom blocks by exact string match (after trimming).
fn compare_custom_blocks(vue_content: &str, verter_content: &str) -> CompareResult {
    if vue_content.trim() == verter_content.trim() {
        CompareResult::Match
    } else {
        CompareResult::Mismatch {
            diffs: vec!["custom block content differs".to_string()],
        }
    }
}

/// Run AST comparison on all vue.js / verter.js pairs in the generated directory.
/// Creates .match files for matching pairs and returns comparison statistics.
fn compare_all_blocks(generated_dir: &Path) -> ComparisonSummary {
    let mut summary = ComparisonSummary::default();

    // Collect all .vue.js files
    let mut vue_files: Vec<PathBuf> = fs::read_dir(generated_dir)
        .expect("Failed to read generated directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_str().map(|s| s.ends_with(".vue.js")).unwrap_or(false))
        .collect();

    vue_files.sort();

    // Simple date without chrono dependency
    let today = {
        let duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let secs = duration.as_secs();
        // Approximate date: days since epoch
        let days = secs / 86400;
        // Simple year/month/day calculation (good enough for logging)
        let mut y = 1970;
        let mut remaining = days;
        loop {
            let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
                366
            } else {
                365
            };
            if remaining < days_in_year {
                break;
            }
            remaining -= days_in_year;
            y += 1;
        }
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let month_days = [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let mut m = 0;
        for &md in &month_days {
            if remaining < md {
                break;
            }
            remaining -= md;
            m += 1;
        }
        format!("{:04}-{:02}-{:02}", y, m + 1, remaining + 1)
    };

    for vue_path in &vue_files {
        let filename = vue_path.file_name().unwrap().to_str().unwrap();

        // Classify into tier
        let (block, mode) = match classify_block_file(filename) {
            Some(bm) => bm,
            None => continue,
        };

        summary.total_pairs += 1;

        // Determine tier stats reference
        let tier = match (block, mode) {
            ("render", "dev") => &mut summary.render_dev,
            ("script", "dev") => &mut summary.script_dev,
            ("render", "ssr") => &mut summary.render_ssr,
            ("script", "prod") => &mut summary.script_prod,
            ("script", "ssr") => &mut summary.script_ssr,
            (b, _) if b.starts_with("style") => &mut summary.styles,
            _ => &mut summary.custom,
        };
        tier.total += 1;

        // Check if .match already exists (resume support)
        let match_path = vue_path.with_extension("js.match");
        if match_path.exists() {
            summary.previously_matched += 1;
            tier.matched += 1;
            continue;
        }

        // Build verter path: replace .vue.js with .verter.js
        let verter_filename = filename.replace(".vue.js", ".verter.js");
        let verter_path = generated_dir.join(&verter_filename);

        if !verter_path.exists() {
            summary.verter_missing += 1;
            tier.mismatched += 1;
            continue;
        }

        // Read both files
        let vue_code = fs::read_to_string(vue_path).unwrap_or_default();
        let verter_code = fs::read_to_string(&verter_path).unwrap_or_default();

        // Compare based on block type
        let is_style = block.starts_with("style");
        let is_js = block == "script" || block == "render";
        let result = if is_js {
            compare_js_blocks(&vue_code, &verter_code)
        } else if is_style {
            compare_style_blocks(&vue_code, &verter_code)
        } else {
            compare_custom_blocks(&vue_code, &verter_code)
        };

        match result {
            CompareResult::Match => {
                summary.matched += 1;
                tier.matched += 1;
                // Create .match file
                let match_content = format!("{{\"status\":\"auto_match\",\"date\":\"{}\"}}", today);
                let _ = fs::write(&match_path, match_content);
            }
            CompareResult::Mismatch { .. } => {
                summary.mismatched += 1;
                tier.mismatched += 1;
            }
            CompareResult::ParseError { side } => {
                if side == "vue" {
                    summary.vue_parse_error += 1;
                } else {
                    summary.verter_parse_error += 1;
                }
                tier.mismatched += 1;
            }
        }
    }

    summary
}
