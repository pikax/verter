//! The AUDITED LIFT COMMAND for TS7 `TypeExpr`-projection parity rows
//! (the TS7 oracle contract §Q4) — the SOLE writer of
//! `LIFTED_ROW_MIGRATIONS`.
//!
//! Run with
//! `cargo run -p verter_session --features oracle-lift --bin oracle_lift -- --row <file.rs>::<fn>`
//! (wrapped by the `pnpm` script `oracle:lift`). `required-features` gates the
//! binary behind `oracle-lift`, so a default `cargo build` / `cargo clippy
//! --workspace --all-targets` skips it entirely.
//!
//! Lifting a row means: its `#[ignore]` is removed, its body is replaced by the
//! self-keyed `#[oracle_row]` driver call, and its retained
//! `LiftMigrationProvenance` — the `migration_fingerprint` plus the
//! `original_body_tokens` the fingerprint was computed from — is recorded. That
//! provenance is the migration-fidelity AUTHORITY every downstream guard
//! validates the registry payload against, so it is never hand-written: this
//! command derives both from the artifacts actually in the tree.
//!
//! What one lift proves before anything is written:
//!
//! 1. the row is SEATED — `ORACLE_QUERY_SPECS` holds its entries with unique,
//!    contiguous `query_ordinal`s from 0, and the whole registry is well-formed;
//! 2. every workspace file the seat names carries VENDORED SOURCE BYTES that are
//!    byte-identical to a checked-in `fixtures/` file;
//! 3. the row has no retained provenance yet (a lift is not a re-lift);
//! 4. the row's test still carries its ORIGINAL `#[ignore]`d body — read from the
//!    source through a `syn` item walk, never a text scan;
//! 5. the row GENUINELY PASSES: the command runs the real engine over the row
//!    (`cargo test -p verter_session --lib -- --exact --include-ignored <path>`)
//!    and requires exactly one passing test;
//! 6. the ORIGINAL body extracts — through the closed `syn` extractor — to a
//!    fidelity tuple whose fingerprint EQUALS the fingerprint of the registry
//!    payload projected through the shared `oracle_registry_fidelity` module. A
//!    seat that does not faithfully reproduce the original query fails here, so
//!    the self-consistent-but-wrong `(spec ∧ snapshot ∧ provenance)` triple
//!    cannot be minted.
//!
//! Only then does it write, and the write is TRANSACTIONAL: the test sources,
//! the provenance table, and the snapshot tree are all restored to their
//! pre-command bytes if snapshot generation, the snapshot-fingerprint mirror
//! check, or the post-lift run of the now-lifted row fails.
//!
//! `--check` re-runs the read-only half over every retained row: the seat, the
//! hermetic re-extraction of the recorded `original_body_tokens`, the registry
//! projection, the lifted shape of the test fn, the snapshot mirror, and the
//! BYTE-STABILITY of the provenance table (the table text must equal the
//! canonical rendering of its own entries, so a hand-edited row is detectable).

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;

use syn::spanned::Spanned;

/// The oracle-query-spec registry, reached as ONE table (the SAME `include!` the
/// `tests/` guards and the `oracle_core::query_specs` module path use), so the
/// seat this command validates is the real executable query payload.
mod oracle_registry {
    include!("../typeinfo/typeinfo_tests/oracle_query_specs.rs");
}

// The closed `syn` migration-fidelity extractor + canonical fingerprint (§Q4) —
// the SOLE migration-fidelity authority. Shared verbatim with the `tests/`
// guards; this command is its lift-time writer, they are its read-time
// validators.
#[path = "../../tests/cases/manifest_data/oracle_migration_extract.rs"]
mod oracle_migration_extract;

// The REGISTRY-side half of the same projection, shared with
// `registry_payload_matches_migration_fingerprint`.
#[path = "../../tests/cases/manifest_data/oracle_registry_fidelity.rs"]
mod oracle_registry_fidelity;

use oracle_migration_extract::{
    canonicalize_body, extract_fidelity, fingerprint, FidelityTuple, ProofShape,
    WorkspaceFileFidelity, MIGRATION_FINGERPRINT_VERSION,
};
use oracle_registry::{LiftMigrationProvenance, LIFTED_ROW_MIGRATIONS};

/// The typeinfo unit-test tree, relative to `CARGO_MANIFEST_DIR`.
const TESTS_TREE: &str = "src/typeinfo/typeinfo_tests";
/// The registry file this command rewrites the provenance table inside.
const REGISTRY_FILE: &str = "src/typeinfo/typeinfo_tests/oracle_query_specs.rs";
/// The checked-in snapshot tree.
const SNAPSHOT_TREE: &str = "src/typeinfo/typeinfo_tests/oracle_snapshots";
/// The vendored fixture sources the seats inline.
const FIXTURES_DIR: &str = "src/typeinfo/typeinfo_tests/fixtures";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match parse_args(&args) {
        Ok(Mode::Check) => run_check(),
        Ok(Mode::Lift(rows)) => run_lift(&rows),
        Err(e) => Err(e),
    };
    match outcome {
        Ok(report) => {
            for line in report {
                eprintln!("oracle_lift: {line}");
            }
        }
        Err(e) => {
            eprintln!("oracle_lift: FAILED — {e}");
            std::process::exit(1);
        }
    }
}

/// What the invocation asked for.
enum Mode {
    /// Read-only re-validation of every retained row.
    Check,
    /// Lift the named `(row_file, row_function)` rows.
    Lift(Vec<RowKey>),
}

/// A row's identity: its `typeinfo_tests` source file and its test fn name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RowKey {
    row_file: String,
    row_function: String,
}

impl std::fmt::Display for RowKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}::{}", self.row_file, self.row_function)
    }
}

fn parse_args(args: &[String]) -> Result<Mode, String> {
    let mut rows = Vec::new();
    let mut check = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--row" => {
                i += 1;
                let spec = args
                    .get(i)
                    .ok_or_else(|| "--row needs a <file.rs>::<fn> argument".to_string())?;
                let (file, func) = spec
                    .split_once("::")
                    .ok_or_else(|| format!("malformed --row {spec:?}; expected <file.rs>::<fn>"))?;
                rows.push(RowKey {
                    row_file: file.to_string(),
                    row_function: func.to_string(),
                });
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
        i += 1;
    }
    match (check, rows.is_empty()) {
        (true, true) => Ok(Mode::Check),
        (true, false) => Err("--check takes no --row arguments".to_string()),
        (false, true) => {
            Err("nothing to do: pass --check, or --row <file.rs>::<fn> to lift".to_string())
        }
        (false, false) => Ok(Mode::Lift(rows)),
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ---------------------------------------------------------------------------
// Transactional writes
// ---------------------------------------------------------------------------

/// Every file the command touched, with the bytes it held BEFORE (or `None` when
/// the command created it). Rolling back restores the tree exactly, so a failure
/// anywhere after the first write leaves nothing behind.
#[derive(Default)]
struct FileTx {
    original: BTreeMap<PathBuf, Option<Vec<u8>>>,
}

impl FileTx {
    /// Record a path's pre-write bytes once (the first record wins, so a path
    /// written twice still rolls back to its original content).
    fn record(&mut self, path: &Path) {
        if self.original.contains_key(path) {
            return;
        }
        let before = std::fs::read(path).ok();
        self.original.insert(path.to_path_buf(), before);
    }

    /// Record every file under `dir` (recursively) so a generator run inside the
    /// transaction can be undone.
    fn record_tree(&mut self, dir: &Path) -> Result<(), String> {
        if !dir.exists() {
            return Ok(());
        }
        let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {dir:?}: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("read_dir {dir:?}: {e}"))?;
            let path = entry.path();
            if path.is_dir() {
                self.record_tree(&path)?;
            } else {
                self.record(&path);
            }
        }
        Ok(())
    }

    fn write(&mut self, path: &Path, contents: &str) -> Result<(), String> {
        self.record(path);
        std::fs::write(path, contents).map_err(|e| format!("write {path:?}: {e}"))
    }

    /// Restore every recorded path, and delete any file the command created
    /// under a recorded tree that was not present before.
    fn rollback(&self, extra_trees: &[PathBuf]) {
        for (path, before) in &self.original {
            match before {
                Some(bytes) => {
                    let _ = std::fs::write(path, bytes);
                }
                None => {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        for tree in extra_trees {
            let _ = remove_unrecorded(tree, &self.original);
        }
    }
}

/// Delete files under `tree` that the transaction never recorded — the files a
/// generator run created after the tree snapshot was taken.
fn remove_unrecorded(
    tree: &Path,
    recorded: &BTreeMap<PathBuf, Option<Vec<u8>>>,
) -> Result<(), String> {
    if !tree.exists() {
        return Ok(());
    }
    let entries = std::fs::read_dir(tree).map_err(|e| format!("read_dir {tree:?}: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir {tree:?}: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            remove_unrecorded(&path, recorded)?;
            let _ = std::fs::remove_dir(&path);
        } else if !recorded.contains_key(&path) {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Seat + vendored-source validation
// ---------------------------------------------------------------------------

/// Prove the row is seated in `ORACLE_QUERY_SPECS`: the registry as a whole is
/// structurally well-formed, and the row holds at least one entry whose
/// `query_ordinal`s are unique and contiguous from 0.
fn validate_seat(row: &RowKey) -> Result<usize, String> {
    oracle_registry::registry_well_formed(oracle_registry::ORACLE_QUERY_SPECS)
        .map_err(|e| format!("the oracle-query-spec registry is malformed: {e:?}"))?;
    let specs = oracle_registry_fidelity::registry_specs_for_row(&row.row_file, &row.row_function);
    if specs.is_empty() {
        return Err(format!(
            "{row}: no ORACLE_QUERY_SPECS seat. A Ts7Oracle row cannot be lifted without a \
             registry entry naming its executable query payload — seat the row first"
        ));
    }
    for (expected, spec) in specs.iter().enumerate() {
        if u16::try_from(expected).ok() != Some(spec.query_ordinal) {
            return Err(format!(
                "{row}: query ordinals must be unique and contiguous from 0, got {:?}",
                specs.iter().map(|s| s.query_ordinal).collect::<Vec<_>>()
            ));
        }
    }
    Ok(specs.len())
}

/// Prove every workspace file the seat names carries VENDORED source bytes that
/// are byte-identical to a checked-in `fixtures/` file. The registry is the
/// source-byte authority, so a seat inlining bytes that exist nowhere on disk
/// would let a lift pin a fabricated fixture.
fn validate_vendored_sources(row: &RowKey, root: &Path) -> Result<(), String> {
    let fixtures = root.join(FIXTURES_DIR);
    let mut on_disk: Vec<String> = Vec::new();
    let entries =
        std::fs::read_dir(&fixtures).map_err(|e| format!("read_dir {fixtures:?}: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir {fixtures:?}: {e}"))?;
        if entry.path().is_file() {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                on_disk.push(text);
            }
        }
    }
    for spec in oracle_registry_fidelity::registry_specs_for_row(&row.row_file, &row.row_function) {
        for file in spec.workspace_files {
            if file.source.is_empty() {
                return Err(format!("{row}: seat file {} has empty source", file.path));
            }
            if !on_disk.iter().any(|t| t == file.source) {
                return Err(format!(
                    "{row}: the seat's vendored source for {} is byte-identical to no file under \
                     {FIXTURES_DIR}. The registry is the source-byte authority: its bytes must be \
                     a checked-in fixture, never a payload that exists only in the seat",
                    file.path
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The row's test source
// ---------------------------------------------------------------------------

/// A row's test fn located structurally in its source file: the byte ranges the
/// rewrite replaces, plus the original body source the fingerprint is computed
/// from.
struct LocatedRow {
    source: String,
    /// The `#[ignore = "…"]` attribute's byte range, including the whitespace
    /// that precedes it on its own line.
    ignore_range: Range<usize>,
    /// The fn's whole `{ … }` block byte range.
    body_range: Range<usize>,
    /// The `{ … }` block source, verbatim.
    body_source: String,
    /// Byte offset of the fn's first attribute — where `#[oracle_row]` goes.
    first_attr_start: usize,
}

fn row_source_path(root: &Path, row: &RowKey) -> PathBuf {
    root.join(TESTS_TREE).join(&row.row_file)
}

/// Parse the row's source file from disk and locate the test fn in its PRE-LIFT
/// shape.
fn locate_ignored_row(root: &Path, row: &RowKey) -> Result<LocatedRow, String> {
    let path = row_source_path(root, row);
    let source = std::fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))?;
    locate_ignored_row_in(&path, source, row)
}

/// Locate the test fn in its PRE-LIFT shape inside the given source text:
/// exactly one `#[ignore = "…"]`, a `#[test]`, no `#[oracle_row]`, and a
/// non-empty body. Anything else is a loud refusal. Taking the text as an
/// argument is what lets SEVERAL rows in ONE file be rewritten in sequence —
/// each row is re-located in the text the previous rewrite produced, so the
/// edits compose instead of overwriting each other.
fn locate_ignored_row_in(path: &Path, source: String, row: &RowKey) -> Result<LocatedRow, String> {
    let file = syn::parse_file(&source).map_err(|e| format!("parse {path:?}: {e}"))?;
    let item_fn = find_fn(&file, &row.row_function)
        .ok_or_else(|| format!("{row}: no `fn {}` in {path:?}", row.row_function))?;

    let mut ignore_attr = None;
    let mut has_test = false;
    for attr in &item_fn.attrs {
        if attr.path().is_ident("oracle_row") {
            return Err(format!(
                "{row}: already carries `#[oracle_row]` — nothing to lift"
            ));
        }
        if attr.path().is_ident("test") {
            has_test = true;
        }
        if attr.path().is_ident("ignore") {
            if ignore_attr.is_some() {
                return Err(format!("{row}: carries more than one `#[ignore]`"));
            }
            ignore_attr = Some(attr);
        }
    }
    if !has_test {
        return Err(format!("{row}: is not a `#[test]`"));
    }
    let ignore_attr = ignore_attr.ok_or_else(|| {
        format!("{row}: carries no `#[ignore]` — only an ignored row is lifted by this command")
    })?;

    let body_range = item_fn.block.span().byte_range();
    let body_source = source
        .get(body_range.clone())
        .ok_or_else(|| format!("{row}: body span {body_range:?} is out of range"))?
        .to_string();
    if item_fn.block.stmts.is_empty() {
        return Err(format!(
            "{row}: has an EMPTY body. There is no original query to extract a \
             migration fingerprint from, so no provenance can be derived"
        ));
    }

    let attr_range = ignore_attr.span().byte_range();
    // Widen to the whole physical line the attribute occupies, so removing it
    // leaves no blank indented line behind. Both ends are derived from the
    // attribute's own span, not from a search for attribute-shaped text.
    let line_start = source[..attr_range.start]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let line_end = source[attr_range.end..]
        .find('\n')
        .map(|i| attr_range.end + i + 1)
        .unwrap_or(source.len());

    let first_attr_start = item_fn
        .attrs
        .first()
        .map(|a| a.span().byte_range().start)
        .unwrap_or(body_range.start);

    Ok(LocatedRow {
        source,
        ignore_range: line_start..line_end,
        body_range,
        body_source,
        first_attr_start,
    })
}

fn find_fn<'a>(file: &'a syn::File, name: &str) -> Option<&'a syn::ItemFn> {
    file.items.iter().find_map(|item| match item {
        syn::Item::Fn(f) if f.sig.ident == name => Some(f),
        _ => None,
    })
}

/// Rewrite the located row into its LIFTED shape: the `#[ignore]` line is
/// removed, `#[oracle_row]` is prepended as the OUTER attribute, and the body
/// becomes `{}` (the proc-macro synthesises the self-keyed driver call). Missing
/// `oracle` / `oracle_row` imports are added structurally after the file's last
/// top-level `use`.
fn rewrite_to_lifted(located: &LocatedRow) -> Result<String, String> {
    let indent = {
        let line_start = located.source[..located.first_attr_start]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        located.source[line_start..located.first_attr_start].to_string()
    };
    // Apply the three edits back-to-front so earlier byte offsets stay valid.
    let mut out = located.source.clone();
    out.replace_range(located.body_range.clone(), "{}");
    out.replace_range(located.ignore_range.clone(), "");
    out.insert_str(
        located.first_attr_start,
        &format!("#[oracle_row]\n{indent}"),
    );
    ensure_lift_imports(&out)
}

/// Add `use super::oracle;` / `use verter_session_oracle_macro::oracle_row;`
/// when the file does not already import them, inserted after the last
/// top-level `use` item (located through the parsed item list).
fn ensure_lift_imports(source: &str) -> Result<String, String> {
    let file = syn::parse_file(source).map_err(|e| format!("re-parse rewritten source: {e}"))?;
    let mut has_oracle = false;
    let mut has_macro = false;
    let mut last_use_end = None;
    for item in &file.items {
        if let syn::Item::Use(u) = item {
            let text = source
                .get(u.span().byte_range())
                .ok_or_else(|| "use-item span out of range".to_string())?;
            if text.contains("super::oracle;") {
                has_oracle = true;
            }
            if text.contains("oracle_row") {
                has_macro = true;
            }
            let end = u.span().byte_range().end;
            last_use_end = Some(match last_use_end {
                Some(prev) if prev > end => prev,
                _ => end,
            });
        }
    }
    if has_oracle && has_macro {
        return Ok(source.to_string());
    }
    let anchor = last_use_end.ok_or_else(|| {
        "the row's source file has no top-level `use` to anchor imports".to_string()
    })?;
    let mut inserted = String::new();
    if !has_oracle {
        inserted.push_str("\nuse super::oracle;");
    }
    if !has_macro {
        inserted.push_str("\nuse verter_session_oracle_macro::oracle_row;");
    }
    let mut out = source.to_string();
    out.insert_str(anchor, &inserted);
    Ok(out)
}

/// Locate a row's test fn in its LIFTED shape and prove it: `#[oracle_row]`, a
/// `#[test]`, no `#[ignore]`, and an empty body.
fn assert_lifted_shape(root: &Path, row: &RowKey) -> Result<(), String> {
    let path = row_source_path(root, row);
    let source = std::fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))?;
    let file = syn::parse_file(&source).map_err(|e| format!("parse {path:?}: {e}"))?;
    let item_fn = find_fn(&file, &row.row_function)
        .ok_or_else(|| format!("{row}: no `fn {}` in {path:?}", row.row_function))?;
    let mut has_oracle_row = false;
    let mut has_test = false;
    for attr in &item_fn.attrs {
        if attr.path().is_ident("oracle_row") {
            has_oracle_row = true;
        }
        if attr.path().is_ident("test") {
            has_test = true;
        }
        if attr.path().is_ident("ignore") {
            return Err(format!(
                "{row}: has retained lift provenance but still carries `#[ignore]`"
            ));
        }
    }
    if !has_oracle_row {
        return Err(format!(
            "{row}: has retained lift provenance but does not carry `#[oracle_row]`"
        ));
    }
    if !has_test {
        return Err(format!("{row}: is not a `#[test]`"));
    }
    if !item_fn.block.stmts.is_empty() {
        return Err(format!(
            "{row}: a lifted row's body must be empty — `#[oracle_row]` synthesises the \
             self-keyed driver call"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Running the row against the real engine
// ---------------------------------------------------------------------------

/// The libtest path a typeinfo row runs under.
fn test_path(row: &RowKey) -> String {
    let module = row.row_file.trim_end_matches(".rs");
    format!("typeinfo::typeinfo_tests::{module}::{}", row.row_function)
}

/// Run the row through the real engine and require EXACTLY ONE passing test.
/// `include_ignored` selects the pre-lift run (the row still carries
/// `#[ignore]`); the post-lift run leaves it off.
fn run_row_test(row: &RowKey, include_ignored: bool) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(cargo);
    cmd.arg("test")
        .arg("-p")
        .arg("verter_session")
        .arg("--lib")
        .arg("--")
        .arg("--exact");
    if include_ignored {
        cmd.arg("--include-ignored");
    }
    cmd.arg(test_path(row));
    let output = cmd
        .output()
        .map_err(|e| format!("{row}: could not run the row's test: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !stdout.contains("1 passed") || !output.status.success() {
        return Err(format!(
            "{row}: the row does NOT pass against the real engine — nothing is lifted.\n\
             --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Provenance table rendering
// ---------------------------------------------------------------------------

/// One retained provenance row in owned form (the compiled-in table plus the
/// rows this run derived).
#[derive(Clone, PartialEq, Eq)]
struct OwnedProvenance {
    row_file: String,
    row_function: String,
    oracle_query_ordinals: u16,
    migration_fingerprint_version: u32,
    migration_fingerprint: String,
    workspace_files: Vec<(String, String)>,
    original_body_tokens: String,
}

impl OwnedProvenance {
    fn from_static(m: &LiftMigrationProvenance) -> Self {
        Self {
            row_file: m.row_file.to_string(),
            row_function: m.row_function.to_string(),
            oracle_query_ordinals: m.oracle_query_ordinals,
            migration_fingerprint_version: m.migration_fingerprint_version,
            migration_fingerprint: m.migration_fingerprint.to_string(),
            workspace_files: m
                .workspace_files
                .iter()
                .map(|(p, h)| ((*p).to_string(), (*h).to_string()))
                .collect(),
            original_body_tokens: m.original_body_tokens.to_string(),
        }
    }
}

/// Render the provenance table's array literal. Deterministic and total: sorted
/// by `(row_file, row_function)`, one field per line, Rust-escaped literals — so
/// the same entry set always renders to the same bytes and `--check` can compare
/// the file against this rendering.
fn render_table(rows: &[OwnedProvenance]) -> String {
    let mut sorted: Vec<&OwnedProvenance> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        (a.row_file.as_str(), a.row_function.as_str())
            .cmp(&(b.row_file.as_str(), b.row_function.as_str()))
    });
    let mut out = String::from("&[\n");
    for m in sorted {
        let files = m
            .workspace_files
            .iter()
            .map(|(p, h)| format!("({p:?}, {h:?})"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "    LiftMigrationProvenance {{\n        row_file: {:?},\n        \
             row_function: {:?},\n        oracle_query_ordinals: {},\n        \
             migration_fingerprint_version: {},\n        migration_fingerprint: {:?},\n        \
             workspace_files: &[{files}],\n        original_body_tokens: {:?},\n    }},\n",
            m.row_file,
            m.row_function,
            m.oracle_query_ordinals,
            m.migration_fingerprint_version,
            m.migration_fingerprint,
            m.original_body_tokens,
        ));
    }
    out.push(']');
    out
}

/// The byte range of the `LIFTED_ROW_MIGRATIONS` initializer expression in the
/// registry source, located through the parsed item list.
fn provenance_table_range(registry_source: &str) -> Result<Range<usize>, String> {
    let file = syn::parse_file(registry_source).map_err(|e| format!("parse registry: {e}"))?;
    for item in &file.items {
        if let syn::Item::Const(c) = item {
            if c.ident == "LIFTED_ROW_MIGRATIONS" {
                return Ok(c.expr.span().byte_range());
            }
        }
    }
    Err("the registry holds no `LIFTED_ROW_MIGRATIONS` const".to_string())
}

// ---------------------------------------------------------------------------
// Lift
// ---------------------------------------------------------------------------

/// The per-row artifacts a validated lift produces, staged before any write.
struct StagedLift {
    row: RowKey,
    provenance: OwnedProvenance,
}

fn run_lift(rows: &[RowKey]) -> Result<Vec<String>, String> {
    let root = manifest_dir();
    let mut report = Vec::new();
    let mut staged: Vec<StagedLift> = Vec::new();

    // ---- Phase 1: validate every row. Nothing is written until all pass. ----
    for row in rows {
        if LIFTED_ROW_MIGRATIONS
            .iter()
            .any(|m| m.row_file == row.row_file && m.row_function == row.row_function)
        {
            return Err(format!(
                "{row}: already carries retained lift provenance. This command is the SOLE \
                 writer of LIFTED_ROW_MIGRATIONS and never re-writes an existing row"
            ));
        }
        let seat_count = validate_seat(row)?;
        validate_vendored_sources(row, &root)?;
        let located = locate_ignored_row(&root, row)?;
        run_row_test(row, true)?;
        report.push(format!("{row}: passes under --include-ignored"));

        let canonical = canonicalize_body(&located.body_source)
            .map_err(|e| format!("{row}: the original body did not canonicalize: {e:?}"))?;
        let workspace_files =
            oracle_registry_fidelity::registry_workspace_files(&row.row_file, &row.row_function)?;
        let body_fidelity = extract_fidelity(
            &canonical,
            &row.row_file,
            &row.row_function,
            workspace_files.clone(),
        )
        .map_err(|e| {
            format!(
                "{row}: the closed extractor could not statically recover the original query \
                 ({e:?}). The row stays ignored — a partial or guessed fingerprint is never written"
            )
        })?;
        let registry_fidelity = oracle_registry_fidelity::registry_fidelity_for_row(
            &row.row_file,
            &row.row_function,
            ProofShape::Ts7Oracle,
        )?;
        let body_fp = fingerprint(&body_fidelity);
        let registry_fp = fingerprint(&registry_fidelity);
        if body_fp != registry_fp {
            return Err(format!(
                "{row}: the seat does NOT reproduce the original body's query.\n  body     {body_fp} \
                 {body_fidelity:?}\n  registry {registry_fp} {registry_fidelity:?}"
            ));
        }
        if usize::from(body_fidelity.declared_query_count) != seat_count {
            return Err(format!(
                "{row}: the original body issues {} quer(ies) but the seat holds {seat_count} entr(ies)",
                body_fidelity.declared_query_count
            ));
        }

        staged.push(StagedLift {
            row: row.clone(),
            provenance: OwnedProvenance {
                row_file: row.row_file.clone(),
                row_function: row.row_function.clone(),
                oracle_query_ordinals: body_fidelity.declared_query_count,
                migration_fingerprint_version: MIGRATION_FINGERPRINT_VERSION,
                migration_fingerprint: body_fp,
                workspace_files: workspace_files
                    .iter()
                    .map(|f| (f.path.clone(), f.content_hash.clone()))
                    .collect(),
                original_body_tokens: canonical,
            },
        });
    }

    // ---- Phase 2: write the rewritten sources + the provenance table. ----
    let mut tx = FileTx::default();
    let snapshot_tree = root.join(SNAPSHOT_TREE);
    tx.record_tree(&snapshot_tree)?;
    let commit = |tx: &mut FileTx| -> Result<(), String> {
        // Rewrite the test sources FILE BY FILE, folding every row's edit through
        // the text the previous row's edit produced — several rows in one file
        // compose instead of overwriting each other.
        let mut by_file: BTreeMap<String, Vec<&StagedLift>> = BTreeMap::new();
        for s in &staged {
            by_file.entry(s.row.row_file.clone()).or_default().push(s);
        }
        for (row_file, rows) in by_file {
            let path = root.join(TESTS_TREE).join(&row_file);
            let mut text =
                std::fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))?;
            for s in rows {
                let located = locate_ignored_row_in(&path, text, &s.row)?;
                text = rewrite_to_lifted(&located)?;
            }
            tx.write(&path, &text)?;
        }
        let registry_path = root.join(REGISTRY_FILE);
        let registry_source = std::fs::read_to_string(&registry_path)
            .map_err(|e| format!("read {registry_path:?}: {e}"))?;
        let range = provenance_table_range(&registry_source)?;
        let mut all: Vec<OwnedProvenance> = LIFTED_ROW_MIGRATIONS
            .iter()
            .map(OwnedProvenance::from_static)
            .collect();
        all.extend(staged.iter().map(|s| s.provenance.clone()));
        let mut updated = registry_source.clone();
        updated.replace_range(range, &render_table(&all));
        tx.write(&registry_path, &updated)
    };
    if let Err(e) = commit(&mut tx) {
        tx.rollback(&[snapshot_tree]);
        return Err(e);
    }

    // ---- Phase 3: snapshots, then the post-lift run. Any failure rolls back. ----
    let finish = || -> Result<Vec<String>, String> {
        generate_snapshots()?;
        let mut lines = Vec::new();
        for s in &staged {
            let recorded = &s.provenance;
            verify_snapshot_mirror(&root, &s.row, recorded)?;
            assert_lifted_shape(&root, &s.row)?;
            run_row_test(&s.row, false)?;
            lines.push(format!(
                "{}: LIFTED — {} (snapshot mirrored, lifted row passes)",
                s.row, recorded.migration_fingerprint
            ));
        }
        Ok(lines)
    };
    match finish() {
        Ok(lines) => {
            report.extend(lines);
            Ok(report)
        }
        Err(e) => {
            tx.rollback(&[snapshot_tree]);
            Err(format!("{e}\n(the tree was restored — nothing was lifted)"))
        }
    }
}

/// Drive the checked-in snapshot generator (the sole tsgo-facing writer).
fn generate_snapshots() -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args([
            "run",
            "-p",
            "verter_session",
            "--features",
            "oracle-gen",
            "--bin",
            "oracle_gen",
        ])
        .output()
        .map_err(|e| format!("could not run oracle_gen: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(format!("oracle_gen failed:\n{stderr}"));
    }
    if stderr.contains("SKIP — tsgo not available") {
        return Err(
            "the pinned tsgo is not available, so no oracle snapshot can be generated. A \
             Ts7Oracle row cannot be lifted without its checked-in snapshot"
                .to_string(),
        );
    }
    Ok(())
}

/// Prove the row now has a checked-in snapshot whose migration-fingerprint
/// mirror equals the provenance this run recorded.
fn verify_snapshot_mirror(
    root: &Path,
    row: &RowKey,
    recorded: &OwnedProvenance,
) -> Result<(), String> {
    let tree = root.join(SNAPSHOT_TREE);
    let mut seen = 0usize;
    let families = std::fs::read_dir(&tree).map_err(|e| format!("read_dir {tree:?}: {e}"))?;
    for family in families {
        let family = family.map_err(|e| format!("read_dir {tree:?}: {e}"))?;
        if !family.path().is_dir() {
            continue;
        }
        let snaps = std::fs::read_dir(family.path())
            .map_err(|e| format!("read_dir {:?}: {e}", family.path()))?;
        for snap in snaps {
            let snap = snap.map_err(|e| format!("read_dir {:?}: {e}", family.path()))?;
            let text = std::fs::read_to_string(snap.path())
                .map_err(|e| format!("read {:?}: {e}", snap.path()))?;
            let doc: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| format!("parse {:?}: {e}", snap.path()))?;
            let matches_row = doc
                .pointer("/row_ref/row_file")
                .and_then(|v| v.as_str())
                .is_some_and(|f| f == row.row_file)
                && doc
                    .pointer("/row_ref/row_function")
                    .and_then(|v| v.as_str())
                    .is_some_and(|f| f == row.row_function);
            if !matches_row {
                continue;
            }
            seen += 1;
            let fp = doc
                .get("migration_fingerprint")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if fp != recorded.migration_fingerprint {
                return Err(format!(
                    "{row}: snapshot {:?} mirrors {fp}, the retained provenance records {}",
                    snap.path(),
                    recorded.migration_fingerprint
                ));
            }
        }
    }
    if seen == 0 {
        return Err(format!(
            "{row}: no checked-in oracle snapshot names the row after generation"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Check
// ---------------------------------------------------------------------------

fn run_check() -> Result<Vec<String>, String> {
    let root = manifest_dir();
    let mut failures: Vec<String> = Vec::new();

    // The provenance table's text must be exactly the canonical rendering of its
    // own entries — the byte-stability property that lets this command re-run
    // and lets a hand-edit be detected.
    let registry_path = root.join(REGISTRY_FILE);
    let registry_source = std::fs::read_to_string(&registry_path)
        .map_err(|e| format!("read {registry_path:?}: {e}"))?;
    let range = provenance_table_range(&registry_source)?;
    let on_disk = registry_source
        .get(range)
        .ok_or_else(|| "LIFTED_ROW_MIGRATIONS span out of range".to_string())?;
    let rendered = render_table(
        &LIFTED_ROW_MIGRATIONS
            .iter()
            .map(OwnedProvenance::from_static)
            .collect::<Vec<_>>(),
    );
    if on_disk != rendered {
        let at = on_disk
            .bytes()
            .zip(rendered.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| on_disk.len().min(rendered.len()));
        let window = |s: &str| {
            let start = s[..at.min(s.len())]
                .char_indices()
                .rev()
                .nth(60)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let end = s.len().min(at + 60);
            s[start..end].to_string()
        };
        failures.push(format!(
            "LIFTED_ROW_MIGRATIONS is not the canonical rendering of its own entries (a \
             hand-edit, or a table this command did not write). First divergence at byte {at}:\n    \
             on disk:  {:?}\n    rendered: {:?}",
            window(on_disk),
            window(&rendered),
        ));
    }

    for m in LIFTED_ROW_MIGRATIONS {
        let row = RowKey {
            row_file: m.row_file.to_string(),
            row_function: m.row_function.to_string(),
        };
        let recorded = OwnedProvenance::from_static(m);
        if let Err(e) = check_one(&root, &row, &recorded) {
            failures.push(e);
        }
    }

    if failures.is_empty() {
        Ok(vec![format!(
            "--check: {} retained lift row(s) re-validated",
            LIFTED_ROW_MIGRATIONS.len()
        )])
    } else {
        Err(format!(
            "--check found {}:\n  {}",
            failures.len(),
            failures.join("\n  ")
        ))
    }
}

fn check_one(root: &Path, row: &RowKey, recorded: &OwnedProvenance) -> Result<(), String> {
    let seat_count = validate_seat(row)?;
    if usize::from(recorded.oracle_query_ordinals) != seat_count {
        return Err(format!(
            "{row}: retained provenance declares {} quer(ies), the seat holds {seat_count}",
            recorded.oracle_query_ordinals
        ));
    }
    validate_vendored_sources(row, root)?;

    // Hermetic re-extraction: the recorded token stream + the recorded workspace
    // files must re-derive the recorded fingerprint, with no VCS archaeology.
    let retained_files: Vec<WorkspaceFileFidelity> = recorded
        .workspace_files
        .iter()
        .map(|(path, content_hash)| WorkspaceFileFidelity {
            path: path.clone(),
            content_hash: content_hash.clone(),
        })
        .collect();
    let body_fidelity: FidelityTuple = extract_fidelity(
        &recorded.original_body_tokens,
        &row.row_file,
        &row.row_function,
        retained_files,
    )
    .map_err(|e| format!("{row}: retained original_body_tokens no longer extract: {e:?}"))?;
    let body_fp = fingerprint(&body_fidelity);
    if body_fp != recorded.migration_fingerprint {
        return Err(format!(
            "{row}: retained original_body_tokens re-derive {body_fp}, not the recorded {}",
            recorded.migration_fingerprint
        ));
    }
    let registry_fp = fingerprint(&oracle_registry_fidelity::registry_fidelity_for_row(
        &row.row_file,
        &row.row_function,
        ProofShape::Ts7Oracle,
    )?);
    if registry_fp != recorded.migration_fingerprint {
        return Err(format!(
            "{row}: the live seat projects {registry_fp}, not the retained {}",
            recorded.migration_fingerprint
        ));
    }
    assert_lifted_shape(root, row)?;
    verify_snapshot_mirror(root, row, recorded)
}
