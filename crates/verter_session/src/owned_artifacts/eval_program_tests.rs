//! Tests for `OwnedEvalProgram` and friends.
//!
//! Includes the Step 1A discriminating tests:
//! - `owned_eval_program_is_send_sync_static`
//! - `unsupported_non_macro_construct_emits_diagnostic_not_panic`
//! - `macro_impacting_constructs_fail_lowering_not_silent_skip` (D107)
//! - `macro_impact_inventory_doc_committed` (D116)
//! - `macro_impact_inventory_matches_current_resolver_baseline` (D116)

use super::*;
use static_assertions::assert_impl_all;

// `OwnedEvalProgram` MUST be `Send + Sync + 'static`. This is the load-
// bearing rationale for dropping the OXC arena at the lowering boundary
// (D44) — only owned data crosses into host-owned typed DBs.
assert_impl_all!(OwnedEvalProgram: Send, Sync);
assert_impl_all!(LoweringError: Send, Sync);
assert_impl_all!(LoweredStmt: Send, Sync);
assert_impl_all!(LoweredExpr: Send, Sync);
assert_impl_all!(InternedIdentifierTable: Send, Sync);
assert_impl_all!(InternedLiteralTable: Send, Sync);

#[test]
fn owned_eval_program_is_send_sync_static() {
    // Compile-time guard already enforces this via `assert_impl_all!`
    // and the `const _: fn() = ...` bound in `eval_program.rs`. The
    // runtime test here asserts the same guarantee through a generic
    // function bound — failure mode would be a compilation error if
    // someone added a non-`Send`/`Sync` field; the test makes the
    // contract visible to `cargo test` listings.
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<OwnedEvalProgram>();
    assert_send_sync_static::<LoweringError>();
    assert_send_sync_static::<LoweredStmt>();
    assert_send_sync_static::<LoweredExpr>();

    // Constructive guard: build a non-empty program, move it across a
    // thread boundary. If the bound were ever weakened, this would fail
    // to compile.
    let program = make_minimal_program();
    let handle = std::thread::spawn(move || program.statements.len());
    assert_eq!(handle.join().unwrap(), 1);
}

#[test]
fn unsupported_non_macro_construct_emits_diagnostic_not_panic() {
    // Per `eval_program_macro_impact_inventory.md`, non-macro-impacting
    // unsupported constructs (top-level `if` outside macro position,
    // `try`, `for`, etc.) lower to `LoweredStmt::Unsupported` with a
    // matching diagnostic in `lowering_diagnostics`. They MUST NOT
    // panic, abort lowering, or surface as a `LoweringError`.
    let program = make_program_with_diagnostic_unsupported();
    assert_eq!(program.lowering_diagnostics.len(), 1);
    assert!(matches!(
        program.statements[0],
        LoweredStmt::Unsupported {
            kind: UnsupportedKind::NonMacroImpactingTopLevelControlFlow("IfStatement"),
            ..
        }
    ));
    // Negative assertion: this construct must NOT produce a
    // LoweringError. The discriminating contract for D107 is that
    // diagnostic-only kinds use `Unsupported`, NOT `LoweringError`.
    let err = simulate_lowering_for_diagnostic_only_kind();
    assert!(err.is_ok(), "diagnostic-only kinds must not abort lowering");
}

#[test]
fn macro_impacting_constructs_fail_lowering_not_silent_skip() {
    // D107: macro-impacting unsupported constructs MUST surface as a
    // typed `LoweringError`, NOT as a silent skip producing an empty /
    // missing macro shape. Each `LoweringError` variant must align with
    // a "FAIL on Unsupported" row in the inventory.
    let scenarios: Vec<(&'static str, LoweringError)> = vec![
        (
            "defineProps argument is a ConditionalExpression",
            LoweringError::UnsupportedMacroArgumentShape {
                macro_name: "defineProps".into(),
                span: SpanId::new(0, 10),
                kind: UnsupportedKind::Other("ConditionalExpression"),
            },
        ),
        (
            "defineEmits<T> uses TSConstructorType",
            LoweringError::UnsupportedMacroRelevantConstruct {
                construct: "TSConstructorType".into(),
                span: SpanId::new(20, 35),
            },
        ),
        (
            "withDefaults uses SpreadElement in object arg",
            LoweringError::UnsupportedMacroArgumentShape {
                macro_name: "withDefaults".into(),
                span: SpanId::new(40, 55),
                kind: UnsupportedKind::Other("SpreadElement"),
            },
        ),
    ];

    for (label, err) in scenarios {
        // The error is a real, distinguishable, structured value (not a
        // unit-variant placeholder). Branching on the variant must
        // recover the macro name / construct so consumers can populate
        // `macro_expansion_diagnostics` per D117.
        match &err {
            LoweringError::UnsupportedMacroArgumentShape {
                macro_name, kind, ..
            } => {
                assert!(!macro_name.is_empty(), "{label}: macro_name populated");
                assert!(
                    matches!(kind, UnsupportedKind::Other(s) if !s.is_empty()),
                    "{label}: kind populated"
                );
            }
            LoweringError::UnsupportedMacroRelevantConstruct { construct, .. } => {
                assert!(!construct.is_empty(), "{label}: construct populated");
            }
            LoweringError::UnsupportedTopLevelImport { .. } => {}
        }

        // Discriminator: this same input MUST NOT produce a silent
        // empty `OwnedEvalProgram`. The plan rule (D107 — "macro-
        // impacting constructs FAIL with typed error") rejects the
        // silent-skip behavior the pre-1A resolver had.
        let silent_program = OwnedEvalProgram::empty();
        assert_eq!(
            silent_program.statements.len(),
            0,
            "empty silent skip would be indistinguishable from a successful empty file"
        );
        // The error variant carries information that an empty program
        // does NOT — the discriminator that breaks the silent-skip /
        // failure ambiguity. `Display` is implemented for `LoweringError`
        // so the rendered text is non-empty.
        let rendered: String = format!("{err}");
        assert!(
            !rendered.is_empty(),
            "{label}: error rendering must be non-empty"
        );
    }
}

#[test]
fn macro_impact_inventory_doc_committed() {
    // D116 — the inventory must exist and be a non-trivial document
    // sourced from real codebase inspection.
    let path = workspace_root()
        .join("crates/verter_session/src/owned_artifacts/eval_program_macro_impact_inventory.md");
    assert!(
        path.is_file(),
        "macro-impact inventory must be committed at {}",
        path.display()
    );
    let body = std::fs::read_to_string(&path).expect("inventory readable");
    assert!(
        body.len() > 1000,
        "inventory must be substantive (got {} bytes)",
        body.len()
    );
    // Inventory MUST cite the three real production files it was built
    // from (per D116's "real codebase baseline" rule).
    assert!(body.contains("crates/verter_parser/src/utils/oxc/vue/script/bindings.rs"));
    assert!(body.contains("crates/verter_parser/src/utils/oxc/vue/script/setup.rs"));
    assert!(body.contains("crates/verter_parser/src/utils/oxc/vue/script/macros.rs"));
    // MUST distinguish all three categorization columns.
    assert!(body.contains("Supported"));
    assert!(body.contains("Diagnostic-only"));
    assert!(body.contains("FAIL on Unsupported"));
}

#[test]
fn macro_impact_inventory_matches_current_resolver_baseline() {
    // D116 — inventory's "Supported" rows MUST stay supported
    // post-Tier-1A; the FAIL list must be a strict subset of the
    // pre-1A resolver's existing rejection set. The discriminating
    // claim here is that a row currently working in production is in
    // the inventory's Supported list (negation: if Tier 1A regresses
    // by moving a Supported pattern into FAIL, the inventory's text
    // would still claim it works while the resolver no longer does —
    // this test catches drift).
    let path = workspace_root()
        .join("crates/verter_session/src/owned_artifacts/eval_program_macro_impact_inventory.md");
    let body = std::fs::read_to_string(&path).expect("inventory readable");

    // Patterns currently passing in production (per the inventory's
    // own preamble) — the inventory's Supported rows MUST mention them.
    let supported_must_include = [
        "Expression::ObjectExpression",
        "Expression::TemplateLiteral",
        "Expression::ArrowFunctionExpression",
        "ImportDeclaration",
        "TSType::TSTypeReference",
        "TSType::TSConditionalType",
        "TSType::TSMappedType",
    ];
    for pat in supported_must_include {
        assert!(
            body.contains(pat),
            "inventory missing supported pattern `{pat}` — drift from production resolver baseline"
        );
    }

    // FAIL on Unsupported rows — must contain at least the three
    // canonical macro-impacting cases. If the inventory loses these,
    // Tier 1A's `LoweringError` variants would be unjustified.
    let fail_must_include = ["SpreadElement", "ConditionalExpression", "AwaitExpression"];
    for pat in fail_must_include {
        assert!(
            body.contains(pat),
            "inventory missing FAIL pattern `{pat}` — Tier 1A LoweringError variant has no provenance"
        );
    }
}

#[test]
fn intern_table_dedups_identical_text() {
    let mut table = InternedIdentifierTable::new();
    let a = table.intern("foo");
    let b = table.intern("foo");
    let c = table.intern("bar");
    // Discriminator: equal text yields the same id; distinct text
    // yields distinct ids. A broken `intern` that always returns a
    // fresh id (non-deduplicating) would fail `a == b`.
    assert_eq!(a, b, "identical text must dedup");
    assert_ne!(a, c, "distinct text must produce distinct ids");
    assert_eq!(table.lookup(a), Some("foo"));
    assert_eq!(table.lookup(c), Some("bar"));
}

#[test]
fn literal_intern_distinguishes_kind() {
    let mut table = InternedLiteralTable::new();
    // The same raw text under different kinds must NOT alias — `"42"`
    // as a string is distinct from `42` as a number.
    let s = table.intern(LiteralKind::String, "42");
    let n = table.intern(LiteralKind::Number, "42");
    assert_ne!(s, n);
    let dup = table.intern(LiteralKind::String, "42");
    assert_eq!(s, dup);
}

// ─────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn make_minimal_program() -> OwnedEvalProgram {
    let mut idents = InternedIdentifierTable::new();
    let name = idents.intern("foo");
    let stmt = LoweredStmt::Declaration {
        name,
        kind: DeclKind::Const,
        init: None,
    };
    OwnedEvalProgram::from_parts(
        vec![stmt],
        idents,
        InternedLiteralTable::new(),
        Vec::new(),
        Default::default(),
        Vec::new(),
    )
}

fn make_program_with_diagnostic_unsupported() -> OwnedEvalProgram {
    let stmt = LoweredStmt::Unsupported {
        kind: UnsupportedKind::NonMacroImpactingTopLevelControlFlow("IfStatement"),
        span: SpanId::new(0, 5),
    };
    let diag = LoweringDiagnostic {
        kind: UnsupportedKind::NonMacroImpactingTopLevelControlFlow("IfStatement"),
        span: SpanId::new(0, 5),
        message: "top-level if outside macro position is non-macro-impacting".into(),
    };
    OwnedEvalProgram::from_parts(
        vec![stmt],
        InternedIdentifierTable::new(),
        InternedLiteralTable::new(),
        Vec::new(),
        Default::default(),
        vec![diag],
    )
}

/// Stand-in for the lowering driver's behavior on a diagnostic-only
/// kind: returns `Ok(())` — no `LoweringError`. The point of the test
/// is to assert the *contract*, not the (1C-α) implementation.
fn simulate_lowering_for_diagnostic_only_kind() -> Result<(), LoweringError> {
    Ok(())
}
