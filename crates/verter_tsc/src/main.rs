//! verter-tsc — Vue SFC type checker (vue-tsc replacement).
//!
//! Generates minimal TypeScript declarations from Vue SFC macros
//! (defineProps, defineEmits, defineModel, defineOptions) and passes them
//! to a TypeScript compiler for type checking.
//!
//! # Type checker resolution
//!
//! The `--noEmit` TYPECHECK path drives the GATED tsgo `--api` engine (the
//! version-pinned rc `@typescript/typescript-<platform>` native binary — the TS 7
//! native compiler) IN-MEMORY: the generated carriers are fed as an in-memory
//! overlay, no temp files. This path is tsgo-only and version-gated; there is NO
//! tsc fallback for type checking. Engine discovery precedence: the explicit
//! `VERTER_TSGO_BIN` override, then the rc engine in the project's `node_modules`.
//!
//! The `--declaration` EMIT path runs `tsgo --project` over temp files (the
//! native-preview `tsgo`, with a tsc fallback if absent), because tsgo `--api`
//! exposes no emit surface. Its search order: `node_modules/@typescript/`
//! `native-preview-<platform>/lib/tsgo` → `node_modules/.bin/tsgo` → PATH → npx cache.
//!
//! # Usage
//!
//!   verter-tsc [--project tsconfig.json] [--noEmit]
//!   verter-tsc --declaration --declarationDir dist/types

mod api_check;
mod checker;
mod error_map;
mod offset_map;
mod reporter;
mod tsconfig;

use std::path::PathBuf;
use std::process;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "verter-tsc",
    version = env!("CARGO_PKG_VERSION"),
    about = "Vue SFC type checker — verter-native vue-tsc replacement",
    long_about = "Generates ComponentPublicInstance declarations from Vue SFC macros\n\
                  and invokes tsgo (or tsc) for type checking. Faster than vue-tsc for large projects.\n\n\
                  The --noEmit typecheck path drives tsgo in-memory via --api (no temp files);\n\
                  the --declaration emit path runs tsgo --project over temp files.\n\
                  Install tsgo: npm install -D @typescript/native-preview"
)]
struct Cli {
    /// Path to tsconfig.json [default: tsconfig.json]
    #[arg(short = 'p', long = "project", value_name = "PATH")]
    project: Option<PathBuf>,

    /// Build mode — type-check a solution-style tsconfig (tsc -b compat).
    /// Accepts optional tsconfig paths as positional arguments.
    #[arg(short = 'b', long = "build")]
    build: bool,

    /// Positional tsconfig paths for build mode (e.g., `verter-tsc -b tsconfig.app.json`)
    #[arg(value_name = "TSCONFIG", trailing_var_arg = true)]
    build_projects: Vec<PathBuf>,

    /// Type check only — do not emit output [default when no emit flags are specified]
    #[arg(long = "noEmit")]
    no_emit: bool,

    /// Emit .d.ts declaration files
    #[arg(long = "declaration", short = 'd')]
    declaration: bool,

    /// Emit .d.ts files only (no JS output)
    #[arg(long = "emitDeclarationOnly")]
    emit_declaration_only: bool,

    /// Output directory for .d.ts declarations
    #[arg(long = "declarationDir", value_name = "DIR")]
    declaration_dir: Option<PathBuf>,

    /// Output directory
    #[arg(long = "outDir", value_name = "DIR")]
    out_dir: Option<PathBuf>,

    /// List all included files and exit
    #[arg(long = "listFiles")]
    list_files: bool,

    /// List emitted files after compilation
    #[arg(long = "listEmittedFiles")]
    list_emitted_files: bool,

    // ── tsc-compat flags (accepted but ignored) ────────────────────
    /// Composite mode (accepted for tsc compat, ignored internally)
    #[arg(long = "composite", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    _composite: Option<String>,

    /// Skip lib check (accepted for tsc compat, ignored internally)
    #[arg(long = "skipLibCheck", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    _skip_lib_check: Option<String>,

    /// Force overwrite (accepted for tsc compat, ignored internally)
    #[arg(long = "force", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    _force: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    // Resolve project path: -p takes precedence, then -b positional, then default
    let mut tsconfig_path = if let Some(ref p) = cli.project {
        p.clone()
    } else if !cli.build_projects.is_empty() {
        cli.build_projects[0].clone()
    } else {
        PathBuf::from("tsconfig.json")
    };

    // tsc compatibility: if given a directory, auto-append tsconfig.json
    if tsconfig_path.is_dir() {
        tsconfig_path.push("tsconfig.json");
    }

    let config = match tsconfig::load_tsconfig(&tsconfig_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "error TS5082: Cannot read tsconfig file '{}'.",
                tsconfig_path.display()
            );
            eprintln!("  {e}");
            process::exit(2);
        }
    };

    if cli.list_files {
        for f in &config.vue_files {
            println!("{}", f.display());
        }
        for f in &config.ts_files {
            println!("{}", f.display());
        }
        return;
    }

    let emit_decl = cli.declaration || cli.emit_declaration_only;
    let no_emit = cli.no_emit || !emit_decl;

    let emit_opts = checker::EmitOptions {
        no_emit,
        declaration: emit_decl,
        declaration_dir: cli
            .declaration_dir
            .or_else(|| cli.out_dir.clone())
            .or_else(|| config.declaration_dir.clone())
            .or_else(|| config.out_dir.clone()),
    };

    eprintln!(
        "verter-tsc: checking {} .vue file(s)...",
        config.vue_files.len()
    );

    let result = checker::run(&config, &tsconfig_path, &emit_opts);

    for diagnostic in &result.diagnostics {
        println!("{diagnostic}");
    }

    if cli.list_emitted_files {
        for f in &result.emitted_files {
            println!("TSFILE: {}", f.display());
        }
    }

    if !result.diagnostics.is_empty() {
        let errors = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reporter::Severity::Error))
            .count();
        eprintln!(
            "Found {errors} error(s) in {} file(s).",
            config.vue_files.len()
        );
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_accepts_composite_with_value() {
        let cli = Cli::try_parse_from([
            "verter-tsc",
            "--noEmit",
            "-p",
            "tsconfig.json",
            "--composite",
            "false",
        ]);
        assert!(
            cli.is_ok(),
            "should accept --composite false: {:?}",
            cli.err()
        );
    }

    #[test]
    fn cli_accepts_composite_bare() {
        let cli = Cli::try_parse_from(["verter-tsc", "--noEmit", "--composite"]);
        assert!(
            cli.is_ok(),
            "should accept bare --composite: {:?}",
            cli.err()
        );
    }

    #[test]
    fn cli_accepts_skip_lib_check() {
        let cli = Cli::try_parse_from(["verter-tsc", "--noEmit", "--skipLibCheck"]);
        assert!(cli.is_ok(), "should accept --skipLibCheck: {:?}", cli.err());
    }

    #[test]
    fn cli_accepts_force() {
        let cli = Cli::try_parse_from(["verter-tsc", "--noEmit", "--force"]);
        assert!(cli.is_ok(), "should accept --force: {:?}", cli.err());
    }

    #[test]
    fn cli_accepts_all_compat_flags_together() {
        let cli = Cli::try_parse_from([
            "verter-tsc",
            "--noEmit",
            "-p",
            "tsconfig.json",
            "--composite",
            "false",
            "--skipLibCheck",
            "--force",
        ]);
        assert!(
            cli.is_ok(),
            "should accept all compat flags: {:?}",
            cli.err()
        );
    }

    #[test]
    fn cli_rejects_removed_use_tsc_flag() {
        // `--use-tsc`/`ForceTsc` was removed: the typecheck path is in-memory
        // tsgo `--api` (tsgo-only, no tsc fallback). clap must reject the flag.
        let cli = Cli::try_parse_from(["verter-tsc", "--noEmit", "--use-tsc"]);
        assert!(
            cli.is_err(),
            "--use-tsc must be rejected after removal, got {:?}",
            cli.ok().map(|_| "parsed")
        );
    }

    #[test]
    fn cli_accepts_build_flag_bare() {
        // `verter-tsc -b` should work like `verter-tsc` (use default tsconfig.json)
        let cli = Cli::try_parse_from(["verter-tsc", "-b"]);
        assert!(cli.is_ok(), "should accept bare -b: {:?}", cli.err());
        let cli = cli.unwrap();
        assert!(cli.build, "-b should set build to true");
        assert!(cli.project.is_none(), "-b alone should not set project");
    }

    #[test]
    fn cli_accepts_build_flag_with_path() {
        // `verter-tsc -b tsconfig.app.json` should treat the argument as the project path
        let cli = Cli::try_parse_from(["verter-tsc", "-b", "tsconfig.app.json"]);
        assert!(cli.is_ok(), "should accept -b with path: {:?}", cli.err());
        let cli = cli.unwrap();
        assert!(cli.build, "-b should set build to true");
    }

    #[test]
    fn cli_accepts_build_long_form() {
        let cli = Cli::try_parse_from(["verter-tsc", "--build"]);
        assert!(cli.is_ok(), "should accept --build: {:?}", cli.err());
        assert!(cli.unwrap().build);
    }

    #[test]
    fn cli_build_overrides_project_with_positional() {
        // `verter-tsc -b tsconfig.app.json` — the positional should be used as project
        let cli =
            Cli::try_parse_from(["verter-tsc", "-b", "tsconfig.app.json"]).expect("should parse");
        assert!(cli.build);
        // The positional path from -b should be available as build_projects
        assert!(
            !cli.build_projects.is_empty(),
            "build_projects should contain the positional path"
        );
        assert_eq!(cli.build_projects[0].to_str().unwrap(), "tsconfig.app.json");
    }
}
