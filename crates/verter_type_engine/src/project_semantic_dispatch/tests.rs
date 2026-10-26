//! Source-shape guards over the dispatcher and its query vocabulary. They
//! read only the engine's own sources, so they stay beside the dispatcher.

/// The retired expand surface: `SemanticQueryKey::Expand`, `ExpandMode`,
/// `SemanticQueryApi::expand` and `build_expand` are not part of the
/// projection-mode surface. Each needle is followed by a punctuation character
/// so it cannot prefix-match an unrelated identifier like
/// `build_expanded_type_text` or a hypothetical `SemanticQueryKey::Expanded`.
///
/// Most of the surface is held by the compiler: a use of a variant, type or
/// method that is not declared does not build, and
/// [`retired_query_vocabulary_is_not_declared`] keeps the key variant
/// undeclared. The names `fn expand(` and `build_expand(` are the part the
/// compiler cannot hold, so this list is matched against the engine crate's
/// sources, where the query surface is declared and implemented.
const EXPAND_NEEDLES: [&str; 8] = [
    "SemanticQueryKey::Expand ",
    "SemanticQueryKey::Expand{",
    "SemanticQueryKey::Expand,",
    "ExpandMode::",
    "SemanticQueryApi::expand(",
    "fn expand(",
    "build_expand(",
    "fn build_expand(",
];

/// Every needle's matches over the engine crate's Rust sources, from one
/// walk, as `path: contains <needle>` lines.
fn engine_source_matches(needles: &[&str]) -> Vec<String> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let matcher = aho_corasick::AhoCorasick::new(needles).expect("valid source needles");
    let mut matches = Vec::new();
    walk_sources(&src, &["rs"], &mut |path| {
        // This file names the needles.
        if path.ends_with("project_semantic_dispatch/tests.rs") {
            return;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };
        for (needle, hit) in needles.iter().zip(matching_needles(&matcher, &content)) {
            if hit {
                matches.push(format!("{}: contains `{needle}`", path.display()));
            }
        }
    });
    matches
}

/// The retired expand names are absent from the engine crate, which declares
/// and implements the query surface. The walk must reach that surface, or
/// the ban would pass vacuously.
#[test]
fn engine_sources_name_no_retired_expand_vocabulary() {
    let anchors = engine_source_matches(&[
        "pub enum SemanticQueryKey {",
        "pub trait SemanticQueryApi {",
    ]);
    assert_eq!(
        anchors.len(),
        2,
        "the walk must read the query surface: {anchors:?}"
    );
    let found = engine_source_matches(&EXPAND_NEEDLES);
    assert!(
        found.is_empty(),
        "Found forbidden Expand/ExpandMode/build_expand references (not part of the four-mode surface):\n{}",
        found.join("\n")
    );
}

/// The query key declares no `Expand` variant, so no crate can build a
/// `SemanticQueryKey::Expand` use.
#[test]
fn retired_query_vocabulary_is_not_declared() {
    let semantic_query_src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("semantic_query.rs"),
    )
    .expect("read semantic_query.rs");
    // Anti-vacuity: the scanner isolates the real enum body.
    assert!(
        enum_body_declares_variant(&semantic_query_src, "SemanticQueryKey", &["Instantiate"])
            .is_some(),
        "declaration scan must isolate the real `enum SemanticQueryKey` body"
    );
    assert_eq!(
        enum_body_declares_variant(&semantic_query_src, "SemanticQueryKey", &["Expand"]),
        None,
        "`enum SemanticQueryKey` must not declare a retired `Expand` variant"
    );
}

/// Scan each source once for all patterns. Overlapping matches are required:
/// `fn build_expand(` also contains `build_expand(`, and both diagnostics
/// belong to the original rule. Pattern order and one report per pattern
/// remain independent of the number and order of matches in the file.
fn matching_needles(matcher: &aho_corasick::AhoCorasick, content: &str) -> Vec<bool> {
    let mut found = vec![false; matcher.patterns_len()];
    for hit in matcher.find_overlapping_iter(content) {
        found[hit.pattern().as_usize()] = true;
    }
    found
}

fn walk_sources(dir: &std::path::Path, exts: &[&str], visit: &mut dyn FnMut(&std::path::Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // DirEntry caches ordinary entry types. Only links (or an unavailable
        // entry type) need metadata; following them preserves the scan scope.
        let is_dir = match entry.file_type() {
            Ok(kind) if !kind.is_symlink() => kind.is_dir(),
            _ => path.is_dir(),
        };
        if is_dir {
            if matches!(
                entry.file_name().to_string_lossy().as_ref(),
                "target" | "node_modules" | ".git" | "dist" | "build" | "out"
            ) {
                continue;
            }
            walk_sources(&path, exts, visit);
        } else if exts
            .iter()
            .any(|e| path.extension().is_some_and(|x| x == *e))
        {
            visit(&path);
        }
    }
}

#[test]
fn source_matching_preserves_overlaps_repetitions_and_identifier_boundaries() {
    let needles = [
        "build_expand(",
        "fn build_expand(",
        "ExpandMode::",
        "fn expand(",
    ];
    let matcher = aho_corasick::AhoCorasick::new(needles).unwrap();
    for content in [
        "fn build_expand( fn build_expand( ExpandMode::Value",
        "fn expand( ExpandMode::Value fn build_expand(",
        "fn build_expanded_type_text( ExpandedMode::Value fn expanded(",
        "文字 fn build_expand( // fn expand(",
        "",
    ] {
        assert_eq!(
            matching_needles(&matcher, content),
            needles.map(|needle| content.contains(needle)),
            "all original substring predicates must agree for {content:?}"
        );
    }
}

#[test]
fn source_walk_preserves_nested_extensions_and_excluded_directories() {
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("remove source-walk fixture");
        }
    }
    let root = std::env::temp_dir().join(format!("verter-source-walk-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let fixture = Fixture(root);
    for dir in [
        "src/nested",
        "target",
        "node_modules",
        ".git",
        "dist",
        "build",
        "out",
    ] {
        std::fs::create_dir_all(fixture.0.join(dir)).unwrap();
        std::fs::write(fixture.0.join(dir).join("covered.ts"), "").unwrap();
    }
    for name in [
        "a.rs",
        "b.ts",
        "c.tsx",
        "d.js",
        "e.mjs",
        "f.cjs",
        "ignore.txt",
    ] {
        std::fs::write(fixture.0.join(name), "").unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixture.0.join("src"), fixture.0.join("linked")).unwrap();
    let mut found = Vec::new();
    walk_sources(
        &fixture.0,
        &["ts", "tsx", "js", "mjs", "cjs"],
        &mut |path| {
            found.push(path.strip_prefix(&fixture.0).unwrap().to_path_buf());
        },
    );
    let mut expected: Vec<_> = [
        "b.ts",
        "c.tsx",
        "d.js",
        "e.mjs",
        "f.cjs",
        "src/nested/covered.ts",
    ]
    .map(std::path::PathBuf::from)
    .into();
    #[cfg(unix)]
    expected.push("linked/nested/covered.ts".into());
    found.sort();
    expected.sort();
    assert_eq!(found, expected);
    let mut rust = Vec::new();
    walk_sources(&fixture.0, &["rs"], &mut |path| {
        rust.push(path.to_path_buf())
    });
    assert_eq!(rust, [fixture.0.join("a.rs")]);
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
/// This test scans the `SemanticNodeData` declaration itself. With the
/// variants undeclared, no crate can build a qualified use of them, so the
/// declaration is the whole guard.
#[test]
fn solver_scratch_only_nodes_never_enter_semantic_graph_store() {
    use std::path::{Path, PathBuf};
    let workspace_root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("crates").is_dir())
        .expect("workspace root with crates/ dir")
        .to_path_buf();

    // ── Declaration scan (backstop completeness) ────────────────────────
    //
    // The workspace needle scan only catches QUALIFIED usage
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

/// The active-instantiation index answers as the stack it indexes: an
/// instantiation is refused while it is active, a declaration stays
/// active while any instantiation of it is, and an entry left from the
/// middle of the stack — a continuation frame stopped between its steps —
/// leaves every index without disturbing the entries around it.
#[test]
fn active_instantiation_index_follows_its_stack() {
    use std::sync::Arc;

    use super::{ActiveInstantiation, ActiveInstantiations};
    use crate::semantic_query::SemanticNodeId;

    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    let declaration = |name: &str| {
        (
            verter_type_expr::OwnedStr::from("/a.ts"),
            owner,
            verter_type_expr::OwnedStr::from(name),
        )
    };
    let entry = |name: &str, arg: u64| ActiveInstantiation {
        identity: declaration(name),
        args: Arc::from([SemanticNodeId(arg)]),
    };
    let mut active = ActiveInstantiations::default();
    assert!(active.push(entry("A", 1)));
    assert!(active.push(entry("B", 1)));
    assert!(
        active.push(entry("A", 2)),
        "A over other arguments is another instantiation"
    );
    assert!(
        !active.push(entry("A", 1)),
        "A over the same arguments is active"
    );
    assert_eq!(active.len(), 3);

    active.leave(&entry("A", 1));
    assert_eq!(active.len(), 2);
    assert!(
        active.declares("/a.ts", owner, "A"),
        "A over 2 is still active"
    );
    assert!(
        active.push(entry("A", 1)),
        "A over 1 left the index with the stack"
    );

    active.pop();
    active.pop();
    assert!(!active.declares("/a.ts", owner, "A"));
    assert!(active.declares("/a.ts", owner, "B"));
    active.pop();
    assert_eq!(active.len(), 0);
    assert!(!active.declares("/a.ts", owner, "B"));
    assert!(
        active.push(entry("B", 1)),
        "nothing is left once the stack is empty"
    );
}
