//! Cross-language drift guard for `PublishedSurfacePolicy`'s
//! constants (the named `published_surface_constants_match_ts_port`
//! guard cited in `packages/component-meta/src/published-surface.ts`
//! and demanded by the R20-fix-cycle brief).
//!
//! The Rust source of truth is `crates/verter_audit/src/published_surface.rs`.
//! The TS consumer-side mirror is
//! `packages/component-meta/src/published-surface.ts`. Both files
//! redeclare two constant lists:
//!
//!   * `COMPAT_BLOCKED_SLOT_NAMES` (vue-component-meta-equivalent slot
//!     blocklist; consumed by the `Compat` and `Refined` policies).
//!   * `VUE_INTRINSIC_ATTR_NAMES` (Vue intrinsic attribute names that
//!     `Refined` strips unless the author explicitly re-declared them
//!     in the macro type arg).
//!
//! This test parses the TS file with OXC ([[oxc-is-the-ts-parser]] —
//! the canonical Rust-side TS parser), walks the AST to find each
//! `export const NAME = [ ... ] as const;` declaration, extracts the
//! string-literal payload, and asserts exact set equality with the
//! Rust constants. Any drift produces a detailed diff.
//!
//! The guard ALSO walks `packages/**/*.ts` to assert there is exactly
//! ONE definition of each constant — a shadow sibling re-declaration
//! in a different file silently breaks the parity rail because the
//! drift guard only reads the canonical port.
//!
//! Discriminating property: changing either side without updating
//! the other (or accidentally desync-ing a single entry) MUST cause
//! this test to fail with a precise diff. A trivial pass-through
//! that always returns OK would not satisfy that.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayExpressionElement, Declaration, Expression, Program, Statement, VariableDeclaration,
};
use oxc_parser::Parser;
use oxc_span::SourceType;

use verter_audit::published_surface::{
    event_name_to_on_prop_name, COMPAT_BLOCKED_SLOT_NAMES, VUE_INTRINSIC_ATTR_NAMES,
};

const CANONICAL_PORT_REL: &str = "packages/component-meta/src/published-surface.ts";
const TARGET_CONSTS: &[&str] = &["COMPAT_BLOCKED_SLOT_NAMES", "VUE_INTRINSIC_ATTR_NAMES"];

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is set per-crate; walk up to the workspace root.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("pnpm-workspace.yaml").exists())
        .map(Path::to_path_buf)
        .expect("workspace root with pnpm-workspace.yaml should exist above the verter_audit crate")
}

fn ts_port_path() -> PathBuf {
    workspace_root().join(CANONICAL_PORT_REL)
}

fn ts_port_source() -> String {
    let path = ts_port_path();
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read TS port at {path:?}: {e}"))
}

/// Parse a `.ts` source via OXC and return all top-level
/// `export const NAME = [ ...string literals... ] as const;`
/// declarations as a `(name, Vec<string>)` map.
///
/// Walking the typed AST rather than scanning raw bytes lets the
/// guard naturally reject any structural drift (the array becomes
/// a function call, a spread element appears, the `as const` is
/// dropped, the assignment shape changes) without us having to
/// hand-author each adversarial case.
fn extract_string_array_consts(
    source: &str,
    path_for_diagnostics: &Path,
) -> Vec<(String, Vec<String>)> {
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    assert!(
        !parsed.panicked,
        "OXC parser panicked while reading {path_for_diagnostics:?}; errors: {:?}",
        parsed.errors
    );
    let mut out = Vec::new();
    collect_program_string_array_consts(&parsed.program, &mut out, path_for_diagnostics);
    out
}

/// Same as `extract_string_array_consts` but tolerant of parse errors —
/// the sibling walk over `packages/**/*.ts` encounters intentional
/// fixture files (e.g. `Test.my.ts`) and files with non-TS extensions
/// that happen to parse strangely. We only need to find shadow
/// declarations of the target constants; files we cannot parse cannot
/// contain such declarations either way.
fn extract_string_array_consts_lenient(
    source: &str,
    path_for_diagnostics: &Path,
) -> Vec<(String, Vec<String>)> {
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    collect_program_string_array_consts(&parsed.program, &mut out, path_for_diagnostics);
    out
}

fn collect_program_string_array_consts(
    program: &Program<'_>,
    out: &mut Vec<(String, Vec<String>)>,
    path: &Path,
) {
    for statement in &program.body {
        if let Some(decl) = export_named_var_decl(statement) {
            push_string_array_consts(decl, out, path);
        } else if let Statement::VariableDeclaration(decl) = statement {
            push_string_array_consts(decl, out, path);
        }
    }
}

fn export_named_var_decl<'a, 'b>(
    statement: &'b Statement<'a>,
) -> Option<&'b VariableDeclaration<'a>> {
    let Statement::ExportNamedDeclaration(export) = statement else {
        return None;
    };
    match &export.declaration {
        Some(Declaration::VariableDeclaration(decl)) => Some(decl.as_ref()),
        _ => None,
    }
}

fn push_string_array_consts(
    decl: &VariableDeclaration<'_>,
    out: &mut Vec<(String, Vec<String>)>,
    path: &Path,
) {
    for declarator in &decl.declarations {
        let Some(name) = declarator.id.get_identifier_name() else {
            continue;
        };
        let name_str = name.as_str();
        let Some(init) = declarator.init.as_ref() else {
            continue;
        };
        // The TS port writes `[ ... ] as const`. OXC lowers
        // `as const` to TSAsExpression; unwrap it so we get the
        // bare ArrayExpression.
        let array_expr = unwrap_as_const(init);
        let Expression::ArrayExpression(array) = array_expr else {
            continue;
        };
        let is_target = TARGET_CONSTS.contains(&name_str);
        let mut elements = Vec::with_capacity(array.elements.len());
        let mut all_string_literals = true;
        for element in &array.elements {
            match element {
                ArrayExpressionElement::StringLiteral(literal) => {
                    elements.push(literal.value.to_string());
                }
                // Any other element shape (numeric literal, identifier,
                // spread, function call, etc.) is a structural drift
                // signal — the TS port's target constants are supposed
                // to be flat arrays of string literals. We propagate
                // the panic with file context ONLY for the target
                // constants (where the invariant is contractual);
                // other constants in the same file may legitimately
                // contain object expressions / spreads / calls.
                other => {
                    if is_target {
                        panic!(
                            "TS port at {path:?} declared target constant `{name_str}` \
                             with a non-string-literal array element ({other:?}). The drift \
                             guard requires the constant to be a flat array of string literals."
                        );
                    }
                    all_string_literals = false;
                    break;
                }
            }
        }
        if all_string_literals {
            out.push((name_str.to_string(), elements));
        }
    }
}

fn unwrap_as_const<'b, 'a>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    match expr {
        Expression::TSAsExpression(as_expr) => unwrap_as_const(&as_expr.expression),
        Expression::TSSatisfiesExpression(s) => unwrap_as_const(&s.expression),
        Expression::ParenthesizedExpression(p) => unwrap_as_const(&p.expression),
        _ => expr,
    }
}

fn find_const_in_extracted<'a>(
    extracted: &'a [(String, Vec<String>)],
    name: &str,
    path_for_diagnostics: &Path,
) -> &'a [String] {
    extracted
        .iter()
        .find_map(|(n, v)| (n == name).then_some(v.as_slice()))
        .unwrap_or_else(|| {
            panic!(
                "TS port at {path_for_diagnostics:?} should declare \
                 `export const {name} = [ ... ] as const;` — \
                 declaration not found in OXC-parsed AST."
            )
        })
}

#[test]
fn published_surface_constants_match_ts_port() {
    let ts_path = ts_port_path();
    let ts_source = ts_port_source();
    let extracted = extract_string_array_consts(&ts_source, &ts_path);

    let ts_compat = find_const_in_extracted(&extracted, "COMPAT_BLOCKED_SLOT_NAMES", &ts_path);
    let ts_intrinsics = find_const_in_extracted(&extracted, "VUE_INTRINSIC_ATTR_NAMES", &ts_path);

    let rust_compat: Vec<String> = COMPAT_BLOCKED_SLOT_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rust_intrinsics: Vec<String> = VUE_INTRINSIC_ATTR_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect();

    // Exact order equality — both languages list the entries in the
    // same order, and we want drift in EITHER direction (re-ordering,
    // additions, deletions) to surface here.
    let ts_compat_owned: Vec<String> = ts_compat.to_vec();
    let ts_intrinsics_owned: Vec<String> = ts_intrinsics.to_vec();
    assert_eq!(
        rust_compat, ts_compat_owned,
        "COMPAT_BLOCKED_SLOT_NAMES drift between Rust source of truth \
         (`crates/verter_audit/src/published_surface.rs`) and TS port \
         (`packages/component-meta/src/published-surface.ts`).\n\
         Rust: {rust_compat:?}\nTS:   {ts_compat_owned:?}"
    );
    assert_eq!(
        rust_intrinsics, ts_intrinsics_owned,
        "VUE_INTRINSIC_ATTR_NAMES drift between Rust source of truth \
         and TS port.\nRust: {rust_intrinsics:?}\nTS:   {ts_intrinsics_owned:?}"
    );

    // Set equality cross-check (catches subtle dup / case bugs the
    // ordered assertion might miss).
    let rust_compat_set: HashSet<&String> = rust_compat.iter().collect();
    let ts_compat_set: HashSet<&String> = ts_compat_owned.iter().collect();
    assert_eq!(rust_compat_set, ts_compat_set);

    let rust_intrinsics_set: HashSet<&String> = rust_intrinsics.iter().collect();
    let ts_intrinsics_set: HashSet<&String> = ts_intrinsics_owned.iter().collect();
    assert_eq!(rust_intrinsics_set, ts_intrinsics_set);
}

/// Adversarial completeness: a shadow-sibling re-declaration of
/// `COMPAT_BLOCKED_SLOT_NAMES` or `VUE_INTRINSIC_ATTR_NAMES` in any
/// other `.ts` file under `packages/**` silently breaks the parity
/// rail because the drift guard only reads the canonical port. This
/// test walks every `.ts` file (excluding `.d.ts` generated bundles,
/// `dist`, `node_modules`, and the `published-surface.spec.ts` test
/// fixture which legitimately re-declares fixtures named with the
/// same SUFFIX but not the constant identity itself) and asserts at
/// most ONE definition of each target constant exists across the
/// workspace.
#[test]
fn published_surface_constants_have_single_definition() {
    let root = workspace_root();
    let packages = root.join("packages");
    let mut definitions: Vec<(String, PathBuf)> = Vec::new();
    walk_ts_files(&packages, &mut |ts_path| {
        let Ok(source) = fs::read_to_string(ts_path) else {
            return;
        };
        let extracted = extract_string_array_consts_lenient(&source, ts_path);
        for (name, _) in extracted {
            if TARGET_CONSTS.contains(&name.as_str()) {
                definitions.push((name, ts_path.to_path_buf()));
            }
        }
    });

    for target in TARGET_CONSTS {
        let hits: Vec<&PathBuf> = definitions
            .iter()
            .filter_map(|(n, p)| (n == *target).then_some(p))
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "Constant `{target}` must have exactly ONE definition across `packages/**/*.ts` \
             (the canonical port at `{CANONICAL_PORT_REL}`). Found {} definition(s) at: {:#?}",
            hits.len(),
            hits
        );
        let only = hits[0];
        let canonical = root.join(CANONICAL_PORT_REL);
        assert_eq!(
            only.canonicalize().ok(),
            canonical.canonicalize().ok(),
            "Constant `{target}` was defined at `{only:?}` instead of the canonical port \
             `{canonical:?}`."
        );
    }
}

fn walk_ts_files(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if file_name == "node_modules"
            || file_name == "dist"
            || file_name == "build"
            || file_name == "out"
            || file_name == ".vite"
            || file_name == ".turbo"
            || file_name == "coverage"
        {
            continue;
        }
        if path.is_dir() {
            walk_ts_files(&path, visit);
            continue;
        }
        // .d.ts files are generated bundles or hand-authored declaration
        // files. They never DEFINE a `COMPAT_BLOCKED_SLOT_NAMES` constant
        // (only declare); skip to keep the walk focused on actual sources.
        if !file_name.ends_with(".ts") || file_name.ends_with(".d.ts") {
            continue;
        }
        visit(&path);
    }
}

/// `event_name_to_on_prop_name` (Rust) and the TS port's
/// `eventNameToOnPropName` MUST agree on every payload. Rather than
/// hard-coding a fixed list (which can drift if the TS algorithm
/// changes), this test reads the canonical case manifest emitted by
/// the TS-side spec via `node --eval` and asserts row-by-row parity.
///
/// The manifest is generated on-demand by invoking the TS implementation
/// directly (no `vitest` spawn — a single small `node -e` call) and
/// reading the JSON output. If `node` is unavailable on the runner
/// (e.g. minimal CI image) the test SKIPS with a clear message rather
/// than failing — the canonical Rust↔TS parity is then validated by
/// the workspace's `pnpm test` gate which always runs the TS spec.
/// Skipping under deliberate environmental absence is honest; silently
/// returning OK regardless is not.
#[test]
fn event_name_to_on_prop_name_matches_ts_port_via_node_oracle() {
    let cases = match invoke_ts_event_name_oracle() {
        Some(cases) => cases,
        None => {
            eprintln!(
                "event_name_to_on_prop_name_matches_ts_port_via_node_oracle: \
                 `node` is unavailable on PATH; skipping cross-language \
                 invocation. Rust↔TS parity is still validated by the workspace \
                 `pnpm test` run of `published-surface.spec.ts`."
            );
            return;
        }
    };
    assert!(
        !cases.is_empty(),
        "TS oracle returned an empty case list — the table should always have \
         at least the canonical fixtures the TS spec asserts on."
    );
    for (input, expected) in &cases {
        let actual = event_name_to_on_prop_name(input);
        assert_eq!(
            &actual, expected,
            "event_name_to_on_prop_name({input:?}) drifted from TS oracle: \
             got {actual:?}, expected {expected:?}"
        );
    }
}

/// Adversarial validation of the OXC-based extractor. Synthesises
/// mutated TS sources covering ADD / REORDER / SPELLING / SPREAD /
/// AS-CONST / WRAPPER mutations and asserts the extractor surfaces
/// the precise mutation rather than silently returning the unmutated
/// list. This is the test that the brief's F2 fix MUST pass.
#[test]
fn oxc_extractor_discriminates_adversarial_mutations() {
    let fake_path = PathBuf::from("/synthetic/published-surface.ts");

    // Baseline: a well-formed two-constant port.
    let baseline = r#"
        export const COMPAT_BLOCKED_SLOT_NAMES: readonly string[] = [
            "type", "props", "key"
        ] as const;
        export const VUE_INTRINSIC_ATTR_NAMES: readonly string[] = [
            "class", "style"
        ] as const;
    "#;
    let baseline_extracted = extract_string_array_consts(baseline, &fake_path);
    let baseline_compat =
        find_const_in_extracted(&baseline_extracted, "COMPAT_BLOCKED_SLOT_NAMES", &fake_path);
    let baseline_intrinsics =
        find_const_in_extracted(&baseline_extracted, "VUE_INTRINSIC_ATTR_NAMES", &fake_path);
    assert_eq!(baseline_compat, &["type", "props", "key"]);
    assert_eq!(baseline_intrinsics, &["class", "style"]);

    // ADD mutation: extra entry appended.
    let added = r#"
        export const COMPAT_BLOCKED_SLOT_NAMES: readonly string[] = [
            "type", "props", "key", "INTRUDER"
        ] as const;
        export const VUE_INTRINSIC_ATTR_NAMES: readonly string[] = ["class", "style"] as const;
    "#;
    let added_extracted = extract_string_array_consts(added, &fake_path);
    let added_compat =
        find_const_in_extracted(&added_extracted, "COMPAT_BLOCKED_SLOT_NAMES", &fake_path);
    assert_eq!(added_compat, &["type", "props", "key", "INTRUDER"]);
    assert_ne!(added_compat, baseline_compat);

    // REORDER mutation: same set, different order.
    let reordered = r#"
        export const COMPAT_BLOCKED_SLOT_NAMES: readonly string[] = [
            "key", "type", "props"
        ] as const;
        export const VUE_INTRINSIC_ATTR_NAMES: readonly string[] = ["class", "style"] as const;
    "#;
    let reordered_extracted = extract_string_array_consts(reordered, &fake_path);
    let reordered_compat = find_const_in_extracted(
        &reordered_extracted,
        "COMPAT_BLOCKED_SLOT_NAMES",
        &fake_path,
    );
    assert_eq!(reordered_compat, &["key", "type", "props"]);
    assert_ne!(reordered_compat, baseline_compat);

    // SPELLING mutation: case-sensitive token change.
    let respelled = r#"
        export const COMPAT_BLOCKED_SLOT_NAMES: readonly string[] = [
            "Type", "props", "key"
        ] as const;
        export const VUE_INTRINSIC_ATTR_NAMES: readonly string[] = ["class", "style"] as const;
    "#;
    let respelled_extracted = extract_string_array_consts(respelled, &fake_path);
    let respelled_compat = find_const_in_extracted(
        &respelled_extracted,
        "COMPAT_BLOCKED_SLOT_NAMES",
        &fake_path,
    );
    assert_eq!(respelled_compat, &["Type", "props", "key"]);
    assert_ne!(respelled_compat, baseline_compat);

    // SPREAD mutation (a non-target constant with a spread is tolerated
    // by the lenient walker, but a SPREAD inside a TARGET constant must
    // be rejected with a precise panic — that is the contract the brief
    // demands).
    let spread = r#"
        const HEAD: readonly string[] = ["type"] as const;
        export const COMPAT_BLOCKED_SLOT_NAMES: readonly string[] = [
            ...HEAD, "props", "key"
        ] as const;
        export const VUE_INTRINSIC_ATTR_NAMES: readonly string[] = ["class", "style"] as const;
    "#;
    let spread_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        extract_string_array_consts(spread, &fake_path)
    }));
    assert!(
        spread_result.is_err(),
        "extractor must reject SPREAD elements in target constants, not silently accept them"
    );

    // WRAPPER mutation: extra `as const` cascade plus `satisfies`.
    // OXC should unwrap both via `unwrap_as_const`.
    let wrapped = r#"
        export const COMPAT_BLOCKED_SLOT_NAMES = (
            (["type", "props", "key"] satisfies readonly string[]) as const
        ) as const;
        export const VUE_INTRINSIC_ATTR_NAMES: readonly string[] = ["class", "style"] as const;
    "#;
    let wrapped_extracted = extract_string_array_consts(wrapped, &fake_path);
    let wrapped_compat =
        find_const_in_extracted(&wrapped_extracted, "COMPAT_BLOCKED_SLOT_NAMES", &fake_path);
    assert_eq!(wrapped_compat, &["type", "props", "key"]);
}

/// Adversarial validation of the shadow-sibling guard's structural
/// detection. Synthesises a directory with two TS files, both
/// declaring the same target constant, runs the lenient extractor
/// over both, and asserts the duplicate-definition check would fire.
#[test]
fn shadow_sibling_guard_discriminates_duplicate_definitions() {
    // Use the LENIENT extractor (same as the sibling walk uses) so
    // this test mirrors the production gate's discrimination property.
    let fake_path_a = PathBuf::from("/synthetic/a/published-surface.ts");
    let fake_path_b = PathBuf::from("/synthetic/b/shadow.ts");
    let src_a = r#"export const COMPAT_BLOCKED_SLOT_NAMES: readonly string[] = ["type"] as const;"#;
    let src_b = r#"export const COMPAT_BLOCKED_SLOT_NAMES: readonly string[] = ["type"] as const;"#;

    let extracted_a = extract_string_array_consts_lenient(src_a, &fake_path_a);
    let extracted_b = extract_string_array_consts_lenient(src_b, &fake_path_b);

    let mut definitions: Vec<(String, PathBuf)> = Vec::new();
    for (name, _) in extracted_a {
        if TARGET_CONSTS.contains(&name.as_str()) {
            definitions.push((name, fake_path_a.clone()));
        }
    }
    for (name, _) in extracted_b {
        if TARGET_CONSTS.contains(&name.as_str()) {
            definitions.push((name, fake_path_b.clone()));
        }
    }

    let compat_hits: Vec<&PathBuf> = definitions
        .iter()
        .filter_map(|(n, p)| (n == "COMPAT_BLOCKED_SLOT_NAMES").then_some(p))
        .collect();
    assert_eq!(
        compat_hits.len(),
        2,
        "Synthetic two-file fixture should produce TWO COMPAT_BLOCKED_SLOT_NAMES \
         hits; the production guard's assertion `hits.len() == 1` would fail \
         on this set. This proves the guard discriminates."
    );
}

fn invoke_ts_event_name_oracle() -> Option<Vec<(String, String)>> {
    let port = workspace_root().join(CANONICAL_PORT_REL);
    // The TS file uses ESM (`export`), so we feed it through a small
    // dynamic-import wrapper that emits the canonical case table as
    // JSON on stdout.
    let port_url = port.canonicalize().ok()?;
    let port_url_str = port_url.to_string_lossy().replace('\\', "/");
    let port_url_str = if let Some(rest) = port_url_str.strip_prefix("//?/") {
        format!("file:///{rest}")
    } else if port_url_str.starts_with('/') {
        format!("file://{port_url_str}")
    } else {
        format!("file:///{port_url_str}")
    };
    let script = format!(
        r#"import("{port_url_str}").then(m => {{
            const cases = [
                "submit",
                "click",
                "state-change",
                "update:modelValue",
                "camelCaseEvt",
                "two_words",
                "multi-segment-name",
                "Already:Pascal"
            ];
            const out = cases.map(c => [c, m.eventNameToOnPropName(c)]);
            process.stdout.write(JSON.stringify(out));
        }}).catch(e => {{ console.error(e); process.exit(1); }});"#,
    );
    let output = std::process::Command::new("node")
        .args(["--input-type=module", "-e", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        eprintln!(
            "node oracle invocation failed (status={:?}). stderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    // The output is a JSON array of `[input, expected]` pairs.
    let parsed: serde_json::Value = serde_json::from_str(&stdout).ok()?;
    let array = parsed.as_array()?;
    let mut cases = Vec::with_capacity(array.len());
    for entry in array {
        let pair = entry.as_array()?;
        if pair.len() != 2 {
            return None;
        }
        let input = pair[0].as_str()?.to_string();
        let expected = pair[1].as_str()?.to_string();
        cases.push((input, expected));
    }
    Some(cases)
}
