//! Handle-capable consumer guards (additive dual-read).
//!
//! Several component-meta consumers are being made HANDLE-CAPABLE: each
//! grows an additive sibling arm that accepts an ALREADY-LOWERED graph
//! node (a `SemanticNodeId` / `HotTypeRef`) and routes it through the
//! SAME query-time dispatch the `TypeExpr` arm reaches — read-compat,
//! ONE resolver. Two invariants protect that work and its ordering
//! against the later (breaking) producer wiring:
//!
//! - **G-A: the `materialize_type_expr(HotTypeRef)` reverse-handle
//!   boundary is TEST-ONLY (not production-visible).** A `HotTypeRef` is
//!   ALREADY the lowered node; a handle arm must reduce it directly, never
//!   materialise it back to a `TypeExpr` and re-lower. The
//!   `materialize_type_expr(HotTypeRef)` harness is `#[cfg(test)]`-gated,
//!   so production code cannot name it and a hot-arm reverse-bridge is a
//!   compile-time impossibility. G-A is now a STRUCTURAL `syn` guard
//!   (`materialize_type_expr_is_not_production_visible`) that asserts the
//!   single `fn materialize_type_expr` definition carries a `cfg(test)`
//!   gate — it REPLACES the former source LINE scanner
//!   (`no_hot_path_materialize_type_expr_bridge`, deleted). It is a
//!   DIFFERENT invariant from the output-materialization capability fence:
//!   the durable `SemanticNodeId -> TypeExpr` OUTPUT boundary is the sealed
//!   `OutputProjector` capability + sealed carriers (compiler-enforced; see
//!   `project_semantic_dispatch/output_materialization.rs`), and the named
//!   interim Kind-B `legacy_semantic_type_expr_bridge` is pinned by the
//!   separate `output_projector_residual_guards`. The global fence
//!   `hot_path_never_calls_materialize_type_expr` stays deferred to a later
//!   block.
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
// G-A — the `materialize_type_expr(HotTypeRef)` reverse-handle boundary is
// TEST-ONLY (structural `#[cfg(test)]`-gating guard). This is a DIFFERENT
// invariant from the output-materialization capability fence (the sealed
// `OutputProjector`) and from the Kind-B `legacy_semantic_type_expr_bridge`
// residual (pinned by `output_projector_residual_guards`).
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

/// Whether the `attrs` gate the item OUT of EVERY non-test build — i.e. the
/// `cfg` predicate ENTAILS the `test` flag (it cannot be satisfied unless
/// `test` is set). This is STRICTER than "names `test` somewhere": only a cfg
/// that is `false` in every build where `test` is unset counts.
///
/// Entailment (does `cfg(P)` hold ⟹ `test` is set?):
/// - `test`                      → YES (it IS `test`).
/// - `all(A, B, …)`              → YES iff ANY arm entails `test` (the
///   conjunction is `false` whenever that arm is `false`, i.e. when `test`
///   is unset).
/// - `any(A, B, …)`              → YES iff EVERY arm entails `test` (the
///   disjunction can still be `true` with `test` unset if any arm can be).
///   So `any(test, debug_assertions)` does NOT entail `test` —
///   `debug_assertions` is ON in ordinary debug builds, making the item
///   PRODUCTION-REACHABLE there. This is the load-bearing fix: the old
///   classifier counted any `test`-naming predicate as gated, blessing the
///   debug-build hole.
/// - anything else (`feature=…`, `debug_assertions`, `not(…)`, a bare other
///   ident) → NO.
///
/// Token-tree / `syn::Meta` inspection of the `cfg(...)` argument;
/// comment/string-blind by construction (a `syn` attribute carries no comment
/// or string-literal tokens that could spoof the `test` ident).
///
/// `pub(crate)` so the OutputProjector residual-guard module
/// (`output_projector_residual_guards`) reuses this ONE rigorous entailment
/// classifier rather than forking a second, cruder substring matcher — the
/// carrier `_for_test` gate guard and the OutputProjector impl-inventory both
/// need "does this cfg ENTAIL test", and divergent classifiers diverge.
pub(crate) fn attrs_have_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let syn::Meta::List(list) = &attr.meta else {
            return false;
        };
        cfg_tokens_entail_test(list.tokens.clone())
    })
}

/// Recursive entailment check over a `cfg(...)` predicate's token stream. See
/// [`attrs_have_cfg_test`] for the entailment rules. The stream is the inside
/// of one predicate: a bare `test` ident, an `all(...)` / `any(...)` /
/// `not(...)` head + parenthesised group, or some other predicate (`feature =
/// "x"`, `debug_assertions`).
///
/// `pub(crate)` — shared with `output_projector_residual_guards` (see
/// [`attrs_have_cfg_test`]).
pub(crate) fn cfg_tokens_entail_test(tokens: proc_macro2::TokenStream) -> bool {
    use proc_macro2::TokenTree;
    let toks: Vec<TokenTree> = tokens.into_iter().collect();
    // Bare `test` (the whole predicate is just the `test` ident).
    if toks.len() == 1 {
        if let TokenTree::Ident(id) = &toks[0] {
            return id == "test";
        }
        return false;
    }
    // `all(...)` / `any(...)` / `not(...)`: head ident + a single Group.
    if toks.len() == 2 {
        if let (TokenTree::Ident(head), TokenTree::Group(g)) = (&toks[0], &toks[1]) {
            let arms = split_top_level_comma(g.stream());
            return match head.to_string().as_str() {
                // Conjunction is false when ANY entailing arm is false ⇒
                // entails test iff ANY arm entails test.
                "all" => arms.into_iter().any(cfg_tokens_entail_test),
                // Disjunction can be true with test unset unless EVERY arm
                // requires test ⇒ entails test iff EVERY arm entails test.
                "any" => !arms.is_empty() && arms.into_iter().all(cfg_tokens_entail_test),
                // `not(test)` is satisfied when test is UNSET ⇒ does not
                // entail test; any other `not(...)` likewise does not.
                _ => false,
            };
        }
    }
    // `feature = "x"`, `debug_assertions`, or any other shape: does not entail test.
    false
}

/// Split a token stream on TOP-LEVEL commas (commas not nested inside a
/// `()` group), returning each comma-separated predicate as its own stream.
/// Nested groups (`any(a, b)`) stay intact within one arm.
///
/// `pub(crate)` — shared with `output_projector_residual_guards` (see
/// [`attrs_have_cfg_test`]).
pub(crate) fn split_top_level_comma(
    stream: proc_macro2::TokenStream,
) -> Vec<proc_macro2::TokenStream> {
    use proc_macro2::{TokenStream, TokenTree};
    let mut out: Vec<TokenStream> = Vec::new();
    let mut cur: Vec<TokenTree> = Vec::new();
    for tt in stream {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => {
                out.push(cur.drain(..).collect());
            }
            _ => cur.push(tt),
        }
    }
    if !cur.is_empty() {
        out.push(cur.into_iter().collect());
    }
    out
}

/// A single sanctioned `cfg(...)` arm for the
/// [`cfg_is_exactly_test_or_test_support`] EXACT recogniser: either the bare
/// `test` ident or the `feature = "test-support"` predicate. Anything else
/// (`unix`, `debug_assertions`, `feature = "prod"`, a nested `all`/`any`) is
/// production-satisfiable and is NOT an allowed arm.
fn cfg_arm_is_test_or_test_support(tokens: proc_macro2::TokenStream) -> bool {
    use proc_macro2::TokenTree;
    let toks: Vec<TokenTree> = tokens.into_iter().collect();
    // Bare `test`.
    if toks.len() == 1 {
        if let TokenTree::Ident(id) = &toks[0] {
            return id == "test";
        }
        return false;
    }
    // `feature = "test-support"` — three tokens: ident `feature`, `=`, a
    // string-literal whose value is exactly `test-support`.
    if toks.len() == 3 {
        if let (TokenTree::Ident(key), TokenTree::Punct(eq), TokenTree::Literal(lit)) =
            (&toks[0], &toks[1], &toks[2])
        {
            if key == "feature" && eq.as_char() == '=' {
                // The literal's string value (strip the surrounding quotes).
                let lit_str = lit.to_string();
                return lit_str == "\"test-support\"";
            }
        }
    }
    false
}

/// Whether a `cfg(...)` predicate's INNER token stream is EXACTLY one of the
/// two sanctioned production-UNREACHABLE shapes:
///   - `test` (the bare predicate), or
///   - `any(test, feature = "test-support")` — order-insensitive on the two
///     arms, but with EXACTLY those two arms and nothing else.
///
/// This is STRICTER than [`cfg_tokens_entail_test`] entailment on purpose:
/// entailment ACCEPTS `all(test, unix)` (genuinely test-only) but REJECTS
/// `any(test, feature = "test-support")` (the `feature` arm does not entail
/// `test`), which is the legitimate dev-/integration-binary gate. The carrier
/// `_for_test` accessor invariant is the canonical narrow gate ONLY — so a
/// disjunction carrying ANY extra production-satisfiable arm (`unix`,
/// `debug_assertions`, another `feature`) FAILS here. Token-tree parsed, never
/// substring-matched, so a reordered-but-valid `any(feature = "test-support",
/// test)` is accepted and a widened `any(test, feature = "test-support",
/// unix)` is rejected.
///
/// `pub(crate)` — shared with `output_projector_residual_guards` (see
/// [`attrs_have_cfg_test`]).
pub(crate) fn cfg_is_exactly_test_or_test_support(tokens: proc_macro2::TokenStream) -> bool {
    use proc_macro2::TokenTree;
    let toks: Vec<TokenTree> = tokens.clone().into_iter().collect();
    // Bare `test`.
    if toks.len() == 1 {
        if let TokenTree::Ident(id) = &toks[0] {
            return id == "test";
        }
        return false;
    }
    // `any( <arms> )`: head ident `any` + a single Group.
    if toks.len() == 2 {
        if let (TokenTree::Ident(head), TokenTree::Group(g)) = (&toks[0], &toks[1]) {
            if head == "any" {
                let arms = split_top_level_comma(g.stream());
                // EXACTLY two arms, one `test` and one `feature =
                // "test-support"` (order-insensitive). Any third arm, a
                // duplicate, or a non-sanctioned arm fails.
                if arms.len() != 2 {
                    return false;
                }
                let mut saw_test = false;
                let mut saw_feature = false;
                for arm in &arms {
                    let arm_toks: Vec<TokenTree> = arm.clone().into_iter().collect();
                    let is_bare_test = arm_toks.len() == 1
                        && matches!(&arm_toks[0], TokenTree::Ident(id) if id == "test");
                    if is_bare_test {
                        saw_test = true;
                    } else if cfg_arm_is_test_or_test_support(arm.clone()) {
                        // The non-`test` sanctioned arm is the
                        // `feature = "test-support"` predicate.
                        saw_feature = true;
                    } else {
                        // A production-satisfiable / unrecognised arm.
                        return false;
                    }
                }
                return saw_test && saw_feature;
            }
        }
    }
    false
}

/// Find every `fn materialize_type_expr` (the EXACT identifier, not a
/// prefix like `materialize_type_expr_until_stable`) defined as a method in
/// `raise.rs`, returning each one's `#[cfg(test)]` gating verdict. Walks
/// the `syn` AST — inherent impl methods at file scope and inside inline
/// modules — so a relocation into a nested `mod` is still seen. Returns the
/// `(is_cfg_test)` flag per definition.
fn materialize_type_expr_def_cfg_test_flags(src: &str) -> Vec<bool> {
    use syn::visit::Visit;
    struct V {
        flags: Vec<bool>,
    }
    impl<'ast> Visit<'ast> for V {
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            if f.sig.ident == "materialize_type_expr" {
                self.flags.push(attrs_have_cfg_test(&f.attrs));
            }
            syn::visit::visit_impl_item_fn(self, f);
        }
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            if f.sig.ident == "materialize_type_expr" {
                self.flags.push(attrs_have_cfg_test(&f.attrs));
            }
            syn::visit::visit_item_fn(self, f);
        }
    }
    let file = syn::parse_file(src).expect("parse raise.rs as a syn file");
    let mut v = V { flags: Vec::new() };
    v.visit_file(&file);
    v.flags
}

#[test]
fn materialize_type_expr_is_not_production_visible() {
    // STRUCTURAL G-A (replaces the deleted line scanner). The
    // `materialize_type_expr(HotTypeRef)` reverse-handle boundary is a
    // TEST-ONLY harness: it must be `#[cfg(test)]`-gated so it is NOT
    // present in production / release builds, and therefore cannot be a
    // reverse-`materialize` re-lower path in any hot handle arm. We assert
    // STRUCTURALLY (via `syn`) that every `fn materialize_type_expr`
    // definition carries a `cfg(test)` gate — a future un-gating (making it
    // production-visible) fails here, which is the structural successor to
    // the old "no production reference" source scan. Once production cannot
    // name the boundary, a production reverse-bridge is a compile-time
    // impossibility, so no call-site scanner is needed.
    const RAISE_REL: &str = "src/project_semantic_dispatch/raise.rs";
    let src = read_rel(RAISE_REL);
    let flags = materialize_type_expr_def_cfg_test_flags(&src);
    assert_eq!(
        flags.len(),
        1,
        "G-A: expected EXACTLY ONE `fn materialize_type_expr` method definition in {RAISE_REL}; \
         found {} — a second definition would create an un-audited boundary",
        flags.len()
    );
    assert!(
        flags[0],
        "G-A (STRUCTURAL): `fn materialize_type_expr` in {RAISE_REL} MUST be `#[cfg(test)]`-gated \
         so it is not production-visible — an un-gated definition would re-introduce a \
         production reverse-`materialize` boundary that a hot handle arm could call. Gate it with \
         `#[cfg(test)]` (or move its production use onto the sealed `OutputProjector` capability)."
    );
}

#[test]
fn materialize_type_expr_not_production_visible_self_test_discriminates() {
    // The classifier the structural guard relies on must FIRE on a
    // production-visible (un-gated) definition and PASS the `#[cfg(test)]`
    // form — proving it would catch a regression that un-gates the
    // boundary.
    // PASS: the cfg-test form (post-fence shape).
    let gated = r#"
        impl<'a> ProjectSemanticDispatch<'a> {
            #[cfg(test)]
            pub(crate) fn materialize_type_expr(&self, handle: HotTypeRef) -> TypeExpr {
                self.output_shell_raise(handle.node()).unwrap()
            }
        }
    "#;
    let gated_flags = materialize_type_expr_def_cfg_test_flags(gated);
    assert_eq!(
        gated_flags,
        vec![true],
        "self-test: a `#[cfg(test)]` def MUST classify as gated"
    );

    // PASS: an `all(test, …)` form ENTAILS test (false in every non-test
    // build), so it classifies as gated.
    let all_gated = r#"
        impl<'a> ProjectSemanticDispatch<'a> {
            #[cfg(all(test, feature = "x"))]
            fn materialize_type_expr(&self, h: HotTypeRef) -> TypeExpr { todo!() }
        }
    "#;
    assert_eq!(
        materialize_type_expr_def_cfg_test_flags(all_gated),
        vec![true],
        "self-test: a `cfg(all(test, …))` def MUST classify as gated (it entails test)"
    );

    // FIRE (RED): a `cfg(any(test, debug_assertions))` form does NOT entail
    // test — `debug_assertions` is ON in ordinary debug builds, so the item
    // is PRODUCTION-REACHABLE there. It MUST classify as NOT test-gated. This
    // is the load-bearing fix for the debug-build blind spot the review found:
    // the old classifier blessed this `any(...)` widening as gated, so a
    // contributor widening the boundary from `cfg(test)` to
    // `cfg(any(test, debug_assertions))` (debug-build-present) would have kept
    // the guard GREEN.
    let any_test_debug = r#"
        impl<'a> ProjectSemanticDispatch<'a> {
            #[cfg(any(test, debug_assertions))]
            fn materialize_type_expr(&self, h: HotTypeRef) -> TypeExpr { todo!() }
        }
    "#;
    assert_eq!(
        materialize_type_expr_def_cfg_test_flags(any_test_debug),
        vec![false],
        "self-test: a `cfg(any(test, debug_assertions))` def MUST classify as NOT test-gated — it \
         is production-reachable in ordinary debug builds; the structural fence would otherwise \
         miss a debug-build-present widening regression"
    );

    // FIRE (RED): a production-visible (un-gated) definition must classify
    // as NOT gated — the structural guard would otherwise miss an un-gating
    // regression.
    let ungated = r#"
        impl<'a> ProjectSemanticDispatch<'a> {
            pub(crate) fn materialize_type_expr(&self, handle: HotTypeRef) -> TypeExpr {
                self.output_shell_raise(handle.node()).unwrap()
            }
        }
    "#;
    assert_eq!(
        materialize_type_expr_def_cfg_test_flags(ungated),
        vec![false],
        "self-test: a production-visible (un-gated) def MUST classify as NOT gated — the \
         structural fence would otherwise miss a visibility-widening regression"
    );

    // FIRE (RED): a `cfg(feature = "x")` gate that does NOT name `test`
    // must classify as NOT test-gated (production-reachable under that
    // feature).
    let feature_only = r#"
        impl<'a> ProjectSemanticDispatch<'a> {
            #[cfg(feature = "oracle-gen")]
            fn materialize_type_expr(&self, h: HotTypeRef) -> TypeExpr { todo!() }
        }
    "#;
    assert_eq!(
        materialize_type_expr_def_cfg_test_flags(feature_only),
        vec![false],
        "self-test: a feature-only `cfg` that does not name `test` MUST classify as NOT \
         test-gated"
    );
}

#[test]
fn cfg_is_exactly_test_or_test_support_self_test_discriminates() {
    // The EXACT recogniser (the carrier `_for_test` gate invariant) must
    // ACCEPT only the two canonical narrow production-unreachable shapes and
    // REJECT a disjunction carrying ANY extra production-satisfiable arm. It
    // is parsed (token-tree), not substring-matched, so a reordered-but-valid
    // form is accepted and a widened form is rejected.
    //
    // Build the inner predicate token stream directly from source so the
    // recogniser sees exactly what `attr.meta` (a `Meta::List`) would carry.
    fn inner(src: &str) -> proc_macro2::TokenStream {
        // `src` is the FULL attribute, e.g. `#[cfg(any(test, feature = "x"))]`.
        let file: syn::File = syn::parse_str(&format!("{src}\nfn __probe() {{}}"))
            .expect("parse synthetic cfg attribute");
        let func = file
            .items
            .iter()
            .find_map(|it| match it {
                syn::Item::Fn(f) if f.sig.ident == "__probe" => Some(f),
                _ => None,
            })
            .expect("find __probe fn");
        let attr = func
            .attrs
            .iter()
            .find(|a| a.path().is_ident("cfg"))
            .expect("find cfg attr");
        match &attr.meta {
            syn::Meta::List(list) => list.tokens.clone(),
            _ => panic!("cfg attr is not a Meta::List"),
        }
    }

    // PASS: bare `cfg(test)`.
    assert!(
        cfg_is_exactly_test_or_test_support(inner("#[cfg(test)]")),
        "self-test: `#[cfg(test)]` MUST be accepted by the EXACT recogniser"
    );
    // PASS: `cfg(any(test, feature = "test-support"))`.
    assert!(
        cfg_is_exactly_test_or_test_support(inner("#[cfg(any(test, feature = \"test-support\"))]")),
        "self-test: `#[cfg(any(test, feature = \"test-support\"))]` MUST be accepted"
    );
    // PASS: order-insensitive — `cfg(any(feature = "test-support", test))`.
    assert!(
        cfg_is_exactly_test_or_test_support(inner("#[cfg(any(feature = \"test-support\", test))]")),
        "self-test: arm order MUST NOT matter — `any(feature = \"test-support\", test)` is the \
         same predicate and MUST be accepted (token-tree parsed, not substring-ordered)"
    );

    // FIRE (RED): `cfg(any(test, feature = "test-support", unix))` — the
    // `unix` arm is TRUE on Unix production builds, so the gate is
    // production-visible there. The OLD substring matcher accepted it.
    assert!(
        !cfg_is_exactly_test_or_test_support(inner(
            "#[cfg(any(test, feature = \"test-support\", unix))]"
        )),
        "self-test: `#[cfg(any(test, feature = \"test-support\", unix))]` MUST be REJECTED — the \
         `unix` arm makes it production-visible on Unix; the substring matcher's hole"
    );
    // FIRE (RED): `cfg(any(test, feature = "test-support", feature = "prod"))`.
    assert!(
        !cfg_is_exactly_test_or_test_support(inner(
            "#[cfg(any(test, feature = \"test-support\", feature = \"prod\"))]"
        )),
        "self-test: a third `feature = \"prod\"` arm MUST be REJECTED — it is production-satisfiable"
    );
    // FIRE (RED): `cfg(any(test, debug_assertions))` — the debug-build hole.
    assert!(
        !cfg_is_exactly_test_or_test_support(inner("#[cfg(any(test, debug_assertions))]")),
        "self-test: `#[cfg(any(test, debug_assertions))]` MUST be REJECTED — `debug_assertions` is \
         ON in ordinary debug builds"
    );
    // FIRE (RED): a non-canonical `feature = "test-support"` ALONE (no test
    // arm) — production-reachable when the feature is enabled in a non-test
    // build is impossible by the self-edge, but it is not the canonical gate
    // and entailment is not the contract here; the EXACT recogniser requires
    // BOTH arms in the disjunction form.
    assert!(
        !cfg_is_exactly_test_or_test_support(inner("#[cfg(feature = \"test-support\")]")),
        "self-test: a lone `#[cfg(feature = \"test-support\")]` (no `test` arm) MUST be REJECTED — \
         the canonical narrow gate is bare `test` or `any(test, feature = \"test-support\")`"
    );
    // FIRE (RED): `all(test, feature = "test-support")` is genuinely test-only
    // but is NOT the canonical narrow carrier gate shape — the EXACT
    // recogniser is intentionally stricter than entailment.
    assert!(
        !cfg_is_exactly_test_or_test_support(inner(
            "#[cfg(all(test, feature = \"test-support\"))]"
        )),
        "self-test: `#[cfg(all(test, …))]` MUST be REJECTED by the EXACT recogniser — the carrier \
         gate is the canonical narrow shape, not an arbitrary test-entailing conjunction"
    );
}

/// The single `raise_node_to_type_expr` definition line in raise.rs (the
/// raw `SemanticNodeId -> Option<TypeExpr>` shell primitive), with its
/// leading whitespace trimmed. There is exactly one such definition (the
/// invariant guard `semantic_node_to_type_expr_has_exactly_one_path`
/// independently asserts the count); this reads it back for the
/// visibility check below. The trailing `(` is a whole-identifier
/// boundary, so any `..._suffix(` variant of the same name stem is a
/// DIFFERENT identifier and is NOT matched.
fn raise_primitive_definition_line() -> String {
    const RAISE_REL: &str = "src/project_semantic_dispatch/raise.rs";
    let src = read_rel(RAISE_REL);
    let mut found: Option<String> = None;
    for line in src.lines() {
        // The EXACT definition: `fn raise_node_to_type_expr(` — the char
        // right after the identifier must be `(`, so any `..._suffix(`
        // variant of the same name stem is NOT matched.
        if let Some(pos) = line.find("fn raise_node_to_type_expr(") {
            // Guard against a hypothetical `..._suffix(` that happens to
            // contain the substring: the matched run must be immediately
            // followed by `(` (it is, by construction of the needle), and
            // the char before `fn` (if any) must be whitespace so a
            // longer identifier ending in `fn` is not caught.
            let before = line[..pos].chars().next_back();
            if before.map(|c| c.is_whitespace()).unwrap_or(true) {
                assert!(
                    found.is_none(),
                    "expected EXACTLY ONE `fn raise_node_to_type_expr(` definition line in \
                     {RAISE_REL}; found a second: {line}"
                );
                found = Some(line.trim().to_string());
            }
        }
    }
    found.unwrap_or_else(|| {
        panic!("no `fn raise_node_to_type_expr(` definition line found in {RAISE_REL}")
    })
}

#[test]
fn raise_node_to_type_expr_primitive_is_module_private() {
    // STRUCTURAL FENCE (anti-regression). The raw `SemanticNodeId ->
    // Option<TypeExpr>` raise primitive `raise_node_to_type_expr` is the
    // single shell raise. After the output-fence migration it is
    // MODULE-PRIVATE: no `pub` / `pub(crate)` / `pub(super)` / `pub(in …)`
    // qualifier. Out-of-module callers must route through the named output
    // boundary fns (`materialize_output_type_expr` /
    // `materialize_reduced_output_type_expr`), and an out-of-module direct
    // call to the primitive is a COMPILE error (E0624 — `raise_node_to_type_expr`
    // is a private INHERENT METHOD on `ProjectSemanticDispatch`, so an
    // out-of-module call is "method is private", not the E0603 used for a
    // private free item / module) — this test locks the visibility so a
    // future widening (re-adding `pub(crate)`) is caught here even before
    // any caller is added.
    let def = raise_primitive_definition_line();
    // The definition line, trimmed, must START with `fn` (a bare
    // module-private fn). Any `pub`-led visibility qualifier precedes the
    // `fn` keyword on the same line, so a `starts_with("fn ")` check is a
    // complete and sufficient module-private assertion.
    assert!(
        def.starts_with("fn "),
        "STRUCTURAL FENCE: `raise_node_to_type_expr` must be MODULE-PRIVATE (a bare `fn`, no \
         `pub`/`pub(crate)`/`pub(super)` qualifier) so an out-of-module call is a compile error; \
         the definition line is: `{def}`"
    );
    // Belt-and-braces: explicitly reject every `pub`-prefixed form so the
    // intent of the check is unmistakable and a reformatting that splits
    // the qualifier onto its own token still trips (the line begins with
    // the qualifier in all rustfmt outputs).
    for forbidden in ["pub fn", "pub(crate) fn", "pub(super) fn", "pub(in"] {
        assert!(
            !def.starts_with(forbidden),
            "STRUCTURAL FENCE: `raise_node_to_type_expr` must NOT be `{forbidden}`-visible — it is \
             the module-private raw primitive; the definition line is: `{def}`"
        );
    }
}

#[test]
fn raise_primitive_module_private_self_test_discriminates() {
    // The visibility classifier the guard relies on must FIRE on every
    // `pub`-visible form and PASS the bare module-private form — proving it
    // would catch a regression that re-widens the primitive.
    let is_module_private = |def: &str| -> bool {
        def.starts_with("fn ")
            && !["pub fn", "pub(crate) fn", "pub(super) fn", "pub(in"]
                .iter()
                .any(|p| def.starts_with(p))
    };
    // PASS: the bare module-private definition (the post-fence shape).
    assert!(
        is_module_private(
            "fn raise_node_to_type_expr(&self, node: SemanticNodeId) -> Option<TypeExpr> {"
        ),
        "self-test: a bare `fn raise_node_to_type_expr(` def MUST classify as module-private"
    );
    // FIRE (RED): every widened form must be rejected.
    for widened in [
        "pub(crate) fn raise_node_to_type_expr(&self, node: SemanticNodeId) -> Option<TypeExpr> {",
        "pub fn raise_node_to_type_expr(&self, node: SemanticNodeId) -> Option<TypeExpr> {",
        "pub(super) fn raise_node_to_type_expr(&self) {",
        "pub(in crate::project_semantic_dispatch) fn raise_node_to_type_expr(&self) {",
    ] {
        assert!(
            !is_module_private(widened),
            "self-test: a `pub`-visible def `{widened}` MUST classify as NOT module-private — the \
             structural fence would otherwise miss a visibility-widening regression"
        );
    }
}

/// The single `raise_and_reduce_with_context` definition line in raise.rs
/// (the reduce-then-raise orchestrator the per-member publication
/// projectors reach through `materialize_reduced_output_type_expr`), with
/// its leading whitespace trimmed. There is exactly one such definition;
/// this reads it back for the visibility check below. The trailing `(`
/// whole-identifier boundary excludes any longer identifier that merely
/// shares the prefix.
fn raise_and_reduce_definition_line() -> String {
    const RAISE_REL: &str = "src/project_semantic_dispatch/raise.rs";
    let src = read_rel(RAISE_REL);
    let mut found: Option<String> = None;
    for line in src.lines() {
        // The EXACT definition: `fn raise_and_reduce_with_context(` — the
        // char right after the identifier must be `(`, so a longer
        // identifier sharing the prefix is NOT matched.
        if let Some(pos) = line.find("fn raise_and_reduce_with_context(") {
            // The matched run is immediately followed by `(` (by
            // construction of the needle); the char before `fn` (if any)
            // must be whitespace so a longer identifier ending in `fn` is
            // not caught.
            let before = line[..pos].chars().next_back();
            if before.map(|c| c.is_whitespace()).unwrap_or(true) {
                assert!(
                    found.is_none(),
                    "expected EXACTLY ONE `fn raise_and_reduce_with_context(` definition line in \
                     {RAISE_REL}; found a second: {line}"
                );
                found = Some(line.trim().to_string());
            }
        }
    }
    found.unwrap_or_else(|| {
        panic!("no `fn raise_and_reduce_with_context(` definition line found in {RAISE_REL}")
    })
}

#[test]
fn raise_and_reduce_with_context_is_subsystem_confined() {
    // STRUCTURAL FENCE (anti-regression), Q4. The reduce-then-raise
    // orchestrator `raise_and_reduce_with_context` is CONFINED to the
    // `project_semantic_dispatch` dispatch subsystem via `pub(super)`: its
    // sole production caller is the in-module `materialize_reduced_output_type_expr`
    // (the named output boundary the per-member publication projectors reach
    // through), while its valid subsystem-level reducer characterization tests
    // live in SIBLING modules of `raise` (`tests`, `carrier_reduction_tests`),
    // which are dispatch-subsystem descendants and so can see a `pub(super)`
    // method. An out-of-SUBSYSTEM direct call is a COMPILE error (E0624). This
    // test locks the visibility so BOTH a widening (re-adding `pub(crate)` or
    // `pub`) AND a wrong-direction narrowing to a bare module-private `fn`
    // (which would break the sibling test callers) are caught here.
    let def = raise_and_reduce_definition_line();
    // The definition line, trimmed, must START with the subsystem-confined
    // form — `pub(super) fn ` or the equivalent explicit
    // `pub(in crate::project_semantic_dispatch) fn `. Any visibility
    // qualifier precedes the `fn` keyword on the same line, so a set of
    // `starts_with` checks is a complete and sufficient confinement assertion.
    assert!(
        def.starts_with("pub(super) fn ")
            || def.starts_with("pub(in crate::project_semantic_dispatch) fn "),
        "STRUCTURAL FENCE (Q4): `raise_and_reduce_with_context` must be CONFINED to the \
         `project_semantic_dispatch` dispatch subsystem (`pub(super) fn` — or the equivalent \
         `pub(in crate::project_semantic_dispatch) fn`) so an out-of-subsystem call is a compile \
         error (E0624); the wrapper `materialize_reduced_output_type_expr` is the named entry; the \
         definition line is: `{def}`"
    );
    // Belt-and-braces: explicitly reject the bare module-private form (a
    // wrong-direction narrowing that would make the sibling-module reducer
    // tests unreachable) and every wider `pub`/`pub(crate)` form. Note
    // `pub(super) fn` does NOT start with `pub fn ` (the char after `pub`
    // is `(`, not a space) nor `pub(crate) fn` (the inner path is `super`,
    // not `crate`), so these checks do not false-fire on the accepted form.
    for forbidden in ["fn ", "pub fn ", "pub(crate) fn "] {
        assert!(
            !def.starts_with(forbidden),
            "STRUCTURAL FENCE (Q4): `raise_and_reduce_with_context` must NOT be `{forbidden}`-visible \
             — it is confined to `project_semantic_dispatch` via `pub(super)`; a bare `fn ` would \
             break its sibling-module (`tests`/`carrier_reduction_tests`) reducer characterization \
             callers, and any `pub`/`pub(crate)` form would widen past the dispatch subsystem; the \
             definition line is: `{def}`"
        );
    }
}

#[test]
fn raise_and_reduce_subsystem_confined_self_test_discriminates() {
    // The visibility classifier the Q4 guard relies on must ACCEPT the
    // subsystem-confined forms (`pub(super)` / `pub(in crate::project_semantic_dispatch)`)
    // and REJECT both a widening (`pub(crate)` / `pub`) AND a wrong-direction
    // narrowing to a bare module-private `fn` (which would make the sibling
    // dispatch-subsystem reducer tests unreachable). This proves the fence
    // discriminates in BOTH directions: it FAILS on a tree where the method
    // is bare `fn` or `pub(crate)`, and PASSES on the `pub(super)` tree.
    let is_subsystem_confined = |def: &str| -> bool {
        def.starts_with("pub(super) fn ")
            || def.starts_with("pub(in crate::project_semantic_dispatch) fn ")
    };
    // PASS: the two accepted subsystem-confined forms (the post-Q4 shape and
    // its explicit equivalent).
    for accepted in [
        "pub(super) fn raise_and_reduce_with_context(&self, node: SemanticNodeId, context: \
         ProjectionReductionContext) -> MaterializedTypeExpr {",
        "pub(in crate::project_semantic_dispatch) fn raise_and_reduce_with_context(&self, node: \
         SemanticNodeId, context: ProjectionReductionContext) -> MaterializedTypeExpr {",
    ] {
        assert!(
            is_subsystem_confined(accepted),
            "self-test: a subsystem-confined def `{accepted}` MUST classify as confined to \
             `project_semantic_dispatch`"
        );
    }
    // FIRE (RED): every NON-confined form must be rejected — the wrong-direction
    // narrowing to a bare module-private `fn` (which would break the sibling
    // test callers — the original 8-A2 build break), AND the widenings the
    // pre-Q4 `pub(crate)` shape and a bare `pub`.
    for rejected in [
        // Bare `fn` — a narrowing to module-private that makes the sibling
        // `tests`/`carrier_reduction_tests` reducer callers unreachable (E0624).
        "fn raise_and_reduce_with_context(&self, node: SemanticNodeId, context: \
         ProjectionReductionContext) -> MaterializedTypeExpr {",
        // The exact pre-Q4 `pub(crate)` shape this migration removed (widening).
        "pub(crate) fn raise_and_reduce_with_context(&self, node: SemanticNodeId, context: \
         ProjectionReductionContext) -> MaterializedTypeExpr {",
        // A bare `pub` (widening).
        "pub fn raise_and_reduce_with_context(&self) {",
    ] {
        assert!(
            !is_subsystem_confined(rejected),
            "self-test: a non-confined def `{rejected}` MUST classify as NOT subsystem-confined — the \
             Q4 structural fence would otherwise miss either a visibility-widening regression or a \
             wrong-direction narrowing to an unreachable bare `fn`"
        );
    }
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
        reason: "the ShapeSubject::MemberValueNode subject reduces an already-lowered node directly \
                 through raise_and_reduce_with_context",
    },
    InventoryRow {
        seam: "imported registry body / member surface",
        status: SeamStatus::HandleNative,
        witness: &[(
            "src/resolver_core/component_meta_query_engine/registry_decl.rs",
            "fn materialize_member_surface_node_core",
        )],
        reason: "the member-surface node-core reduces an already-lowered registry/member body \
                 node through the dispatch; the TypeExpr arm lowers-then-delegates to it. The \
                 node-core is module-private (a forgeable SemanticNodeId never crosses the \
                 query-engine boundary); out-of-subtree production callers reach the surface \
                 through the demand APIs (materialize_pick_member_surface / project_expr_surface_shape)",
    },
    InventoryRow {
        seam: "owner collection body",
        status: SeamStatus::HandleNative,
        witness: &[(
            "src/resolver_core/component_meta_query_engine/registry_decl.rs",
            "fn materialize_member_surface_node_core",
        )],
        reason: "the owner-collection handle arm reduces a body node through the shared \
                 member-surface node-core (owner-collection scope axis, nested = false) without \
                 touching the TypeExpr-keyed OwnerCollectionDb; the thin owner-collection \
                 pass-through wrapper folded into the node-core itself",
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
