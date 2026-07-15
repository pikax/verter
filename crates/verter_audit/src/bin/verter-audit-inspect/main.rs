#![deny(missing_docs)]
//! `verter-audit-inspect` — CLI for inspecting Verter audit records.
//!
//! Audit records are JSON files written by producers under
//! `target/audit-captures/<fixture>/<request_id>.json` (the corpus
//! harness's default location) or wherever the
//! `VERTER_COMPONENT_META_AUDIT_JSON_OUT` env var points. This binary
//! is a thin reader over [`verter_audit::RequestAuditRecord`] that
//! aggregates, filters, and diffs sets of records on disk.
//!
//! Subcommands:
//! - `summary <dir>` — per-kind counts, total duration, slowest
//!   records, cache-hit rate.
//! - `record <request_id> [--dir <dir>]` — pretty-print or JSON-dump
//!   a single record.
//! - `cache-heatmap <dir>` — per-cache-layer hit/miss attribution,
//!   summed across every record.
//! - `compare <dir-a> <dir-b>` — diff two record sets (counts,
//!   durations, cache hit rates).
//!
//! The CLI emits human-readable text by default; pass `--json` to
//! serialize the same payload as JSON for programmatic consumers.

use std::path::PathBuf;
use std::process;

use clap::{Args, Parser, Subcommand};

mod compare;
mod heatmap;
mod io;
mod record_cmd;
mod summary;

/// Top-level CLI definition.
#[derive(Parser)]
#[command(
    name = "verter-audit-inspect",
    version = env!("CARGO_PKG_VERSION"),
    about = "Inspect Verter audit records — summaries, single-record dumps, cache heatmaps, and diffs",
    long_about = "Reads RequestAuditRecord JSON files written by Verter producers and aggregates / \
                  filters / diffs them. Records are loaded from a directory (recursively); each \
                  `*.json` file is parsed as one record. Use `--json` on any subcommand to emit \
                  machine-readable output instead of the default human-readable text."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Subcommand dispatch.
#[derive(Subcommand)]
enum Command {
    /// Summarise every record in `<dir>` — per-kind counts, total
    /// duration, cache hit rate, and the slowest 5 records.
    Summary(SummaryArgs),
    /// Print a single record by `request_id`. Searches `<dir>`
    /// (default `.`) recursively for a record with the matching id.
    Record(RecordArgs),
    /// Sum per-cache-layer hit/miss counters across every record in
    /// `<dir>` and print the heatmap (descending by total events).
    CacheHeatmap(CacheHeatmapArgs),
    /// Compare two record directories — print the per-kind delta,
    /// total-duration delta, and cache-hit-rate delta.
    Compare(CompareArgs),
}

/// Shared `--json` flag — opt-in machine-readable output.
#[derive(Args, Clone, Copy)]
struct OutputFormat {
    /// Emit machine-readable JSON instead of human-readable text.
    #[arg(long = "json", default_value_t = false)]
    json: bool,
}

/// `summary` subcommand arguments.
#[derive(Args)]
struct SummaryArgs {
    /// Directory containing `*.json` audit records (recursively).
    #[arg(value_name = "DIR")]
    dir: PathBuf,
    #[command(flatten)]
    format: OutputFormat,
}

/// `record` subcommand arguments.
#[derive(Args)]
struct RecordArgs {
    /// The request_id to look up. Records embed the id as a decimal
    /// string in the JSON envelope.
    #[arg(value_name = "REQUEST_ID")]
    request_id: String,
    /// Directory to search for the record. Defaults to the current
    /// directory.
    #[arg(long = "dir", value_name = "DIR", default_value = ".")]
    dir: PathBuf,
    #[command(flatten)]
    format: OutputFormat,
}

/// `cache-heatmap` subcommand arguments.
#[derive(Args)]
struct CacheHeatmapArgs {
    /// Directory containing `*.json` audit records (recursively).
    #[arg(value_name = "DIR")]
    dir: PathBuf,
    #[command(flatten)]
    format: OutputFormat,
}

/// `compare` subcommand arguments.
#[derive(Args)]
struct CompareArgs {
    /// Baseline record directory ("a").
    #[arg(value_name = "DIR_A")]
    dir_a: PathBuf,
    /// Comparison record directory ("b").
    #[arg(value_name = "DIR_B")]
    dir_b: PathBuf,
    #[command(flatten)]
    format: OutputFormat,
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Command::Summary(args) => summary::run(&args.dir, args.format),
        Command::Record(args) => record_cmd::run(&args.request_id, &args.dir, args.format),
        Command::CacheHeatmap(args) => heatmap::run(&args.dir, args.format),
        Command::Compare(args) => compare::run(&args.dir_a, &args.dir_b, args.format),
    };
    process::exit(exit_code);
}
