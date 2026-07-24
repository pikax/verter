//! verter-tsc — Vue SFC type checker (vue-tsc replacement).
//!
//! Generates minimal TypeScript declarations from Vue SFC macros
//! (defineProps, defineEmits, defineModel, defineOptions) and type-checks them
//! with the tsgo (TypeScript 7 native) engine.
//!
//! # Engine resolution
//!
//! Both paths resolve the engine through the SAME capability-validated
//! toolchain resolver (`verter_tsgo_api::toolchain::discovery`). Precedence,
//! highest first: `--tsgo-bin` → `VERTER_TSGO_BIN` → PATH → project-local
//! ancestor `node_modules` (bounded version probe + support policy + a
//! capability smoke per candidate). An explicitly-named `--tsgo-bin` engine is
//! validated like any other candidate and a failure is a hard user error —
//! never a silent fall-through to a lower tier. (The shared resolver also
//! knows update-cache and bundled-sidecar tiers, but verter-tsc ships
//! neither, so they can never win here.) Verter supports tsgo STABLE
//! `>=7.0.2, <7.1.0` only — RCs, nightlies, and newer minors are refused —
//! and there is no fallback to the legacy TypeScript compiler.
//!
//! The `--noEmit` TYPECHECK path drives the gated tsgo `--api` engine IN-MEMORY
//! (the generated carriers are fed as an in-memory overlay, no temp files).
//! The `--declaration` EMIT path runs `tsgo --project` over temp files (tsgo
//! `--api` exposes no emit surface).
//!
//! # Usage
//!
//!   verter-tsc [--project tsconfig.json] [--noEmit]
//!   verter-tsc --declaration --declarationDir dist/types

mod api_check;
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
                  and type-checks them with the tsgo (TypeScript 7 native) engine.\n\
                  Faster than vue-tsc for large projects.\n\n\
                  The --noEmit typecheck path drives tsgo in-memory via --api (no temp files);\n\
                  the --declaration emit path runs tsgo --project over temp files.\n\n\
                  Engine: tsgo STABLE 7.0.x only (>=7.0.2, <7.1.0), resolved in order via\n\
                  --tsgo-bin, VERTER_TSGO_BIN, PATH, or project-local node_modules.\n\
                  Install one: npm install -D typescript@7.0.2"
)]
struct Cli {
    /// Path to tsconfig.json [default: tsconfig.json]
    #[arg(short = 'p', long = "project", value_name = "PATH")]
    project: Option<PathBuf>,

    /// Path to a specific tsgo engine binary. Takes precedence over
    /// VERTER_TSGO_BIN, PATH, and project-local node_modules. Must be a
    /// supported tsgo (stable >=7.0.2, <7.1.0) — an unusable path is a hard
    /// error, never a silent fall-through.
    #[arg(long = "tsgo-bin", value_name = "PATH")]
    tsgo_bin: Option<PathBuf>,

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

    // The in-memory `--api` typecheck is tsgo-only with NO tsc fallback. A hard
    // failure (engine absent, connect/init/updateSnapshot/protocol/project-not-found)
    // is SURFACED as a non-zero exit + a stderr note — it must NEVER be swallowed
    // into an empty diagnostic set that exits 0 (a broken engine masquerading as a
    // clean typecheck). Exit 2 distinguishes this infrastructure failure from a
    // type-error run (exit 1) and a config-load failure (also exit 2).
    let result = match checker::run(&config, &tsconfig_path, &emit_opts, cli.tsgo_bin.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            process::exit(2);
        }
    };
    debug_assert_eq!(
        result.public_api_outcomes.len(),
        config.vue_files.len(),
        "public API projection must retain exactly one outcome per input carrier"
    );

    for diagnostic in &result.diagnostics {
        println!("{diagnostic}");
    }
    for failure in &result.public_api_failures {
        println!("{failure}");
    }

    if cli.list_emitted_files {
        for f in &result.emitted_files {
            println!("TSFILE: {}", f.display());
        }
    }

    if !result.diagnostics.is_empty() || !result.public_api_failures.is_empty() {
        let errors = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, reporter::Severity::Error))
            .count();
        let errors = errors + result.public_api_failures.len();
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
    use clap::{CommandFactory, Parser};

    // ── DISCRIMINATING (H11): the public help text must describe the SHIPPED
    //    toolchain policy (stable 7.0.x window, the --tsgo-bin → env → PATH →
    //    project-local precedence) and must NOT point users at rejected channels
    //    (native-preview, npx, a tsc fallback) or at tiers that can never succeed
    //    for this binary (no downloader writes the update cache; verter-tsc
    //    ships no bundled sidecar). ────────────────────────────────────────────
    #[test]
    fn help_text_matches_the_shipped_toolchain_policy() {
        let help = Cli::command().render_long_help().to_string();
        for rejected in [
            "native-preview",
            "npx",
            "tsc fallback",
            "(or tsc)",
            "update cache",
            "bundled",
        ] {
            assert!(
                !help.contains(rejected),
                "the help text must not reference the rejected channel `{rejected}`:\n{help}"
            );
        }
        for expected in ["7.0", "VERTER_TSGO_BIN", "--tsgo-bin"] {
            assert!(
                help.contains(expected),
                "the help text must describe the shipped policy (`{expected}`):\n{help}"
            );
        }
    }

    #[test]
    fn cli_accepts_tsgo_bin() {
        let cli = Cli::try_parse_from([
            "verter-tsc",
            "--noEmit",
            "-p",
            "tsconfig.json",
            "--tsgo-bin",
            "/opt/tsgo/bin/tsgo",
        ]);
        assert!(cli.is_ok(), "should accept --tsgo-bin: {:?}", cli.err());
        let cli = cli.unwrap();
        assert_eq!(
            cli.tsgo_bin.as_deref(),
            Some(std::path::Path::new("/opt/tsgo/bin/tsgo"))
        );
    }

    #[test]
    fn cli_tsgo_bin_defaults_to_none() {
        let cli = Cli::try_parse_from(["verter-tsc", "--noEmit"]).expect("should parse");
        assert!(cli.tsgo_bin.is_none());
    }

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
