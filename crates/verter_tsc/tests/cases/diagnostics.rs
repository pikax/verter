//! End-to-end diagnostic tests for `verter-tsc`.
//!
//! Invokes the `verter-tsc` binary on a fixture project with intentional
//! TypeScript errors and validates the diagnostic output. Tests the full
//! pipeline: CLI → tsconfig → .vue compilation → TSX → tsgo/tsc → source
//! map remapping → diagnostic output.

use std::path::{Path, PathBuf};
use std::process::Command;

// ── Diagnostic parsing ──────────────────────────────────────────────────

#[derive(Debug)]
struct Diag {
    file: String,
    line: u32,
    col: u32,
    ts_code: u32,
    message: String,
}

/// Parse verter-tsc stdout into structured diagnostics.
/// Format: `file(line,col): error TSxxxx: message`
fn parse_diagnostics(output: &str) -> Vec<Diag> {
    let mut result = Vec::new();
    for line in output.lines() {
        if let Some(d) = parse_diag_line(line) {
            result.push(d);
        }
    }
    result
}

fn parse_diag_line(line: &str) -> Option<Diag> {
    let paren_start = line.find('(')?;
    let paren_end = line[paren_start..].find(')')? + paren_start;

    let file = &line[..paren_start];
    let coords = &line[paren_start + 1..paren_end];

    let mut parts = coords.splitn(2, ',');
    let line_n: u32 = parts.next()?.trim().parse().ok()?;
    let col_n: u32 = parts.next()?.trim().parse().ok()?;

    let rest = line[paren_end + 1..].trim();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim();

    let rest = if let Some(after) = rest.strip_prefix("error ") {
        after
    } else {
        rest.strip_prefix("warning ")?
    };

    let rest = rest.strip_prefix("TS")?;
    let colon = rest.find(':')?;
    let ts_code: u32 = rest[..colon].parse().ok()?;
    let message = rest[colon + 1..].trim().to_string();

    let file = file.replace('\\', "/");

    Some(Diag {
        file,
        line: line_n,
        col: col_n,
        ts_code,
        message,
    })
}

// ── Assertion helpers ───────────────────────────────────────────────────

/// Assert at least one diagnostic with the given TS code exists in the file.
fn assert_has_error(diags: &[Diag], file_suffix: &str, ts_code: u32) {
    let matching: Vec<_> = diags
        .iter()
        .filter(|d| d.file.ends_with(file_suffix) && d.ts_code == ts_code)
        .collect();
    assert!(
        !matching.is_empty(),
        "expected TS{ts_code} in {file_suffix}, found none.\nAll diags for file: {:#?}",
        diags
            .iter()
            .filter(|d| d.file.ends_with(file_suffix))
            .collect::<Vec<_>>()
    );
}

/// Assert at least N errors in the given file.
fn assert_min_errors(diags: &[Diag], file_suffix: &str, min: usize) {
    let count = diags
        .iter()
        .filter(|d| d.file.ends_with(file_suffix))
        .count();
    assert!(
        count >= min,
        "expected >= {min} errors in {file_suffix}, found {count}.\nDiags: {:#?}",
        diags
            .iter()
            .filter(|d| d.file.ends_with(file_suffix))
            .collect::<Vec<_>>()
    );
}

/// Assert zero errors for a file.
fn assert_no_errors(diags: &[Diag], file_suffix: &str) {
    let found: Vec<_> = diags
        .iter()
        .filter(|d| d.file.ends_with(file_suffix))
        .collect();
    assert!(
        found.is_empty(),
        "expected 0 errors in {file_suffix}, found {}:\n{:#?}",
        found.len(),
        found
    );
}

/// Assert an error exists at an exact line in the given file.
fn assert_error_at(diags: &[Diag], file_suffix: &str, line: u32, ts_code: u32) {
    let matching: Vec<_> = diags
        .iter()
        .filter(|d| d.file.ends_with(file_suffix) && d.line == line && d.ts_code == ts_code)
        .collect();
    assert!(
        !matching.is_empty(),
        "expected TS{ts_code} at {file_suffix}:{line}, found none.\nAll diags for file: {:#?}",
        diags
            .iter()
            .filter(|d| d.file.ends_with(file_suffix))
            .collect::<Vec<_>>()
    );
}

/// Assert an error exists at an EXACT `(line, col)` in `file_suffix` for `ts_code`,
/// whose message contains `msg_substr` (e.g. the quoted symbol name `'unusedVar'`).
///
/// This is stricter than [`assert_error_at`]: it pins the column (so a collapse to
/// `(1,1)` or a wrong-column remap fails) AND a message substring (so a wrong-symbol
/// regression fails). The substring is matched structurally against the parsed
/// `Diag.message` — no brittle full-message equality.
fn assert_error_at_named(
    diags: &[Diag],
    file_suffix: &str,
    line: u32,
    col: u32,
    ts_code: u32,
    msg_substr: &str,
) {
    let matching: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.file.ends_with(file_suffix)
                && d.line == line
                && d.col == col
                && d.ts_code == ts_code
                && d.message.contains(msg_substr)
        })
        .collect();
    assert!(
        !matching.is_empty(),
        "expected TS{ts_code} at {file_suffix}:({line},{col}) whose message contains \
         {msg_substr:?}, found none.\nAll diags for file: {:#?}",
        diags
            .iter()
            .filter(|d| d.file.ends_with(file_suffix))
            .collect::<Vec<_>>()
    );
}

/// Assert no diagnostic points to a temp .tsx file (source map remapping check).
fn assert_no_tsx_paths(diags: &[Diag]) {
    for d in diags {
        assert!(
            !d.file.ends_with(".tsx"),
            "diagnostic points to temp TSX file instead of .vue: {} (TS{} at {}:{},{})",
            d.file,
            d.ts_code,
            d.file,
            d.line,
            d.col
        );
    }
}

/// Assert all diagnostics have valid column numbers (> 0).
fn assert_valid_columns(diags: &[Diag]) {
    for d in diags {
        assert!(
            d.col > 0,
            "diagnostic has col=0 (should be 1-indexed): {} TS{} at {}:{},{}",
            d.file,
            d.ts_code,
            d.file,
            d.line,
            d.col
        );
    }
}

// ── Setup helpers ───────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/verter_tsc/ -> workspace root
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("could not find workspace root")
        .to_path_buf()
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join("fixtures")
        .join("diagnostics")
}

/// Copy the fixture directory to a temp dir and create a node_modules junction/symlink.
fn setup_temp_project() -> Option<(tempfile::TempDir, PathBuf)> {
    let root = workspace_root();
    let node_modules_src = root.join("packages").join("example").join("node_modules");

    if !node_modules_src.join("vue").exists() {
        eprintln!("SKIP: packages/example/node_modules/vue not found — run `pnpm install` first");
        return None;
    }

    let temp = tempfile::TempDir::new().expect("failed to create temp dir");
    let temp_path = temp.path().to_path_buf();

    // Copy fixture files to temp dir
    copy_dir_recursive(&fixture_dir(), &temp_path).expect("failed to copy fixture");

    // Create node_modules junction/symlink
    let nm_dest = temp_path.join("node_modules");
    create_junction_or_symlink(&node_modules_src, &nm_dest);

    if !nm_dest.join("vue").exists() {
        eprintln!("SKIP: failed to create node_modules junction/symlink");
        return None;
    }

    Some((temp, temp_path))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn create_junction_or_symlink(src: &Path, dest: &Path) {
    // Use junction on Windows (doesn't require admin privileges)
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dest)
        .arg(src)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if let Ok(s) = status {
        if !s.success() {
            // Fall back to dir symlink
            let _ = std::os::windows::fs::symlink_dir(src, dest);
        }
    }
}

#[cfg(not(windows))]
fn create_junction_or_symlink(src: &Path, dest: &Path) {
    let _ = std::os::unix::fs::symlink(src, dest);
}

// ── Main test ───────────────────────────────────────────────────────────

#[test]
fn verter_tsc_diagnostics_e2e() {
    let (temp_dir, temp_path) = match setup_temp_project() {
        Some(t) => t,
        None => return, // skip
    };

    let bin = verter_test_support::cargo_test_binary_path!("verter-tsc");
    let output = Command::new(bin)
        .arg("--noEmit")
        .arg("-p")
        .arg(temp_path.join("tsconfig.json"))
        .output()
        .expect("failed to execute verter-tsc");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    eprintln!("=== STDERR ===\n{stderr}");
    eprintln!("=== STDOUT ===\n{stdout}");

    let diags = parse_diagnostics(&stdout);

    // If we got zero diagnostics but expected errors, the checker might not
    // be installed — skip rather than fail.
    if diags.is_empty() {
        eprintln!("SKIP: verter-tsc produced no diagnostics — tsgo/tsc may not be available");
        drop(temp_dir);
        return;
    }

    // ── Negative assertions: valid files must have 0 errors ─────────
    assert_no_errors(&diags, "types.ts");
    assert_no_errors(&diags, "BaseButton.vue");
    assert_no_errors(&diags, "GenericList.vue");
    assert_no_errors(&diags, "StatusBadge.vue");

    // ── Positive assertions: error files must have errors ───────────

    // PropErrors.vue — wrong User fields (TS2322 x4), wrong scalars (TS2322 x2)
    assert_has_error(&diags, "PropErrors.vue", 2322);
    assert_min_errors(&diags, "PropErrors.vue", 6);

    // TemplateExprErrors.vue — TS2339 (undefinedVar, count.length),
    // TS2345 (toFixed('bad')), TS2362 (msg*5)
    assert_has_error(&diags, "TemplateExprErrors.vue", 2339);
    assert_has_error(&diags, "TemplateExprErrors.vue", 2345);
    assert_has_error(&diags, "TemplateExprErrors.vue", 2362);
    assert_min_errors(&diags, "TemplateExprErrors.vue", 4);

    // EmitErrors.vue — TS2769 (no overload matches) for each wrong emit call
    assert_has_error(&diags, "EmitErrors.vue", 2769);
    assert_min_errors(&diags, "EmitErrors.vue", 3);

    // ImportErrors.vue — TS2305 (NonExistent), TS2322 (wrong User fields), TS6133 (unused)
    assert_has_error(&diags, "ImportErrors.vue", 2305);
    assert_has_error(&diags, "ImportErrors.vue", 2322);
    assert_min_errors(&diags, "ImportErrors.vue", 5);

    // ScriptSetupErrors.vue — TS2345 (ref<number>('hello')), TS2322 (computed, reactive fields),
    //                          TS6133 (unusedVar, user)
    assert_has_error(&diags, "ScriptSetupErrors.vue", 2345);
    assert_has_error(&diags, "ScriptSetupErrors.vue", 2322);
    assert_has_error(&diags, "ScriptSetupErrors.vue", 6133);
    assert_min_errors(&diags, "ScriptSetupErrors.vue", 7);

    // CrossComponentErrors.vue — TS2322 for User fields, Status, and scalar types
    assert_has_error(&diags, "CrossComponentErrors.vue", 2322);
    assert_min_errors(&diags, "CrossComponentErrors.vue", 6);

    // VModelErrors.vue — TS2345 for ref type mismatches
    assert_has_error(&diags, "VModelErrors.vue", 2345);
    assert_min_errors(&diags, "VModelErrors.vue", 3);

    // SlotErrors.vue — TS2339 for wrong methods on boolean/number
    assert_has_error(&diags, "SlotErrors.vue", 2339);
    assert_min_errors(&diags, "SlotErrors.vue", 2);

    // GenericErrors.vue — TS2322 (wrong PaginatedResult fields), TS2353 (unknown prop)
    assert_has_error(&diags, "GenericErrors.vue", 2322);
    assert_min_errors(&diags, "GenericErrors.vue", 4);

    // ReactivityErrors.vue — TS2345 (ref<string[]>(42)), TS2322 (reactive fields, watch),
    //                          TS6133 (unused 'bad')
    assert_has_error(&diags, "ReactivityErrors.vue", 2345);
    assert_has_error(&diags, "ReactivityErrors.vue", 2322);
    assert_min_errors(&diags, "ReactivityErrors.vue", 6);

    // ComposableErrors.vue — TS2322 (number→string, string→number), TS6133 (unused 'bad')
    assert_has_error(&diags, "ComposableErrors.vue", 2322);
    assert_min_errors(&diags, "ComposableErrors.vue", 2);

    // GenericInstanceErrors.vue — TS2322 for assigning narrowed generic to number
    assert_has_error(&diags, "GenericInstanceErrors.vue", 2322);
    assert_error_at(&diags, "GenericInstanceErrors.vue", 5, 2322);

    // DirectiveErrors.vue — local directives are resolved as authored bindings, so
    // invalid modifier and value types surface instead of an unused-binding artifact.
    assert_has_error(&diags, "DirectiveErrors.vue", 2345);
    assert_has_error(&diags, "DirectiveErrors.vue", 2353);
    assert_min_errors(&diags, "DirectiveErrors.vue", 5);

    // OptionsApiErrors.vue — TS2322 in methods (string → number)
    assert_has_error(&diags, "OptionsApiErrors.vue", 2322);
    assert_error_at(&diags, "OptionsApiErrors.vue", 22, 2322);

    // OptionsApiAdvanced.vue — computed getter/setter, lifecycle hooks, methods
    // TS2322 in badAssign() (number → string)
    assert_has_error(&diags, "OptionsApiAdvanced.vue", 2322);
    assert_error_at(&diags, "OptionsApiAdvanced.vue", 26, 2322);

    // OptionsApiConsumer.vue — cross-component Options API prop checking
    // The IDE path generates defineComponent() exports that TS resolves prop types from.
    assert_has_error(&diags, "OptionsApiConsumer.vue", 2322);

    // PositionControls.vue — a source-backed script diagnostic keeps its
    // EXACT authored file, line, and column through the checker's source-map
    // conversion. The fixture's script follows its template (block offset: the
    // authored full-SFC line differs from the generated and block-relative
    // lines) and declares U+1D11E before each anchor (2 UTF-16 units / 4 UTF-8
    // bytes / 1 codepoint — only a UTF-16 column lands on the anchor). The
    // TS2345 pin at (21,40) fails if the remap degrades to the covering
    // declarator anchor's column 19; a byte- or codepoint-counted column lands
    // elsewhere again.
    assert_min_errors(&diags, "PositionControls.vue", 2);
    assert_error_at_named(
        &diags,
        "PositionControls.vue",
        20,
        17,
        2322,
        "Type 'number' is not assignable to type 'string'",
    );
    assert_error_at_named(
        &diags,
        "PositionControls.vue",
        21,
        40,
        2345,
        "Argument of type 'number' is not assignable to parameter of type 'string'",
    );

    // ── Source map / span mapping validation ────────────────────────

    // No diagnostic should point to a .tsx temp file
    assert_no_tsx_paths(&diags);

    // All columns should be valid (1-indexed, > 0)
    assert_valid_columns(&diags);

    // All .vue diagnostics should have lines within the file (sanity)
    for d in &diags {
        if d.file.ends_with(".vue") {
            assert!(
                d.line >= 1 && d.line <= 200,
                "suspicious line number {} in {}: likely a source map bug",
                d.line,
                d.file
            );
        }
    }

    // ── Pinned position assertions ──────────────────────────────────
    // These verify source map remapping accuracy for specific known positions.
    // If a fixture file changes, update both the file and these assertions.

    // TemplateExprErrors.vue — all 4 template errors have correct line mapping
    assert_error_at(&diags, "TemplateExprErrors.vue", 7, 2339); // undefinedVar
    assert_error_at(&diags, "TemplateExprErrors.vue", 9, 2339); // count.length
    assert_error_at(&diags, "TemplateExprErrors.vue", 11, 2345); // toFixed('bad')
    assert_error_at(&diags, "TemplateExprErrors.vue", 13, 2362); // msg * 5

    // EmitErrors.vue — emit calls at specific lines
    assert_error_at(&diags, "EmitErrors.vue", 8, 2769); // emit('submit', 42)
    assert_error_at(&diags, "EmitErrors.vue", 10, 2769); // emit('submit', {wrong:true})
    assert_error_at(&diags, "EmitErrors.vue", 12, 2769); // emit('count', 'not-a-number')

    // ScriptSetupErrors.vue — ref/computed/reactive at known lines
    assert_error_at(&diags, "ScriptSetupErrors.vue", 6, 2345); // ref<number>('hello')
    assert_error_at(&diags, "ScriptSetupErrors.vue", 10, 2322); // computed return type
                                                                // ISSUE-7: an unused top-level `<script setup>` local maps TS6133 back to its
                                                                // SOURCE decl line (the IDE codegen no longer keeps it artificially live via
                                                                // the `___VERTER___unwrapped` value-read). Both proven-unused locals are pinned
                                                                // at their exact decl `(line, col)` AND by symbol name in the SAME run, so a
                                                                // collapse to `(1,1)`, a wrong-line/column remap, or a wrong/missing symbol
                                                                // all fail this gate (not just a "some TS6133 somewhere in the file" check).
                                                                // `const unusedVar = ...` — used nowhere; the declarator anchor maps to
                                                                // (14, 7) — the exact `unusedVar` name column.
    assert_error_at_named(&diags, "ScriptSetupErrors.vue", 14, 7, 6133, "'unusedVar'");
    // `const user = reactive<User>(...)` — never read after declaration; the
    // declarator anchor maps to (17, 7) — the exact `user` name column.
    assert_error_at_named(&diags, "ScriptSetupErrors.vue", 17, 7, 6133, "'user'");
    assert_error_at(&diags, "ScriptSetupErrors.vue", 18, 2322); // reactive User.id
    assert_error_at(&diags, "ScriptSetupErrors.vue", 19, 2322); // reactive User.name
    assert_error_at(&diags, "ScriptSetupErrors.vue", 20, 2322); // reactive User.email
    assert_error_at(&diags, "ScriptSetupErrors.vue", 21, 2322); // reactive User.age

    // ImportErrors.vue — import error and field errors
    assert_error_at(&diags, "ImportErrors.vue", 3, 2305); // NonExistent import
    assert_error_at(&diags, "ImportErrors.vue", 9, 2322); // id: 'not-a-number'
    assert_error_at(&diags, "ImportErrors.vue", 10, 2322); // name: 123
    assert_error_at(&diags, "ImportErrors.vue", 11, 2322); // email: true
    assert_error_at(&diags, "ImportErrors.vue", 12, 2322); // age: 'old'

    // PropErrors.vue — User fields and scalar mismatches
    assert_error_at(&diags, "PropErrors.vue", 6, 2322); // id: 'not-a-number'
    assert_error_at(&diags, "PropErrors.vue", 7, 2322); // name: 123
    assert_error_at(&diags, "PropErrors.vue", 8, 2322); // email: true
    assert_error_at(&diags, "PropErrors.vue", 9, 2322); // age: 'old'
    assert_error_at(&diags, "PropErrors.vue", 13, 2322); // count: 'five'
    assert_error_at(&diags, "PropErrors.vue", 15, 2322); // label: true

    // CrossComponentErrors.vue — User fields, Status, scalar
    assert_error_at(&diags, "CrossComponentErrors.vue", 6, 2322); // id: true
    assert_error_at(&diags, "CrossComponentErrors.vue", 7, 2322); // name: 42
    assert_error_at(&diags, "CrossComponentErrors.vue", 13, 2322); // status: 'unknown'
    assert_error_at(&diags, "CrossComponentErrors.vue", 16, 2322); // name: 100

    // VModelErrors.vue — ref type mismatches
    assert_error_at(&diags, "VModelErrors.vue", 5, 2345); // ref<string>(42)
    assert_error_at(&diags, "VModelErrors.vue", 8, 2345); // ref<number>(false)
    assert_error_at(&diags, "VModelErrors.vue", 11, 2345); // ref<string[]>('not-array')

    // ReactivityErrors.vue
    assert_error_at(&diags, "ReactivityErrors.vue", 6, 2345); // ref<string[]>(42)
    assert_error_at(&diags, "ReactivityErrors.vue", 10, 2322); // reactive User.id
    assert_error_at(&diags, "ReactivityErrors.vue", 19, 2322); // watch bad assignment

    // SlotErrors.vue — wrong methods
    assert_error_at(&diags, "SlotErrors.vue", 10, 2339); // active.toFixed
    assert_error_at(&diags, "SlotErrors.vue", 11, 2339); // count.toLowerCase

    // GenericErrors.vue — PaginatedResult fields
    assert_error_at(&diags, "GenericErrors.vue", 6, 2322); // items: 'not-an-array'
    assert_error_at(&diags, "GenericErrors.vue", 7, 2322); // total: 'not-a-number'
    assert_error_at(&diags, "GenericErrors.vue", 8, 2322); // page: false
    assert_error_at(&diags, "GenericErrors.vue", 17, 2353); // {name:'x'} no 'id'

    // ComposableErrors.vue
    assert_error_at(&diags, "ComposableErrors.vue", 11, 2322); // number→string
    assert_error_at(&diags, "ComposableErrors.vue", 13, 2322); // string→number

    // ── Summary ─────────────────────────────────────────────────────
    let total = diags.len();
    let vue_diags = diags.iter().filter(|d| d.file.ends_with(".vue")).count();
    let ts_diags = diags.iter().filter(|d| d.file.ends_with(".ts")).count();
    eprintln!("=== SUMMARY ===");
    eprintln!("Total diagnostics: {total}");
    eprintln!("  .vue files: {vue_diags}");
    eprintln!("  .ts files:  {ts_diags}");

    // Print per-file breakdown
    let mut file_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for d in &diags {
        *file_counts.entry(&d.file).or_insert(0) += 1;
    }
    let mut sorted: Vec<_> = file_counts.into_iter().collect();
    sorted.sort_by_key(|(f, _)| f.to_string());
    for (file, count) in &sorted {
        eprintln!("  {file}: {count} error(s)");
    }

    drop(temp_dir);
}

// ── ECRS2: Svelte admission / diagnostic attribution ────────────────────

fn svelte_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join("fixtures")
        .join("svelte_diagnostics")
}

fn resolve_rc_engine() -> Option<PathBuf> {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Lsp,
        Some(workspace_root()),
    );
    verter_tsgo_api::toolchain::discovery::resolve_blocking(&request)
        .ok()
        .map(|resolution| resolution.path)
}

/// Copy the Svelte fixture tree and junction workspace `node_modules` (needs
/// `svelte`). Skip only when that package is genuinely absent.
fn setup_svelte_temp_project() -> Option<(tempfile::TempDir, PathBuf)> {
    let node_modules_src = workspace_root().join("node_modules");
    if !node_modules_src.join("svelte").exists() {
        eprintln!("SKIP: workspace node_modules/svelte not found — run `pnpm install` first");
        return None;
    }

    let temp = tempfile::TempDir::new().expect("failed to create temp dir");
    let temp_path = temp.path().to_path_buf();
    copy_dir_recursive(&svelte_fixture_dir(), &temp_path).expect("failed to copy svelte fixture");

    let nm_dest = temp_path.join("node_modules");
    create_junction_or_symlink(&node_modules_src, &nm_dest);
    if !nm_dest.join("svelte").exists() {
        eprintln!("SKIP: failed to create node_modules junction/symlink for svelte");
        return None;
    }

    Some((temp, temp_path))
}

fn run_svelte_verter_tsc(project: &Path, tsconfig: &Path) -> Option<(Vec<Diag>, String, String)> {
    let engine = resolve_rc_engine()?;
    let bin = verter_test_support::cargo_test_binary_path!("verter-tsc");
    let output = Command::new(bin)
        .env("VERTER_TSGO_BIN", &engine)
        .arg("--noEmit")
        .arg("-p")
        .arg(tsconfig)
        .current_dir(project)
        .output()
        .expect("failed to execute verter-tsc");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    eprintln!("=== STDERR ===\n{stderr}");
    eprintln!("=== STDOUT ===\n{stdout}");
    Some((parse_diagnostics(&stdout), stdout, stderr))
}

/// ECRS2-AC1: a supported Svelte script mismatch yields TS2322 on the
/// config-project route; the clean counterpart does not.
#[test]
fn svelte_script_mismatch_is_attributed_and_clean_is_silent() {
    let Some((temp, root)) = setup_svelte_temp_project() else {
        return;
    };
    let Some((diags, stdout, stderr)) = run_svelte_verter_tsc(&root, &root.join("tsconfig.json"))
    else {
        eprintln!(
            "SKIP: rc tsgo `--api` engine not found — set VERTER_TSGO_BIN or run `pnpm install`"
        );
        drop(temp);
        return;
    };

    assert!(
        !diags.is_empty(),
        "admitted Svelte script mismatch must produce diagnostics, not an empty set \
         (admission drop). stdout={stdout}\nstderr={stderr}"
    );
    assert_has_error(&diags, "ScriptMismatch.svelte", 2322);
    assert_error_at(&diags, "ScriptMismatch.svelte", 2, 2322);
    assert_no_errors(&diags, "Clean.svelte");
    assert_no_tsx_paths(&diags);
    drop(temp);
}

/// ECRS2-AC1: an admitted Svelte template mismatch yields a meaningful
/// diagnostic attributed to the `.svelte` source.
#[test]
fn svelte_template_mismatch_is_attributed() {
    let Some((temp, root)) = setup_svelte_temp_project() else {
        return;
    };
    let Some((diags, stdout, stderr)) = run_svelte_verter_tsc(&root, &root.join("tsconfig.json"))
    else {
        eprintln!(
            "SKIP: rc tsgo `--api` engine not found — set VERTER_TSGO_BIN or run `pnpm install`"
        );
        drop(temp);
        return;
    };

    assert!(
        !diags.is_empty(),
        "admitted Svelte template mismatch must produce diagnostics. stdout={stdout}\nstderr={stderr}"
    );
    assert_error_at(&diags, "TemplateMismatch.svelte", 5, 2551);
    assert_no_tsx_paths(&diags);
    drop(temp);
}

/// ECRS2-AC2: include/exclude and nested import discriminate admission.
/// `src/excluded` and `outside/` are not admitted; nested `NestedMismatch.svelte`
/// is admitted (TS2322) — a clean nested file would not discriminate the walk.
#[test]
fn svelte_config_include_exclude_and_nested_discriminate_admission() {
    let Some((temp, root)) = setup_svelte_temp_project() else {
        return;
    };
    let Some((diags, stdout, stderr)) = run_svelte_verter_tsc(&root, &root.join("tsconfig.json"))
    else {
        eprintln!(
            "SKIP: rc tsgo `--api` engine not found — set VERTER_TSGO_BIN or run `pnpm install`"
        );
        drop(temp);
        return;
    };

    assert!(
        !diags.is_empty(),
        "config-project Svelte admission must produce diagnostics for included negatives. \
         stdout={stdout}\nstderr={stderr}"
    );
    assert_no_errors(&diags, "Skipped.svelte");
    assert_no_errors(&diags, "Outside.svelte");
    assert_no_errors(&diags, "Child.svelte");
    assert_no_errors(&diags, "NestedParent.svelte");
    assert_no_errors(&diags, "Shadowing.svelte");
    assert_error_at(&diags, "NestedMismatch.svelte", 2, 2322);
    drop(temp);
}

/// ECRS2-AC2: an extension-specific `*.ts` include does not own `.svelte`.
#[test]
fn svelte_ts_glob_does_not_admit_svelte_carriers() {
    let Some((temp, root)) = setup_svelte_temp_project() else {
        return;
    };
    let Some((diags, _stdout, stderr)) =
        run_svelte_verter_tsc(&root, &root.join("tsconfig.ts-only.json"))
    else {
        eprintln!(
            "SKIP: rc tsgo `--api` engine not found — set VERTER_TSGO_BIN or run `pnpm install`"
        );
        drop(temp);
        return;
    };

    assert!(
        stderr.contains("checking 0"),
        "a TS-only include must admit zero Svelte carriers; stderr={stderr}"
    );
    assert_no_errors(&diags, "ScriptMismatch.svelte");
    assert_no_errors(&diags, "TemplateMismatch.svelte");
    drop(temp);
}

/// ECRS2-AC2: the current CLI has no direct-file source root. A positional
/// `.svelte` path is parsed as a tsconfig, skipped as invalid JSON, checks 0
/// carriers, and exits 0. That is the unimplemented route (CLI/CLITS), named
/// here — not a typecheck of the file. Do not treat exit 0 as a successful
/// check of the source.
#[test]
fn svelte_direct_file_path_is_not_a_current_cli_contract() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let svelte = temp.path().join("ScriptMismatch.svelte");
    std::fs::write(
        &svelte,
        "<script lang=\"ts\">\n  const count: number = \"not-a-number\";\n</script>\n<p>{count}</p>\n",
    )
    .expect("write svelte");

    let bin = verter_test_support::cargo_test_binary_path!("verter-tsc");
    let output = Command::new(bin)
        .arg("--noEmit")
        .arg(&svelte)
        .current_dir(temp.path())
        .output()
        .expect("failed to execute verter-tsc");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Current CLI: a positional path is a tsconfig (`-b` / default project),
    // not a source root. A `.svelte` file is skipped as invalid JSON and the
    // run checks 0 carriers. That is the unimplemented route — named here —
    // not a typecheck of the file.
    assert!(
        stderr.contains("skipping tsconfig") || stderr.contains("checking 0"),
        "positional `.svelte` must be named as a non-source tsconfig path, not \
         silently typechecked.\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        parse_diagnostics(&stdout)
            .iter()
            .all(|d| !d.file.ends_with("ScriptMismatch.svelte")),
        "positional `.svelte` must not be treated as an admitted source root: {stdout}"
    );
    assert!(
        output.status.success(),
        "current CLI names this unimplemented route by skipping the file as a \
         tsconfig and exiting 0; a non-zero exit would be a different contract.\n\
         stdout={stdout}\nstderr={stderr}"
    );
    drop(temp);
}

/// ECRS2: admitting a Svelte carrier must not clobber user `paths`/`baseUrl`.
/// Mixed Vue+Svelte files importing through `@/` must not report TS2307.
#[test]
fn svelte_user_paths_aliases_survive_svelte_admission() {
    let Some((temp, root)) = setup_svelte_temp_project() else {
        return;
    };
    let Some((diags, stdout, stderr)) = run_svelte_verter_tsc(&root, &root.join("tsconfig.json"))
    else {
        eprintln!(
            "SKIP: rc tsgo `--api` engine not found — set VERTER_TSGO_BIN or run `pnpm install`"
        );
        drop(temp);
        return;
    };

    let alias_misses: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.ts_code == 2307
                && (d.file.ends_with("UseSvelte.svelte") || d.file.ends_with("UseVue.vue"))
        })
        .collect();
    assert!(
        alias_misses.is_empty(),
        "admitting Svelte must not drop user paths aliases (TS2307 on @/util). \
         stdout={stdout}\nstderr={stderr}\nmisses={alias_misses:#?}"
    );
    assert_error_at(&diags, "ScriptMismatch.svelte", 2, 2322);
    assert_no_errors(&diags, "UseSvelte.svelte");
    assert_no_errors(&diags, "UseVue.vue");
    drop(temp);
}

/// Copy only `Clean.svelte` into a temp project. `hoist_node_modules` places
/// the svelte junction at the parent of the project (workspace-hoist layout)
/// so the project root has no physical `node_modules`.
fn setup_svelte_only_project(hoist_node_modules: bool) -> Option<(tempfile::TempDir, PathBuf)> {
    let node_modules_src = workspace_root().join("node_modules");
    if !node_modules_src.join("svelte").exists() {
        eprintln!("SKIP: workspace node_modules/svelte not found — run `pnpm install` first");
        return None;
    }

    let temp = tempfile::TempDir::new().expect("failed to create temp dir");
    let project = if hoist_node_modules {
        let parent_nm = temp.path().join("node_modules");
        create_junction_or_symlink(&node_modules_src, &parent_nm);
        if !parent_nm.join("svelte").exists() {
            eprintln!("SKIP: failed to create parent node_modules junction/symlink for svelte");
            return None;
        }
        let project = temp.path().join("packages").join("web");
        std::fs::create_dir_all(&project).expect("packages/web");
        project
    } else {
        let nm_dest = temp.path().join("node_modules");
        create_junction_or_symlink(&node_modules_src, &nm_dest);
        if !nm_dest.join("svelte").exists() {
            eprintln!("SKIP: failed to create node_modules junction/symlink for svelte");
            return None;
        }
        temp.path().to_path_buf()
    };

    std::fs::create_dir_all(project.join("src")).expect("src");
    std::fs::copy(
        svelte_fixture_dir().join("src").join("Clean.svelte"),
        project.join("src").join("Clean.svelte"),
    )
    .expect("copy Clean.svelte");
    std::fs::write(
        project.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "strict": true,
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "skipLibCheck": true
  },
  "include": ["src"]
}
"#,
    )
    .expect("write tsconfig");
    Some((temp, project))
}

fn collect_emitted_dts(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "ts")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".d.ts"))
            {
                out.push(path);
            }
        }
    }
    walk(dir, &mut out);
    out
}

/// Overlay `directoryExists` must imply project-root `node_modules` so a
/// hoisted layout still resolves `@verter/svelte-jsx` (no TS2875 on a clean
/// Svelte file).
#[test]
fn svelte_jsx_runtime_resolves_when_node_modules_is_hoisted() {
    let Some((temp, root)) = setup_svelte_only_project(true) else {
        return;
    };
    assert!(
        !root.join("node_modules").exists(),
        "hoisted layout: project root must not have a physical node_modules"
    );
    let Some((diags, stdout, stderr)) = run_svelte_verter_tsc(&root, &root.join("tsconfig.json"))
    else {
        eprintln!(
            "SKIP: rc tsgo `--api` engine not found — set VERTER_TSGO_BIN or run `pnpm install`"
        );
        drop(temp);
        return;
    };

    assert!(
        stderr.contains("checking 1"),
        "Clean.svelte must be admitted so the overlay lookup runs. \
         stdout={stdout}\nstderr={stderr}"
    );
    assert!(
        diags.iter().all(|d| d.ts_code != 2875 && d.ts_code != 7026),
        "hoisted layout must resolve @verter/svelte-jsx (no TS2875/TS7026). \
         stdout={stdout}\nstderr={stderr}\ndiags={diags:#?}"
    );
    assert_no_errors(&diags, "Clean.svelte");
    drop(temp);
}

/// ECRS2-AC2: `--declaration` on admitted Svelte is unimplemented (CLI/CLITS).
/// Name it on stderr; do not emit `.svelte.d.ts`. Svelte-only so the Vue
/// declaration engine is not required.
#[test]
fn svelte_declaration_is_named_unimplemented_at_cli() {
    let Some((temp, root)) = setup_svelte_only_project(false) else {
        return;
    };
    let Some(engine) = resolve_rc_engine() else {
        eprintln!(
            "SKIP: rc tsgo `--api` engine not found — set VERTER_TSGO_BIN or run `pnpm install`"
        );
        drop(temp);
        return;
    };

    let out_dir = root.join("out");
    std::fs::create_dir_all(&out_dir).expect("declarationDir");
    let bin = verter_test_support::cargo_test_binary_path!("verter-tsc");
    let output = Command::new(bin)
        .env("VERTER_TSGO_BIN", &engine)
        .arg("-p")
        .arg(root.join("tsconfig.json"))
        .arg("--declaration")
        .arg("--emitDeclarationOnly")
        .arg("--declarationDir")
        .arg(&out_dir)
        .current_dir(&root)
        .output()
        .expect("failed to execute verter-tsc");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("=== STDERR ===\n{stderr}");
    eprintln!("=== STDOUT ===\n{stdout}");

    assert!(
        stderr.contains("--declaration does not emit Svelte carriers yet")
            && stderr.contains("skipped 1 file"),
        "unimplemented Svelte --declaration must be named on stderr. \
         stdout={stdout}\nstderr={stderr}"
    );
    let emitted = collect_emitted_dts(&out_dir);
    assert!(
        emitted.iter().all(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            !name.contains(".svelte")
        }),
        "Svelte --declaration must not emit a .svelte.d.ts: {emitted:?}"
    );
    drop(temp);
}

/// Svelte admission must not skip `run_declaration_stage` when `ts_files`
/// remain. A project with Clean.svelte + util.ts still emits util.d.ts.
#[test]
fn svelte_project_still_emits_typescript_declarations() {
    let Some((temp, root)) = setup_svelte_only_project(false) else {
        return;
    };
    std::fs::copy(
        svelte_fixture_dir().join("src").join("util.ts"),
        root.join("src").join("util.ts"),
    )
    .expect("copy util.ts");
    let Some(engine) = resolve_rc_engine() else {
        eprintln!(
            "SKIP: rc tsgo `--api` engine not found — set VERTER_TSGO_BIN or run `pnpm install`"
        );
        drop(temp);
        return;
    };

    let out_dir = root.join("out");
    std::fs::create_dir_all(&out_dir).expect("declarationDir");
    let bin = verter_test_support::cargo_test_binary_path!("verter-tsc");
    let output = Command::new(bin)
        .env("VERTER_TSGO_BIN", &engine)
        .arg("-p")
        .arg(root.join("tsconfig.json"))
        .arg("--declaration")
        .arg("--emitDeclarationOnly")
        .arg("--declarationDir")
        .arg(&out_dir)
        .current_dir(&root)
        .output()
        .expect("failed to execute verter-tsc");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("=== STDERR ===\n{stderr}");
    eprintln!("=== STDOUT ===\n{stdout}");

    assert!(
        stderr.contains("--declaration does not emit Svelte carriers yet")
            && stderr.contains("skipped 1 file"),
        "Svelte skip notice must remain when TS emit still runs. \
         stdout={stdout}\nstderr={stderr}"
    );
    let emitted = collect_emitted_dts(&out_dir);
    assert!(
        emitted.iter().any(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy() == "util.d.ts")
        }),
        "Svelte-only projects must still emit declarations for ts_files: {emitted:?}\n\
         stdout={stdout}\nstderr={stderr}"
    );
    assert!(
        emitted.iter().all(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().contains(".svelte"))
        }),
        "must not emit a .svelte.d.ts: {emitted:?}"
    );
    drop(temp);
}
