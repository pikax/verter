//! verter-tsc — Vue SFC type checker (vue-tsc replacement).
//!
//! Generates minimal TypeScript declarations from Vue SFC macros
//! (defineProps, defineEmits, defineModel, defineOptions) and passes them
//! to a TypeScript compiler for type checking.
//!
//! # Type checker resolution
//!
//! By default, verter-tsc looks for `tsgo` (the Go-based TypeScript compiler
//! from `@typescript/native-preview`) first, falling back to `tsc` if not
//! found. tsgo is ~10x faster than tsc and produces identical diagnostics.
//!
//! Search order for tsgo:
//! 1. `node_modules/@typescript/native-preview-<platform>/lib/tsgo` (native binary)
//! 2. `node_modules/.bin/tsgo` (npm shim, walking up parent dirs)
//! 3. `tsgo` on PATH
//! 4. npx cache
//!
//! If tsgo is not found, falls back to `tsc` (same search strategy).
//! Use `--use-tsc` to skip tsgo and force tsc.
//!
//! # Usage
//!
//!   verter-tsc [--project tsconfig.json] [--noEmit]
//!   verter-tsc --declaration --declarationDir dist/types
//!   verter-tsc --use-tsc --noEmit   # force tsc instead of tsgo

mod checker;
mod error_map;
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
                  By default, uses tsgo (~10x faster) if available, falling back to tsc.\n\
                  Install tsgo: npm install -D @typescript/native-preview\n\
                  Use --use-tsc to force the classic TypeScript compiler."
)]
struct Cli {
    /// Path to tsconfig.json [default: tsconfig.json]
    #[arg(short = 'p', long = "project", value_name = "PATH")]
    project: Option<PathBuf>,

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

    /// Force using tsc instead of tsgo (tsgo is preferred by default for ~10x speed)
    #[arg(long = "use-tsc")]
    use_tsc: bool,

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

    let mut tsconfig_path = cli
        .project
        .clone()
        .unwrap_or_else(|| PathBuf::from("tsconfig.json"));

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

    let binary = if cli.use_tsc {
        checker::TypeCheckerBinary::ForceTsc
    } else {
        checker::TypeCheckerBinary::TsgoWithFallback
    };

    let result = checker::run(&config, &tsconfig_path, &emit_opts, binary);

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
    fn cli_accepts_use_tsc_flag() {
        let cli =
            Cli::try_parse_from(["verter-tsc", "--noEmit", "--use-tsc"]).expect("should parse");
        assert!(cli.use_tsc, "--use-tsc should set use_tsc to true");
    }

    #[test]
    fn cli_defaults_use_tsc_to_false() {
        let cli = Cli::try_parse_from(["verter-tsc", "--noEmit"]).expect("should parse");
        assert!(
            !cli.use_tsc,
            "use_tsc should default to false (prefer tsgo)"
        );
    }

    #[test]
    fn cli_use_tsc_with_all_flags() {
        let cli = Cli::try_parse_from([
            "verter-tsc",
            "--noEmit",
            "-p",
            "tsconfig.json",
            "--use-tsc",
            "--composite",
            "false",
        ]);
        assert!(
            cli.is_ok(),
            "should accept --use-tsc with other flags: {:?}",
            cli.err()
        );
        assert!(cli.unwrap().use_tsc);
    }
}
