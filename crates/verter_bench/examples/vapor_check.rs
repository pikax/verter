use oxc_parser::Parser;
use oxc_span::SourceType;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;
use walkdir::WalkDir;

use verter_core::compile::{compile, CodegenOptions, VerterCompileOptions};

// ── File discovery ──────────────────────────────────────────────────

fn find_vue_files(dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != "node_modules" && name != "dist" && !name.starts_with('.')
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension() == Some("vue".as_ref()))
        .map(|e| e.into_path())
        .collect()
}

fn find_test_repos_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("VERTER_TEST_REPOS") {
        let p = PathBuf::from(path);
        if p.is_dir() {
            return Some(p);
        }
    }
    let workspace_repos = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".integration-tests")
        .join("repos");
    if workspace_repos.is_dir() {
        return Some(workspace_repos);
    }
    None
}

// ── Error types ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum ErrorKind {
    Panic(String),
    CompileError(String),
    ScriptParseError(String),
    TemplateParseError(String),
}

impl ErrorKind {
    fn category(&self) -> &'static str {
        match self {
            ErrorKind::Panic(_) => "PANIC",
            ErrorKind::CompileError(_) => "COMPILE_ERROR",
            ErrorKind::ScriptParseError(_) => "SCRIPT_PARSE_ERROR",
            ErrorKind::TemplateParseError(_) => "TEMPLATE_PARSE_ERROR",
        }
    }

    fn message(&self) -> &str {
        match self {
            ErrorKind::Panic(m)
            | ErrorKind::CompileError(m)
            | ErrorKind::ScriptParseError(m)
            | ErrorKind::TemplateParseError(m) => m,
        }
    }
}

#[derive(Debug)]
struct FileError {
    path: PathBuf,
    project: String,
    errors: Vec<ErrorKind>,
    /// Generated script output (saved for investigation).
    script_output: Option<String>,
    /// Generated template output (saved for investigation).
    template_output: Option<String>,
}

// ── Summary types ───────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct Summary {
    total_files: usize,
    pass_count: usize,
    fail_count: usize,
    panic_count: usize,
    duration_ms: u64,
    error_groups: Vec<ErrorGroup>,
    per_project: Vec<ProjectSummary>,
}

#[derive(Debug, serde::Serialize)]
struct ErrorGroup {
    category: String,
    message: String,
    count: usize,
    sample_files: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct ProjectSummary {
    name: String,
    total: usize,
    pass: usize,
    fail: usize,
}

// ── Normalize error messages for grouping ───────────────────────────

fn normalize_error(msg: &str) -> String {
    // Strip line/column numbers and spans to group similar errors
    let msg = msg.trim();
    // Truncate long messages for grouping
    if msg.len() > 120 {
        msg[..120].to_string()
    } else {
        msg.to_string()
    }
}

// ── Main ────────────────────────────────────────────────────────────

fn main() {
    let root = find_test_repos_root().expect(
        "Test repos not found. Set VERTER_TEST_REPOS env var to the path of verter-test-repos.",
    );

    println!("Test repos root: {}", root.display());
    println!("Mode: VAPOR (force_vapor: true)\n");

    // Discover projects
    let mut projects: Vec<(String, Vec<PathBuf>)> = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(&root)
        .expect("Failed to read test repos directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    // Projects to skip (unsupported architectures, e.g. Vue Class Components)
    let skip_projects: &[&str] = &["MQTTX"];

    for entry in entries {
        let project_name = entry.file_name().to_string_lossy().to_string();
        if skip_projects.iter().any(|s| *s == project_name) {
            println!("  {} — SKIPPED (unsupported)", project_name);
            continue;
        }
        let vue_files = find_vue_files(&entry.path());
        if !vue_files.is_empty() {
            println!("  {} — {} .vue files", project_name, vue_files.len());
            projects.push((project_name, vue_files));
        }
    }

    let total_files: usize = projects.iter().map(|(_, f)| f.len()).sum();
    println!(
        "\nTotal: {} .vue files across {} projects\n",
        total_files,
        projects.len()
    );

    let start = Instant::now();

    // Process all files in parallel, collecting errors
    let file_errors: Mutex<Vec<FileError>> = Mutex::new(Vec::new());
    let pass_count = std::sync::atomic::AtomicUsize::new(0);

    let all_files: Vec<(String, PathBuf)> = projects
        .iter()
        .flat_map(|(name, files)| files.iter().map(move |f| (name.clone(), f.clone())))
        .collect();

    all_files.par_iter().for_each(|(project, file_path)| {
        let content = match fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                file_errors.lock().unwrap().push(FileError {
                    path: file_path.clone(),
                    project: project.clone(),
                    errors: vec![ErrorKind::CompileError(format!("read error: {}", e))],
                    script_output: None,
                    template_output: None,
                });
                return;
            }
        };

        let filename = file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Catch panics
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let allocator = oxc_allocator::Allocator::new();
            let options = CodegenOptions {
                filename: Some(filename.clone()),
                ..Default::default()
            };
            let verter_opts = VerterCompileOptions {
                force_js: false,
                force_vapor: true,
                ..Default::default()
            };
            compile(&content, &options, &verter_opts, &allocator)
        }));

        match result {
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "unknown panic".to_string()
                };
                file_errors.lock().unwrap().push(FileError {
                    path: file_path.clone(),
                    project: project.clone(),
                    errors: vec![ErrorKind::Panic(msg)],
                    script_output: None,
                    template_output: None,
                });
            }
            Ok(compile_result) => {
                let mut errors = Vec::new();

                // Check compile errors
                for diag in &compile_result.errors {
                    errors.push(ErrorKind::CompileError(format!(
                        "{:?}: {}",
                        diag.severity, diag.message
                    )));
                }

                // Determine script language from attrs
                let script_lang = compile_result.script.as_ref().and_then(|s| {
                    s.attrs
                        .iter()
                        .find(|(k, _)| k == "lang")
                        .map(|(_, v)| v.as_str())
                });

                // Validate script output with OXC using the correct source type
                if let Some(ref script) = compile_result.script {
                    if !script.code.trim().is_empty() {
                        let alloc = oxc_allocator::Allocator::new();
                        let source_type = match script_lang {
                            Some("ts") => SourceType::ts().with_module(true),
                            Some("tsx") => SourceType::tsx(),
                            Some("jsx") => SourceType::jsx(),
                            _ => SourceType::mjs(),
                        };
                        let parsed = Parser::new(&alloc, &script.code, source_type).parse();
                        if !parsed.errors.is_empty() {
                            let first_err = parsed.errors[0].to_string();
                            errors.push(ErrorKind::ScriptParseError(first_err));
                        }
                    }
                }

                // Validate template output with OXC
                // Use TS source type if script is TS (template expressions may contain TS)
                if let Some(ref template) = compile_result.template {
                    if !template.code.trim().is_empty() {
                        let alloc = oxc_allocator::Allocator::new();
                        let source_type = match script_lang {
                            Some("ts") => SourceType::ts().with_module(true),
                            Some("tsx") => SourceType::tsx(),
                            Some("jsx") => SourceType::jsx(),
                            _ => SourceType::mjs(),
                        };
                        // Wrap template code to make it parseable as a module
                        let wrapped = format!("import {{ }} from \"vue\";\n{}", template.code);
                        let parsed = Parser::new(&alloc, &wrapped, source_type).parse();
                        if !parsed.errors.is_empty() {
                            let first_err = parsed.errors[0].to_string();
                            errors.push(ErrorKind::TemplateParseError(first_err));
                        }
                    }
                }

                if errors.is_empty() {
                    pass_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                } else {
                    file_errors.lock().unwrap().push(FileError {
                        path: file_path.clone(),
                        project: project.clone(),
                        errors,
                        script_output: compile_result.script.as_ref().map(|s| s.code.clone()),
                        template_output: compile_result.template.as_ref().map(|t| t.code.clone()),
                    });
                }
            }
        }
    });

    let duration_ms = start.elapsed().as_millis() as u64;
    let file_errors = file_errors.into_inner().unwrap();
    let pass = pass_count.load(std::sync::atomic::Ordering::SeqCst);
    let fail = file_errors.len();

    // ── Group errors ────────────────────────────────────────────────
    let mut group_map: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut panic_count = 0usize;

    for fe in &file_errors {
        for err in &fe.errors {
            if matches!(err, ErrorKind::Panic(_)) {
                panic_count += 1;
            }
            let key = (err.category().to_string(), normalize_error(err.message()));
            group_map
                .entry(key)
                .or_default()
                .push(fe.path.display().to_string());
        }
    }

    let mut error_groups: Vec<ErrorGroup> = group_map
        .into_iter()
        .map(|((category, message), files)| ErrorGroup {
            category,
            message,
            count: files.len(),
            sample_files: files.into_iter().take(3).collect(),
        })
        .collect();
    error_groups.sort_by(|a, b| b.count.cmp(&a.count));

    // ── Per-project breakdown ───────────────────────────────────────
    let mut per_project: Vec<ProjectSummary> = Vec::new();
    for (project_name, vue_files) in &projects {
        let project_total = vue_files.len();
        let project_fail = file_errors
            .iter()
            .filter(|fe| &fe.project == project_name)
            .count();
        per_project.push(ProjectSummary {
            name: project_name.clone(),
            total: project_total,
            pass: project_total - project_fail,
            fail: project_fail,
        });
    }

    // ── Print summary ───────────────────────────────────────────────
    println!("═══════════════════════════════════════════════════════════");
    println!("  Vapor batch validation results");
    println!("═══════════════════════════════════════════════════════════");
    println!(
        "  Total: {}  Pass: {}  Fail: {}  Panics: {}",
        total_files, pass, fail, panic_count
    );
    println!("  Duration: {} ms", duration_ms);
    println!();

    if !error_groups.is_empty() {
        println!("── Error groups (by frequency) ─────────────────────────");
        for (i, eg) in error_groups.iter().enumerate().take(30) {
            println!(
                "  {:2}. [{}] x{}: {}",
                i + 1,
                eg.category,
                eg.count,
                eg.message
            );
            for sample in &eg.sample_files {
                println!("      → {}", sample);
            }
        }
        println!();
    }

    println!("── Per-project breakdown ───────────────────────────────");
    for ps in &per_project {
        let pct = if ps.total > 0 {
            (ps.pass as f64 / ps.total as f64) * 100.0
        } else {
            100.0
        };
        println!(
            "  {:<45} {:>4}/{:>4} pass ({:.0}%)",
            ps.name, ps.pass, ps.total, pct
        );
    }

    // ── Write summary JSON ──────────────────────────────────────────
    let summary = Summary {
        total_files,
        pass_count: pass,
        fail_count: fail,
        panic_count,
        duration_ms,
        error_groups,
        per_project,
    };

    let example_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("vapor_check");
    fs::create_dir_all(&example_dir).ok();
    let summary_path = example_dir.join("summary.json");
    let json = serde_json::to_string_pretty(&summary).expect("Failed to serialize summary");
    fs::write(&summary_path, &json).expect("Failed to write summary.json");

    // ── Write failing outputs for investigation ──────────────────────
    let failed_dir = example_dir.join("failed_outputs");
    // Clean previous run
    if failed_dir.exists() {
        fs::remove_dir_all(&failed_dir).ok();
    }
    fs::create_dir_all(&failed_dir).ok();

    let mut saved = 0usize;
    for fe in &file_errors {
        // Skip files that only have compile errors (no generated output)
        let has_parse_error = fe.errors.iter().any(|e| {
            matches!(
                e,
                ErrorKind::ScriptParseError(_) | ErrorKind::TemplateParseError(_)
            )
        });
        // Also save panics (they're interesting for vapor)
        let has_panic = fe.errors.iter().any(|e| matches!(e, ErrorKind::Panic(_)));
        if !has_parse_error && !has_panic {
            continue;
        }

        // Build a safe filename: project__filename
        let stem = fe
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let base = format!("{}__{}", fe.project, stem);

        if has_panic {
            let out_path = failed_dir.join(format!("{}.panic.txt", base));
            let mut content = String::with_capacity(200);
            content.push_str("// SOURCE: ");
            content.push_str(&fe.path.display().to_string());
            content.push('\n');
            for err in &fe.errors {
                content.push_str("// ERROR [");
                content.push_str(err.category());
                content.push_str("]: ");
                content.push_str(err.message());
                content.push('\n');
            }
            fs::write(&out_path, &content).ok();
            saved += 1;
        }

        if let Some(ref tpl) = fe.template_output {
            if fe
                .errors
                .iter()
                .any(|e| matches!(e, ErrorKind::TemplateParseError(_)))
            {
                let out_path = failed_dir.join(format!("{}.template.js", base));
                // Prepend error info as a comment
                let mut content = String::with_capacity(tpl.len() + 200);
                content.push_str("// SOURCE: ");
                content.push_str(&fe.path.display().to_string());
                content.push('\n');
                for err in &fe.errors {
                    content.push_str("// ERROR [");
                    content.push_str(err.category());
                    content.push_str("]: ");
                    content.push_str(err.message());
                    content.push('\n');
                }
                content.push_str("//\n");
                content.push_str(tpl);
                fs::write(&out_path, &content).ok();
                saved += 1;
            }
        }

        if let Some(ref script) = fe.script_output {
            if fe
                .errors
                .iter()
                .any(|e| matches!(e, ErrorKind::ScriptParseError(_)))
            {
                let out_path = failed_dir.join(format!("{}.script.js", base));
                let mut content = String::with_capacity(script.len() + 200);
                content.push_str("// SOURCE: ");
                content.push_str(&fe.path.display().to_string());
                content.push('\n');
                for err in &fe.errors {
                    content.push_str("// ERROR [");
                    content.push_str(err.category());
                    content.push_str("]: ");
                    content.push_str(err.message());
                    content.push('\n');
                }
                content.push_str("//\n");
                content.push_str(script);
                fs::write(&out_path, &content).ok();
                saved += 1;
            }
        }
    }
    println!(
        "Saved {} failing outputs to: {}",
        saved,
        failed_dir.display()
    );
    println!("\nSummary written to: {}", summary_path.display());
}
