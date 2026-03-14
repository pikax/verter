//! Matrix Comparator for Verter vs Vue output.
//!
//! Reads a capture_manifest.json, pairs Vue vs Verter modules by (mode, module_key),
//! compares via OXC parse + AST checks, and writes diffs.jsonl + summary.json.
//!
//! Usage:
//!   cargo run -p verter_bench --example check_matrix -- --manifest <path/to/capture_manifest.json>

use oxc_parser::Parser;
use oxc_span::SourceType;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;

// ─── Data Contracts ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CaptureManifest {
    #[allow(dead_code)]
    schema: String,
    run_id: String,
    project_root: String,
    entries: Vec<ManifestEntry>,
    #[serde(default)]
    errors: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    compiler: String,
    mode: String,
    module_key: String,
    source_vue_path: String,
    block_kind: String,
    captured_file: String,
}

#[derive(Debug, Serialize)]
struct DiffRow {
    id: String,
    mode: String,
    category: String,
    severity: String,
    module_key: String,
    source_vue_path: String,
    vue_file: Option<String>,
    verter_file: Option<String>,
    reason: String,
    recommended_test: String,
    suspected_files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SummaryJson {
    run_id: String,
    total_pairs: usize,
    matched: usize,
    category_a: usize,
    category_b: usize,
    category_c: usize,
    category_d: usize,
    category_e: usize,
    by_mode: HashMap<String, ModeSummary>,
}

#[derive(Debug, Default, Serialize)]
struct ModeSummary {
    total: usize,
    matched: usize,
    category_a: usize,
    category_b: usize,
    category_c: usize,
    category_d: usize,
    category_e: usize,
}

// ─── Known Limitation Patterns ───────────────────────────────────────────────

fn is_known_limitation(diffs: &[String]) -> bool {
    for diff in diffs {
        let d = diff.to_lowercase();
        // Hoist identifier name-only differences
        if d.contains("hoisted constant") && d.contains("count differs") {
            continue;
        }
        // Asset/filepath resolution gaps
        if d.contains("asset") || d.contains("filepath") || d.contains("path resolution") {
            continue;
        }
        // Cross-file type inference
        if d.contains("cross-file") || d.contains("type inference") {
            continue;
        }
        return false;
    }
    true
}

// ─── JS Comparison ───────────────────────────────────────────────────────────

enum CompareResult {
    Match,
    Mismatch {
        diffs: Vec<String>,
        category: String,
    },
    ParseError {
        side: String,
        error: String,
    },
}

fn compare_js(vue_code: &str, verter_code: &str) -> CompareResult {
    let alloc_v = oxc_allocator::Allocator::default();
    let alloc_o = oxc_allocator::Allocator::default();
    let source_type = SourceType::mjs();

    let vue_parsed = Parser::new(&alloc_v, vue_code, source_type).parse();
    let verter_parsed = Parser::new(&alloc_o, verter_code, source_type).parse();

    if !vue_parsed.errors.is_empty() {
        return CompareResult::ParseError {
            side: "vue".into(),
            error: vue_parsed
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        };
    }
    if !verter_parsed.errors.is_empty() {
        return CompareResult::ParseError {
            side: "verter".into(),
            error: verter_parsed
                .errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        };
    }

    let mut diffs = Vec::new();

    // Compare import sources
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

    // Compare import specifiers per source
    for source_mod in vue_imports.intersection(&verter_imports) {
        let extract_specs = |parsed: &oxc_ast::ast::Program, source: &str| -> BTreeSet<String> {
            parsed
                .body
                .iter()
                .filter_map(|s| {
                    if let oxc_ast::ast::Statement::ImportDeclaration(decl) = s {
                        if decl.source.value.as_str() == source {
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
                .collect()
        };

        let vue_specs = extract_specs(&vue_parsed.program, source_mod);
        let verter_specs = extract_specs(&verter_parsed.program, source_mod);

        if vue_specs != verter_specs {
            diffs.push(format!(
                "specifiers for '{}' differ: vue={:?}, verter={:?}",
                source_mod, vue_specs, verter_specs
            ));
        }
    }

    // Compare non-import statement count
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

    // Compare hoisted constant count
    fn count_hoisted(code: &str) -> usize {
        code.lines()
            .filter(|l| l.trim().starts_with("const _hoisted_"))
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
        // Classify
        let category = if is_known_limitation(&diffs) {
            "E".to_string()
        } else if diffs.iter().any(|d| d.contains("import sources differ")) {
            "D".to_string()
        } else if diffs.iter().any(|d| d.contains("statement count differs")) {
            "C".to_string()
        } else {
            "D".to_string()
        };
        CompareResult::Mismatch { diffs, category }
    }
}

// ─── CSS Comparison ──────────────────────────────────────────────────────────

fn compare_css(vue_code: &str, verter_code: &str) -> CompareResult {
    let normalize = |s: &str| -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") };
    if normalize(vue_code) == normalize(verter_code) {
        CompareResult::Match
    } else {
        CompareResult::Mismatch {
            diffs: vec!["style content differs after normalization".into()],
            category: "D".into(),
        }
    }
}

// ─── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let manifest_path = args
        .iter()
        .position(|a| a == "--manifest")
        .and_then(|i| args.get(i + 1))
        .expect("Usage: check_matrix --manifest <path>");

    let manifest_path = PathBuf::from(manifest_path);
    let run_dir = manifest_path
        .parent()
        .expect("manifest must be in a directory");

    println!("Reading manifest: {}", manifest_path.display());

    let manifest_str = fs::read_to_string(&manifest_path).expect("Failed to read manifest");
    let manifest: CaptureManifest =
        serde_json::from_str(&manifest_str).expect("Failed to parse manifest");

    println!(
        "  Entries: {}, Errors: {}",
        manifest.entries.len(),
        manifest.errors.len()
    );

    // Group by (mode, module_key) → { vue: entry, verter: entry }
    let mut pairs: HashMap<String, (Option<&ManifestEntry>, Option<&ManifestEntry>)> =
        HashMap::new();

    for entry in &manifest.entries {
        let key = format!("{}:{}", entry.mode, entry.module_key);
        let pair = pairs.entry(key).or_insert((None, None));
        match entry.compiler.as_str() {
            "vue" => pair.0 = Some(entry),
            "verter" => pair.1 = Some(entry),
            _ => {}
        }
    }

    let mut all_diffs: Vec<DiffRow> = Vec::new();
    let mut total_pairs = 0;
    let mut matched = 0;
    let mut by_mode: HashMap<String, ModeSummary> = HashMap::new();

    for (key, (vue_entry, verter_entry)) in &pairs {
        total_pairs += 1;

        let mode = key.split(':').next().unwrap_or("unknown");
        let mode_stats = by_mode.entry(mode.to_string()).or_default();
        mode_stats.total += 1;

        match (vue_entry, verter_entry) {
            (None, Some(ve)) => {
                // Missing Vue output (unusual)
                let diff = DiffRow {
                    id: key.clone(),
                    mode: ve.mode.clone(),
                    category: "B".into(),
                    severity: "P1".into(),
                    module_key: ve.module_key.clone(),
                    source_vue_path: ve.source_vue_path.clone(),
                    vue_file: None,
                    verter_file: Some(ve.captured_file.clone()),
                    reason: "Missing Vue output".into(),
                    recommended_test: format!("Add e2e parity test for {}", ve.source_vue_path),
                    suspected_files: vec![],
                };
                mode_stats.category_b += 1;
                all_diffs.push(diff);
            }
            (Some(ve), None) => {
                // Missing Verter output
                let diff = DiffRow {
                    id: key.clone(),
                    mode: ve.mode.clone(),
                    category: "B".into(),
                    severity: "P1".into(),
                    module_key: ve.module_key.clone(),
                    source_vue_path: ve.source_vue_path.clone(),
                    vue_file: Some(ve.captured_file.clone()),
                    verter_file: None,
                    reason: "Missing Verter output".into(),
                    recommended_test: format!("Add e2e parity test for {}", ve.source_vue_path),
                    suspected_files: vec![],
                };
                mode_stats.category_b += 1;
                all_diffs.push(diff);
            }
            (Some(vue_e), Some(verter_e)) => {
                let vue_path = run_dir.join(&vue_e.captured_file);
                let verter_path = run_dir.join(&verter_e.captured_file);

                let vue_code = fs::read_to_string(&vue_path).unwrap_or_default();
                let verter_code = fs::read_to_string(&verter_path).unwrap_or_default();

                let is_style = vue_e.block_kind.starts_with("style");
                let result = if is_style {
                    compare_css(&vue_code, &verter_code)
                } else {
                    compare_js(&vue_code, &verter_code)
                };

                match result {
                    CompareResult::Match => {
                        matched += 1;
                        mode_stats.matched += 1;
                    }
                    CompareResult::Mismatch { diffs, category } => {
                        let severity = match category.as_str() {
                            "A" => "P0",
                            "B" => "P1",
                            "C" | "D" => "P2",
                            _ => "TRACKED",
                        };

                        match category.as_str() {
                            "A" => mode_stats.category_a += 1,
                            "B" => mode_stats.category_b += 1,
                            "C" => mode_stats.category_c += 1,
                            "D" => mode_stats.category_d += 1,
                            _ => mode_stats.category_e += 1,
                        }

                        let ssr_flag = match vue_e.mode.as_str() {
                            "ssr" | "prod_ssr" => true,
                            _ => false,
                        };
                        let prod_flag = match vue_e.mode.as_str() {
                            "prod" | "prod_ssr" => true,
                            _ => false,
                        };

                        all_diffs.push(DiffRow {
                            id: key.clone(),
                            mode: vue_e.mode.clone(),
                            category: category.clone(),
                            severity: severity.into(),
                            module_key: vue_e.module_key.clone(),
                            source_vue_path: vue_e.source_vue_path.clone(),
                            vue_file: Some(vue_e.captured_file.clone()),
                            verter_file: Some(verter_e.captured_file.clone()),
                            reason: diffs.join("; "),
                            recommended_test: format!(
                                "Add e2e parity test in codegen.rs with mode flags ssr={},is_production={}",
                                ssr_flag, prod_flag
                            ),
                            suspected_files: vec![
                                "crates/verter_core/src/codegen/vue/template/element.rs"
                                    .into(),
                            ],
                        });
                    }
                    CompareResult::ParseError { side, error } => {
                        let (category, severity) = if side == "verter" {
                            ("A", "P0")
                        } else {
                            ("E", "TRACKED")
                        };

                        match category {
                            "A" => mode_stats.category_a += 1,
                            _ => mode_stats.category_e += 1,
                        }

                        all_diffs.push(DiffRow {
                            id: key.clone(),
                            mode: vue_e.mode.clone(),
                            category: category.into(),
                            severity: severity.into(),
                            module_key: vue_e.module_key.clone(),
                            source_vue_path: vue_e.source_vue_path.clone(),
                            vue_file: Some(vue_e.captured_file.clone()),
                            verter_file: Some(verter_e.captured_file.clone()),
                            reason: format!("{} parse failure: {}", side, error),
                            recommended_test: format!(
                                "Add e2e parity test for {} ({} parse error)",
                                vue_e.source_vue_path, side
                            ),
                            suspected_files: if side == "verter" {
                                vec![
                                    "crates/verter_core/src/codegen/vue/template/element.rs"
                                        .into(),
                                ]
                            } else {
                                vec![]
                            },
                        });
                    }
                }
            }
            (None, None) => {} // Should not happen
        }
    }

    // Sort diffs: A first, then B, C, D, E
    all_diffs.sort_by(|a, b| a.category.cmp(&b.category).then(a.mode.cmp(&b.mode)));

    // Write diffs.jsonl
    let diffs_path = run_dir.join("diffs.jsonl");
    let diffs_content: String = all_diffs
        .iter()
        .map(|d| serde_json::to_string(d).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&diffs_path, &diffs_content).expect("Failed to write diffs.jsonl");

    // Write summary.json
    let summary = SummaryJson {
        run_id: manifest.run_id.clone(),
        total_pairs,
        matched,
        category_a: all_diffs.iter().filter(|d| d.category == "A").count(),
        category_b: all_diffs.iter().filter(|d| d.category == "B").count(),
        category_c: all_diffs.iter().filter(|d| d.category == "C").count(),
        category_d: all_diffs.iter().filter(|d| d.category == "D").count(),
        category_e: all_diffs.iter().filter(|d| d.category == "E").count(),
        by_mode,
    };

    let summary_path = run_dir.join("summary.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .expect("Failed to write summary.json");

    // Print summary
    println!("\nComparison Summary:");
    println!("  Total pairs:  {}", total_pairs);
    println!("  Matched:      {}", matched);
    println!("  Category A:   {} (Invalid JS - P0)", summary.category_a);
    println!(
        "  Category B:   {} (Missing Module - P1)",
        summary.category_b
    );
    println!(
        "  Category C:   {} (AST Structure - P2)",
        summary.category_c
    );
    println!("  Category D:   {} (Wrong Values - P2)", summary.category_d);
    println!(
        "  Category E:   {} (Cosmetic/Known - TRACKED)",
        summary.category_e
    );
    println!("\n  Output:");
    println!("    {}", diffs_path.display());
    println!("    {}", summary_path.display());
}
