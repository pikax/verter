//! Architecture guard — the `SemanticGraphStore` query-node signature
//! builder is **provenance-pure**.
//!
//! `semantic_query_memo::semantic_graph_read_set_signature` builds the
//! self-version-rooted [`crate::fact_signature_helpers::ReadSetSignature`]
//! carrier for a `SemanticGraphStore` query-node memo entry. It MUST
//! root the entry on the content identity the cold build OBSERVED at
//! value-compute time — never a current-content re-read at
//! signature-build time. A current-content re-read inside a signature
//! builder reopens the publish race: an `upsert` landing between a
//! producer's value-compute and signature-build would root a stale
//! value on post-edit content, which then validates on warm reads
//! instead of missing.
//!
//! This guard extracts the builder's brace-balanced function body and
//! asserts it calls NONE of the current-content-reading primitives.
//! Any current-content read inside the graph signature builder MUST
//! route through the caller-supplied observed identity — the
//! `observed_self_roots` parameter (each `(canonical, observed_hash)`
//! captured once at the value source) and the traced fact set — never
//! through:
//!
//! - `authoritative_current_content_hash` — the current-content
//!   whole-hash oracle.
//! - `current_file_facts` — the current-content parse-fact reader.
//! - `parse_fact_ref(` — the deleted current-content parse-fact
//!   builder (matched with its opening paren so it does not false-match
//!   the provenance-pure `parse_fact_ref_for_observed_current_content`).
//! - `self_root_fact` — the deleted current-content self-root re-read
//!   helper.
//! - `shallow_file_state` — the base-host-only shallow-state oracle (it
//!   re-reads content and, under a session overlay, reads the base file
//!   hash rather than the overlay's).
//!
//! Re-introducing any forbidden token inside the builder flips this
//! guard RED. A self-test exercises the scanner against a synthetic
//! violation string so the scan cannot pass vacuously.
//!
//! # Scan scope — IMPORTANT
//!
//! This guard scans ONLY the brace-balanced body of
//! `semantic_graph_read_set_signature`. It assumes that function
//! builds the `ReadSetSignature` facts INLINE — it has no fact-building
//! sub-helper today. If a future change factors fact construction into
//! a sub-helper (or otherwise moves a current-content read out of this
//! body), the scan would no longer cover it and a forbidden re-read
//! could slip in unflagged. Any such refactor MUST extend the scan set
//! here to include every helper the builder calls to build facts, so
//! the provenance-purity invariant stays enforced end-to-end. The
//! `arch-guard:graph-signature-builder-provenance-pure` source comment
//! on the builder records the same obligation.

use std::fs;
use std::path::PathBuf;

/// Read a `verter_session` source file relative to `src/`.
fn read_session_source(relative: &str) -> String {
    let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(cargo_manifest_dir);
    path.push("src");
    path.push(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

/// Extract the brace-balanced body (including the outer `{ }`) of the
/// first function whose signature contains `needle`.
fn extract_fn_body<'a>(src: &'a str, needle: &str) -> &'a str {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in source"));
    let after_sig = &src[start..];
    let open = after_sig
        .find('{')
        .unwrap_or_else(|| panic!("expected an opening brace for `{needle}`"));
    let bytes = after_sig.as_bytes();
    let mut depth = 0usize;
    let mut idx = open;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &after_sig[open..=idx];
                }
            }
            _ => {}
        }
        idx += 1;
    }
    panic!("expected a brace-balanced body for `{needle}`");
}

/// Each token, if present in the graph signature builder body,
/// reopens the publish race. `parse_fact_ref(` is matched with its
/// opening paren so it does not false-match the provenance-pure
/// `parse_fact_ref_for_observed_current_content`.
const FORBIDDEN: &[&str] = &[
    "authoritative_current_content_hash",
    "current_file_facts",
    "parse_fact_ref(",
    "self_root_fact",
    "shallow_file_state",
];

/// `semantic_graph_read_set_signature` — the `SemanticGraphStore`
/// query-node carrier builder — MUST be provenance-pure. It takes the
/// observed self-root identities as a parameter and never re-reads
/// current content.
#[test]
fn semantic_graph_signature_builder_is_provenance_pure() {
    let src = read_session_source("semantic_query_memo/mod.rs");
    let body = extract_fn_body(&src, "pub(crate) fn semantic_graph_read_set_signature(");
    for forbidden in FORBIDDEN {
        assert!(
            !body.contains(forbidden),
            "`semantic_graph_read_set_signature` MUST NOT call `{forbidden}` — it is a \
             current-content read and reopens the publish race the provenance-pure \
             signature builder closes. Root the entry on the caller-supplied \
             `observed_self_roots` (each captured once at the value source) and the \
             traced fact set instead. Body:\n{body}"
        );
    }
}

/// Self-test: the scanner discriminates. A synthetic body containing a
/// forbidden token MUST be flagged; a body without one MUST NOT. Without
/// this, a scanner that silently matched nothing would pass vacuously.
#[test]
fn scanner_flags_a_planted_violation() {
    let clean = "fn builder() { let x = observed_self_roots.len(); x }";
    for forbidden in FORBIDDEN {
        assert!(
            !clean.contains(forbidden),
            "scanner self-test: a clean builder body must contain no forbidden token"
        );
    }

    // A planted violation: a `shallow_file_state` re-read in the body.
    let violating = "fn builder() { let h = ctx.shallow_file_state(c).whole_hash; h }";
    assert!(
        violating.contains("shallow_file_state"),
        "scanner self-test: a `shallow_file_state` re-read MUST be detected — if not, the \
         production guard above passes vacuously"
    );

    // `parse_fact_ref(` must match the bare builder but NOT the
    // provenance-pure `parse_fact_ref_for_observed_current_content`.
    let pure_call = "let f = parse_fact_ref_for_observed_current_content(ctx, c, h, k, l);";
    assert!(
        !pure_call.contains("parse_fact_ref("),
        "scanner self-test: `parse_fact_ref(` must NOT false-match the provenance-pure \
         `parse_fact_ref_for_observed_current_content`"
    );
    let bare_call = "let f = parse_fact_ref(ctx, c, k, l);";
    assert!(
        bare_call.contains("parse_fact_ref("),
        "scanner self-test: `parse_fact_ref(` MUST match the bare current-content builder"
    );

    // Sanity: the scanned source file exists and the builder is in it.
    let src = read_session_source("semantic_query_memo/mod.rs");
    assert!(
        src.contains("pub(crate) fn semantic_graph_read_set_signature("),
        "the graph signature builder must be present in semantic_query_memo/mod.rs"
    );
}
