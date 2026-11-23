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
//! neither, so they can never win here.) Verter supports the pinned TypeScript
//! release line only
//! ([`verter_tsgo_api::toolchain::policy::SUPPORTED_TSGO_RANGE_LABEL`]) —
//! nightlies, earlier releases and newer minors are refused — and there is no
//! fallback to the legacy TypeScript compiler.
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

#[macro_use]
extern crate verter_debug_assert;

mod api_check;
mod checker;
mod error_map;
mod position;
mod reporter;
mod sfc_driver;
mod tsconfig;

use std::io::Write;
use std::path::PathBuf;
use std::process;

use clap::Parser;

/// The process allocator (mimalloc on native targets): see `verter_alloc`.
#[global_allocator]
static GLOBAL: verter_alloc::Allocator = verter_alloc::ALLOCATOR;

#[derive(Parser)]
#[command(
    name = "verter-tsc",
    version = env!("CARGO_PKG_VERSION"),
    about = "Vue SFC type checker — verter-native vue-tsc replacement",
    long_about = concat!(
        "Generates ComponentPublicInstance declarations from Vue SFC macros\n\
         and type-checks them with the tsgo (TypeScript 7 native) engine.\n\
         Faster than vue-tsc for large projects.\n\n\
         The --noEmit typecheck path drives tsgo in-memory via --api (no temp files);\n\
         the --declaration emit path runs tsgo --project over temp files.\n\n\
         Engine: tsgo >=",
        verter_tsgo_api::typescript_pin!(),
        ", <",
        verter_tsgo_api::typescript_line_ceiling!(),
        ", resolved in order via\n\
         --tsgo-bin, VERTER_TSGO_BIN, PATH, or project-local node_modules.\n\
         Install one: npm install -D typescript@",
        verter_tsgo_api::typescript_pin!()
    )
)]
struct Cli {
    /// Path to tsconfig.json [default: tsconfig.json]
    #[arg(short = 'p', long = "project", value_name = "PATH")]
    project: Option<PathBuf>,

    /// Path to a specific tsgo engine binary. Takes precedence over
    /// VERTER_TSGO_BIN, PATH, and project-local node_modules. Must be a
    /// supported tsgo (the pinned TypeScript release line) — an unusable path
    /// is a hard error, never a silent fall-through.
    #[arg(long = "tsgo-bin", value_name = "PATH")]
    tsgo_bin: Option<PathBuf>,

    /// Build mode — type-check a solution-style tsconfig (tsc -b compat).
    /// Accepts optional tsconfig paths as positional arguments.
    #[arg(short = 'b', long = "build")]
    build: bool,

    /// Positional tsconfig paths for build mode (e.g., `verter-tsc -b tsconfig.app.json`).
    /// Flags may follow them, as with `tsc -b`.
    #[arg(value_name = "TSCONFIG")]
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

    /// Code unit the reported column is counted in. Lines and columns are
    /// one-based; the column unit is explicit, never inferred. A position that
    /// cannot be converted exactly is reported without a line/column.
    #[arg(
        long = "position-encoding",
        value_name = "ENCODING",
        value_enum,
        default_value_t = position::ColumnEncoding::Utf16
    )]
    position_encoding: position::ColumnEncoding,

    /// Report only diagnostics at this position: `FILE:LINE:CHAR`, one-based
    /// line and column in the selected `--position-encoding`.
    #[arg(
        long = "at",
        value_name = "FILE:LINE:CHAR",
        value_parser = position::Selector::parse_line_char,
        conflicts_with = "offset"
    )]
    at: Option<position::Selector>,

    /// Report only diagnostics at this position: `FILE:OFFSET`, a zero-based
    /// offset in the selected `--position-encoding` counted from the start of
    /// the file.
    #[arg(
        long = "offset",
        value_name = "FILE:OFFSET",
        value_parser = position::Selector::parse_offset
    )]
    offset: Option<position::Selector>,

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

    // ── projection-driver qualification (test builds only) ─────────
    /// Check Vue carriers through the SFC projection driver.
    #[cfg(feature = "projection-route-qualification")]
    #[arg(long = "projection-route", hide = true)]
    projection_route: bool,

    /// Recheck on every change (projection driver only).
    #[cfg(feature = "projection-route-qualification")]
    #[arg(long = "watch", hide = true)]
    watch: bool,

    /// Serve the content mapper over standard input and output.
    #[cfg(feature = "projection-route-qualification")]
    #[arg(long = "content-mapper", hide = true)]
    content_mapper: bool,
}

/// The request the projection driver checks for this command line: the `-p`
/// target, or the `-b` roots (`tsconfig.json` when none is named).
#[cfg(feature = "projection-route-qualification")]
fn driver_request(cli: &Cli) -> sfc_driver::DriverRequest {
    let (mode, configs) = if cli.build {
        let roots = if cli.build_projects.is_empty() {
            vec![PathBuf::from("tsconfig.json")]
        } else {
            cli.build_projects.clone()
        };
        (sfc_driver::BuildMode::Build, roots)
    } else {
        let project = cli
            .project
            .clone()
            .or_else(|| cli.build_projects.first().cloned())
            .unwrap_or_else(|| PathBuf::from("tsconfig.json"));
        (sfc_driver::BuildMode::Project, vec![project])
    };
    let declaration = cli.declaration || cli.emit_declaration_only;
    sfc_driver::DriverRequest {
        mode,
        configs,
        no_emit: cli.no_emit,
        declaration,
        declaration_dir: cli.declaration_dir.clone().or_else(|| cli.out_dir.clone()),
        tsgo_bin: cli.tsgo_bin.clone(),
        force: cli._force.as_deref().is_some_and(|value| value != "false"),
        watch: cli.watch,
    }
}

/// The projection driver's entries: `Some(exit code)` when one of them was
/// asked for, `None` for the existing route.
#[cfg(feature = "projection-route-qualification")]
fn projection_route(cli: &Cli) -> Option<i32> {
    if cli.content_mapper {
        let status =
            sfc_driver::mapper_server::serve(&mut std::io::stdin(), &mut std::io::stdout());
        return Some(status as i32);
    }
    if !cli.projection_route {
        return None;
    }
    let request = driver_request(cli);
    if request.watch {
        let mut source = match sfc_driver::FileSystemChangeSource::new() {
            Ok(source) => source,
            Err(error) => {
                eprintln!("verter-tsc: cannot watch for changes: {error}");
                return Some(sfc_driver::ExitStatus::Failed as i32);
            }
        };
        let status = sfc_driver::run_watch(
            &request,
            &mut source,
            &std::time::Instant::now,
            &mut std::io::stdout().lock(),
        );
        return Some(status as i32);
    }
    // The Svelte lane of the selected config. Its host is built only when a
    // project asks for a Svelte surface, and its check runs only when a
    // project has carriers the driver does not own.
    let svelte_config = legacy_tsconfig_path(cli);
    let svelte = tsconfig::load_tsconfig(&svelte_config).ok().map(|config| {
        let lane = checker::SvelteSurfaces::new(&config.root_dir, &svelte_config);
        (config, lane)
    });
    let outcome = match sfc_driver::run(
        &request,
        svelte
            .as_ref()
            .map(|(_, lane)| lane as &dyn sfc_driver::ForeignSurfaceSource),
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("{error}");
            return Some(sfc_driver::ExitStatus::Failed as i32);
        }
    };
    let svelte = svelte
        .filter(|_| {
            outcome
                .projects
                .iter()
                .any(|project| !project.unowned_carriers.is_empty())
        })
        .map(|(mut config, lane)| {
            config
                .vue_files
                .retain(|path| checker::is_svelte_carrier(path));
            let opts = checker::EmitOptions {
                no_emit: true,
                declaration: false,
                declaration_dir: None,
            };
            checker::run_with(
                &lane,
                &config,
                &svelte_config,
                &opts,
                cli.tsgo_bin.as_deref(),
                &vue_surfaces(&outcome),
            )
        });
    let checked = outcome.carriers_checked
        + svelte
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map_or(0, |result| result.admitted_carriers);
    report_checking(checked);
    let mut out = std::io::stdout().lock();
    let mut errors = match write_driver_outcome(cli, &outcome, &mut out) {
        Ok(errors) => errors,
        Err(message) => {
            eprintln!("verter-tsc: {message}");
            return Some(sfc_driver::ExitStatus::Failed as i32);
        }
    };
    let mut status = sfc_driver::exit_status(&outcome) as i32;
    match svelte {
        Some(Ok(result)) => {
            if let Err(message) = write_check_result(cli, &result, &mut out) {
                eprintln!("verter-tsc: {message}");
                return Some(sfc_driver::ExitStatus::Failed as i32);
            }
            errors += error_count(&result.diagnostics) + result.public_api_failures.len();
            if !result.diagnostics.is_empty() || !result.public_api_failures.is_empty() {
                status = status.max(sfc_driver::ExitStatus::Diagnostics as i32);
            }
        }
        Some(Err(error)) => {
            eprintln!("{error}");
            status = sfc_driver::ExitStatus::Failed as i32;
        }
        None => {}
    }
    if errors > 0 {
        report_found(errors, checked);
    }
    Some(status)
}

/// The Vue checking modules the Svelte lane serves for its `.vue` imports:
/// every carrier of each project that has Svelte members, once per path.
#[cfg(feature = "projection-route-qualification")]
fn vue_surfaces(outcome: &sfc_driver::RunOutcome) -> Vec<sfc_driver::ForeignSurface> {
    let mut surfaces: Vec<sfc_driver::ForeignSurface> = Vec::new();
    for project in &outcome.projects {
        if project.unowned_carriers.is_empty() {
            continue;
        }
        for carrier in &project.carriers {
            let path = carrier.result.virtual_file.to_string();
            if surfaces.iter().all(|surface| surface.path != path) {
                surfaces.push(sfc_driver::ForeignSurface {
                    carrier: PathBuf::from(&*carrier.carrier),
                    path,
                    text: Some(carrier.result.text.clone()),
                });
            }
        }
    }
    surfaces
}

/// Write the driver's outcome to `out` through the printing both routes
/// share, and return its error count.
#[cfg(feature = "projection-route-qualification")]
fn write_driver_outcome(
    cli: &Cli,
    outcome: &sfc_driver::RunOutcome,
    out: &mut dyn Write,
) -> Result<usize, String> {
    let diagnostics = sfc_driver::outcome_diagnostics(outcome);
    let positions = sfc_driver::outcome_renderer(outcome, cli.position_encoding);
    write_diagnostics(cli, &positions, &diagnostics, out)?;
    write_emitted_files(cli, &outcome.emitted_files, out)?;
    Ok(error_count(&diagnostics))
}

/// Write the checker's result to `out` through the printing both routes
/// share. Public-API failures follow the diagnostics, whatever is selected.
fn write_check_result(
    cli: &Cli,
    result: &checker::CheckResult,
    out: &mut dyn Write,
) -> Result<(), String> {
    let positions = reporter::PositionRenderer::new(cli.position_encoding)
        .with_virtual_sources(&result.virtual_sources);
    write_diagnostics(cli, &positions, &result.diagnostics, out)?;
    for failure in &result.public_api_failures {
        writeln!(out, "{failure}").map_err(write_failure)?;
    }
    write_emitted_files(cli, &result.emitted_files, out)
}

/// The stderr line that opens a check.
fn report_checking(files: usize) {
    eprintln!("verter-tsc: checking {files} .vue file(s)...");
}

/// The stderr line that closes a check that reported something.
fn report_found(errors: usize, files: usize) {
    eprintln!("Found {errors} error(s) in {files} file(s).");
}

/// How many of `diagnostics` are errors.
fn error_count(diagnostics: &[reporter::Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reporter::Severity::Error))
        .count()
}

/// Write each diagnostic's line to `out`, or only the lines at the `--at` /
/// `--offset` position when one is selected. `Err` is the message for a
/// selector that names no position in its file, or for a failed write.
fn write_diagnostics(
    cli: &Cli,
    positions: &reporter::PositionRenderer,
    diagnostics: &[reporter::Diagnostic],
    out: &mut dyn Write,
) -> Result<(), String> {
    let selected = cli
        .at
        .as_ref()
        .or(cli.offset.as_ref())
        .map(|selector| {
            positions
                .resolve_selector(selector)
                .map(|position| (selector.file(), position))
        })
        .transpose()?;
    for diagnostic in diagnostics {
        if let Some((file, position)) = selected {
            if !positions.is_at(diagnostic, file, position) {
                continue;
            }
        }
        writeln!(out, "{}", positions.render(diagnostic)).map_err(write_failure)?;
    }
    Ok(())
}

/// The `TSFILE:` lines `--listEmittedFiles` asks for.
fn write_emitted_files(cli: &Cli, files: &[PathBuf], out: &mut dyn Write) -> Result<(), String> {
    if cli.list_emitted_files {
        for file in files {
            writeln!(out, "TSFILE: {}", file.display()).map_err(write_failure)?;
        }
    }
    Ok(())
}

fn write_failure(error: std::io::Error) -> String {
    format!("cannot write diagnostics: {error}")
}

/// The config the existing route checks: `-p`, then the first `-b`
/// positional, then `tsconfig.json`. A directory names its `tsconfig.json`,
/// as with `tsc`.
fn legacy_tsconfig_path(cli: &Cli) -> PathBuf {
    let mut path = if let Some(ref project) = cli.project {
        project.clone()
    } else if let Some(first) = cli.build_projects.first() {
        first.clone()
    } else {
        PathBuf::from("tsconfig.json")
    };
    if path.is_dir() {
        path.push("tsconfig.json");
    }
    path
}

fn main() {
    verter_alloc::configure();
    let cli = Cli::parse();

    #[cfg(feature = "projection-route-qualification")]
    if let Some(code) = projection_route(&cli) {
        process::exit(code);
    }

    let tsconfig_path = legacy_tsconfig_path(&cli);

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

    // An empty selection is only a clean pass when `include` matched nothing:
    // when `exclude` removed every matched carrier, say so instead of silently
    // reporting zero errors.
    if config.vue_files.is_empty() {
        let matched = tsconfig::include_matched_carrier_count(&tsconfig_path);
        if matched > 0 {
            eprintln!(
                "verter-tsc: warning: `include` matched {matched} .vue file(s) in {}, but \
                 `exclude` removed all of them; no .vue file is checked.",
                tsconfig_path.display()
            );
        }
    }

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
            .clone()
            .or_else(|| cli.out_dir.clone())
            .or_else(|| config.declaration_dir.clone())
            .or_else(|| config.out_dir.clone()),
    };

    report_checking(config.vue_files.len());

    // The in-memory `--api` typecheck is tsgo-only with NO tsc fallback. A hard
    // failure (engine absent, connect/init/createSnapshot/protocol/project-not-found)
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
    verter_debug_assert_eq!(
        result.public_api_outcomes.len(),
        result.admitted_carriers,
        "public API projection must retain exactly one outcome per ADMITTED carrier \
         (a refused SFC has no companion to project, and is reported as its own \
         Verter-native diagnostic instead)"
    );

    if let Err(message) = write_check_result(&cli, &result, &mut std::io::stdout().lock()) {
        eprintln!("verter-tsc: {message}");
        process::exit(2);
    }

    if !result.diagnostics.is_empty() || !result.public_api_failures.is_empty() {
        let errors = error_count(&result.diagnostics) + result.public_api_failures.len();
        report_found(errors, config.vue_files.len());
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    // ── DISCRIMINATING (H11): the public help text must describe the SHIPPED
    //    toolchain policy (the pinned release line, the --tsgo-bin → env → PATH →
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
        for expected in [
            verter_tsgo_api::toolchain::policy::TYPESCRIPT_PIN,
            "VERTER_TSGO_BIN",
            "--tsgo-bin",
        ] {
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

    /// The column unit is explicit and defaults to the `tsc` convention; the
    /// two position selectors parse strictly and cannot be combined.
    #[test]
    fn cli_position_encoding_and_selectors() {
        let cli = Cli::try_parse_from(["verter-tsc", "--noEmit"]).expect("should parse");
        assert_eq!(cli.position_encoding, position::ColumnEncoding::Utf16);
        assert!(cli.at.is_none() && cli.offset.is_none());

        let cli = Cli::try_parse_from([
            "verter-tsc",
            "--position-encoding",
            "utf8",
            "--at",
            "src/App.vue:3:5",
        ])
        .expect("should parse");
        assert_eq!(cli.position_encoding, position::ColumnEncoding::Utf8);
        assert_eq!(
            cli.at,
            Some(position::Selector::LineChar {
                file: "src/App.vue".into(),
                line: 3,
                column: 5
            })
        );

        let cli = Cli::try_parse_from(["verter-tsc", "--offset", "src/App.vue:12"])
            .expect("should parse");
        assert_eq!(
            cli.offset,
            Some(position::Selector::Offset {
                file: "src/App.vue".into(),
                offset: 12
            })
        );

        for bad in [
            vec!["--position-encoding", "utf-16"],
            vec!["--at", "App.vue:0:1"],
            vec!["--offset", "App.vue:-1"],
            vec!["--at", "App.vue:1:1", "--offset", "App.vue:1"],
        ] {
            let mut argv = vec!["verter-tsc"];
            argv.extend(bad.iter().copied());
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "{bad:?} must be refused"
            );
        }

        #[cfg(feature = "projection-route-qualification")]
        driver_outcome_prints_in_the_selected_encoding_at_the_selected_position();
    }

    /// The driver's outcome goes through the same printing as the existing
    /// route: the column in the selected encoding, only the diagnostics at an
    /// `--at` position, a refused selector, the `TSFILE:` lines, and an error
    /// count over the whole run.
    #[cfg(feature = "projection-route-qualification")]
    fn driver_outcome_prints_in_the_selected_encoding_at_the_selected_position() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("App.vue");
        // `é` is one UTF-16 unit and two UTF-8 bytes, so the `{{` after it is
        // column 8 in UTF-16 and column 9 in UTF-8.
        let file = path.to_string_lossy().into_owned();
        verter_workspace::native_fs::NativeFs::new()
            .write_file(&file, "<template>\n  <p>é {{ a }}</p>\n</template>\n")
            .expect("write");
        let diagnostic = |line, col, message: &str| {
            reporter::Diagnostic::verter(
                file.clone(),
                line,
                col,
                reporter::VerterCode::CarrierSyntax,
                message.to_string(),
            )
        };
        let outcome = sfc_driver::RunOutcome {
            projects: vec![sfc_driver::ProjectCheck {
                config: std::sync::Arc::from("tsconfig.json"),
                carriers: vec![],
                imported_carriers: vec![],
                unowned_carriers: vec![],
                foreign_surfaces: vec![],
                diagnostics: vec![diagnostic(2, 1, "line start"), diagnostic(2, 8, "brace")],
                binding: None,
                raw: vec![],
                attributed: vec![],
                engine_failure: None,
            }],
            graph_problems: vec![],
            carriers_checked: 1,
            emitted_files: vec![PathBuf::from("types").join("App.vue.d.ts")],
        };
        let print = |args: &[&str]| {
            let mut argv = vec!["verter-tsc", "--projection-route"];
            argv.extend_from_slice(args);
            let cli = Cli::try_parse_from(argv).expect("should parse");
            let mut out = Vec::new();
            write_driver_outcome(&cli, &outcome, &mut out)
                .map(|errors| (String::from_utf8(out).expect("utf-8"), errors))
        };

        let (printed, errors) = print(&[]).expect("prints");
        assert_eq!(
            printed,
            format!(
                "{file}(2,1): error VTER1004: line start\n{file}(2,8): error VTER1004: brace\n"
            )
        );
        assert_eq!(errors, 2);

        let at = format!("{file}:2:9");
        let (printed, errors) = print(&[
            "--position-encoding",
            "utf8",
            "--at",
            &at,
            "--listEmittedFiles",
        ])
        .expect("prints");
        assert_eq!(
            printed,
            format!(
                "{file}(2,9): error VTER1004: brace\nTSFILE: {}\n",
                PathBuf::from("types").join("App.vue.d.ts").display()
            ),
            "only the diagnostic at the selected UTF-8 position, then the emitted files"
        );
        assert_eq!(errors, 2, "the count covers the run, not the selection");

        let outside = format!("{file}:9:1");
        let refused = print(&["--at", &outside]).expect_err("a line the file lacks");
        assert!(
            refused.contains("selected position"),
            "the selector is refused, not matched against nothing: {refused}"
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

    /// `tsc -b` reads a flag after its roots as a flag (`tsc -b tsconfig.json
    /// --verbose`), so a flag after the build roots is never another root.
    #[test]
    fn cli_build_flags_after_the_roots_are_flags() {
        let cli = Cli::try_parse_from([
            "verter-tsc",
            "-b",
            "tsconfig.build.json",
            "packages/simulation",
            "apps/web",
            "--tsgo-bin",
            "tools/tsgo/lib/tsc",
        ])
        .expect("should parse");
        assert!(cli.build);
        assert_eq!(
            cli.build_projects,
            [
                PathBuf::from("tsconfig.build.json"),
                PathBuf::from("packages/simulation"),
                PathBuf::from("apps/web"),
            ]
        );
        assert_eq!(cli.tsgo_bin, Some(PathBuf::from("tools/tsgo/lib/tsc")));

        let cli = Cli::try_parse_from(["verter-tsc", "-b", "apps/web", "--noEmit", "apps/admin"])
            .expect("should parse");
        assert!(cli.no_emit);
        assert_eq!(
            cli.build_projects,
            [PathBuf::from("apps/web"), PathBuf::from("apps/admin")]
        );
    }

    /// An unknown flag after the roots is an argument error, as `tsc -b` makes
    /// it, never a root named after the flag.
    #[test]
    fn cli_build_rejects_an_unknown_flag_after_the_roots() {
        let error = Cli::try_parse_from(["verter-tsc", "-b", "apps/web", "--no-such-flag"])
            .err()
            .expect("an unknown flag must not parse as a root");
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    /// The binary runs on the allocator `verter_alloc` selects for it: mimalloc
    /// on a native build, so a block from the global heap is mimalloc's.
    #[test]
    fn the_selected_global_allocator_serves_this_binary() {
        assert_eq!(
            verter_alloc::mimalloc_serves_global_allocations(),
            verter_alloc::SELECTED == verter_alloc::AllocatorKind::Mimalloc
        );
    }
}
