//! Batch Vue SFC codegen and type checking.
//!
//! Two-phase pipeline:
//!
//! **Phase A — Validation (TSX):**
//!   For each .vue file → `compile()` with `CompileTarget::IDE` → full TSX with source map.
//!   Type-checks script body + template. Reports ALL type errors.
//!
//! **Phase B — Declaration Generation (TSC):**
//!   For each .vue file → `generate_tsc_output()` → write `.tsc.tsx` to tempdir.
//!   Only when `--declaration` is requested.
//!
//! Both phases invoke `tsgo` (or `tsc`) as a subprocess and remap diagnostics
//! via source maps back to `.vue` positions.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use oxc_allocator::Allocator;
use rayon::prelude::*;
use tempfile::TempDir;
use verter_core::compile::{CodegenOptions, CompileTarget, VerterCompileOptions};
use verter_core::tsc::generate_tsc_output;

use crate::error_map::map_tsc_position;
use crate::reporter::{self, Diagnostic, TscDiagnostic};
use crate::tsconfig::{strip_unc_prefix, TsConfig};

/// Which type-checker binary to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeCheckerBinary {
    /// Try tsgo first, fall back to tsc if not found (default).
    TsgoWithFallback,
    /// Force tsc (skip tsgo discovery).
    ForceTsc,
}

/// Options controlling what the checker emits.
pub struct EmitOptions {
    /// Type-check only (don't write .d.ts files).
    pub no_emit: bool,
    /// Emit .d.ts declaration files.
    pub declaration: bool,
    /// Directory to write declarations into.
    pub declaration_dir: Option<PathBuf>,
}

/// Result of a type-checking run.
pub struct CheckResult {
    pub diagnostics: Vec<Diagnostic>,
    pub emitted_files: Vec<PathBuf>,
}

/// Phase A: Generate full TSX (script body + template) for every `.vue` file in parallel.
///
/// Uses `compile()` with `CompileTarget::IDE` for full type checking.
/// Returns `(vue_path, tsx_code, tsx_path)` tuples written to `temp_dir`.
fn generate_all_tsx(vue_files: &[PathBuf], temp_dir: &Path) -> Vec<(PathBuf, String, PathBuf)> {
    vue_files
        .par_iter()
        .map(|vue_path| {
            let source = match fs::read_to_string(vue_path) {
                Ok(s) => s,
                Err(_) => return None,
            };

            let raw_name = vue_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Component");
            let component_name = sanitize_component_name(raw_name);

            let filename = vue_path.to_string_lossy().replace('\\', "/");
            let alloc = Allocator::default();
            let options = CodegenOptions {
                filename: Some(filename),
                target: CompileTarget::TSX,
                skip_source_map: false,
                embed_ambient_types: false,
                ..Default::default()
            };
            let verter_options = VerterCompileOptions {
                source_map: true,
                ..Default::default()
            };
            let result = verter_core::compile::compile(&source, &options, &verter_options, &alloc);

            let tsx_block = result.tsx?;

            // Rewrite relative imports (both `import('...')` and `from '...'` patterns)
            let vue_dir = vue_path.parent().unwrap_or(Path::new("."));
            let mut code = rewrite_relative_imports(&tsx_block.code, vue_dir);

            // Append inline source map so `map_tsc_position()` can remap errors.
            if !tsx_block.source_map.is_empty() {
                let encoded =
                    base64::prelude::BASE64_STANDARD.encode(tsx_block.source_map.as_bytes());
                code.push_str(&format!(
                    "\n//# sourceMappingURL=data:application/json;base64,{encoded}\n"
                ));
            }

            let hash = simple_hash(vue_path.to_string_lossy().as_bytes());
            let tsx_name = format!("{component_name}_{hash:016x}.tsx");
            let tsx_path = temp_dir.join(&tsx_name);

            if fs::write(&tsx_path, &code).is_err() {
                return None;
            }

            Some((vue_path.clone(), code, tsx_path))
        })
        .flatten()
        .collect()
}

/// Phase B: Generate minimal TSC declaration output for every `.vue` file in parallel.
///
/// Uses `generate_tsc_output()` (macro extraction only) for declaration generation.
/// Returns `(vue_path, tsc_code, tsc_tsx_path)` tuples written to `temp_dir`.
fn generate_all_tsc(vue_files: &[PathBuf], temp_dir: &Path) -> Vec<(PathBuf, String, PathBuf)> {
    vue_files
        .par_iter()
        .map(|vue_path| {
            let source = match fs::read_to_string(vue_path) {
                Ok(s) => s,
                Err(_) => return None,
            };

            let raw_name = vue_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Component");
            let component_name = sanitize_component_name(raw_name);

            let tsc_out = generate_tsc_output(&source, &component_name);

            // Rewrite relative import() paths in the generated code to absolute paths.
            // The .tsc.tsx files live in a temp dir, so relative imports like
            // `import('./types')` need to resolve from the .vue file's directory.
            let vue_dir = vue_path.parent().unwrap_or(Path::new("."));
            let code = rewrite_relative_imports(&tsc_out.code, vue_dir);

            // Write as <stem>.tsc.tsx in temp_dir.
            // Use a hash of the path to avoid collisions.
            let hash = simple_hash(vue_path.to_string_lossy().as_bytes());
            let tsc_tsx_name = format!("{component_name}_{hash:016x}.tsc.tsx");
            let tsc_tsx_path = temp_dir.join(&tsc_tsx_name);

            if fs::write(&tsc_tsx_path, &code).is_err() {
                return None;
            }

            Some((vue_path.clone(), code, tsc_tsx_path))
        })
        .flatten()
        .collect()
}

/// Run the full type-checking pipeline.
pub fn run(
    config: &TsConfig,
    tsconfig_path: &Path,
    opts: &EmitOptions,
    binary: TypeCheckerBinary,
) -> CheckResult {
    if config.vue_files.is_empty() {
        return CheckResult {
            diagnostics: Vec::new(),
            emitted_files: Vec::new(),
        };
    }

    // CRITICAL: The temp dir MUST be inside the project root so that tsc can
    // resolve node_modules (e.g. `import("vue")`) from the generated .tsx
    // files. Using the system temp dir (different drive/path) breaks module
    // resolution and produces incorrect results.
    let temp_dir = match TempDir::new_in(&config.root_dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "verter-tsc: failed to create temp directory in {}: {e}",
                config.root_dir.display()
            );
            return CheckResult {
                diagnostics: Vec::new(),
                emitted_files: Vec::new(),
            };
        }
    };

    // ── Phase A: Validation (TSX) ───────────────────────────────────
    // Generate full TSX output (script body + template) for type checking.
    // This catches type errors that the minimal macro-only .tsc.tsx would miss.
    let validation_generated = generate_all_tsx(&config.vue_files, temp_dir.path());

    // ── Phase B: Declaration Generation (TSC) ────────────────────────
    // Only when --declaration is requested. Uses the minimal macro-only codegen.
    let declaration_generated = if opts.declaration {
        // Use a subdirectory to keep Phase A and Phase B files separate.
        let decl_dir = temp_dir.path().join("_tsc");
        let _ = fs::create_dir_all(&decl_dir);
        Some(generate_all_tsc(&config.vue_files, &decl_dir))
    } else {
        None
    };

    // Write a wildcard module declaration so tsc can resolve `import X from '*.vue'`.
    // Without this, .ts files importing .vue components get TS2307 errors.
    let shims_path = temp_dir.path().join("vue-shims.d.ts");
    let _ = fs::write(
        &shims_path,
        "declare module '*.vue' {\n  \
         import type { DefineComponent } from 'vue'\n  \
         const component: DefineComponent<{}, {}, any>\n  \
         export default component\n}\n",
    );

    // Write a single shared @verter/types declaration file. Individual TSX files
    // import from "@verter/types" but don't embed the ambient module block
    // (embed_ambient_types=false), avoiding duplicate declarations across files.
    let types_path = temp_dir.path().join("__verter_types.d.ts");
    let _ = fs::write(&types_path, verter_core::VERTER_TYPES_AMBIENT_MODULE);

    // Build validation file list (Phase A TSX files).
    let mut tsx_to_vue: HashMap<String, (PathBuf, String)> = HashMap::new();
    let mut validation_paths: Vec<PathBuf> = vec![shims_path.clone(), types_path];

    for (vue_path, tsx_code, tsx_path) in &validation_generated {
        let canon = strip_unc_prefix(&tsx_path.canonicalize().unwrap_or_else(|_| tsx_path.clone()));
        tsx_to_vue.insert(
            canon.to_string_lossy().replace('\\', "/"),
            (vue_path.clone(), tsx_code.clone()),
        );
        validation_paths.push(tsx_path.clone());
    }

    // Build declaration file list (Phase B TSC files).
    // Also track tsc generated files for declaration post-processing.
    let generated_for_decl = if let Some(ref decl_gen) = declaration_generated {
        let mut tsc_tsx_paths: Vec<PathBuf> = vec![shims_path];
        for (vue_path, tsc_code, tsc_tsx_path) in decl_gen {
            let canon = strip_unc_prefix(
                &tsc_tsx_path
                    .canonicalize()
                    .unwrap_or_else(|_| tsc_tsx_path.clone()),
            );
            tsx_to_vue.insert(
                canon.to_string_lossy().replace('\\', "/"),
                (vue_path.clone(), tsc_code.clone()),
            );
            tsc_tsx_paths.push(tsc_tsx_path.clone());
        }
        // When emitting declarations, tsc needs to see the original .ts files too
        for ts_path in &config.ts_files {
            tsc_tsx_paths.push(ts_path.clone());
        }
        Some(tsc_tsx_paths)
    } else {
        None
    };

    // Find a type-checker binary.
    let root = strip_unc_prefix(&config.root_dir);
    let checker_bin = match binary {
        TypeCheckerBinary::TsgoWithFallback => {
            if let Some(tsgo) = reporter::find_tsgo(&root) {
                eprintln!("verter-tsc: using tsgo at {}", tsgo.display());
                strip_unc_prefix(&tsgo)
            } else if let Some(tsc) = reporter::find_tsc(&root) {
                eprintln!("verter-tsc: tsgo not found, falling back to tsc");
                strip_unc_prefix(&tsc)
            } else {
                eprintln!(
                    "verter-tsc: no type checker found. \
                     Install tsgo: npm install -D @typescript/native-preview\n\
                     Or install tsc: npm install -D typescript"
                );
                return CheckResult {
                    diagnostics: Vec::new(),
                    emitted_files: Vec::new(),
                };
            }
        }
        TypeCheckerBinary::ForceTsc => {
            if let Some(tsc) = reporter::find_tsc(&root) {
                eprintln!("verter-tsc: using tsc at {}", tsc.display());
                strip_unc_prefix(&tsc)
            } else {
                eprintln!(
                    "verter-tsc: TypeScript compiler (tsc) not found. \
                     Install it with: npm install -D typescript"
                );
                return CheckResult {
                    diagnostics: Vec::new(),
                    emitted_files: Vec::new(),
                };
            }
        }
    };

    // ── Phase A: Validation ─────────────────────────────────────────
    // Use full TSX files (script body + template) with --noEmit for type checking.
    let validation_opts = EmitOptions {
        no_emit: true,
        declaration: false,
        declaration_dir: None,
    };
    let validation_tsconfig = write_temp_tsconfig(
        temp_dir.path(),
        tsconfig_path,
        &validation_paths,
        &validation_opts,
        &config.root_dir,
    );

    let diagnostics = match validation_tsconfig {
        Ok(tsconfig_path) => match invoke_checker(&checker_bin, &tsconfig_path, &validation_opts) {
            Ok(raw_output) => {
                let raw_diags = reporter::parse_tsc_output(&raw_output);
                remap_diagnostics(raw_diags, &tsx_to_vue)
            }
            Err(e) => {
                eprintln!("verter-tsc: Phase A (validation) failed: {e}");
                Vec::new()
            }
        },
        Err(e) => {
            eprintln!("verter-tsc: failed to write validation tsconfig: {e}");
            Vec::new()
        }
    };

    // ── Phase B: Declaration Generation ──────────────────────────────
    let emitted_files = if let Some(tsc_tsx_paths) = generated_for_decl {
        let decl_opts = EmitOptions {
            no_emit: false,
            declaration: true,
            declaration_dir: opts.declaration_dir.clone(),
        };
        let decl_tsconfig = write_temp_tsconfig(
            temp_dir.path(),
            tsconfig_path,
            &tsc_tsx_paths,
            &decl_opts,
            &config.root_dir,
        );

        match decl_tsconfig {
            Ok(tsconfig_path) => {
                match invoke_checker(&checker_bin, &tsconfig_path, &decl_opts) {
                    Ok(_) => {
                        // Post-process: rename .tsc.tsx.d.ts → .vue.d.ts
                        if let (Some(decl_dir), Some(ref decl_gen)) =
                            (&opts.declaration_dir, &declaration_generated)
                        {
                            postprocess_vue_declarations(decl_dir, decl_gen, &config.root_dir);
                        }
                        opts.declaration_dir
                            .as_ref()
                            .map(|d| collect_dts_files(d))
                            .unwrap_or_default()
                    }
                    Err(e) => {
                        eprintln!("verter-tsc: Phase B (declarations) failed: {e}");
                        Vec::new()
                    }
                }
            }
            Err(e) => {
                eprintln!("verter-tsc: failed to write declaration tsconfig: {e}");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    CheckResult {
        diagnostics,
        emitted_files,
    }
}

/// Invoke the type-checker binary and return its combined stdout+stderr output.
fn invoke_checker(
    checker_bin: &Path,
    tsconfig_path: &Path,
    opts: &EmitOptions,
) -> Result<String, String> {
    let mut cmd = if cfg!(target_os = "windows")
        && !reporter::is_native_binary(checker_bin)
        && checker_bin
            .extension()
            .map(|e| e.eq_ignore_ascii_case("cmd"))
            .unwrap_or(false)
    {
        let mut c = std::process::Command::new("cmd.exe");
        c.arg("/C").arg(checker_bin);
        c
    } else {
        std::process::Command::new(checker_bin)
    };

    let tsconfig_clean = strip_unc_prefix(tsconfig_path);
    cmd.arg("--project").arg(&tsconfig_clean);
    if opts.no_emit {
        cmd.arg("--noEmit");
    }
    if opts.declaration {
        cmd.arg("--declaration");
        if let Some(dir) = &opts.declaration_dir {
            cmd.arg("--declarationDir").arg(dir);
        }
    }

    let output = cmd
        .output()
        .map_err(|e| format!("failed to run {}: {e}", checker_bin.display()))?;

    Ok(String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr))
}

/// Write a synthetic tsconfig.json in `temp_dir` that:
/// - Extends the original tsconfig
/// - Includes all .tsc.tsx files
/// - Sets `rootDir` to `root_dir` so tsc mirrors the source tree in output
fn write_temp_tsconfig(
    temp_dir: &Path,
    original_tsconfig: &Path,
    tsc_tsx_files: &[PathBuf],
    opts: &EmitOptions,
    root_dir: &Path,
) -> Result<PathBuf, String> {
    let original_abs = strip_unc_prefix(
        &original_tsconfig
            .canonicalize()
            .map_err(|e| format!("cannot resolve original tsconfig: {e}"))?,
    );

    // Build file list with absolute paths (strip \\?\ prefix for Windows compatibility).
    let files: Vec<String> = tsc_tsx_files
        .iter()
        .map(|p| {
            let canon = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
            strip_unc_prefix(&canon)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    let mut compiler_options = serde_json::json!({
        "skipLibCheck": true,
        "noEmit": opts.no_emit,
        // Disable composite mode: the parent tsconfig may have `composite: true`
        // which requires all referenced files to be in the project file list.
        // Our .tsc.tsx files import from project .ts files that aren't listed.
        "composite": false,
        // Fix rootDir so tsc mirrors the source tree structure in declarationDir.
        // Without this, tsc computes rootDir from the common ancestor of all input
        // files, which is unpredictable when mixing temp-dir .tsc.tsx and source .ts.
        "rootDir": root_dir.to_string_lossy().replace('\\', "/"),
    });
    // Phase A (validation) uses TSX files that contain JSX syntax.
    // Standard Vue TSX config: `jsx: "preserve"` + `jsxImportSource: "vue"`.
    if !opts.declaration {
        compiler_options["jsx"] = serde_json::json!("react-jsx");
        compiler_options["jsxImportSource"] = serde_json::json!("vue");
        // Clear jsxFactory/jsxFragmentFactory — they conflict with react-jsx mode.
        // The parent tsconfig may set these (e.g. `jsxFactory: "vue"`), and tsc
        // errors if both jsxFactory and react-jsx are present.
        compiler_options["jsxFactory"] = serde_json::json!(null);
        compiler_options["jsxFragmentFactory"] = serde_json::json!(null);
    }
    if opts.declaration {
        compiler_options["declaration"] = serde_json::json!(true);
        compiler_options["emitDeclarationOnly"] = serde_json::json!(true);
        if let Some(dir) = &opts.declaration_dir {
            compiler_options["declarationDir"] =
                serde_json::json!(dir.to_string_lossy().replace('\\', "/"));
        }
    }

    let tsconfig_json = serde_json::json!({
        "extends": original_abs.to_string_lossy().replace('\\', "/"),
        "files": files,
        // Override parent's `include` to prevent scanning the source tree.
        // All files are listed explicitly in `files` (generated .tsc.tsx + shims,
        // plus original .ts files when emitting declarations).
        "include": [],
        "compilerOptions": compiler_options,
    });

    let suffix = if opts.declaration { "decl" } else { "check" };
    let temp_tsconfig = temp_dir.join(format!("verter-tsc-{suffix}.tsconfig.json"));
    std::fs::write(
        &temp_tsconfig,
        serde_json::to_string_pretty(&tsconfig_json)
            .map_err(|e| format!("serialization error: {e}"))?,
    )
    .map_err(|e| format!("write error: {e}"))?;

    Ok(temp_tsconfig)
}

/// Remap raw tsc diagnostics from `.tsc.tsx` positions to `.vue` positions.
fn remap_diagnostics(
    raw: Vec<TscDiagnostic>,
    tsx_to_vue: &HashMap<String, (PathBuf, String)>,
) -> Vec<Diagnostic> {
    raw.into_iter()
        .map(|d| {
            // Try to find a matching vue entry using a suffix match on the file path.
            let file_canon = strip_unc_prefix(
                &PathBuf::from(&d.file)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&d.file)),
            );
            let file_key = file_canon.to_string_lossy().replace('\\', "/");

            // Direct map lookup.
            let maybe_vue = tsx_to_vue.get(&file_key).or_else(|| {
                // Fallback: suffix match (tsc may shorten paths).
                tsx_to_vue.iter().find_map(|(k, v)| {
                    if k.ends_with(&file_key) || file_key.ends_with(k.as_str()) {
                        Some(v)
                    } else {
                        None
                    }
                })
            });

            let (remapped_file, remapped_line, remapped_col) =
                if let Some((vue_path, tsc_code)) = maybe_vue {
                    // Try source map lookup.
                    if let Some((src_name, pos)) = map_tsc_position(tsc_code, d.line, d.col) {
                        // If the mapped source name is a URL/absolute path, try to canonicalize it.
                        let display_path = if src_name.starts_with("file://") {
                            src_name
                                .trim_start_matches("file:///")
                                .trim_start_matches("file://")
                                .replace("%20", " ")
                        } else if src_name.starts_with('/') || src_name.contains(':') {
                            src_name
                        } else {
                            // Relative — resolve against vue file's parent.
                            vue_path
                                .parent()
                                .map(|p| p.join(&src_name).to_string_lossy().into_owned())
                                .unwrap_or(src_name)
                        };
                        (
                            Some(display_path.replace('\\', "/")),
                            pos.line + 1,
                            pos.col + 1,
                        )
                    } else {
                        // Source map lookup failed: report at line 1 of the .vue file.
                        (Some(vue_path.to_string_lossy().replace('\\', "/")), 1, 1)
                    }
                } else {
                    (None, d.line, d.col)
                };

            d.into_diagnostic(remapped_file, remapped_line, remapped_col)
        })
        .collect()
}

/// Collect all `.d.ts` files under a directory.
fn collect_dts_files(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().map(|ext| ext == "ts").unwrap_or(false)
                && e.file_name().to_string_lossy().ends_with(".d.ts")
        })
        .map(|e| e.into_path())
        .collect()
}

/// Rewrite relative import paths in generated code to absolute paths.
///
/// The generated files are placed in a temp directory, so relative imports need
/// to be resolved relative to the original .vue file's directory, not the temp dir.
///
/// Handles two patterns:
/// - `import('./types')` — dynamic import syntax
/// - `from './types'` — ES module import/export syntax
fn rewrite_relative_imports(code: &str, vue_dir: &Path) -> String {
    let mut result = String::with_capacity(code.len());
    let mut rest = code;

    loop {
        // Find the earliest occurrence of either pattern.
        let import_paren = rest.find("import(");
        let from_kw = rest.find("from ");

        let (pos, kind) = match (import_paren, from_kw) {
            (Some(a), Some(b)) if a <= b => (a, ImportKind::DynamicImport),
            (Some(_), Some(b)) => (b, ImportKind::FromKeyword),
            (Some(a), None) => (a, ImportKind::DynamicImport),
            (None, Some(b)) => (b, ImportKind::FromKeyword),
            (None, None) => break,
        };

        result.push_str(&rest[..pos]);

        match kind {
            ImportKind::DynamicImport => {
                let after = &rest[pos + 7..]; // skip "import("
                match rewrite_quoted_path(after, vue_dir) {
                    Some((rewritten, consumed)) => {
                        result.push_str("import(");
                        result.push_str(&rewritten);
                        rest = &after[consumed..];
                    }
                    None => {
                        result.push_str("import(");
                        rest = after;
                    }
                }
            }
            ImportKind::FromKeyword => {
                let after = &rest[pos + 5..]; // skip "from "
                match rewrite_quoted_path(after, vue_dir) {
                    Some((rewritten, consumed)) => {
                        result.push_str("from ");
                        result.push_str(&rewritten);
                        rest = &after[consumed..];
                    }
                    None => {
                        result.push_str("from ");
                        rest = after;
                    }
                }
            }
        }
    }

    result.push_str(rest);
    result
}

enum ImportKind {
    DynamicImport,
    FromKeyword,
}

/// Try to extract a quoted path, resolve it if relative, and return the rewritten
/// quoted string plus the number of bytes consumed from `after` (including closing quote).
fn rewrite_quoted_path(after: &str, vue_dir: &Path) -> Option<(String, usize)> {
    let quote = match after.chars().next() {
        Some(q @ '\'') | Some(q @ '"') => q,
        _ => return None,
    };
    let path_start = 1; // skip opening quote
    let path_end = after[path_start..].find(quote)? + path_start;
    let import_path = &after[path_start..path_end];

    let result = if import_path.starts_with("./") || import_path.starts_with("../") {
        let resolved = vue_dir.join(import_path);
        let abs_path = resolved.to_string_lossy().replace('\\', "/");
        format!("{quote}{abs_path}{quote}")
    } else {
        format!("{quote}{import_path}{quote}")
    };

    // consumed = opening quote + path + closing quote
    Some((result, path_end + 1))
}

/// Sanitize a component name to be a valid JavaScript identifier.
///
/// - Prepends `_` if it starts with a digit (e.g. `404` → `_404`)
/// - Replaces non-alphanumeric chars with `_`
/// - Prefixes JS reserved words (e.g. `default` → `_default`, `export` → `_export`)
fn sanitize_component_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();

    let result = if sanitized.is_empty() {
        "Component".to_string()
    } else if sanitized.chars().next().unwrap().is_ascii_digit() {
        format!("_{sanitized}")
    } else {
        sanitized
    };

    // Prefix reserved words
    match result.as_str() {
        "default" | "export" | "import" | "class" | "function" | "return" | "var" | "let"
        | "const" | "if" | "else" | "for" | "while" | "do" | "switch" | "case" | "break"
        | "continue" | "new" | "delete" | "typeof" | "void" | "this" | "with" | "throw" | "try"
        | "catch" | "finally" | "in" | "of" | "yield" | "await" | "async" | "extends" | "super"
        | "static" | "enum" | "implements" | "interface" | "package" | "private" | "protected"
        | "public" => format!("_{result}"),
        _ => result,
    }
}

/// Simple non-cryptographic hash for generating unique temp file names.
fn simple_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Build a map from emitted `.d.ts` filenames to target `.vue.d.ts` relative paths.
///
/// key = "Button_a1b2c3d4.tsc.tsx.d.ts"  (the filename tsc emits)
/// val = "src/components/Button.vue.d.ts" (target relative path under declarationDir)
fn build_dts_rename_map(
    generated: &[(PathBuf, String, PathBuf)],
    root_dir: &Path,
) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();
    for (vue_path, _, tsc_tsx_path) in generated {
        // The filename tsc emits: e.g. "Button_a1b2c3d4.tsc.tsx.d.ts"
        let emitted_name = format!(
            "{}.d.ts",
            tsc_tsx_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );

        // Target: vue_path relative to root_dir, with .d.ts appended.
        // e.g. /project/src/Button.vue → src/Button.vue.d.ts
        let rel = vue_path
            .strip_prefix(root_dir)
            .unwrap_or(vue_path.as_path());
        let target = PathBuf::from(format!("{}.d.ts", rel.to_string_lossy().replace('\\', "/")));

        map.insert(emitted_name, target);
    }
    map
}

/// Post-process tsc-emitted `.vue` declaration files.
///
/// Renames `.tsc.tsx.d.ts` files to `.vue.d.ts` with correct directory structure,
/// rewrites absolute `import()` paths to relative, and cleans up artifacts.
fn postprocess_vue_declarations(
    decl_dir: &Path,
    generated: &[(PathBuf, String, PathBuf)],
    root_dir: &Path,
) {
    let rename_map = build_dts_rename_map(generated, root_dir);
    if rename_map.is_empty() {
        return;
    }

    // Also build a map from tsc.tsx stem → vue relative path (without .d.ts) for import rewriting.
    // key = "Button_a1b2c3d4.tsc.tsx" → val = "src/components/Button.vue"
    let mut import_rewrite_map: HashMap<String, String> = HashMap::new();
    for (vue_path, _, tsc_tsx_path) in generated {
        let tsx_stem = tsc_tsx_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let rel = vue_path
            .strip_prefix(root_dir)
            .unwrap_or(vue_path.as_path());
        import_rewrite_map.insert(tsx_stem, rel.to_string_lossy().replace('\\', "/"));
    }

    // Scan declarationDir recursively for .tsc.tsx.d.ts files.
    let entries: Vec<PathBuf> = walkdir::WalkDir::new(decl_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tsc.tsx.d.ts"))
        .map(|e| e.into_path())
        .collect();

    for entry in &entries {
        let filename = entry
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        if let Some(target_rel) = rename_map.get(&filename) {
            let target_path = decl_dir.join(target_rel);

            // Read and rewrite imports.
            let content = match fs::read_to_string(entry) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let target_dir = target_path.parent().unwrap_or(decl_dir);
            let rewritten =
                rewrite_dts_imports(&content, target_dir, root_dir, &import_rewrite_map);

            // Create parent directories and write.
            if let Some(parent) = target_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&target_path, rewritten);

            // Delete original temp-named file.
            let _ = fs::remove_file(entry);
        }
    }

    // Delete vue-shims.d.ts artifact if emitted.
    let shims = decl_dir.join("vue-shims.d.ts");
    if shims.exists() {
        let _ = fs::remove_file(&shims);
    }

    // Clean up empty directories left behind.
    cleanup_empty_dirs(decl_dir);
}

/// Rewrite absolute `import("...")` paths in `.d.ts` content.
///
/// During stage 1, `rewrite_relative_imports` converts relative paths to absolute.
/// tsc propagates these into `.d.ts` output. This function converts them back to
/// relative paths from the target `.vue.d.ts` file's location.
///
/// Also rewrites references to `.tsc.tsx` temp files to their `.vue` counterparts.
fn rewrite_dts_imports(
    content: &str,
    target_dir: &Path,
    root_dir: &Path,
    import_rewrite_map: &HashMap<String, String>,
) -> String {
    let mut result = String::with_capacity(content.len());
    let mut rest = content;

    let root_str = root_dir.to_string_lossy().replace('\\', "/");

    while let Some(pos) = rest.find("import(") {
        result.push_str(&rest[..pos]);
        let after_import = &rest[pos + 7..]; // skip "import("

        // Determine quote character.
        let quote = match after_import.chars().next() {
            Some(q @ '\'') | Some(q @ '"') => q,
            _ => {
                result.push_str("import(");
                rest = after_import;
                continue;
            }
        };

        // Find closing quote.
        let path_start = 1; // skip opening quote
        let path_end = match after_import[path_start..].find(quote) {
            Some(i) => path_start + i,
            None => {
                result.push_str("import(");
                rest = after_import;
                continue;
            }
        };

        let import_path = &after_import[path_start..path_end];
        let rewritten_path = rewrite_single_import_path(
            import_path,
            target_dir,
            &root_str,
            root_dir,
            import_rewrite_map,
        );

        result.push_str(&format!("import({quote}{rewritten_path}{quote}"));
        rest = &after_import[path_end + 1..]; // skip closing quote
    }

    result.push_str(rest);
    result
}

/// Rewrite a single import path from a `.d.ts` file.
///
/// Handles three cases:
/// 1. Absolute path under root_dir → convert to relative from target_dir
/// 2. Path containing a `.tsc.tsx` temp file reference → replace with `.vue` path
/// 3. Bare module import → preserve unchanged
fn rewrite_single_import_path(
    import_path: &str,
    target_dir: &Path,
    root_str: &str,
    root_dir: &Path,
    import_rewrite_map: &HashMap<String, String>,
) -> String {
    let normalized = import_path.replace('\\', "/");

    // Case 1: Absolute path starting with root_dir.
    if let Some(stripped) = normalized.strip_prefix(root_str) {
        let rel_from_root = stripped.trim_start_matches('/');
        let target_path = root_dir.join(rel_from_root);
        let target_dir_normalized = target_dir.to_string_lossy().replace('\\', "/");
        return compute_relative_path(
            &target_dir_normalized,
            &target_path.to_string_lossy().replace('\\', "/"),
        );
    }

    // Case 2: Check if this path references a .tsc.tsx temp file.
    // tsc may emit relative paths like "./Button_a1b2c3d4.tsc.tsx"
    for (tsx_name, vue_rel) in import_rewrite_map {
        let tsx_stem = tsx_name.trim_end_matches(".tsc.tsx");
        if normalized.contains(tsx_stem) && normalized.contains(".tsc.tsx") {
            // Replace with relative path to the .vue file.
            let vue_abs = root_dir.join(vue_rel);
            let target_dir_normalized = target_dir.to_string_lossy().replace('\\', "/");
            return compute_relative_path(
                &target_dir_normalized,
                &vue_abs.to_string_lossy().replace('\\', "/"),
            );
        }
    }

    // Case 3: Bare module import — preserve as-is.
    normalized
}

/// Compute a relative path from `from_dir` to `to_path`.
///
/// Both inputs should use forward slashes. Returns a path starting with `./` or `../`.
fn compute_relative_path(from_dir: &str, to_path: &str) -> String {
    let from_parts: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to_path.split('/').filter(|s| !s.is_empty()).collect();

    // Find common prefix length.
    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Number of `..` needed = remaining segments in from_dir.
    let ups = from_parts.len() - common;
    let remaining = &to_parts[common..];

    let mut parts: Vec<&str> = vec![".."; ups];
    parts.extend_from_slice(remaining);

    if parts.is_empty() {
        ".".to_string()
    } else if ups == 0 {
        format!("./{}", parts.join("/"))
    } else {
        parts.join("/")
    }
}

/// Remove empty directories under `dir` (bottom-up).
fn cleanup_empty_dirs(dir: &Path) {
    // Collect directories bottom-up (deepest first).
    let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir() && e.path() != dir)
        .map(|e| e.into_path())
        .collect();
    // Sort by depth descending (longest path first).
    dirs.sort_by_key(|b| std::cmp::Reverse(b.components().count()));
    for d in dirs {
        // remove_dir only succeeds if empty.
        let _ = fs::remove_dir(&d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: write a minimal tsconfig and read back the `files` array.
    fn written_files(tsc_tsx_files: &[PathBuf], declaration: bool) -> Vec<String> {
        let temp = tempfile::TempDir::new().unwrap();
        // Write a base tsconfig for `extends` to reference.
        let base_tsconfig = temp.path().join("base-tsconfig.json");
        fs::write(&base_tsconfig, r#"{ "compilerOptions": {} }"#).unwrap();

        let opts = EmitOptions {
            no_emit: !declaration,
            declaration,
            declaration_dir: if declaration {
                Some(temp.path().join("dist"))
            } else {
                None
            },
        };

        let result = write_temp_tsconfig(
            temp.path(),
            &base_tsconfig,
            tsc_tsx_files,
            &opts,
            temp.path(),
        );
        let tsconfig_path = result.expect("write_temp_tsconfig should succeed");
        let content = fs::read_to_string(&tsconfig_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        json["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn write_temp_tsconfig_includes_all_provided_files() {
        let temp = tempfile::TempDir::new().unwrap();
        let shim = temp.path().join("vue-shims.d.ts");
        let tsx = temp.path().join("App_abc123.tsc.tsx");
        let ts_file = temp.path().join("index.ts");
        // Create the files so canonicalize works.
        fs::write(&shim, "").unwrap();
        fs::write(&tsx, "").unwrap();
        fs::write(&ts_file, "").unwrap();

        let files = written_files(&[shim, tsx, ts_file], true);
        assert_eq!(files.len(), 3, "should include shim + tsx + ts file");
    }

    #[test]
    fn write_temp_tsconfig_no_emit_only_tsx_files() {
        let temp = tempfile::TempDir::new().unwrap();
        let shim = temp.path().join("vue-shims.d.ts");
        let tsx = temp.path().join("App_abc123.tsc.tsx");
        fs::write(&shim, "").unwrap();
        fs::write(&tsx, "").unwrap();

        let files = written_files(&[shim, tsx], false);
        assert_eq!(files.len(), 2, "should include only shim + tsx");
    }

    #[test]
    fn write_temp_tsconfig_declaration_sets_emit_flags() {
        let temp = tempfile::TempDir::new().unwrap();
        let base_tsconfig = temp.path().join("base-tsconfig.json");
        fs::write(&base_tsconfig, r#"{ "compilerOptions": {} }"#).unwrap();

        let opts = EmitOptions {
            no_emit: false,
            declaration: true,
            declaration_dir: Some(temp.path().join("dist")),
        };

        let result =
            write_temp_tsconfig(temp.path(), &base_tsconfig, &[], &opts, temp.path()).unwrap();
        let content = fs::read_to_string(&result).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        let co = &json["compilerOptions"];
        assert_eq!(co["declaration"], true, "should set declaration: true");
        assert_eq!(
            co["emitDeclarationOnly"], true,
            "should set emitDeclarationOnly: true"
        );
        assert_eq!(co["noEmit"], false, "should set noEmit: false");
        // rootDir must be set
        assert!(co["rootDir"].is_string(), "should set rootDir");
        // include must be empty (all files listed explicitly)
        assert_eq!(
            json["include"],
            serde_json::json!([]),
            "include must be empty"
        );
    }

    #[test]
    fn rewrite_relative_imports_rewrites_dotslash() {
        let code = r#"import('./types').Props"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("/project/src/./types"),
            "should resolve relative path: {result}"
        );
        assert!(
            !result.contains("'./types'"),
            "original relative path should be replaced"
        );
    }

    #[test]
    fn rewrite_relative_imports_preserves_absolute() {
        let code = r#"import('vue').DefineComponent"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("'vue'"),
            "absolute import should be preserved: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_from_keyword() {
        let code = r#"import type { Props } from './types'"#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("/project/src/./types"),
            "from keyword relative path should be rewritten: {result}"
        );
        assert!(
            !result.contains("from './types'"),
            "original relative path should be replaced: {result}"
        );
    }

    #[test]
    fn rewrite_relative_imports_from_keyword_preserves_bare_module() {
        let code = r#"import { defineComponent } from "vue""#;
        let result = rewrite_relative_imports(code, Path::new("/project/src"));
        assert!(
            result.contains("\"vue\""),
            "bare module import should be preserved: {result}"
        );
    }

    #[test]
    fn sanitize_component_name_handles_digit_prefix() {
        assert_eq!(sanitize_component_name("404"), "_404");
    }

    #[test]
    fn sanitize_component_name_handles_reserved_word() {
        assert_eq!(sanitize_component_name("default"), "_default");
        assert_eq!(sanitize_component_name("export"), "_export");
    }

    #[test]
    fn sanitize_component_name_handles_special_chars() {
        assert_eq!(sanitize_component_name("my-component"), "my_component");
    }

    // ── dts post-processing tests ──────────────────────────────────

    #[test]
    fn build_dts_rename_map_basic() {
        let root = PathBuf::from("/project");
        let generated = vec![(
            PathBuf::from("/project/src/components/Button.vue"),
            String::new(),
            PathBuf::from("/tmp/abc/Button_a1b2c3d4.tsc.tsx"),
        )];

        let map = build_dts_rename_map(&generated, &root);

        // Positive: correct mapping
        assert_eq!(
            map.get("Button_a1b2c3d4.tsc.tsx.d.ts"),
            Some(&PathBuf::from("src/components/Button.vue.d.ts")),
            "should map tsc.tsx.d.ts to vue.d.ts relative path"
        );
        // Negative: no other entries
        assert_eq!(map.len(), 1, "should have exactly one entry");
    }

    #[test]
    fn build_dts_rename_map_multiple_same_basename() {
        let root = PathBuf::from("/project");
        let hash1 = simple_hash(b"/project/src/A/Button.vue");
        let hash2 = simple_hash(b"/project/src/B/Button.vue");
        let generated = vec![
            (
                PathBuf::from("/project/src/A/Button.vue"),
                String::new(),
                PathBuf::from(format!("/tmp/Button_{hash1:016x}.tsc.tsx")),
            ),
            (
                PathBuf::from("/project/src/B/Button.vue"),
                String::new(),
                PathBuf::from(format!("/tmp/Button_{hash2:016x}.tsc.tsx")),
            ),
        ];

        let map = build_dts_rename_map(&generated, &root);

        assert_eq!(map.len(), 2, "both entries should be present");
        assert_eq!(
            map.get(&format!("Button_{hash1:016x}.tsc.tsx.d.ts")),
            Some(&PathBuf::from("src/A/Button.vue.d.ts"))
        );
        assert_eq!(
            map.get(&format!("Button_{hash2:016x}.tsc.tsx.d.ts")),
            Some(&PathBuf::from("src/B/Button.vue.d.ts"))
        );
    }

    #[test]
    fn compute_relative_path_same_dir() {
        let result = compute_relative_path("/project/src", "/project/src/types");
        assert_eq!(result, "./types", "same dir should start with ./");
        assert!(!result.contains(".."), "should not go up");
    }

    #[test]
    fn compute_relative_path_parent() {
        let result = compute_relative_path("/project/src/components", "/project/src/types/index");
        assert_eq!(result, "../types/index", "should go up one level");
    }

    #[test]
    fn compute_relative_path_deeply_nested() {
        let result = compute_relative_path("/project/src/deep/nested/dir", "/project/lib/other");
        // 4 levels up: dir → nested → deep → src → project, then into lib/other
        assert_eq!(result, "../../../../lib/other");
    }

    #[test]
    fn compute_relative_path_sibling() {
        let result = compute_relative_path("/project/src/a", "/project/src/b/file");
        assert_eq!(result, "../b/file");
    }

    #[test]
    fn rewrite_dts_imports_absolute_to_relative() {
        let content =
            r#"import("D:/project/src/types").Props & import("D:/project/src/utils").Helper"#;
        let target_dir = Path::new("D:/project/src/components");
        let root_dir = Path::new("D:/project");
        let import_map = HashMap::new();

        let result = rewrite_dts_imports(content, target_dir, root_dir, &import_map);

        // Positive: relative paths
        assert!(
            result.contains("../types"),
            "should rewrite to relative: {result}"
        );
        assert!(
            result.contains("../utils"),
            "should rewrite utils to relative: {result}"
        );
        // Negative: no absolute paths
        assert!(
            !result.contains("D:/project"),
            "absolute paths should be removed: {result}"
        );
    }

    #[test]
    fn rewrite_dts_imports_preserves_bare_modules() {
        let content = r#"import("vue").DefineComponent & import("@vueuse/core").UseFn"#;
        let target_dir = Path::new("/project/src");
        let root_dir = Path::new("/project");
        let import_map = HashMap::new();

        let result = rewrite_dts_imports(content, target_dir, root_dir, &import_map);

        assert!(
            result.contains("\"vue\""),
            "bare vue import should be preserved: {result}"
        );
        assert!(
            result.contains("\"@vueuse/core\""),
            "scoped package import should be preserved: {result}"
        );
        // Negative: No path rewriting applied
        assert!(
            !result.contains("./vue"),
            "bare module should not become relative: {result}"
        );
    }

    #[test]
    fn rewrite_dts_imports_cross_directory() {
        let content = r#"import("/project/src/shared/types").Foo"#;
        let target_dir = Path::new("/project/src/components/ui");
        let root_dir = Path::new("/project");
        let import_map = HashMap::new();

        let result = rewrite_dts_imports(content, target_dir, root_dir, &import_map);

        assert!(
            result.contains("../../shared/types"),
            "should compute correct relative path: {result}"
        );
        assert!(
            !result.contains("/project/"),
            "absolute path should be removed: {result}"
        );
    }

    #[test]
    fn rewrite_dts_imports_tsc_tsx_references() {
        let content = r#"import("./Modal_deadbeef01234567.tsc.tsx").ModalProps"#;
        let target_dir = Path::new("/project/src/components");
        let root_dir = Path::new("/project");
        let mut import_map = HashMap::new();
        import_map.insert(
            "Modal_deadbeef01234567.tsc.tsx".to_string(),
            "src/views/Modal.vue".to_string(),
        );

        let result = rewrite_dts_imports(content, target_dir, root_dir, &import_map);

        assert!(
            result.contains("../views/Modal.vue"),
            "should rewrite .tsc.tsx ref to .vue relative path: {result}"
        );
        assert!(
            !result.contains(".tsc.tsx"),
            ".tsc.tsx should not appear in output: {result}"
        );
    }

    #[test]
    fn postprocess_creates_correct_structure() {
        let temp = tempfile::TempDir::new().unwrap();
        let decl_dir = temp.path().join("dist/types");
        let root_dir = temp.path().join("project");

        // Simulate tsc output: a temp subdir with .tsc.tsx.d.ts files.
        let temp_subdir = decl_dir.join("tmp_abc");
        fs::create_dir_all(&temp_subdir).unwrap();

        let hash = simple_hash(b"project/src/Button.vue");
        let emitted_name = format!("Button_{hash:016x}.tsc.tsx.d.ts");
        let dts_content = r#"export declare const Button: {};"#;
        fs::write(temp_subdir.join(&emitted_name), dts_content).unwrap();

        // Also create vue-shims.d.ts artifact.
        fs::write(decl_dir.join("vue-shims.d.ts"), "declare module '*.vue' {}").unwrap();

        let generated = vec![(
            root_dir.join("src/Button.vue"),
            String::new(),
            PathBuf::from(format!("/tmp/Button_{hash:016x}.tsc.tsx")),
        )];

        postprocess_vue_declarations(&decl_dir, &generated, &root_dir);

        // Positive: correct file created
        let target = decl_dir.join("src/Button.vue.d.ts");
        assert!(target.exists(), "should create src/Button.vue.d.ts");
        let result_content = fs::read_to_string(&target).unwrap();
        assert!(
            result_content.contains("export declare const Button"),
            "content should be preserved"
        );

        // Negative: temp file removed
        assert!(
            !temp_subdir.join(&emitted_name).exists(),
            "original .tsc.tsx.d.ts should be deleted"
        );

        // Negative: vue-shims.d.ts removed
        assert!(
            !decl_dir.join("vue-shims.d.ts").exists(),
            "vue-shims.d.ts should be deleted"
        );
    }

    #[test]
    fn generate_all_tsx_includes_inline_source_map() {
        let temp = tempfile::TempDir::new().unwrap();
        let vue_path = temp.path().join("Test.vue");
        fs::write(
            &vue_path,
            "<script setup lang=\"ts\">\nlet a = 1;\na = {}\n</script>\n<template><div /></template>\n",
        )
        .unwrap();

        let out_dir = temp.path().join("out");
        fs::create_dir_all(&out_dir).unwrap();

        let results = generate_all_tsx(&[vue_path], &out_dir);
        assert_eq!(results.len(), 1, "should produce one TSX file");

        let (_vue, tsx_code, _tsx_path) = &results[0];
        // Positive: inline source map marker present
        assert!(
            tsx_code.contains("//# sourceMappingURL=data:application/json;base64,"),
            "TSX code must include inline source map for error remapping"
        );
    }

    #[test]
    fn generate_all_tsx_source_map_remaps_script_body() {
        let temp = tempfile::TempDir::new().unwrap();
        let vue_content = "<script setup lang=\"ts\">\nlet a = 1;\na = {}\n</script>\n<template><div /></template>\n";
        let vue_path = temp.path().join("Test.vue");
        fs::write(&vue_path, vue_content).unwrap();

        let out_dir = temp.path().join("out");
        fs::create_dir_all(&out_dir).unwrap();

        let results = generate_all_tsx(&[vue_path.clone()], &out_dir);
        let (_vue, tsx_code, _tsx_path) = &results[0];

        // Find the line of `a = {}` in the generated TSX.
        let tsx_line_1 = tsx_code
            .lines()
            .enumerate()
            .find(|(_, l)| l.contains("a = {}"))
            .map(|(i, _)| i as u32 + 1)
            .expect("TSX should contain `a = {}`");

        // Remap via source map — should resolve back to the .vue file.
        let (source_name, pos) = crate::error_map::map_tsc_position(tsx_code, tsx_line_1, 1)
            .expect("source map lookup should succeed");

        // The source should be the .vue file path used as `filename`.
        assert!(
            source_name.contains("Test.vue"),
            "source should be Test.vue, got: {source_name}"
        );

        // In the original .vue, `a = {}` is on line 3 (1-indexed).
        // Source map positions are 0-indexed, so line 2.
        assert_eq!(pos.line, 2, "should map to line 3 (0-indexed: 2) in .vue");
    }

    #[test]
    fn write_temp_tsconfig_validation_includes_jsx_option() {
        let temp = tempfile::TempDir::new().unwrap();
        let base_tsconfig = temp.path().join("base-tsconfig.json");
        fs::write(&base_tsconfig, r#"{ "compilerOptions": {} }"#).unwrap();

        let opts = EmitOptions {
            no_emit: true,
            declaration: false,
            declaration_dir: None,
        };

        let result =
            write_temp_tsconfig(temp.path(), &base_tsconfig, &[], &opts, temp.path()).unwrap();
        let content = fs::read_to_string(&result).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Positive: jsx option set for TSX parsing
        let jsx_val = json["compilerOptions"]["jsx"]
            .as_str()
            .expect("jsx compiler option should be set in validation tsconfig");
        assert_eq!(
            jsx_val, "react-jsx",
            "jsx should be react-jsx for Vue TSX type checking"
        );

        // Positive: jsxImportSource set for Vue JSX types
        let jsx_import_source = json["compilerOptions"]["jsxImportSource"]
            .as_str()
            .expect("jsxImportSource should be set");
        assert_eq!(jsx_import_source, "vue", "jsxImportSource should be vue");
    }

    #[test]
    fn write_temp_tsconfig_includes_root_dir() {
        let temp = tempfile::TempDir::new().unwrap();
        let base_tsconfig = temp.path().join("base-tsconfig.json");
        fs::write(&base_tsconfig, r#"{ "compilerOptions": {} }"#).unwrap();

        let root = temp.path().join("my-project");
        fs::create_dir_all(&root).unwrap();

        let opts = EmitOptions {
            no_emit: false,
            declaration: true,
            declaration_dir: Some(temp.path().join("dist")),
        };

        let result = write_temp_tsconfig(temp.path(), &base_tsconfig, &[], &opts, &root).unwrap();
        let content = fs::read_to_string(&result).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();

        let root_dir_val = json["compilerOptions"]["rootDir"]
            .as_str()
            .expect("rootDir should be a string");
        assert!(
            root_dir_val.contains("my-project"),
            "rootDir should contain the project dir: {root_dir_val}"
        );
        // Negative: no backslashes in rootDir
        assert!(
            !root_dir_val.contains('\\'),
            "rootDir should use forward slashes: {root_dir_val}"
        );
    }
}
