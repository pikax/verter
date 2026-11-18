// Discovery of integration-test case modules, and the committed
// `tests/cases/mod.rs` rendered from it.
//
// Shared by the generator command (`pnpm gen:test-cases`) and by the
// freshness test each crate keeps for its own `tests/cases/mod.rs`; both
// `include!` this file, so it is plain std-only code with no dependency.
//
// Discovery rules, applied to the direct children of `tests/cases/`:
//
// - `<entry>.rs` becomes the module `<entry>`.
// - `<entry>/mod.rs` becomes the module `<entry>` (a hand-written group).
// - `<entry>/` with no `mod.rs` and only `.rs` files below it becomes an inline
//   module over its children, recursively.
// - `<entry>/` next to `<entry>.rs` holds that file's own children and is
//   skipped, as is any directory with data files or a non-identifier name.
// - Hidden files are ignored. `mod.rs` (the generated file itself) and
//   `support/` are never entries; `DiscoveryRules::ignored_dirs` names further
//   support-only directories.
//
// The rendered file lists the modules sorted by module name, the order
// rustfmt keeps, with `#[path]` only where a module name differs from its
// entry name and `#[cfg]` only where a rule gates the entry.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Directory names that never hold entries, in every crate.
const ALWAYS_IGNORED_DIRS: &[&str] = &["support"];

/// The command that rewrites every committed `tests/cases/mod.rs`.
pub const REGENERATE_COMMAND: &str = "pnpm gen:test-cases";

/// The crates whose integration binary loads its entries from a generated
/// `tests/cases/mod.rs`.
pub const DISCOVERED_PACKAGES: &[&str] = &[
    "verter_session",
    "verter_semantic",
    "verter_compiler",
    "verter_lsp",
];

/// Per-crate adjustments to discovery.
pub struct DiscoveryRules<'a> {
    /// Directories under `tests/cases/` that contain only `.rs` support code
    /// reached through `#[path]`, beyond the always-ignored `support`.
    pub ignored_dirs: &'a [&'a str],
    /// `(entry, module)` pairs for entries whose module name must differ
    /// from the file or directory name, to keep test paths stable.
    pub renames: &'a [(&'a str, &'a str)],
    /// `(entry, cfg predicate)` pairs for entries compiled only under a
    /// feature, such as `feature = "semantic-observe"`.
    pub cfg_gates: &'a [(&'a str, &'a str)],
}

const OBSERVE: &str = "feature = \"semantic-observe\"";

/// The discovery rules of one of the `DISCOVERED_PACKAGES`.
pub fn rules_for(package: &str) -> DiscoveryRules<'static> {
    match package {
        "verter_session" => DiscoveryRules {
            // Support-only sources reached through `#[path]` from other entries.
            ignored_dirs: &["block_2_canary", "manifest_data"],
            // Keeps the test paths of the architecture guards unchanged.
            renames: &[("architecture", "architecture_guards")],
            // Entries that read optional audit capture and run in the
            // instrumentation lane only.
            cfg_gates: &[
                ("component_meta_audit", OBSERVE),
                ("corpus_audit_tests", OBSERVE),
                ("css_attribution_chargeable", OBSERVE),
            ],
        },
        "verter_compiler" => DiscoveryRules {
            ignored_dirs: &[],
            renames: &[],
            cfg_gates: &[("svelte_oracle_harness", "feature = \"svelte-oracle\"")],
        },
        "verter_lsp" => DiscoveryRules {
            ignored_dirs: &[],
            renames: &[],
            cfg_gates: &[
                ("lsp_audit_cancellation_finalizes_with_marker", OBSERVE),
                ("lsp_audit_diagnostics_completion", OBSERVE),
                ("lsp_audit_hover_record", OBSERVE),
                ("lsp_audit_query_methods", OBSERVE),
                ("lsp_audit_tls_propagation", OBSERVE),
            ],
        },
        _ => DiscoveryRules {
            ignored_dirs: &[],
            renames: &[],
            cfg_gates: &[],
        },
    }
}

#[derive(Debug)]
pub enum DiscoveryError {
    Io {
        path: PathBuf,
        message: String,
    },
    DuplicateEntry {
        module: String,
        first: PathBuf,
        second: PathBuf,
    },
    CaseCollision {
        first: String,
        second: String,
    },
    InvalidName {
        path: PathBuf,
    },
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, message } => write!(f, "cannot read `{}`: {message}", path.display()),
            Self::DuplicateEntry {
                module,
                first,
                second,
            } => write!(
                f,
                "duplicate test case entry `{module}`: `{}` and `{}`",
                first.display(),
                second.display()
            ),
            Self::CaseCollision { first, second } => write!(
                f,
                "test case entries `{first}` and `{second}` differ only by letter case"
            ),
            Self::InvalidName { path } => write!(
                f,
                "test case entry `{}` is not a valid module name",
                path.display()
            ),
        }
    }
}

enum EntryKind {
    SourceFile,
    HandWrittenDir,
    GeneratedDir,
}

struct Entry {
    entry: String,
    module: String,
    kind: EntryKind,
    path: PathBuf,
}

fn io_error(path: &Path, error: &std::io::Error) -> DiscoveryError {
    DiscoveryError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// True when `dir` holds at least one `.rs` file and nothing but `.rs` files
/// and directories of the same kind, ignoring hidden files.
fn holds_only_rust_sources(dir: &Path) -> Result<bool, DiscoveryError> {
    let mut saw_source = false;
    for child in fs::read_dir(dir).map_err(|e| io_error(dir, &e))? {
        let path = child.map_err(|e| io_error(dir, &e))?.path();
        if is_hidden(&path) {
            continue;
        }
        if path.is_dir() {
            if !holds_only_rust_sources(&path)? {
                return Ok(false);
            }
            saw_source = true;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            saw_source = true;
        } else {
            return Ok(false);
        }
    }
    Ok(saw_source)
}

fn scan_group(
    dir: &Path,
    rules: &DiscoveryRules<'_>,
    top_level: bool,
) -> Result<Vec<Entry>, DiscoveryError> {
    let mut entries = Vec::new();
    for child in fs::read_dir(dir).map_err(|e| io_error(dir, &e))? {
        let path = child.map_err(|e| io_error(dir, &e))?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            return Err(DiscoveryError::InvalidName { path });
        };
        if is_hidden(&path) {
            continue;
        }
        let (entry, kind) = if path.is_dir() {
            if !is_identifier(&name)
                || (top_level
                    && (ALWAYS_IGNORED_DIRS.contains(&name.as_str())
                        || rules.ignored_dirs.contains(&name.as_str())))
            {
                continue;
            }
            if path.join("mod.rs").is_file() {
                (name, EntryKind::HandWrittenDir)
            } else if dir.join(format!("{name}.rs")).is_file() || !holds_only_rust_sources(&path)? {
                continue;
            } else {
                (name, EntryKind::GeneratedDir)
            }
        } else if let Some(stem) = name.strip_suffix(".rs") {
            if top_level && stem == "mod" {
                continue;
            }
            if !is_identifier(stem) {
                return Err(DiscoveryError::InvalidName { path });
            }
            (stem.to_owned(), EntryKind::SourceFile)
        } else {
            continue;
        };
        let module = if top_level {
            rules
                .renames
                .iter()
                .find(|(from, _)| *from == entry)
                .map_or_else(|| entry.clone(), |(_, to)| (*to).to_owned())
        } else {
            entry.clone()
        };
        entries.push(Entry {
            entry,
            module,
            kind,
            path,
        });
    }
    entries.sort_by(|a, b| a.module.cmp(&b.module));
    reject_collisions(&entries)?;
    Ok(entries)
}

fn reject_collisions(entries: &[Entry]) -> Result<(), DiscoveryError> {
    let mut seen: Vec<(String, &Entry)> = Vec::with_capacity(entries.len());
    for entry in entries {
        let folded = entry.module.to_ascii_lowercase();
        if let Some((_, earlier)) = seen.iter().find(|(name, _)| *name == folded) {
            return Err(if earlier.module == entry.module {
                DiscoveryError::DuplicateEntry {
                    module: entry.module.clone(),
                    first: earlier.path.clone(),
                    second: entry.path.clone(),
                }
            } else {
                DiscoveryError::CaseCollision {
                    first: earlier.module.clone(),
                    second: entry.module.clone(),
                }
            });
        }
        seen.push((folded, entry));
    }
    Ok(())
}

fn render_group(
    dir: &Path,
    rules: &DiscoveryRules<'_>,
    top_level: bool,
    depth: usize,
    out: &mut String,
) -> Result<(), DiscoveryError> {
    let indent = "    ".repeat(depth);
    for entry in scan_group(dir, rules, top_level)? {
        if top_level {
            if let Some((_, predicate)) = rules
                .cfg_gates
                .iter()
                .find(|(name, _)| *name == entry.entry)
            {
                let _ = writeln!(out, "{indent}#[cfg({predicate})]");
            }
            if entry.module != entry.entry {
                let target = match entry.kind {
                    EntryKind::SourceFile => format!("{}.rs", entry.entry),
                    _ => format!("{}/mod.rs", entry.entry),
                };
                let _ = writeln!(out, "{indent}#[path = \"{target}\"]");
            }
        }
        match entry.kind {
            EntryKind::SourceFile | EntryKind::HandWrittenDir => {
                let _ = writeln!(out, "{indent}mod {};", entry.module);
            }
            EntryKind::GeneratedDir => {
                let _ = writeln!(out, "{indent}mod {} {{", entry.module);
                render_group(&entry.path, rules, false, depth + 1, out)?;
                let _ = writeln!(out, "{indent}}}");
            }
        }
    }
    Ok(())
}

/// Renders the contents of `<package dir>/tests/cases/mod.rs` from the
/// entries found in `cases_dir`.
pub fn render_cases_module(
    package: &str,
    cases_dir: &Path,
    rules: &DiscoveryRules<'_>,
) -> Result<String, DiscoveryError> {
    let mut out = format!(
        "//! Integration-test entries of the consolidated `{package}` binary.\n\
         //!\n\
         //! Generated by `{REGENERATE_COMMAND}` from the contents of this directory:\n\
         //! every `<entry>.rs` file and every `<entry>/` directory is one module. Do\n\
         //! not edit by hand; a freshness test fails with the command when this file\n\
         //! differs from the directory.\n\n"
    );
    render_group(cases_dir, rules, true, 0, &mut out)?;
    Ok(out)
}

/// The text of a file with line endings normalised to `\n`.
pub fn normalized_text(path: &Path) -> std::io::Result<String> {
    Ok(fs::read_to_string(path)?.replace("\r\n", "\n"))
}
