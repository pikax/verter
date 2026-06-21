//! Handle-capable consumer guards (additive dual-read).
//!
//! Several component-meta consumers are being made HANDLE-CAPABLE: each
//! grows an additive sibling arm that accepts an ALREADY-LOWERED graph
//! node (a `SemanticNodeId` / `HotTypeRef`) and routes it through the
//! SAME query-time dispatch the `TypeExpr` arm reaches — read-compat,
//! ONE resolver. Two invariants protect that work and its ordering
//! against the later (breaking) producer wiring:
//!
//! - **G-A: no reverse `materialize_type_expr` bridge in a hot handle
//!   arm.** A `HotTypeRef` is ALREADY the lowered node; a handle arm
//!   must reduce it directly, never materialise it back to a `TypeExpr`
//!   and re-lower. The single reverse boundary `materialize_type_expr`
//!   is permitted ONLY at the public-DTO / output / compat seams (an
//!   explicit, rationale-bearing allowlist). This is the SCOPED
//!   precursor to the later global fence
//!   `hot_path_never_calls_materialize_type_expr` (enabled last, when
//!   the transitional allowlists reach zero).
//!
//! - **G-B: per-inventory ordering.** Each listed hot carrier has a
//!   handle-native consumer present in the production tree BEFORE the
//!   producer is converted to emit handles. Deferred carriers (the
//!   `verter_semantic` prepared-wrapper payloads, which have no session
//!   resolution-input consumer and cannot gain a `HotTypeRef` without
//!   violating the crate boundary) are recorded as such and backed by a
//!   short-lived absence-of-direct-reference tripwire: non-test
//!   production `verter_session` source must not directly NAME the
//!   deferred prepared-wrapper payload API (the four payload type names
//!   or the `.target_args` field). This is an ordering tripwire, NOT a
//!   semantic dataflow proof — it does not prove no possible consumer
//!   exists, only that none directly references the API yet.
//!
//! Both guards are mechanical source scans with paired self-tests that
//! prove they discriminate (fire on a synthetic violation, pass on the
//! known-good shape) per the Stub-Prevention contract.

use std::collections::HashSet;
use std::path::PathBuf;

use walkdir::WalkDir;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_rel(rel: &str) -> String {
    let path = crate_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn is_test_file(rel: &str) -> bool {
    rel.ends_with("_tests.rs")
        || rel.ends_with("/tests.rs")
        || rel.contains("/tests/")
        || rel.contains("/tests_")
}

/// Production `.rs` files under `crates/verter_session/src`, relative
/// to the crate root, test fixtures excluded.
fn production_src_files() -> Vec<(String, String)> {
    let src_root = crate_root().join("src");
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for entry in WalkDir::new(&src_root) {
        let entry = entry.expect("walkdir entry");
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let rel = path
            .strip_prefix(crate_root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if is_test_file(&rel) || !seen.insert(rel.clone()) {
            continue;
        }
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        out.push((rel, src));
    }
    out
}

// ===========================================================================
// G-A — no reverse `materialize_type_expr` bridge in a hot handle arm.
// ===========================================================================

/// Whether `c` is an identifier-continuation character.
fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Character that may legitimately precede the boundary IDENTIFIER (a
/// method-call dot, a path `:`, or a whitespace boundary). An identifier
/// char (`_`, alnum) before it means a DIFFERENT, longer identifier
/// (e.g. `xmaterialize_type_expr`), not the boundary.
fn is_ident_boundary_before(c: Option<char>) -> bool {
    match c {
        None => true,
        Some(c) => !is_ident_char(c),
    }
}

/// Returns `true` when a source line REFERENCES the EXACT
/// `materialize_type_expr` boundary as a non-definition use — the WHOLE
/// identifier (a non-identifier char before AND after it, so
/// `materialize_type_expr_until_stable` / `xmaterialize_type_expr` do NOT
/// match), where that occurrence is not the boundary DEFINITION (`fn `
/// right before the identifier). Detection is REFERENCE-based, not
/// call-based: a bare reference with no following `(` — method-item
/// indirection like `let f = Dispatch::materialize_type_expr;` — is a
/// hit, as is `*out = self.materialize_type_expr(h);`. The line is
/// scanned for EVERY occurrence, so a
/// `fn materialize_type_expr_bridge(...) { self.materialize_type_expr(h) }`
/// line — exempt on its def-looking prefix — is still caught on the inner
/// reference. Line comments (`//`, `///`) are not code: a whole `//`-led
/// line is skipped, and a trailing `//` comment is stripped before
/// matching (a `::` path is NOT a `//`, so `Dispatch::materialize_type_expr`
/// survives the strip). The trailing-comment strip is string-literal-aware:
/// it cuts at the first `//` that is NOT inside a double-quoted string, so a
/// `//` inside a literal (e.g. `"http://x"`) does not hide a real reference
/// that follows it on the same line. LIMITATION: only double-quoted string
/// literals are modeled — raw strings (`r#"..."#`), byte strings (`b"..."`),
/// and char literals (`'"'`) are NOT, so a `//` after one of those corner
/// cases could be mis-stripped; the common `"http://"` case IS handled.
fn line_references_materialize_boundary(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return false;
    }
    // Strip a trailing line comment so a `// ... materialize_type_expr`
    // note on a code line is not a reference. The strip is
    // string-literal-aware: it cuts at the first `//` that is NOT inside a
    // double-quoted string, so a `//` inside a literal (e.g. `"http://x"`)
    // does not hide a real reference that follows it on the same line.
    let comment_start = {
        let bytes = line.as_bytes();
        let mut in_string = false;
        let mut found = None;
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'"' {
                // Toggle on every `"` whose immediately-preceding char is
                // not a backslash (a simple, sufficient rule for this
                // guard — see the doc comment's documented limitation).
                let escaped = i > 0 && bytes[i - 1] == b'\\';
                if !escaped {
                    in_string = !in_string;
                }
            } else if !in_string && b == b'/' && bytes.get(i + 1) == Some(&b'/') {
                found = Some(i);
                break;
            }
            i += 1;
        }
        found
    };
    let code = match comment_start {
        Some(pos) => &line[..pos],
        None => line,
    };
    const ID: &str = "materialize_type_expr";
    let bytes = code.as_bytes();
    let mut search = 0usize;
    while let Some(rel) = code[search..].find(ID) {
        let start = search + rel;
        let end = start + ID.len();
        search = end;
        // Whole-identifier boundary on the left: a different longer
        // identifier (suffix/prefix) is not the boundary.
        let before = code[..start].chars().next_back();
        if !is_ident_boundary_before(before) {
            continue;
        }
        // Whole-identifier boundary on the right: the next char must NOT
        // be an identifier char (so `materialize_type_expr_until_stable` —
        // an identifier continuation — is not the boundary identifier).
        if let Some(nc) = bytes.get(end).map(|b| *b as char) {
            if is_ident_char(nc) {
                continue;
            }
        }
        // Is this occurrence the DEFINITION (`fn` immediately before the
        // identifier)? The single boundary definition is the permitted
        // reverse seam; a definition is not a consumer. Any other
        // reference — call OR bare method-item reference — is a hit.
        let prefix = code[..start].trim_end();
        if prefix.ends_with("fn") {
            continue;
        }
        return true;
    }
    false
}

/// Every production-source REFERENCE site of the `materialize_type_expr`
/// boundary, INCLUDING inside the boundary's own file. The boundary
/// definition line itself is excluded (it is a declaration, not a
/// consumer-side reference).
fn materialize_boundary_reference_sites() -> Vec<String> {
    let mut hits = Vec::new();
    for (rel, src) in production_src_files() {
        for (i, line) in src.lines().enumerate() {
            if line_references_materialize_boundary(line) {
                hits.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    hits
}

#[test]
fn no_hot_path_materialize_type_expr_bridge() {
    let hits = materialize_boundary_reference_sites();
    assert!(
        hits.is_empty(),
        "G-A: `materialize_type_expr` is REFERENCED in production source (outside its own boundary \
         DEFINITION) — a handle arm must reduce the node DIRECTLY through the dispatch, never \
         materialise it back to a `TypeExpr` and re-lower, and must never take a method-item \
         reference to the reverse boundary. The reverse boundary is output/compat only; it has NO \
         production reference until the later output-fence stage. Offending sites:\n{}",
        hits.join("\n")
    );
}

/// Count the EXACT boundary-definition sites (`fn materialize_type_expr(`
/// — the whole identifier immediately followed by `(`) across all
/// production files. There must be exactly ONE, in raise.rs.
fn boundary_definition_sites() -> Vec<String> {
    let mut sites = Vec::new();
    for (rel, src) in production_src_files() {
        for (i, line) in src.lines().enumerate() {
            // A definition: `fn` then the WHOLE identifier then `(`.
            if let Some(pos) = line.find("fn materialize_type_expr") {
                let after = &line[pos + "fn materialize_type_expr".len()..];
                // The char right after the identifier must be `(` (or
                // whitespace then `(`), NOT an identifier continuation —
                // so `fn materialize_type_expr_bridge(` is NOT counted.
                let next = after.chars().next();
                let is_exact = matches!(next, Some('(') | Some(' ') | Some('<'));
                if is_exact {
                    sites.push(format!("{rel}:{}", i + 1));
                }
            }
        }
    }
    sites
}

#[test]
fn g_a_exactly_one_boundary_definition_in_raise() {
    // Anti-vacuity + anti-evasion: there must be EXACTLY ONE boundary
    // definition, and it must live in the single reverse-boundary file.
    // A second `fn materialize_type_expr(` anywhere (a duplicate /
    // relocated definition) would silently create a second exempt site.
    let sites = boundary_definition_sites();
    assert_eq!(
        sites.len(),
        1,
        "G-A: there must be EXACTLY ONE `materialize_type_expr` boundary definition; found: {sites:?}"
    );
    assert!(
        sites[0].starts_with("src/project_semantic_dispatch/raise.rs:"),
        "G-A: the single boundary definition must live in raise.rs; found at {}",
        sites[0]
    );
}

#[test]
fn g_a_self_test_scanner_discriminates() {
    // The detector must (a) fire on a real call, including a space-split
    // call and a call INSIDE the boundary's own file, (b) fire on a bare
    // (no-paren) method-item reference, (c) fire on a `*`-prefixed code
    // line, (d) skip the boundary DEFINITION, and (e) skip comments.
    assert!(
        line_references_materialize_boundary("    let t = dispatch.materialize_type_expr(handle);"),
        "self-test: a direct `materialize_type_expr(` call MUST be a hit"
    );
    assert!(
        line_references_materialize_boundary(
            "    let t = dispatch.materialize_type_expr (handle);"
        ),
        "self-test: a space-split `materialize_type_expr (` call MUST be a hit"
    );
    assert!(
        line_references_materialize_boundary("    self.materialize_type_expr"),
        "self-test: a bare reference (paren on a following line) MUST be a hit — a future hot \
         bridge cannot evade by line-splitting"
    );
    // P2 EVASION 1: a `*`-prefixed CODE line (`*out = ...`) MUST be caught
    // — the old detector skipped any `*`-led line, letting this evade.
    assert!(
        line_references_materialize_boundary("    *out = self.materialize_type_expr(handle);"),
        "self-test: a `*`-prefixed CODE line that calls the boundary MUST be a hit — a leading \
         `*` is dereference syntax here, not a comment marker"
    );
    // P2 EVASION 2: method-item indirection — a bare reference with NO
    // immediate `(` — MUST be caught. The old call-only detector required
    // a following paren, letting this evade.
    assert!(
        line_references_materialize_boundary(
            "    let f = ProjectSemanticDispatch::materialize_type_expr;"
        ),
        "self-test: a method-item reference with NO immediate `(` MUST be a hit — reference-based \
         detection forbids taking the reverse boundary as a function item"
    );
    // P1 EVASION: a `//`-bearing STRING LITERAL before a real reference
    // MUST NOT hide it — the trailing-comment strip is string-literal-aware
    // (the old naive `find("//")` truncated at the `//` inside `"http://x"`
    // and missed the real method-item reference after it).
    assert!(
        line_references_materialize_boundary(
            "    let _url = \"http://x\"; let f = ProjectSemanticDispatch::materialize_type_expr;"
        ),
        "self-test: a `//`-bearing STRING LITERAL before a real reference MUST NOT hide it — the \
         comment strip is string-literal-aware"
    );
    assert!(
        !line_references_materialize_boundary("    pub(crate) fn materialize_type_expr(&self) {"),
        "self-test: the boundary DEFINITION line MUST NOT be a hit"
    );
    assert!(
        !line_references_materialize_boundary(
            "/// round-trips through `materialize_type_expr` here"
        ),
        "self-test: a doc-comment reference MUST NOT be a hit"
    );
    assert!(
        !line_references_materialize_boundary(
            "    // materialize_type_expr(handle) would be a reverse bridge here"
        ),
        "self-test: a line-comment reference MUST NOT be a hit"
    );
    assert!(
        !line_references_materialize_boundary(
            "    foo(handle); // see materialize_type_expr for the reverse seam"
        ),
        "self-test: a TRAILING line-comment reference on a code line MUST NOT be a hit — the \
         `//`-to-EOL comment is stripped before matching"
    );
    // ANTI-EVASION: a same-line `fn materialize_type_expr_bridge(...) {
    // self.materialize_type_expr(h) }` — exempt on its def-looking
    // prefix — MUST still be caught on the inner real reference. This is
    // the prefixed-name-definition evasion the def-exemption must not allow.
    assert!(
        line_references_materialize_boundary(
            "    fn materialize_type_expr_bridge(&self, h: H) { self.materialize_type_expr(h) }"
        ),
        "self-test: a prefixed-name fake definition that wraps a REAL `materialize_type_expr` \
         reference MUST be caught on the inner reference — the def-exemption matches the EXACT \
         identifier only"
    );
    // A DIFFERENT identifier that merely shares the prefix must NOT be a
    // hit (no false positive) — neither the suffixed call nor the
    // production `materialize_type_expr_until_stable` instrumentation name.
    assert!(
        !line_references_materialize_boundary("    let x = self.materialize_type_expr_other(h);"),
        "self-test: `materialize_type_expr_other` is a DIFFERENT identifier and MUST NOT be a hit"
    );
    assert!(
        !line_references_materialize_boundary(
            "    let materialize_type_expr_until_stable_calls = 0;"
        ),
        "self-test: `materialize_type_expr_until_stable_calls` is a DIFFERENT identifier and MUST \
         NOT be a hit"
    );
    // The real boundary definition (whole identifier) must NOT be a hit
    // even with a generic clause.
    assert!(
        !line_references_materialize_boundary(
            "    pub(crate) fn materialize_type_expr<T>(&self) -> T {"
        ),
        "self-test: the generic boundary definition MUST NOT be a hit"
    );
}

// ===========================================================================
// G-B — per-inventory: each hot carrier has a handle-native consumer
// BEFORE producer conversion; deferred carriers are recorded with a
// reason and backed by a short-lived absence-of-direct-reference tripwire
// — non-test production source must not directly name the deferred
// payload API (an ordering tripwire, not a semantic dataflow proof).
// ===========================================================================

/// Status of a handle-capable carrier inventory row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SeamStatus {
    /// A real session seam: the handle-native consumer must be PRESENT.
    HandleNative,
    /// The carrier's payload is read only inside `verter_semantic`
    /// (no `verter_session` resolution-input consumer); it cannot gain
    /// a `HotTypeRef` without a `verter_semantic -> verter_session` dep (the reverse of the existing direction, forbidden by `no_verter_semantic_to_verter_session_dep`)
    /// reversal. Deferred until the producer is wired to emit handles. A
    /// short-lived absence-of-direct-reference tripwire asserts non-test
    /// `verter_session` production source does not directly NAME the
    /// deferred payload API (the four payload type names / `.target_args`)
    /// — an ordering tripwire, not a semantic dataflow proof.
    Stage5Deferred,
}

/// A handle-capable carrier inventory row.
struct InventoryRow {
    /// Human label for the seam.
    seam: &'static str,
    status: SeamStatus,
    /// For `HandleNative`: `(file, needle)` proving the handle-native
    /// consumer is PRESENT — the needle is the consumer symbol that
    /// accepts an already-lowered node. For `Stage5Deferred`: the
    /// `verter_semantic` payload API name (or `.target_args` field) that
    /// MUST NOT be directly referenced anywhere in non-test
    /// `verter_session` production source (the first tuple element is a
    /// label only; the scan is whole-tree over `src/`).
    witness: &'static [(&'static str, &'static str)],
    /// Rationale (required for every row; the deferral reason for a
    /// deferred row).
    reason: &'static str,
}

/// The handle-capable carrier inventory.
const STAGE4_CARRIER_INVENTORY: &[InventoryRow] = &[
    InventoryRow {
        seam: "ShapeSubject (member value)",
        status: SeamStatus::HandleNative,
        witness: &[(
            "src/meta_resolve/materialize/field_types.rs",
            "fn reduce_member_value_graph_native_with_context",
        )],
        reason: "the ShapeSubject::SemanticNode subject reduces an already-lowered node directly \
                 through raise_and_reduce_with_context",
    },
    InventoryRow {
        seam: "imported registry body / member surface",
        status: SeamStatus::HandleNative,
        witness: &[(
            "src/resolver_core/component_meta_query_engine/registry_decl.rs",
            "fn materialize_member_surface_node",
        )],
        reason: "the member-surface node-core reduces an already-lowered registry/member body \
                 node through the dispatch; the TypeExpr arm lowers-then-delegates to it",
    },
    InventoryRow {
        seam: "owner collection body",
        status: SeamStatus::HandleNative,
        witness: &[(
            "src/resolver_core/component_meta_query_engine/registry_decl.rs",
            "fn owner_collection_surface_from_node",
        )],
        reason: "the owner-collection handle arm reduces a body node through the shared \
                 member-surface node-core without touching the TypeExpr-keyed OwnerCollectionDb",
    },
    InventoryRow {
        seam: "registry symbolic-alias root classification",
        status: SeamStatus::HandleNative,
        witness: &[(
            "src/meta_resolve/exactness.rs",
            "fn node_root_should_stay_symbolic",
        )],
        reason: "the graph-native root classifier mirrors the TypeExpr-shape predicate over \
                 SemanticNodeData roots (a ROOT-KIND classifier, no resolution)",
    },
    InventoryRow {
        seam: "PreparedWrapperShape Opaque/Transform + PreparedForwardPayload.target_args",
        status: SeamStatus::Stage5Deferred,
        // The deferred prepared-wrapper payload API names this row covers
        // — the four payload TYPE names (whole-identifier exact) plus the
        // forward payload's `.target_args` field access. The deferred
        // check below asserts NONE of these is directly referenced in
        // non-test verter_session production source.
        witness: &[
            ("verter_session src", "PreparedKeyFilterShape"),
            ("verter_session src", "PreparedKeyRemapShape"),
            ("verter_session src", "PreparedValueRuleShape"),
            ("verter_session src", "PreparedForwardPayload"),
            // The forward payload's symbolic type-args field access —
            // named in the row, so witnessed exactly.
            ("verter_session src", ".target_args"),
        ],
        reason:
            "these prepared-wrapper payloads live in verter_semantic and are read ONLY by \
                 the verter_semantic solver; verter_session has no resolution-input consumer, so \
                 they cannot carry a HotTypeRef without the forbidden reverse `verter_semantic -> verter_session` dep. \
                 Deferred until the producer is wired to emit handles; the producer stays dormant. \
                 A short-lived absence-of-direct-reference tripwire asserts non-test verter_session \
                 production source does not directly name this payload API — an ordering tripwire, \
                 not a semantic dataflow proof.",
    },
];

/// The 1-based line containing `byte_pos` in `src`.
fn line_of(src: &str, byte_pos: usize) -> usize {
    src[..byte_pos].bytes().filter(|b| *b == b'\n').count() + 1
}

/// Scans a production source string for any DIRECT reference to the
/// deferred prepared-wrapper payload API named by `patterns`. A pattern
/// is one of two shapes:
///
/// - a TYPE name (e.g. `PreparedKeyFilterShape`) — matched as a WHOLE
///   identifier (the char immediately before AND after the match must
///   NOT be an identifier char), so the legitimate `PreparedTypeDecl` /
///   `PreparedValueDecl` / `PreparedProjectionClass` / longer-suffix and
///   prefixed forms do NOT trip;
/// - a FIELD access (a pattern starting with `.`, e.g. `.target_args`) —
///   the field identifier matched whole on the right, preceded (skipping
///   any whitespace / newline) by a `.`, so `payload.target_args`,
///   `payload . target_args`, and a newline-split `payload\n.target_args`
///   all trip while `target_args_extra` / a bare `target_args` do not.
///
/// This is a presence ban (classification-only mentions count): the
/// invariant is absence-of-direct-reference, not no-dataflow. Returns one
/// `"<needle> @ line <n>"` entry per match.
fn file_names_deferred_payload(src: &str, patterns: &[&str]) -> Vec<String> {
    let mut hits = Vec::new();
    for pat in patterns {
        if let Some(field) = pat.strip_prefix('.') {
            // Field-access token: `.<field>`.
            let mut search = 0usize;
            while let Some(rel) = src[search..].find(field) {
                let start = search + rel;
                let end = start + field.len();
                search = end;
                // Whole identifier on the right (a longer ident such as
                // `target_args_extra` is not the field).
                if let Some(nc) = src[end..].chars().next() {
                    if is_ident_char(nc) {
                        continue;
                    }
                }
                // Left: skip whitespace / newlines, require a `.`.
                if src[..start].trim_end().ends_with('.') {
                    hits.push(format!("{pat} @ line {}", line_of(src, start)));
                }
            }
        } else {
            // Whole-identifier type name.
            let mut search = 0usize;
            while let Some(rel) = src[search..].find(pat) {
                let start = search + rel;
                let end = start + pat.len();
                search = end;
                let before_ok = is_ident_boundary_before(src[..start].chars().next_back());
                let after_ok = src[end..]
                    .chars()
                    .next()
                    .map(|c| !is_ident_char(c))
                    .unwrap_or(true);
                if before_ok && after_ok {
                    hits.push(format!("{pat} @ line {}", line_of(src, start)));
                }
            }
        }
    }
    hits
}

#[test]
fn stage4_carrier_inventory_handle_native_consumers_present() {
    // Every `HandleNative` row must have its handle-native consumer
    // PRESENT in the production tree. This is the ordering gate: a real
    // hot carrier must be handle-capable BEFORE the producer is wired to emit handles. The
    // guard goes RED if an implementer removed a handle arm (or never
    // added it), proving it is not vacuous.
    let mut missing = Vec::new();
    for row in STAGE4_CARRIER_INVENTORY {
        assert!(
            !row.reason.trim().is_empty(),
            "inventory row `{}` must carry a non-empty reason",
            row.seam
        );
        if row.status != SeamStatus::HandleNative {
            continue;
        }
        for (file, needle) in row.witness {
            let path = crate_root().join(file);
            let present = std::fs::read_to_string(&path)
                .map(|src| src.contains(needle))
                .unwrap_or(false);
            if !present {
                missing.push(format!(
                    "seam `{}`: handle-native consumer `{needle}` NOT found in {file}",
                    row.seam
                ));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "G-B: a hot carrier has NO handle-native consumer — it must be handle-capable \
         BEFORE the producer is wired to emit handles. Missing:\n{}",
        missing.join("\n")
    );
}

/// The deferred prepared-wrapper payload API names the row bans,
/// sourced from the inventory row's `witness` (so the row and the check
/// cannot drift): the four payload type names plus the `.target_args`
/// field-access token.
fn deferred_payload_patterns() -> Vec<&'static str> {
    STAGE4_CARRIER_INVENTORY
        .iter()
        .filter(|r| r.status == SeamStatus::Stage5Deferred)
        .flat_map(|r| r.witness.iter().map(|(_, needle)| *needle))
        .collect()
}

#[test]
fn stage4_deferred_carriers_have_no_session_resolution_consumer() {
    // NAME NOTE: the historical name says "no session resolution consumer",
    // but this guard enforces the narrower, honest invariant below — a
    // whole-file ABSENCE-OF-DIRECT-REFERENCE tripwire over non-test
    // verter_session production source for the deferred prepared-wrapper
    // payload API. It is an ordering tripwire, NOT a semantic dataflow proof:
    // it does not prove no possible consumer exists, only that none directly
    // NAMES the API yet.
    //
    // Every `Stage5Deferred` row asserts the deferral is still
    // legitimate via a short-lived absence-of-direct-reference tripwire:
    // non-test verter_session production source must not directly NAME
    // the deferred prepared-wrapper payload API. The check scans the
    // WHOLE of each production `src/` file (not function windows) for a
    // direct reference to one of the four payload type names (whole
    // identifier) or the `.target_args` field access — so an aliased
    // import (`use ... as KF`) and a cross-function split (extract in one
    // fn, lower in another) are both caught, since the NAME appears
    // regardless of which function or alias surrounds it. This is an
    // ORDERING tripwire, NOT a semantic dataflow proof: it does not prove
    // no possible consumer exists. If a direct reference appears, the
    // producer wiring has begun — flip the inventory row to HandleNative
    // and add a handle arm.
    let payload_patterns = deferred_payload_patterns();
    // Anti-vacuity: the patterns must include the four named payload
    // type names plus the forward payload's `.target_args` field, so the
    // deferral is machine-recorded for every carrier the row names.
    for required in [
        "PreparedKeyFilterShape",
        "PreparedKeyRemapShape",
        "PreparedValueRuleShape",
        "PreparedForwardPayload",
        ".target_args",
    ] {
        assert!(
            payload_patterns.contains(&required),
            "G-B: the deferred-carrier ban must cover `{required}` — the inventory row names it"
        );
    }

    let mut violations = Vec::new();
    for (rel, src) in production_src_files() {
        for hit in file_names_deferred_payload(&src, &payload_patterns) {
            violations.push(format!("{rel}: {hit}"));
        }
    }
    assert!(
        violations.is_empty(),
        "G-B: non-test verter_session production source directly NAMES a deferred prepared-wrapper \
         payload API — the producer wiring has begun, so the deferral is no longer legitimate. \
         Flip the inventory row to HandleNative and add a handle arm.\n{}",
        violations.join("\n")
    );
}

#[test]
fn g_b_self_test_deferred_detector_catches_direct_aliased_and_split() {
    // The presence-ban detector MUST fire on every evasion the old
    // co-location scan let through, and on a classification-only mention.
    let p = deferred_payload_patterns();

    // (1) DIRECT reference.
    assert!(
        !file_names_deferred_payload("    let x = PreparedKeyFilterShape::Opaque(expr);", &p)
            .is_empty(),
        "self-test: a DIRECT `PreparedKeyFilterShape` reference MUST be caught"
    );

    // (2) ALIASED import — EVASION A. The type name is on the `use` line,
    // so a name-presence ban catches it where an aliased-substring
    // co-location scan would not.
    assert!(
        !file_names_deferred_payload("use crate::a::b::PreparedKeyFilterShape as KF;", &p)
            .is_empty(),
        "self-test: an ALIASED import (`use ... PreparedKeyFilterShape as KF`) MUST be caught — \
         this is EVASION A"
    );

    // (3) CROSS-FN SPLIT — EVASION C. fn1 names the payload, fn2 lowers;
    // a whole-file scan finds the NAME regardless of which fn it is in.
    let cross_fn = "\
fn extract(shape: &S) -> Expr {
    match &shape.remap {
        PreparedKeyRemapShape::Opaque(expr) => expr.clone(),
        _ => Expr::default(),
    }
}
fn lower(d: &Dispatch, expr: &Expr) -> SemanticNodeId {
    d.lower_type_expr_in_scope_with_mode(expr)
}
";
    assert!(
        !file_names_deferred_payload(cross_fn, &p).is_empty(),
        "self-test: a CROSS-FN split (PreparedKeyRemapShape named in one fn, lowered in another) \
         MUST be caught by the whole-file name ban — this is EVASION C"
    );

    // (4) CLASSIFICATION-ONLY mention (no resolution call) — still a hit:
    // it is a presence ban, absence-of-reference is the invariant.
    assert!(
        !file_names_deferred_payload(
            "    let _ = matches!(shape.x, PreparedValueRuleShape::Transform(_));",
            &p
        )
        .is_empty(),
        "self-test: a CLASSIFICATION-ONLY `PreparedValueRuleShape` mention MUST be caught — the \
         ban is on direct reference, not on dataflow"
    );

    // (5) `.target_args` field access — direct and whitespace/newline
    // split forms.
    assert!(
        !file_names_deferred_payload("    let a = payload.target_args.clone();", &p).is_empty(),
        "self-test: a `.target_args` field access MUST be caught"
    );
    assert!(
        !file_names_deferred_payload("    let a = payload\n        .target_args;", &p).is_empty(),
        "self-test: a newline-split `payload\\n    .target_args` access MUST be caught"
    );
    assert!(
        !file_names_deferred_payload("    let a = payload . target_args;", &p).is_empty(),
        "self-test: a whitespace-split `payload . target_args` access MUST be caught"
    );
}

#[test]
fn g_b_self_test_deferred_detector_no_false_positive_on_legit_prepared_idents() {
    // The load-bearing anti-false-positive test: session production uses
    // many other `Prepared*` identifiers (and field accesses) that share
    // a PREFIX with a banned name but are NOT it. The whole-identifier
    // ban MUST report ZERO hits on all of them.
    let p = deferred_payload_patterns();

    // (6) NONE of the banned names / `.target_args`, but many legit
    // sibling identifiers (incl. `PreparedValueDecl`, which shares the
    // `PreparedValue` prefix with the banned `PreparedValueRuleShape`).
    let legit = "\
fn uses_legit(b: &PreparedDeclBundle) -> PreparedTypeDecl {
    let _v: PreparedValueDecl = b.value_decl();
    let _c = PreparedProjectionClass::DirectMembers;
    let _m = b.member_index;
    let _p = PreparedMember::default();
    b.type_decl()
}
";
    assert!(
        file_names_deferred_payload(legit, &p).is_empty(),
        "self-test: legit sibling `Prepared*` idents (PreparedTypeDecl / PreparedValueDecl / \
         PreparedProjectionClass / PreparedDeclBundle / PreparedMember) and `.member_index` MUST \
         NOT trip the whole-identifier ban: {:?}",
        file_names_deferred_payload(legit, &p)
    );

    // (7) WHOLE-IDENTIFIER boundary: a longer suffix and a prefixed form
    // of a banned name MUST NOT trip; the bare banned name MUST trip.
    assert!(
        file_names_deferred_payload("    let _x: PreparedForwardPayloadExtra = todo!();", &p)
            .is_empty(),
        "self-test: `PreparedForwardPayloadExtra` (longer suffix) MUST NOT trip"
    );
    assert!(
        file_names_deferred_payload("    let _x: MyPreparedForwardPayload = todo!();", &p)
            .is_empty(),
        "self-test: `MyPreparedForwardPayload` (prefixed) MUST NOT trip"
    );
    assert!(
        !file_names_deferred_payload("    let _x = PreparedForwardPayload { args };", &p)
            .is_empty(),
        "self-test: the bare `PreparedForwardPayload` (struct-literal form) MUST trip"
    );
    assert!(
        !file_names_deferred_payload("    let _x = PreparedForwardPayload::default();", &p)
            .is_empty(),
        "self-test: the bare `PreparedForwardPayload` (path form) MUST trip"
    );
    // `target_args` NOT preceded by `.`, and `.target_args_extra`, MUST
    // NOT trip the field-access ban.
    assert!(
        file_names_deferred_payload("    fn target_args(&self) -> Args { todo!() }", &p).is_empty(),
        "self-test: a `target_args` identifier not preceded by `.` MUST NOT trip"
    );
    assert!(
        file_names_deferred_payload("    let _ = payload.target_args_extra;", &p).is_empty(),
        "self-test: `.target_args_extra` (longer field) MUST NOT trip"
    );
}

#[test]
fn g_b_self_test_inventory_is_well_formed_and_discriminating() {
    // Non-vacuity 1: the inventory must contain BOTH at least one
    // HandleNative row and the deferred row — a degenerate inventory
    // (all deferred, or empty) would pass trivially.
    let handle_native = STAGE4_CARRIER_INVENTORY
        .iter()
        .filter(|r| r.status == SeamStatus::HandleNative)
        .count();
    let deferred = STAGE4_CARRIER_INVENTORY
        .iter()
        .filter(|r| r.status == SeamStatus::Stage5Deferred)
        .count();
    assert!(
        handle_native >= 4,
        "self-test: the inventory must enumerate every real session seam (>=4 HandleNative \
         rows); got {handle_native}"
    );
    assert!(
        deferred >= 1,
        "self-test: the inventory must record the deferred prepared-wrapper carriers; got \
         {deferred}"
    );

    // Non-vacuity 2: the presence check discriminates — a needle that is
    // KNOWN ABSENT must report missing.
    let absent_present = std::fs::read_to_string(
        crate_root().join("src/resolver_core/component_meta_query_engine/registry_decl.rs"),
    )
    .map(|src| src.contains("fn this_handle_arm_does_not_exist_xyzzy"))
    .unwrap_or(false);
    assert!(
        !absent_present,
        "self-test: a deliberately-absent needle must NOT be found — proving the presence check \
         discriminates present from absent"
    );
}

// ===========================================================================
// Structural-carrier producer guard SET — the single structural-carrier producer
// is COMPILER-CONFINED to ONE module (`macro_arg_producer.rs`), which owns the
// module-private raw lowerer, the macro hot-mirror builder, and the binder-seed
// builder; the owner declares it as a PRIVATE `mod macro_arg_producer;`
// re-exporting only `macro_type_arg_hot_ref` + `MacroHotMirror`. A second
// structural-carrier producer is therefore UNREPRESENTABLE BY CONSTRUCTION: no
// foreign module can name the private builders (a compile error), and the
// producer is collapsed into one module so no same-owner file can name them
// either (a third caller is a compile error). The set is six guards: the PRIMARY
// module-private lowerer guard
// (`structural_carrier_producer_lowerer_is_module_private` — the raw lowerer is
// a bare module-private fn in `macro_arg_producer.rs`, not re-exported), the
// PARENT-SHAPE narrowness guard (`structural_carrier_producer_module_is_narrow` —
// the owner directory contains ONLY `mod.rs`, `macro_arg_producer.rs`, and test
// modules), together the compiler-enforced make-unrepresentable layer; plus the
// SMALL no-reintroduce-a-surface backstop
// (`macro_arg_producer_has_no_production_expansion_surface` — no production
// macro / `macro_rules!` / `include!` / proc-macro attribute / `#[derive]` on a
// producer-capable item / out-of-line-or-`#[path]` mod / `#[macro_use]`, the only
// same-module code-gen class the structure cannot already make a compile error),
// the file-scope ordering tripwire
// (`no_production_macro_arg_eager_lowering_outside_mirror`), the purity guard
// (`macro_hot_mirror_producer_is_pure_no_route_resolution`), and the BOUNDED
// entry-surface token tripwire
// (`macro_hot_mirror_exposes_single_crate_visible_producer_entry`). The witness
// below pins that set into the registry; it does not re-define those guards.
// ===========================================================================

#[test]
fn structural_carrier_producer_guards_remain_registered() {
    // The structural-carrier producer is collapsed into ONE module
    // (`macro_arg_producer.rs`) whose producer-capable code is module-private and
    // reachable from outside only through the re-exported `macro_type_arg_hot_ref`.
    // This witness pins the replacement guard SET into BOTH the registry and the
    // assertion file, catching an accidental removal of the single-engine
    // producer defense.
    let registry = read_rel("tests/cases/g_misc0/critical_rules_have_guards.rs");
    let guards = read_rel("tests/cases/architecture_guards.rs");

    // Every guard in the SET must be BOTH registered (in the registry) AND
    // defined as a real `fn …(` test in architecture_guards.rs — a renamed
    // hollow reference (registry mention without the assertion) fails here.
    const REQUIRED_GUARDS: &[(&str, &str)] = &[
        (
            "structural_carrier_producer_lowerer_is_module_private",
            "the PRIMARY make-unrepresentable guard: the raw structural lowerer is a bare \
             module-private fn in `macro_arg_producer.rs` and not re-exported, so no other module \
             can name it",
        ),
        (
            "structural_carrier_producer_module_is_narrow",
            "the PARENT-SHAPE guard: the owner directory contains ONLY the single producer module \
             `macro_arg_producer.rs`, mod.rs, and test modules — so there is no other file that \
             could name the module-private lowering builders",
        ),
        (
            "macro_arg_producer_has_no_production_expansion_surface",
            "the SMALL no-reintroduce-a-surface backstop: `macro_arg_producer.rs` declares NO \
             production (non-`#[cfg(test)]`) macro invocation / `macro_rules!` / `include!` / \
             proc-macro attribute / `#[derive(…)]` on a producer-capable item / \
             out-of-line-or-`#[path]` child mod / `#[macro_use]` — the only same-module \
             code-generation class the compiler module-privacy cannot already make a compile \
             error; only the sanctioned `#[cfg(test)] #[path] mod *_tests;` wiring is allowlisted",
        ),
        (
            "no_production_macro_arg_eager_lowering_outside_mirror",
            "the file-scope ordering tripwire: no production macro-arg eager lowering outside the \
             single producer module `macro_arg_producer.rs`",
        ),
        (
            "macro_hot_mirror_producer_is_pure_no_route_resolution",
            "the PURITY guard: the producer must not route-resolve imports / read the prepared-decl \
             bundle (pure structural-carrier lowering; seeding re-sources from the route-free \
             IndexedReady)",
        ),
        (
            "macro_hot_mirror_exposes_single_crate_visible_producer_entry",
            "the BOUNDED entry-surface tripwire: only the sanctioned `macro_type_arg_hot_ref` is a \
             crate-visible producer fn of the owner module",
        ),
    ];

    for (guard, why) in REQUIRED_GUARDS {
        assert!(
            registry.contains(guard),
            "the structural-carrier producer guard `{guard}` must remain registered — {why}"
        );
        // The guard must exist as a REAL `fn …(` test definition in
        // architecture_guards.rs — a registry-only mention is a hollow rename.
        let def_needle = format!("fn {guard}(");
        assert!(
            guards.contains(&def_needle),
            "the guard `{guard}` must have a REAL `{def_needle}` test definition in \
             architecture_guards.rs (not just a registry/prose mention) — {why}"
        );
    }

    // The RETIRED guard names must NOT linger anywhere (renamed faithfully, not
    // duplicated): the old privacy-guard identity is gone.
    assert!(
        !registry.contains("structural_lowerer_production_entry_is_macro_hot_mirror_private")
            && !guards.contains("structural_lowerer_production_entry_is_macro_hot_mirror_private"),
        "the retired guard `structural_lowerer_production_entry_is_macro_hot_mirror_private` must \
         be fully replaced by `structural_carrier_producer_lowerer_is_module_private` — no stale \
         reference may linger in the registry or assertion file"
    );

    // The scanner-cluster guards DELETED by the structural collapse (a second
    // producer is now a compile error, so the source-scanner rails are gone) must
    // NOT linger in either registry: their names are removed, not hollow-renamed.
    // The compiler-confinement (module-private builders in one private module) +
    // the surviving make-unrepresentable/narrowness/expansion-surface guards
    // replace them.
    const RETIRED_SCANNER_GUARDS: &[&str] = &[
        "structural_lowerer_called_only_through_the_witness_gated_wrapper",
        "structural_carrier_producer_lower_has_no_expansion_surface",
        "structural_carrier_producer_witnesses_are_unforgeable",
        "script_setup_binder_helper_is_module_private",
        "verter_session_production_has_no_macro_use_extern_crate",
    ];
    for retired in RETIRED_SCANNER_GUARDS {
        assert!(
            !registry.contains(retired),
            "the retired scanner-cluster guard `{retired}` must be fully removed from the registry \
             — the structural collapse to one compiler-confined producer module replaces it; no \
             stale reference may linger"
        );
        assert!(
            !guards.contains(&format!("fn {retired}(")),
            "the retired scanner-cluster guard `{retired}` must have NO `fn {retired}(` definition \
             in architecture_guards.rs — it was deleted, not renamed"
        );
    }
}

// ===========================================================================
// Hot prepared-decl CARRIER guards — the session-owned `HotPrepared*` carriers
// must own NO transitive `TypeExpr`. The TYPE-MEANING proof is now the
// COMPILER: every carrier `#[derive(verter_no_typeexpr::NoTypeExpr)]`s, and an
// `assert_impl_all!(_: NoTypeExpr)` in `hot_prepared.rs` fails the BUILD if any
// carrier field owns (transitively, through any alias / re-export / nested
// owner) a `TypeExpr`. The two source guards below are NARROW defenses that do
// NOT re-prove type meaning:
//
//   * the COVERAGE guard asserts every `Hot*` carrier is opted into BOTH the
//     derive and the `assert_impl_all!` set — so a NEW carrier with neither is
//     forced to classify itself (it cannot silently sidestep the compiler
//     proof);
//   * the HAND-IMPL guard asserts no hand-written `NoTypeExpr` /
//     `NoTypeExprWitness` impl exists anywhere in `verter_session/src/**`
//     EXCEPT the single audited `HotTypeRef` witness — closing the one route
//     (a hand-written witness) that could otherwise satisfy the bound without
//     the field-recursive derive.
//
// Each guard has a paired self-test proving it discriminates (fires on a
// synthetic violation, passes on the known-good shape).
// ===========================================================================

/// The hot-carrier source file the coverage guard parses.
const HOT_PREPARED_REL: &str = "src/resolver_core/hot_prepared.rs";

/// Every `Hot*`-prefixed `struct`/`enum` name declared in `hot_prepared.rs`.
/// This is the carrier inventory the coverage guard cross-checks against the
/// derive sites and the `assert_impl_all!` set: a new carrier that this scan
/// finds but that is missing from either opt-in REDS.
fn declared_hot_carriers(src: &str) -> Vec<String> {
    let file = syn::parse_file(src).expect("parse hot_prepared.rs");
    let mut names = Vec::new();
    for item in &file.items {
        let ident = match item {
            syn::Item::Struct(s) => &s.ident,
            syn::Item::Enum(e) => &e.ident,
            _ => continue,
        };
        let name = ident.to_string();
        if name.starts_with("Hot") {
            names.push(name);
        }
    }
    names
}

/// Whether `name`'s `struct`/`enum` definition in `src` carries a
/// `#[derive(... NoTypeExpr ...)]`. Parses the item's outer attributes and
/// looks for a `derive` attribute whose token stream names `NoTypeExpr` — so a
/// carrier that drops the derive is detected regardless of derive-list order or
/// the leading path segments (`verter_no_typeexpr::NoTypeExpr`).
fn carrier_has_no_type_expr_derive(src: &str, name: &str) -> bool {
    let file = syn::parse_file(src).expect("parse hot_prepared.rs");
    for item in &file.items {
        let (ident, attrs) = match item {
            syn::Item::Struct(s) => (&s.ident, &s.attrs),
            syn::Item::Enum(e) => (&e.ident, &e.attrs),
            _ => continue,
        };
        if *ident != name {
            continue;
        }
        for attr in attrs {
            if !attr.path().is_ident("derive") {
                continue;
            }
            let mut found = false;
            // `parse_nested_meta` walks each derive entry (a path like
            // `verter_no_typeexpr::NoTypeExpr` or a bare `NoTypeExpr`); the last
            // path segment is the trait name.
            let _ = attr.parse_nested_meta(|meta| {
                if meta
                    .path
                    .segments
                    .last()
                    .is_some_and(|seg| seg.ident == "NoTypeExpr")
                {
                    found = true;
                }
                Ok(())
            });
            if found {
                return true;
            }
        }
        return false;
    }
    false
}

/// Every type named in an `assert_impl_all!(<Type>: NoTypeExpr)` invocation in
/// `src`. The coverage guard requires every declared carrier to appear here, so
/// a carrier that derives the trait but is never asserted (the `assert_impl_all!`
/// is what turns the bound into a build failure) is still caught.
fn assert_impl_all_no_type_expr_subjects(src: &str) -> Vec<String> {
    let mut subjects = Vec::new();
    for raw in src.lines() {
        let line = raw.trim();
        let Some(rest) = line.strip_prefix("assert_impl_all!(") else {
            continue;
        };
        // Only the `: NoTypeExpr)` form — not an unrelated `assert_impl_all!`.
        let Some(colon) = rest.find(':') else {
            continue;
        };
        let (subject, bound) = rest.split_at(colon);
        if !bound.contains("NoTypeExpr") {
            continue;
        }
        let subject = subject.trim();
        if !subject.is_empty() {
            subjects.push(subject.to_string());
        }
    }
    subjects
}

#[test]
fn every_hot_carrier_opts_into_no_type_expr() {
    // COVERAGE — not type meaning. Each `Hot*` carrier in `hot_prepared.rs` must
    // (a) carry `#[derive(NoTypeExpr)]` AND (b) appear in an
    // `assert_impl_all!(_: NoTypeExpr)` entry. The compiler owns the transitive
    // type proof; this only forces a NEW carrier to opt in (a carrier with
    // neither would skip the proof silently).
    let src = read_rel(HOT_PREPARED_REL);
    let carriers = declared_hot_carriers(&src);
    assert!(
        carriers.len() >= 15,
        "expected the full hot-carrier inventory (≥15) in {HOT_PREPARED_REL}; found {}: {carriers:?} \
         — if a carrier was intentionally removed, update this floor with the new count",
        carriers.len()
    );

    let asserted = assert_impl_all_no_type_expr_subjects(&src);
    let mut missing_derive = Vec::new();
    let mut missing_assert = Vec::new();
    for carrier in &carriers {
        if !carrier_has_no_type_expr_derive(&src, carrier) {
            missing_derive.push(carrier.clone());
        }
        if !asserted.iter().any(|s| s == carrier) {
            missing_assert.push(carrier.clone());
        }
    }
    assert!(
        missing_derive.is_empty(),
        "every `Hot*` carrier in {HOT_PREPARED_REL} must `#[derive(verter_no_typeexpr::NoTypeExpr)]` \
         — these do NOT: {missing_derive:?}. Add the derive (or, if the field genuinely cannot be \
         TypeExpr-free, the carrier is mis-designed)."
    );
    assert!(
        missing_assert.is_empty(),
        "every `Hot*` carrier must appear in an `assert_impl_all!(_: NoTypeExpr)` entry in \
         {HOT_PREPARED_REL} (the assert is what turns the unsatisfiable bound into a BUILD failure) \
         — these are missing: {missing_assert:?}"
    );
}

#[test]
fn every_hot_carrier_opts_into_no_type_expr_self_test_discriminates() {
    // The detector must FIRE on a carrier missing the derive, and on a carrier
    // missing the `assert_impl_all!` entry — so a future weakening that lets
    // either slip through is caught here.
    let planted_missing_derive = "\
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
struct HotGood { a: u32 }

#[derive(Debug, Clone)]
struct HotMissingDerive { b: u32 }

assert_impl_all!(HotGood: NoTypeExpr);
assert_impl_all!(HotMissingDerive: NoTypeExpr);
";
    let carriers = declared_hot_carriers(planted_missing_derive);
    assert!(
        carriers.contains(&"HotGood".to_string())
            && carriers.contains(&"HotMissingDerive".to_string()),
        "self-test: both synthetic carriers must be discovered; got {carriers:?}"
    );
    assert!(
        carrier_has_no_type_expr_derive(planted_missing_derive, "HotGood"),
        "self-test: `HotGood` carries the derive and MUST be detected as such"
    );
    assert!(
        !carrier_has_no_type_expr_derive(planted_missing_derive, "HotMissingDerive"),
        "self-test: `HotMissingDerive` lacks the derive and MUST be detected as MISSING it — if \
         this passed, the coverage guard would green-light a carrier that skips the compiler proof"
    );

    // The `assert_impl_all!` subject scan must capture exactly the named subjects.
    let subjects = assert_impl_all_no_type_expr_subjects(planted_missing_derive);
    assert!(
        subjects.contains(&"HotGood".to_string())
            && subjects.contains(&"HotMissingDerive".to_string()),
        "self-test: the assert-subject scan must capture both named subjects; got {subjects:?}"
    );

    // A carrier present but NOT asserted must be flagged by the missing-assert
    // arm: discriminate that path too.
    let planted_missing_assert = "\
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
struct HotNotAsserted { a: u32 }
";
    let not_asserted_subjects = assert_impl_all_no_type_expr_subjects(planted_missing_assert);
    assert!(
        !not_asserted_subjects.contains(&"HotNotAsserted".to_string()),
        "self-test: `HotNotAsserted` has no `assert_impl_all!` entry, so the subject scan must NOT \
         list it (the missing-assert arm then reds it); got {not_asserted_subjects:?}"
    );
}

/// The audited single exception to the hand-impl ban: the one
/// `impl … NoTypeExprWitness … for HotTypeRef` in `semantic_query.rs`. The
/// invariant allows EXACTLY this one witness — identified by an EXACT whole-ident
/// match on the self-type's last path segment (never a substring, so
/// `HotTypeRefAlias` / `HotTypeRefSneaky` are NOT exempted) AND by the FILE it is
/// found in (see [`is_audited_witness_file`], so a forged `HotTypeRef` /
/// `other::HotTypeRef` in any other file is NOT exempted). Any OTHER
/// hand-written `NoTypeExpr` / `NoTypeExprWitness` impl in
/// `verter_session/src/**` is a violation.
const AUDITED_HAND_WITNESS_SELF_TY: &str = "HotTypeRef";

/// A hand-written `impl … NoTypeExpr[Witness] … for <SelfTy>` discovered in a
/// source file: the self-type's last path-segment ident (for the exact audited
/// match) plus a rendered form for the error message.
struct HandWrittenWitnessImpl {
    /// Last path segment ident of the impl's self type (e.g. `HotTypeRef`,
    /// `HotTypeRefAlias`, `SneakyForgery`). Whole-ident — the audited-exception
    /// check compares this with `==`, never `contains`/`starts_with`.
    self_ty: String,
    /// Human-readable `impl <Trait> for <SelfTy>` rendering for diagnostics.
    rendered: String,
}

/// The last path-segment ident of a `syn::Type`, if it is a (possibly qualified)
/// path type — `verter_no_typeexpr::__private::NoTypeExprWitness` → `Some(
/// "NoTypeExprWitness")`, `HotTypeRefAlias` → `Some("HotTypeRefAlias")`. Non-path
/// self/trait types (references, tuples, …) yield `None`.
fn type_path_last_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(tp) => tp.path.segments.last().map(|seg| seg.ident.to_string()),
        _ => None,
    }
}

/// Collects every hand-written `NoTypeExpr[Witness]` trait impl ANYWHERE in a
/// parsed file — at file scope, inside an inline `mod m { … }`, inside a `fn`
/// body, or nested inside another impl. Driven by `syn::visit::Visit` so the
/// walk reaches every `syn::ItemImpl` regardless of its lexical nesting; a
/// top-level-only `for item in &file.items` loop would miss any impl below the
/// first level (e.g. inside an inline module), which a production file may
/// carry.
struct WitnessImplCollector {
    hits: Vec<HandWrittenWitnessImpl>,
}

impl<'ast> syn::visit::Visit<'ast> for WitnessImplCollector {
    fn visit_item_impl(&mut self, imp: &'ast syn::ItemImpl) {
        if let Some((_, trait_path, _)) = &imp.trait_ {
            if let Some(trait_ident) = trait_path.segments.last().map(|seg| seg.ident.to_string()) {
                if trait_ident == "NoTypeExpr" || trait_ident == "NoTypeExprWitness" {
                    let self_ty = type_path_last_ident(&imp.self_ty)
                        .unwrap_or_else(|| "<non-path self type>".to_string());
                    let trait_rendered = trait_path
                        .segments
                        .iter()
                        .map(|seg| seg.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::");
                    self.hits.push(HandWrittenWitnessImpl {
                        rendered: format!("impl {trait_rendered} for {self_ty} {{}}"),
                        self_ty,
                    });
                }
            }
        }
        // Default recursion: an impl nested inside this impl (or anything below
        // it) is reached too — harmless completeness.
        syn::visit::visit_item_impl(self, imp);
    }
}

/// Hand-written `impl … NoTypeExpr[Witness] … for …` items in `src`. The blanket
/// bridge makes the public `NoTypeExpr` non-hand-implementable (a manual impl is
/// `E0119`), but the HIDDEN `NoTypeExprWitness` CAN be hand-written for a local
/// type — that is the one route that satisfies the bound WITHOUT the
/// field-recursive derive, so it must be banned save the audited `HotTypeRef`
/// exception.
///
/// Parses the file with `syn` (consistent with the sibling coverage guard
/// `every_hot_carrier_opts_into_no_type_expr`) and RECURSIVELY visits every
/// `syn::ItemImpl` via `syn::visit::Visit` — at file scope AND inside inline
/// `mod` blocks / fn bodies. A TRAIT impl whose trait path's LAST segment ident
/// is `NoTypeExpr` or `NoTypeExprWitness` is a hand-written witness impl.
/// Resolving the item (not the text) means a LINE-SPLIT or reformatted
/// `impl …\n    for X {}` is caught identically to a single-line one — the
/// previous per-line `starts_with("impl") && contains(...) && contains(" for ")`
/// scan missed any impl split across lines. Visiting recursively (not the prior
/// top-level `for item in &file.items` loop) additionally catches an impl nested
/// inside an inline module or fn body, which a top-level-only walk skipped. The
/// derive emits its `impl` from a proc-macro token stream, never as a source
/// `impl … for` item, so a `#[derive(NoTypeExpr)]` carrier is not flagged.
fn hand_written_no_type_expr_impls(src: &str) -> Vec<HandWrittenWitnessImpl> {
    let file = syn::parse_file(src).expect("parse production source as a syn file");
    let mut collector = WitnessImplCollector { hits: Vec::new() };
    syn::visit::Visit::visit_file(&mut collector, &file);
    collector.hits
}

/// Whether the relative production-source path `rel` is the SINGLE audited
/// witness file. The audited-exception gate is FILE-PRECISE on the FULL relative
/// path (NOT the basename): the one sanctioned witness is THE
/// `impl …NoTypeExprWitness for HotTypeRef {}` in `src/semantic_query.rs`, so a
/// same-named file in ANY subdirectory (`src/foo/semantic_query.rs`) is NOT
/// audited — a basename-only match would have wrongly exempted such an impostor.
/// `production_src_files` already yields the `/`-normalized `src/...` form;
/// normalize `\` → `/` here belt-and-suspenders so a Windows-style
/// `src\semantic_query.rs` `rel` also matches on any host.
fn is_audited_witness_file(rel: &str) -> bool {
    rel.replace('\\', "/") == "src/semantic_query.rs"
}

#[test]
fn no_hand_written_no_type_expr_impls_except_audited_hot_type_ref() {
    // DEFENSE-IN-DEPTH, honestly scoped — NOT the semantic proof. The compiler
    // (derive + assert_impl_all) owns transitive type meaning. This bans the one
    // hand-written-witness escape hatch everywhere in production source except
    // the single audited `HotTypeRef` witness.
    //
    // The `syn::visit` scan RECURSES into nested items (inline `mod` blocks, fn
    // bodies), so a witness impl is caught regardless of lexical nesting, not
    // only at file scope. The audited exception is FILE-PRECISE: the EXACT
    // `HotTypeRef` self type is exempt ONLY when found in `semantic_query.rs`, so
    // a forged `HotTypeRef` (or `other::HotTypeRef`, whose last segment is also
    // `HotTypeRef`) witness in any OTHER file is a violation — it cannot
    // masquerade as the one sanctioned witness.
    //
    // Two residuals remain, both DELIBERATE-hostile (not accidental drift):
    //   (1) a `use ...::NoTypeExprWitness as Alias; impl Alias for X` trait-name
    //       alias — the trait path's last segment would be `Alias`, not
    //       `NoTypeExprWitness`, so this `syn` scan does not flag it; and
    //   (2) a witness impl emitted from a MACRO token stream — `syn::visit` does
    //       NOT descend into macro token bodies (a documented syn limitation;
    //       see `crates/verter_session/Cargo.toml`), so an `impl` generated
    //       inside a macro invocation is invisible to this walk.
    // Both are backstopped by the field-recursive `#[derive(NoTypeExpr)]` +
    // `assert_impl_all!` on every carrier — a forged witness cannot hide a
    // `TypeExpr` in a derived carrier's field (the field-recursion still fails
    // the build), which is the real semantic proof. With the recursive walk and
    // the full-path-precise exemption, this scan stays DEFENSE-IN-DEPTH for the
    // realistic accidental case — any formatting, any line-split, any nesting —
    // not the semantic proof.
    let mut violations = Vec::new();
    let mut audited_seen = false;
    for (rel, src) in production_src_files() {
        let in_audited_file = is_audited_witness_file(&rel);
        for hit in hand_written_no_type_expr_impls(&src) {
            // EXACT whole-ident match on the self type AND file-precision — the
            // exemption applies ONLY to `HotTypeRef` in `semantic_query.rs`.
            // `HotTypeRefAlias` / `HotTypeRefSneaky` (wrong ident) and a
            // `HotTypeRef` in any other file (wrong file) are NOT exempted (a
            // `contains` on the ident, or an ident-only match without the file
            // gate, would have wrongly exempted them).
            if hit.self_ty == AUDITED_HAND_WITNESS_SELF_TY && in_audited_file {
                audited_seen = true;
                continue;
            }
            violations.push(format!("{rel}: {}", hit.rendered));
        }
    }
    assert!(
        violations.is_empty(),
        "no hand-written `NoTypeExpr`/`NoTypeExprWitness` impl may appear in \
         `verter_session/src/**` except the single audited witness for `{AUDITED_HAND_WITNESS_SELF_TY}` \
         in semantic_query.rs — found: {violations:?}. A new type that needs the marker must \
         `#[derive(NoTypeExpr)]` (field-recursive), never hand-write the witness."
    );
    assert!(
        audited_seen,
        "the audited `impl … NoTypeExprWitness for {AUDITED_HAND_WITNESS_SELF_TY}` must be PRESENT in \
         verter_session/src — its absence means the single sanctioned witness was deleted (the \
         `HotTypeRef` handle would then fail its own `assert_impl_all!`), not that the ban is clean."
    );
}

#[test]
fn no_hand_written_no_type_expr_impls_self_test_discriminates() {
    // The detector must FIRE on a planted second hand-impl and PASS the audited
    // `HotTypeRef` one — so a future weakening cannot silently re-open the
    // hand-witness route.
    let planted = "\
impl verter_no_typeexpr::__private::NoTypeExprWitness for SneakyForgery {}
impl verter_no_typeexpr::__private::NoTypeExprWitness for HotTypeRef {}
";
    let hits = hand_written_no_type_expr_impls(planted);
    assert!(
        hits.iter().any(|h| h.self_ty == "SneakyForgery"),
        "self-test: the scan MUST flag a planted second hand-witness `SneakyForgery` — if it \
         missed it, a forged witness on a non-derived type would pass; got selves {:?}",
        hits.iter().map(|h| &h.self_ty).collect::<Vec<_>>()
    );
    assert!(
        hits.iter()
            .any(|h| h.self_ty == AUDITED_HAND_WITNESS_SELF_TY),
        "self-test: the scan MUST also see the audited `HotTypeRef` witness (so the allowlist arm \
         is reachable); got selves {:?}",
        hits.iter().map(|h| &h.self_ty).collect::<Vec<_>>()
    );

    // NEGATIVE control: a `#[derive(NoTypeExpr)]` line (what every carrier uses)
    // must NOT be mistaken for a hand-written impl — the derive emits no
    // `impl … for` source item (it expands from a proc-macro token stream).
    let good = "\
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
struct HotThing { a: u32 }
";
    let good_hits = hand_written_no_type_expr_impls(good);
    assert!(
        good_hits.is_empty(),
        "self-test: a `#[derive(NoTypeExpr)]` carrier must NOT be flagged as a hand-written impl — \
         the derive is the sanctioned route; got selves {:?}",
        good_hits.iter().map(|h| &h.self_ty).collect::<Vec<_>>()
    );
}

#[test]
fn hand_impl_scan_catches_line_split_impl_a_single_line_scan_would_miss() {
    // FINDING-1 REGRESSION: a hand-witness impl SPLIT across lines —
    //   impl verter_no_typeexpr::__private::NoTypeExprWitness
    //       for SneakyForgery {}
    // — has NO single source line that is `starts_with("impl")` AND
    // `contains("NoTypeExpr")` AND `contains(" for ")` simultaneously, so the
    // previous per-line detector EVADED it. The `syn` scan resolves the IMPL
    // ITEM regardless of formatting, so it flags the split impl identically.
    let line_split = "\
impl verter_no_typeexpr::__private::NoTypeExprWitness
    for SneakyForgery {}
";
    let hits = hand_written_no_type_expr_impls(line_split);
    assert!(
        hits.iter().any(|h| h.self_ty == "SneakyForgery"),
        "self-test: the `syn` scan MUST flag a LINE-SPLIT hand-witness impl for `SneakyForgery` — \
         the prior single-line detector missed it; got selves {:?}",
        hits.iter().map(|h| &h.self_ty).collect::<Vec<_>>()
    );

    // Discriminating proof that the OLD single-line predicate genuinely MISSED
    // this exact shape — no individual trimmed line satisfies all three of the
    // old conjuncts, so the legacy scan would have produced ZERO hits here.
    let legacy_would_have_hit = line_split.lines().any(|raw| {
        let line = raw.trim();
        line.starts_with("impl") && line.contains("NoTypeExpr") && line.contains(" for ")
    });
    assert!(
        !legacy_would_have_hit,
        "self-test invariant: the legacy single-line scan must MISS this line-split impl (that is \
         the bug the `syn` rewrite closes) — if a single line now satisfies all three conjuncts, \
         this regression no longer discriminates the fix"
    );
}

#[test]
fn hand_impl_audited_exception_is_exact_self_ty_not_prefix() {
    // FINDING-2 REGRESSION: the audited exception is the EXACTLY-`HotTypeRef`
    // self type. A self type that merely STARTS with `HotTypeRef`
    // (`HotTypeRefAlias`, `HotTypeRefSneaky`) is a VIOLATION — the prior
    // `contains("NoTypeExprWitness for HotTypeRef")` substring match wrongly
    // exempted them. Both the exact-`HotTypeRef` exemption and the
    // `HotTypeRefAlias` violation are exercised through the SAME classification
    // the production guard uses (exact `== AUDITED_HAND_WITNESS_SELF_TY`).
    let planted = "\
impl verter_no_typeexpr::__private::NoTypeExprWitness for HotTypeRef {}
impl verter_no_typeexpr::__private::NoTypeExprWitness for HotTypeRefAlias {}
";
    let hits = hand_written_no_type_expr_impls(planted);

    // The exact `HotTypeRef` impl is recognised as the audited exception.
    assert!(
        hits.iter().any(|h| h.self_ty == AUDITED_HAND_WITNESS_SELF_TY),
        "self-test: the EXACT `HotTypeRef` self type must be present as the audited exception; got \
         selves {:?}",
        hits.iter().map(|h| &h.self_ty).collect::<Vec<_>>()
    );

    // `HotTypeRefAlias` is captured with a DISTINCT whole self-ty that does NOT
    // equal the audited self type — so the production guard's
    // `hit.self_ty == AUDITED_HAND_WITNESS_SELF_TY` classifier treats it as a
    // VIOLATION (it would have been wrongly exempted by a `contains` check).
    let alias_self = hits
        .iter()
        .map(|h| h.self_ty.as_str())
        .find(|s| *s == "HotTypeRefAlias")
        .expect("self-test: the `HotTypeRefAlias` impl must be discovered as a hand-witness impl");
    assert_ne!(
        alias_self, AUDITED_HAND_WITNESS_SELF_TY,
        "self-test: `HotTypeRefAlias` must NOT equal the audited self type — the exact whole-ident \
         match is what stops a `HotTypeRef`-prefixed name from stealing the exemption"
    );

    // Belt-and-braces: replicate the production guard's split and assert the
    // alias lands in the violations bucket, the exact name in the audited bucket.
    let mut violations = Vec::new();
    let mut audited_seen = false;
    for hit in &hits {
        if hit.self_ty == AUDITED_HAND_WITNESS_SELF_TY {
            audited_seen = true;
        } else {
            violations.push(hit.self_ty.clone());
        }
    }
    assert!(
        audited_seen,
        "self-test: exact `HotTypeRef` must be exempted"
    );
    assert!(
        violations.contains(&"HotTypeRefAlias".to_string()),
        "self-test: `HotTypeRefAlias` must be flagged as a violation, not exempted; violations = \
         {violations:?}"
    );
}

#[test]
fn hand_impl_scan_recurses_into_inline_module_a_top_level_walk_would_miss() {
    // REGRESSION: a hand-witness impl nested INSIDE an inline `mod m { … }` —
    //   mod evil_inner {
    //       impl …NoTypeExprWitness for NestedForgery {}
    //   }
    // — is NOT a top-level item; a `for item in &file.items` walk visits only the
    // `Item::Mod` and never descends, so it returned ZERO hits for the nested
    // impl. The `syn::visit` rewrite reaches every `ItemImpl` regardless of
    // nesting, so it flags `NestedForgery`.
    let nested = "\
mod evil_inner {
    impl verter_no_typeexpr::__private::NoTypeExprWitness for NestedForgery {}
}
";
    let hits = hand_written_no_type_expr_impls(nested);
    assert!(
        hits.iter().any(|h| h.self_ty == "NestedForgery"),
        "self-test: the recursive scan MUST flag a hand-witness impl nested inside an inline \
         module (`NestedForgery`) — a top-level-only walk missed it; got selves {:?}",
        hits.iter().map(|h| &h.self_ty).collect::<Vec<_>>()
    );

    // Discriminating proof that the OLD top-level-only walk genuinely MISSED this
    // exact shape: replicate the prior `for item in &file.items` loop here and
    // assert it produces ZERO `NestedForgery` hits — so the recursion (not a
    // formatting accident) is what closes the gap.
    let top_level_only_misses = {
        let file = syn::parse_file(nested).expect("parse nested-impl fixture");
        let mut found = false;
        for item in &file.items {
            let syn::Item::Impl(imp) = item else {
                continue;
            };
            let Some((_, trait_path, _)) = &imp.trait_ else {
                continue;
            };
            let Some(trait_ident) = trait_path.segments.last().map(|seg| seg.ident.to_string())
            else {
                continue;
            };
            if trait_ident != "NoTypeExpr" && trait_ident != "NoTypeExprWitness" {
                continue;
            }
            if type_path_last_ident(&imp.self_ty).as_deref() == Some("NestedForgery") {
                found = true;
            }
        }
        found
    };
    assert!(
        !top_level_only_misses,
        "self-test invariant: the legacy top-level-only walk must MISS this inline-module-nested \
         impl (that is the coverage gap the recursive rewrite closes) — if a top-level walk now \
         sees `NestedForgery`, this regression no longer discriminates the fix"
    );
}

#[test]
fn hand_impl_audited_exception_is_file_precise_not_ident_only() {
    // REGRESSION: the audited exception is FILE-PRECISE. The single sanctioned
    // witness is `impl …NoTypeExprWitness for HotTypeRef {}` in `semantic_query.rs`.
    // A forged `HotTypeRef` (or `other::HotTypeRef`, whose last segment is also
    // `HotTypeRef`) witness in any OTHER production file is a VIOLATION — an
    // ident-only exemption (`hit.self_ty == AUDITED…` without the file gate) would
    // have wrongly exempted it. Drive the production guard's file-gated
    // classification directly via `is_audited_witness_file`.

    // The file gate itself is FULL-PATH-exact: only the EXACT relative path
    // `src/semantic_query.rs` (under any separator) is the audited file.
    assert!(
        is_audited_witness_file("src/semantic_query.rs"),
        "self-test: `src/semantic_query.rs` IS the audited witness file"
    );
    assert!(
        is_audited_witness_file("src\\semantic_query.rs"),
        "self-test: a Windows-style `src\\semantic_query.rs` IS the audited witness file (the gate \
         is path-separator-portable)"
    );
    // DISCRIMINATING: a same-named file in a SUBDIRECTORY (`src/foo/semantic_query.rs`)
    // is NOT the audited file — full-path-exact rejects it, whereas a basename-only
    // gate (`rsplit(['/','\\']).next() == Some("semantic_query.rs")`) would have
    // WRONGLY exempted it. This sub-assertion FAILS against the basename gate and
    // PASSES against the full-path gate.
    assert!(
        !is_audited_witness_file("src/foo/semantic_query.rs"),
        "self-test: a same-named file in a SUBDIRECTORY (`src/foo/semantic_query.rs`) is NOT the \
         audited witness file — the gate is full-path-exact, not basename-only; a basename-only \
         match would have wrongly exempted this impostor"
    );
    assert!(
        !is_audited_witness_file("src/resolver_core/hot_prepared.rs"),
        "self-test: a non-`semantic_query.rs` file is NOT the audited witness file"
    );
    assert!(
        !is_audited_witness_file("src/other/semantic_query_helpers.rs"),
        "self-test: a file whose name merely CONTAINS `semantic_query` (but is not exactly \
         `semantic_query.rs`) is NOT the audited witness file"
    );

    // A `HotTypeRef` hit, classified through the SAME `self_ty == AUDITED && file`
    // gate the production guard uses, in the audited file is EXEMPT and in any
    // other file is a VIOLATION. Both forms (a plain `HotTypeRef` self type and a
    // qualified `other::HotTypeRef`, last segment `HotTypeRef`) are exercised.
    let forged_in_other_file = "\
impl verter_no_typeexpr::__private::NoTypeExprWitness for HotTypeRef {}
impl verter_no_typeexpr::__private::NoTypeExprWitness for other::HotTypeRef {}
";
    let hits = hand_written_no_type_expr_impls(forged_in_other_file);
    assert_eq!(
        hits.iter().filter(|h| h.self_ty == "HotTypeRef").count(),
        2,
        "self-test: both the plain and the `other::`-qualified `HotTypeRef` witnesses resolve to a \
         last-segment self type of `HotTypeRef`; got selves {:?}",
        hits.iter().map(|h| &h.self_ty).collect::<Vec<_>>()
    );

    // Classify under the audited file → both exempt, zero violations.
    let classify = |rel: &str| -> Vec<String> {
        let in_audited = is_audited_witness_file(rel);
        hits.iter()
            .filter(|h| !(h.self_ty == AUDITED_HAND_WITNESS_SELF_TY && in_audited))
            .map(|h| h.self_ty.clone())
            .collect()
    };
    assert!(
        classify("src/semantic_query.rs").is_empty(),
        "self-test: `HotTypeRef` witnesses in `semantic_query.rs` are exempt — zero violations there"
    );

    // Classify under ANY OTHER file → the file gate fails, so BOTH `HotTypeRef`
    // hits become violations. An ident-only exemption would have (wrongly)
    // exempted them.
    let violations_elsewhere = classify("src/resolver_core/hot_prepared.rs");
    assert_eq!(
        violations_elsewhere,
        vec!["HotTypeRef".to_string(), "HotTypeRef".to_string()],
        "self-test: a forged `HotTypeRef` / `other::HotTypeRef` witness in a NON-`semantic_query.rs` \
         file is a VIOLATION — the file gate is what stops it masquerading as the audited witness; \
         got {violations_elsewhere:?}"
    );

    // DISCRIMINATING (full-path-exact, not basename): classify under a SUBDIRECTORY
    // same-named file `src/foo/semantic_query.rs` → the file gate fails (the audited
    // path is the EXACT `src/semantic_query.rs`), so BOTH forged `HotTypeRef`
    // witnesses are VIOLATIONS there. A basename-only gate would have exempted them
    // (and produced zero violations), so this assertion FAILS against the basename
    // form and PASSES against the full-path form.
    let violations_in_subdir_same_name = classify("src/foo/semantic_query.rs");
    assert_eq!(
        violations_in_subdir_same_name,
        vec!["HotTypeRef".to_string(), "HotTypeRef".to_string()],
        "self-test: a forged `HotTypeRef` witness in a SUBDIRECTORY same-named file \
         `src/foo/semantic_query.rs` is a VIOLATION — only the EXACT `src/semantic_query.rs` path \
         is audited; a basename-only gate would have wrongly exempted it; got \
         {violations_in_subdir_same_name:?}"
    );
}

// The transitive-`TypeExpr`-freedom of every carrier field is proven by the
// compiler `NoTypeExpr` derive + `assert_impl_all!` (above) — which resolve the
// real field type, so an aliased / re-exported / nested `TypeExpr` owner fails
// the build. The coverage + hand-impl guards above are the only source-level
// rails, and neither re-proves type meaning.
// NOTE on the HotTypeRef R6 non-keyability check. It is enforced by TWO rails,
// neither of which is a source-text scan in THIS file:
//
//   (1) the DERIVE vector — `hot_type_ref_is_distinct_handle_and_not_hash_or_ord_derived`
//       in `tests/cases/architecture_guards.rs`, which extracts the FULL
//       stacked-derive vector via the shared `carrier_struct_derive_list`
//       helper (unioning EVERY `#[derive(...)]` line above the struct) and
//       rejects `Hash`/`Ord` whole-tokens; and
//   (2) any IMPL form — a COMPILER assertion next to the struct in production
//       source: `assert_not_impl_any!(HotTypeRef: std::hash::Hash, std::cmp::Ord,
//       std::cmp::PartialOrd);` in `semantic_query.rs`. It fails to COMPILE if
//       `HotTypeRef` ever implements any of those traits — by derive OR by a
//       hand-written `impl` ANYWHERE in the crate, under ANY import aliasing.
//
// The compiler assertion strictly SUPERSEDES a source-text manual-impl scanner
// (a scan can only see one file and is evadable by file location or import
// aliasing). `assert_not_impl_any!` closes the hand-written-`impl` gap
// structurally — any-file, any-alias — so no source scan is duplicated in this
// file.

#[test]
fn verter_semantic_has_no_session_dep_is_confirmed_present() {
    // The hot carriers live in `verter_session` and reference `verter_semantic`
    // SCALAR types (ResolvedRootIdentity / TypeDeclKind / DeclProvenance / …) —
    // the ALLOWED direction (session → semantic). The REVERSE edge (which
    // would let the lower compat-DTO crate carry session `HotTypeRef` handles)
    // is banned by the EXISTING crate-level guard
    // `no_verter_semantic_to_verter_session_dep` in architecture_guards.rs.
    // That guard is crate-level, so the new `hot_prepared` module is
    // automatically covered. This test CONFIRMS the existing guard is present
    // (a real `fn` definition, not a hollow rename) rather than duplicating
    // it.
    let guards = read_rel("tests/cases/architecture_guards.rs");
    assert!(
        guards.contains("fn no_verter_semantic_to_verter_session_dep("),
        "the existing crate-level reverse-dep guard \
         `no_verter_semantic_to_verter_session_dep` must remain a real `fn` test in \
         architecture_guards.rs — it covers the new hot_prepared module (session → semantic is \
         the allowed direction; the reverse edge is banned)."
    );
    // Anti-vacuity: the guard's own subject (the reverse crate name) must be
    // named in its body, so a renamed-but-hollow guard fails here too.
    assert!(
        guards.contains("crates/verter_semantic/Cargo.toml"),
        "the reverse-dep guard must read `crates/verter_semantic/Cargo.toml` — confirming it is \
         the real crate-level dependency-direction check, not a hollow stub"
    );
}
