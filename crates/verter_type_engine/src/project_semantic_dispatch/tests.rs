//! Source-shape guards over the dispatcher and its query vocabulary. They
//! read only workspace sources, so they stay beside the dispatcher.

/// `SemanticQueryKey::Expand`, `ExpandMode`, `SemanticQueryApi::expand`,
/// `build_expand`, and `ExpandMode::` are absent across the workspace's
/// Rust crate sources and TypeScript packages. These identifiers are not
/// part of the projection-mode surface; this test fails loudly if any
/// survive.
///
/// This is the sole enforcement point. The retired-terminology CI scanner
/// that used to cover it alongside this test has been removed: it guarded
/// vocabulary from a completed refactor, and a name-keyed text scanner is
/// exactly the guard shape the landed-scanner bar forbids.
#[test]
fn expand_variant_and_expand_mode_absent_from_workspace() {
    use std::path::{Path, PathBuf};
    let workspace_root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("crates").is_dir())
        .expect("workspace root with crates/ dir")
        .to_path_buf();

    // Each needle is followed by a punctuation character so it cannot
    // prefix-match an unrelated identifier like `build_expanded_type_text`
    // or `SemanticQueryKey::Expanded` (a hypothetical future variant
    // outside this track). `ExpandMode` is bare because Rust requires the
    // `ExpandMode::Foo` prefix anywhere it surfaces — there is no
    // identifier whose first characters are `ExpandMode` followed by
    // anything other than `::` in this workspace.
    let needles = [
        "SemanticQueryKey::Expand ",
        "SemanticQueryKey::Expand{",
        "SemanticQueryKey::Expand,",
        "ExpandMode::",
        "SemanticQueryApi::expand(",
        "fn expand(",
        "build_expand(",
        "fn build_expand(",
    ];

    let exclude_files = [
        // The test itself contains the needle strings. Post-§5.2 split
        // the dispatcher module lives as a directory; the old singleton
        // path is retained in case anyone reconstructs it for grep
        // purposes.
        "project_semantic_dispatch.rs",
        "project_semantic_dispatch\\tests.rs",
        "project_semantic_dispatch/tests.rs",
        // Design docs that describe the retirement of the singleton path.
        "generic-navigation-prep-plan.md",
        "feedback-2026-04-19-gennav.md",
        "tmp-plan.md",
    ];

    let mut violations: Vec<String> = Vec::new();
    let mut visit = |path: &Path| {
        let lossy = path.to_string_lossy();
        if exclude_files.iter().any(|n| lossy.ends_with(n)) {
            return;
        }
        // build_expanded_type_text / build_expanded_type_expr are
        // unrelated text-construction helpers in
        // verter_semantic::analysis::macros — the script's needles are
        // tightened above (`build_expand(` and `fn build_expand`) to
        // avoid colliding with them.
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };
        for needle in &needles {
            if content.contains(needle) {
                violations.push(format!("{}: contains `{}`", path.display(), needle));
            }
        }
    };

    fn walk(dir: &std::path::Path, exts: &[&str], visit: &mut dyn FnMut(&std::path::Path)) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name();
            if p.is_dir() {
                if matches!(
                    name.to_string_lossy().as_ref(),
                    "target" | "node_modules" | ".git" | "dist" | "build" | "out"
                ) {
                    continue;
                }
                walk(&p, exts, visit);
            } else if exts.iter().any(|e| p.extension().is_some_and(|x| x == *e)) {
                visit(&p);
            }
        }
    }

    walk(&workspace_root.join("crates"), &["rs"], &mut visit);
    walk(
        &workspace_root.join("packages"),
        &["ts", "tsx", "js", "mjs", "cjs"],
        &mut visit,
    );
    assert!(
        violations.is_empty(),
        "Found forbidden Expand/ExpandMode/build_expand references (not part of the four-mode surface):\n{}",
        violations.join("\n")
    );
}

/// If `src` declares `enum <enum_name> { ... }` and that enum body declares a
/// top-level variant whose name is any of `variants`, return the matched
/// variant name; otherwise `None`.
///
/// The scan is SCOPED to the named enum's body ONLY — isolated by brace-
/// balanced matching from the `{` after the enum name to its matching close
/// brace, over comment-stripped source. This scoping is load-bearing: in
/// `semantic_query.rs`, `enum QueryError` legitimately has a
/// `RecursiveRef { name: Arc<str>, args: std::sync::Arc::from([]) }` variant (the
/// `Opaque(QueryError::RecursiveRef)` home), and isolating the
/// `SemanticNodeData` body excludes it so the §7.18 declaration scan never
/// false-trips on the QueryError variant.
///
/// A match is whole-variant-token: a trimmed body line that STARTS WITH the
/// variant name followed by `{`, `(`, `,`, or whitespace / end-of-line — so
/// `SomeRestThing` / `Restful` do NOT false-match a `Rest` needle.
fn enum_body_declares_variant(src: &str, enum_name: &str, variants: &[&str]) -> Option<String> {
    let stripped = strip_line_comments(src);
    let header = format!("enum {enum_name}");
    // Locate the declaration whose name is EXACTLY `enum_name` (the char after
    // the name must be a non-identifier boundary, so `enum Foo` does not match
    // an `enum FooBar` prefix-collision).
    let mut search_from = 0usize;
    let enum_pos = loop {
        let rel = stripped[search_from..].find(&header)?;
        let abs = search_from + rel;
        let after = abs + header.len();
        let boundary = match stripped[after..].chars().next() {
            None => true,
            Some(c) => c == '<' || c == '{' || c.is_whitespace(),
        };
        if boundary {
            break abs;
        }
        search_from = after;
    };
    // Brace-balance from the `{` opening the body to its matching close.
    let brace_rel = stripped[enum_pos..].find('{')?;
    let body_start = enum_pos + brace_rel + 1;
    let bytes = stripped.as_bytes();
    let mut depth = 1usize;
    let mut idx = body_start;
    let mut body_end = stripped.len();
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    body_end = idx;
                    break;
                }
            }
            _ => {}
        }
        idx += 1;
    }
    // Whole-variant-token scan over the isolated body lines.
    for raw in stripped[body_start..body_end].lines() {
        let line = raw.trim();
        for v in variants {
            if let Some(rest) = line.strip_prefix(v) {
                let delimited = match rest.chars().next() {
                    None => true,
                    Some(c) => c == '{' || c == '(' || c == ',' || c.is_whitespace(),
                };
                if delimited {
                    return Some((*v).to_string());
                }
            }
        }
    }
    None
}

/// Solver scratch-only node kinds (`Rest`, `RecursiveRef`) MUST NOT
/// have dedicated [`SemanticNodeData`] variants per §7.18. This is a
/// build-level invariant: walking the crate source and asserting the
/// variants are absent lets a future agent notice instantly if someone
/// tries to promote a scratch-only node into the publication graph.
///
/// Why each is scratch-only — never a graph variant:
/// - Standalone `Rest` (`...T` outside a tuple-element slot) is a
///   category error as a publishable type: a bare rest node has no
///   keyspace / members / projection / assignability of its own.
///   Tuple-rest fidelity is first-class metadata on `TupleElement.rest`,
///   round-tripped through the `Tuple` materialize arm — NOT a
///   `SemanticNodeData::Rest` carrier.
/// - A `RecursiveRef` back-edge is demand-time-minted and publishes as
///   [`SemanticNodeData::Opaque`] carrying `QueryError::RecursiveRef`,
///   which the reverse boundary raises to `TypeExpr::RecursiveRef` —
///   there is no dedicated `RecursiveRef` semantic-node variant.
///
/// `Infer` is intentionally NOT in this list: it has a concrete
/// semantic role as the named placeholder in a conditional's
/// `extends` clause, substituted in the true branch when the check
/// decides Assignable. Keeping it as a scratch-only shape would
/// conflict with the `InferBind` origin-edge lifecycle; the explicit
/// first-class variant avoids the scope-as-discriminator anti-pattern
/// structurally.
///
/// Each needle is followed by punctuation so it cannot prefix-
/// match an unrelated identifier (same discipline as
/// [`expand_variant_and_expand_mode_absent_from_workspace`]).
#[test]
fn solver_scratch_only_nodes_never_enter_semantic_graph_store() {
    use std::path::{Path, PathBuf};
    let workspace_root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("crates").is_dir())
        .expect("workspace root with crates/ dir")
        .to_path_buf();

    let needles = [
        "SemanticNodeData::Rest(",
        "SemanticNodeData::Rest{",
        "SemanticNodeData::Rest ",
        "SemanticNodeData::RecursiveRef{",
        "SemanticNodeData::RecursiveRef(",
        "SemanticNodeData::RecursiveRef ",
    ];

    let exclude_files = [
        "project_semantic_dispatch.rs",
        "project_semantic_dispatch\\tests.rs",
        "project_semantic_dispatch/tests.rs",
        "generic-navigation-prep-plan.md",
        "feedback-2026-04-19-gennav.md",
        "tmp-plan.md",
    ];

    let mut violations: Vec<String> = Vec::new();
    let mut visit = |path: &Path| {
        let lossy = path.to_string_lossy();
        if exclude_files.iter().any(|n| lossy.ends_with(n)) {
            return;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };
        for needle in &needles {
            if content.contains(needle) {
                violations.push(format!("{}: contains `{}`", path.display(), needle));
            }
        }
    };
    fn walk(dir: &std::path::Path, exts: &[&str], visit: &mut dyn FnMut(&std::path::Path)) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name();
            if p.is_dir() {
                if matches!(
                    name.to_string_lossy().as_ref(),
                    "target" | "node_modules" | ".git" | "dist" | "build" | "out"
                ) {
                    continue;
                }
                walk(&p, exts, visit);
            } else if exts.iter().any(|e| p.extension().is_some_and(|x| x == *e)) {
                visit(&p);
            }
        }
    }
    walk(&workspace_root.join("crates"), &["rs"], &mut visit);
    walk(
        &workspace_root.join("packages"),
        &["ts", "tsx", "js", "mjs", "cjs"],
        &mut visit,
    );
    assert!(
        violations.is_empty(),
        "Solver scratch-only nodes (Rest/RecursiveRef) must never appear as \
         SemanticNodeData variants — they stay solver-scratch per\nFound:\n{}",
        violations.join("\n")
    );

    // ── Declaration scan (backstop completeness) ────────────────────────
    //
    // The needle scan above only catches QUALIFIED usage
    // (`SemanticNodeData::Rest(...)` etc.). It NEVER inspects the
    // `enum SemanticNodeData { ... }` DECLARATION, where a re-added variant is
    // written `Rest { ... }` / `Rest(...)` WITHOUT the `SemanticNodeData::`
    // prefix and could then be used internally as `Self::Rest`. Add a scan of
    // the enum declaration itself so a standalone re-added `Rest` /
    // `RecursiveRef` variant cannot slip past the §7.18 backstop.
    let semantic_query_src = std::fs::read_to_string(
        workspace_root
            .join("crates")
            .join("verter_type_engine")
            .join("src")
            .join("semantic_query.rs"),
    )
    .expect("read semantic_query.rs for the SemanticNodeData declaration scan");
    // Anti-vacuity: the scanner actually isolated the real enum body (a known
    // current variant, `Alias`, is found) — so a `None` from the Rest/
    // RecursiveRef scan below means "no such variant", not "body not found".
    assert_eq!(
        enum_body_declares_variant(&semantic_query_src, "SemanticNodeData", &["Alias"]).as_deref(),
        Some("Alias"),
        "declaration scan must isolate the real `enum SemanticNodeData` body \
         (its `Alias` variant must be found) — a miss here means the body \
         isolation broke, not that Rest/RecursiveRef is absent"
    );
    if let Some(variant) = enum_body_declares_variant(
        &semantic_query_src,
        "SemanticNodeData",
        &["Rest", "RecursiveRef"],
    ) {
        panic!(
            "`enum SemanticNodeData` declares a scratch-only `{variant}` variant \
             — Rest/RecursiveRef must stay solver-scratch (§7.18) and never gain \
             a dedicated SemanticNodeData variant. `RecursiveRef` publishes as \
             `Opaque(QueryError::RecursiveRef)`; a standalone `Rest` is a \
             category error as a publishable type."
        );
    }

    // Self-discrimination for the declaration scanner (exercises the REAL
    // body-isolation + whole-token logic — never a bare `literal.contains`):
    //   SCOPING — a clean `SemanticNodeData` body alongside a SEPARATE
    //   `QueryError` enum carrying `RecursiveRef` must NOT trip: the body
    //   isolation excludes the QueryError variant (the exact real-file shape).
    let scoped = concat!(
        "pub enum SemanticNodeData {\n",
        "    Alias(SemanticNodeId),\n",
        "    Object(SurfaceView),\n",
        "}\n",
        "\n",
        "pub enum QueryError {\n",
        "    Miss,\n",
        "    RecursiveRef { name: Arc<str>, args: std::sync::Arc::from([]) },\n",
        "}\n",
    );
    assert!(
        enum_body_declares_variant(scoped, "SemanticNodeData", &["Rest", "RecursiveRef"]).is_none(),
        "self-test (scoping): a `QueryError::RecursiveRef` variant must NOT trip \
         the `SemanticNodeData` body scan — body isolation must exclude QueryError"
    );
    //   POSITIVE — a `SemanticNodeData` body re-adding a standalone `Rest`
    //   variant TRIPS the scan (even with a sibling QueryError::RecursiveRef).
    let with_rest = concat!(
        "pub enum SemanticNodeData {\n",
        "    Alias(SemanticNodeId),\n",
        "    Rest { inner: SemanticNodeId },\n",
        "}\n",
        "\n",
        "pub enum QueryError {\n",
        "    RecursiveRef { name: Arc<str>, args: std::sync::Arc::from([]) },\n",
        "}\n",
    );
    assert_eq!(
        enum_body_declares_variant(with_rest, "SemanticNodeData", &["Rest", "RecursiveRef"])
            .as_deref(),
        Some("Rest"),
        "self-test (positive): a re-added `Rest` variant in the SemanticNodeData \
         body must trip the declaration scan"
    );
    //   WHOLE-TOKEN — `Restful` / `SomeRestThing`-shaped variants must NOT
    //   false-match the `Rest` needle.
    let lookalikes = concat!(
        "pub enum SemanticNodeData {\n",
        "    Restful(SemanticNodeId),\n",
        "    SomeRestThing { inner: SemanticNodeId },\n",
        "}\n",
    );
    assert!(
        enum_body_declares_variant(lookalikes, "SemanticNodeData", &["Rest", "RecursiveRef"])
            .is_none(),
        "self-test (whole-token): `Restful` / `SomeRestThing` must NOT \
         false-match the `Rest` needle"
    );
}

/// Strip `//`-line comments from each line of `src` (everything from the first
/// `//` to end-of-line). Used so the brace-balanced enum-body isolation below
/// is not thrown off by `{` / `}` that appear only inside doc / line comments
/// (the `SemanticNodeData` rustdoc is full of `{ ... }` code examples), and so
/// the variant scan never false-matches a variant name that appears only in
/// prose.
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

mod substitution;
