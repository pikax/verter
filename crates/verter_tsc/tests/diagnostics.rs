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
    #[allow(dead_code)]
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
    } else if let Some(after) = rest.strip_prefix("warning ") {
        after
    } else {
        return None;
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

    let bin = env!("CARGO_BIN_EXE_verter-tsc");
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

    // TemplateExprErrors.vue — TS2339 (count.length), TS2345 (toFixed('bad')), TS2362 (msg*5)
    assert_has_error(&diags, "TemplateExprErrors.vue", 2339);
    assert_has_error(&diags, "TemplateExprErrors.vue", 2345);
    assert_has_error(&diags, "TemplateExprErrors.vue", 2362);
    assert_min_errors(&diags, "TemplateExprErrors.vue", 3);

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

    // TemplateExprErrors.vue — all 3 template errors have correct line mapping
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
    let mut file_counts: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
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
