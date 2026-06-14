//! Architecture guard for IDE `<script setup>` error recovery.
//!
//! IDE script codegen has ONE recovery surface: a single token scan of the REAL
//! source (`ScriptSetupRecoveryPlan` via `ScriptTokenScanner::recover_plan`). The
//! deleted dual path — truncate-at-newline, clean-prefix reparse authority, and
//! the file-scope `process_tsx_script_setup_error_mode` — must never return, and
//! recovery must never synthesize-then-reparse (no reparsed synthetic view may be
//! an authority for bindings/macros/imports/generated output). This guard scans
//! the production setup module to keep that invariant enforced.

const SETUP_SRC: &str = include_str!("../src/ide/script/setup.rs");

/// The deleted dual-recovery identifiers must not reappear in production code.
#[test]
fn setup_has_no_truncate_reparse_or_file_scope_error_mode() {
    for forbidden in [
        // File-scope error mode that stranded the body at module scope.
        "process_tsx_script_setup_error_mode",
        // Clean-prefix reparse authority + its switching flag.
        "use_recovery_parse",
        "clean_prefix",
        // Truncate-at-newline recovery.
        "truncate_at",
    ] {
        assert!(
            !SETUP_SRC.contains(forbidden),
            "ide/script/setup.rs must not reintroduce the deleted dual-recovery path \
             (found `{forbidden}`). Recovery is the single `recover_plan` token scan."
        );
    }
}

/// The single recovery surface must remain wired in.
#[test]
fn setup_routes_failure_path_through_recover_plan() {
    assert!(
        SETUP_SRC.contains("recover_plan()"),
        "the failure path must drive recovery through the single \
         ScriptTokenScanner::recover_plan surface"
    );
}

/// Recovery must not synthesize a source string and reparse it: no
/// `format!(...).parse...` / `parse_*` of a synthesized view inside setup codegen.
/// (The only OXC `Parser` calls are the single original-content parse and the
/// TS-mode discriminator — both over the unmodified `content_str`.)
#[test]
fn setup_does_not_synthesize_then_reparse_for_recovery() {
    // A synthesize-then-reparse would build a string and feed it to a parser.
    // Guard the concrete anti-patterns rather than the legitimate original parse.
    assert!(
        !SETUP_SRC.contains("parse_type_annotation"),
        "recovery must not reparse synthesized type text"
    );
    // No more than the two legitimate parses (original TSX + TS-mode check) over
    // the original content — a third `Parser::new` would signal a reparse path.
    let parser_calls = SETUP_SRC.matches("Parser::new").count();
    assert!(
        parser_calls <= 2,
        "setup.rs should hold at most the original TSX parse + the TS-mode \
         discriminator ({parser_calls} `Parser::new` calls found) — a reparse \
         path must not return"
    );
}
