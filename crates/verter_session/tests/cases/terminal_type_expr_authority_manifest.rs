//! Terminal `TypeExpr`-authority manifest guard.
//!
//! Joins the PRODUCTION `TypeExpr` authority surface (enumerated structurally
//! from source with `syn`) against the committed classification manifest
//! `docs/arch/stage10-terminal-authority-manifest.md`, and fails on any
//! unclassified, stale, or resurrected row. Every production item that names
//! the bare `TypeExpr` identifier must carry exactly one manifest disposition:
//!
//! - `C1` — sealed output / syntax (output materialisation, wire/JSON/display
//!   payloads, JSDoc `{Type}` text, oracle/diagnostic vocabulary). Never a
//!   query-time semantic decision.
//! - `C2` — sanctioned lowering ingress (the transient OXC→typed-IR product,
//!   producer decl-body / macro-payload lowering handed straight to the one
//!   shared dispatch, and producer-side fact minting over transient authored
//!   bodies). Consumed once, turned into content-free nodes/facts.
//! - `C3` — dead: zero production consumers. A plain `C3` row is a TOMBSTONE
//!   (the item is deleted; the guard REDS if it reappears). A `C3-pending`
//!   row is a compiler-certified dead item still in-tree (rustc `dead_code`
//!   on the default lib target — evidence recorded per row in the manifest);
//!   the guard REDS when the row goes stale (item deleted → flip to `C3`).
//!
//! A live site making a query-time SEMANTIC decision from a `TypeExpr` fits
//! none of the classes — it must NOT be papered over with a `C1`/`C2` row; it
//! is a residual migration hole to escalate.
//!
//! ## Status — sanctioned WIP scanner (transitional rail)
//!
//! This guard and the manifest it joins are a TRANSITIONAL migration-
//! completeness rail, the same sanctioned-WIP class as the residual
//! body-reader inventory (`residual_type_expr_body_reader_inventory.rs`) it
//! supersedes: a name/location-keyed enumeration is acceptable here because
//! both are squashed out when the durable structural rail (the crate-boundary
//! + `NoTypeExpr` witness confinement) lands, at which point this file and the
//! manifest doc are deleted together. It is NOT a landed structural guard and
//! must not be extended into one.
//!
//! ## What is enumerated (and what is deliberately not)
//!
//! Enumerated per-item (top-level `fn` / `struct` / `enum` / `trait` /
//! `type` / `const` / `static` / `union` / module-level macro invocation,
//! keyed `(crate, file, item)`, nested inline modules prefixing the item):
//! every production `src/**` file of `verter_session`, `verter_semantic`,
//! `verter_session_query`, `verter_parser`, and `verter_compiler`. An item is
//! an authority-surface row iff its token stream contains the bare ident
//! `TypeExpr` (word-exact: `NoTypeExpr` / `TypeExprId` / doc-comment prose /
//! string literals do not match) or a file-local `use ... TypeExpr as X;`
//! rename of it (the alias `X` matches too — renaming does not slip past the
//! scan). Impl blocks enumerate PER MEMBER: each `fn` / `const` / `type`
//! member that individually names `TypeExpr` keys its own row
//! (`impl <Name>::fn <method>`), so a NEW `TypeExpr`-naming method inside an
//! already-classified impl mints a NEW unclassified key instead of riding
//! the impl's row; the impl HEADER (generics / self type / trait path, plus
//! rare non-fn/const/type members) keys the bare `impl <Name>` row only when
//! it names `TypeExpr` itself. `cfg` gating is negation-aware: an item is
//! excluded only when its predicate is definitely FALSE in the default
//! production build (`cfg(test)`, `cfg(any(test, feature = "oracle-gen"))`,
//! `cfg(any(test, feature = "test-support"))` — both features guard-verified
//! production-unreachable); `cfg(not(test))` is PRODUCTION and enumerates.
//!
//! Excluded, each with a verified gate the guard re-checks structurally:
//! - `#[cfg(test)]`-gated items/modules and `*_tests.rs` / `tests/` files
//!   (not production).
//! - `typeinfo/oracle_core/**` — gated `#[cfg(any(test, feature =
//!   "oracle-gen"))]` at its `mod` declaration (asserted below); no
//!   production consumer enables `oracle-gen`.
//! - `src/bin/oracle_gen.rs` / `src/bin/oracle_upgrade.rs` — `[[bin]]`
//!   targets behind `required-features = ["oracle-gen"]` (asserted below).
//! - the two vocabulary crates, covered by crate-level BLANKET rows plus a
//!   structural leaf assertion instead of per-item rows: `verter_type_expr`
//!   (the IR definition itself — the syntax vocabulary both sanctioned
//!   classes speak) and `verter_type_expr_oxc` (the OXC→`TypeExpr` lowering
//!   producer). Neither may depend on `verter_semantic` / `verter_session`,
//!   so neither can reach the store/dispatch to make a query-time decision;
//!   the guard REDS if such a dependency appears.
//!
//! `verter_protocol` / `verter_ffi` (sealed wire/output DTO crates above the
//! session) are outside this enumeration; their `TypeExpr` surface is the
//! permitted protocol survivor class owned by the Typeinfo Wire Contract
//! guards.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::TokenTree;
use syn::Item;
use walkdir::WalkDir;

/// The manifest doc joined against, workspace-relative.
const MANIFEST_DOC: &str = "docs/arch/stage10-terminal-authority-manifest.md";

/// Crates whose production `src/**` items are enumerated per-item.
const ENUMERATED_CRATES: &[&str] = &[
    "verter_session",
    "verter_semantic",
    "verter_session_query",
    "verter_parser",
    "verter_compiler",
];

/// Crates covered by a crate-level blanket row + the leaf assertion.
const BLANKET_CRATES: &[(&str, &str)] =
    &[("verter_type_expr", "C1"), ("verter_type_expr_oxc", "C2")];

fn workspace_root() -> PathBuf {
    // crates/verter_session -> crates -> workspace root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn is_test_file(rel: &str) -> bool {
    rel.ends_with("_tests.rs")
        || rel.ends_with("/tests.rs")
        || rel.contains("/tests/")
        || rel.contains("/tests_")
}

/// Files excluded because a verified feature/cfg gate keeps them out of the
/// default production build (the gates themselves are asserted in
/// [`excluded_subtrees_are_really_gated`]).
fn is_gated_file(crate_name: &str, rel: &str) -> bool {
    crate_name == "verter_session"
        && (rel.starts_with("src/typeinfo/oracle_core/")
            || rel == "src/typeinfo/oracle_core.rs"
            || rel.starts_with("src/typeinfo/typeinfo_tests/")
            || rel == "src/typeinfo/typeinfo_tests.rs"
            || rel == "src/bin/oracle_gen.rs"
            || rel == "src/bin/oracle_upgrade.rs")
}

/// Cargo features that are guard-verified PRODUCTION-UNREACHABLE, so a
/// `feature = "<name>"` cfg atom evaluates FALSE in the production config:
/// `oracle-gen` (asserted by [`excluded_subtrees_are_really_gated`] below)
/// and `test-support` (asserted by the dedicated structural guard
/// `tests/cases/g_misc1/test_support_feature_off_in_default_build.rs` — a
/// transitive-closure parse of the `[features] default` table).
const PRODUCTION_UNREACHABLE_FEATURES: &[&str] = &["oracle-gen", "test-support"];

/// Tri-state evaluation of a `cfg` predicate under the default PRODUCTION
/// build configuration: `test` is FALSE, the guard-verified
/// production-unreachable features are FALSE, every other atom is UNKNOWN
/// (conservatively kept in the enumeration).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CfgEval {
    True,
    False,
    Unknown,
}

fn eval_cfg_meta(meta: &syn::Meta) -> CfgEval {
    match meta {
        syn::Meta::Path(p) => {
            if p.is_ident("test") {
                CfgEval::False
            } else {
                CfgEval::Unknown
            }
        }
        syn::Meta::NameValue(nv) => {
            let is_production_unreachable_feature = nv.path.is_ident("feature")
                && matches!(
                    &nv.value,
                    syn::Expr::Lit(l)
                        if matches!(
                            &l.lit,
                            syn::Lit::Str(s)
                                if PRODUCTION_UNREACHABLE_FEATURES
                                    .contains(&s.value().as_str())
                        )
                );
            if is_production_unreachable_feature {
                CfgEval::False
            } else {
                CfgEval::Unknown
            }
        }
        syn::Meta::List(list) => {
            let inner: Vec<syn::Meta> = list
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                )
                .map(|p| p.into_iter().collect())
                .unwrap_or_default();
            match list.path.get_ident().map(|i| i.to_string()).as_deref() {
                // NEGATION-AWARE: `cfg(not(test))` is PRODUCTION (True), not
                // a test gate — the word `test` appearing under `not(..)`
                // must not exclude the item.
                Some("not") => match inner.first().map(eval_cfg_meta) {
                    Some(CfgEval::True) => CfgEval::False,
                    Some(CfgEval::False) => CfgEval::True,
                    _ => CfgEval::Unknown,
                },
                Some("any") => {
                    let evals: Vec<CfgEval> = inner.iter().map(eval_cfg_meta).collect();
                    if evals.contains(&CfgEval::True) {
                        CfgEval::True
                    } else if !evals.is_empty() && evals.iter().all(|e| *e == CfgEval::False) {
                        CfgEval::False
                    } else {
                        CfgEval::Unknown
                    }
                }
                Some("all") => {
                    let evals: Vec<CfgEval> = inner.iter().map(eval_cfg_meta).collect();
                    if evals.contains(&CfgEval::False) {
                        CfgEval::False
                    } else if evals.iter().all(|e| *e == CfgEval::True) {
                        CfgEval::True
                    } else {
                        CfgEval::Unknown
                    }
                }
                _ => CfgEval::Unknown,
            }
        }
    }
}

/// Whether an attribute set gates the item OUT of the default production
/// build: some `cfg` predicate evaluates definitely-FALSE in production
/// (`cfg(test)`, `cfg(any(test, feature = "oracle-gen"))`, ...). Negation is
/// evaluated, not word-matched: `cfg(not(test))` is production and does NOT
/// exclude.
fn attrs_are_test_gated(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg")
            && a.parse_args::<syn::Meta>()
                .map(|m| eval_cfg_meta(&m) == CfgEval::False)
                .unwrap_or(false)
    })
}

/// Whether a token stream contains the bare ident `TypeExpr` (word-exact;
/// literals — including doc-comment strings — and other idents never match)
/// or one of the file-local rename needles (`use ... TypeExpr as X;`).
fn tokens_contain_type_expr(
    tokens: proc_macro2::TokenStream,
    alias_needles: &BTreeSet<String>,
) -> bool {
    let mut stack = vec![tokens];
    while let Some(ts) = stack.pop() {
        for tree in ts {
            match tree {
                TokenTree::Ident(ident) => {
                    if ident == "TypeExpr" || alias_needles.contains(&ident.to_string()) {
                        return true;
                    }
                }
                TokenTree::Group(g) => stack.push(g.stream()),
                TokenTree::Punct(_) | TokenTree::Literal(_) => {}
            }
        }
    }
    false
}

/// Collect the file-local alias needles: every `use ... TypeExpr as X;`
/// rename anywhere in the file (any module depth, gated or not — the
/// conservative over-match direction) makes `X` a needle for the whole file,
/// so renaming `TypeExpr` does not slip an item past the scan.
fn collect_alias_needles(items: &[Item], out: &mut BTreeSet<String>) {
    fn collect_use_renames(tree: &syn::UseTree, out: &mut BTreeSet<String>) {
        match tree {
            syn::UseTree::Path(p) => collect_use_renames(&p.tree, out),
            syn::UseTree::Group(g) => {
                for t in &g.items {
                    collect_use_renames(t, out);
                }
            }
            syn::UseTree::Rename(r) => {
                if r.ident == "TypeExpr" {
                    out.insert(r.rename.to_string());
                }
            }
            syn::UseTree::Name(_) | syn::UseTree::Glob(_) => {}
        }
    }
    for item in items {
        match item {
            Item::Use(u) => collect_use_renames(&u.tree, out),
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    collect_alias_needles(inner, out);
                }
            }
            _ => {}
        }
    }
}

fn last_path_segment(path: &syn::Path) -> String {
    path.segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn self_type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => last_path_segment(&p.path),
        syn::Type::Reference(r) => self_type_name(&r.elem),
        syn::Type::Paren(p) => self_type_name(&p.elem),
        other => {
            // Normalise whitespace for non-path self types (rare).
            let mut s = quote::ToTokens::to_token_stream(other).to_string();
            s.retain(|c| !c.is_whitespace());
            s
        }
    }
}

/// Render an item's manifest key (`kind name`), or `None` when the item kind
/// is never an authority site on its own (`use` imports, extern crates).
/// Impl blocks are enumerated per-member in [`collect_items`], not here.
fn item_key(item: &Item) -> Option<(String, Vec<syn::Attribute>)> {
    use quote::ToTokens;
    let (key, attrs) = match item {
        Item::Fn(f) => (format!("fn {}", f.sig.ident), f.attrs.clone()),
        Item::Struct(s) => (format!("struct {}", s.ident), s.attrs.clone()),
        Item::Enum(e) => (format!("enum {}", e.ident), e.attrs.clone()),
        Item::Union(u) => (format!("union {}", u.ident), u.attrs.clone()),
        Item::Trait(t) => (format!("trait {}", t.ident), t.attrs.clone()),
        Item::Type(t) => (format!("type {}", t.ident), t.attrs.clone()),
        Item::Const(c) => (format!("const {}", c.ident), c.attrs.clone()),
        Item::Static(s) => (format!("static {}", s.ident), s.attrs.clone()),
        Item::Macro(m) => {
            let name = m
                .ident
                .as_ref()
                .map(|i| format!("macro_rules {i}"))
                .unwrap_or_else(|| format!("macro {}", last_path_segment(&m.mac.path)));
            (name, m.attrs.clone())
        }
        Item::Use(_) | Item::ExternCrate(_) => return None,
        Item::Impl(_) => unreachable!("impls are enumerated per-member, not keyed"),
        Item::Mod(_) => unreachable!("modules are recursed, not keyed"),
        other => (
            {
                let mut s = other.to_token_stream().to_string();
                s.truncate(40);
                format!("item {s}")
            },
            Vec::new(),
        ),
    };
    Some((key, attrs))
}

/// The impl's manifest display name (`impl Trait for Type` / `impl Type`).
fn impl_name(i: &syn::ItemImpl) -> String {
    match &i.trait_ {
        Some((_, trait_path, _)) => format!(
            "impl {} for {}",
            last_path_segment(trait_path),
            self_type_name(&i.self_ty)
        ),
        None => format!("impl {}", self_type_name(&i.self_ty)),
    }
}

/// Enumerate an impl block PER MEMBER: each non-test-gated `fn` / `const` /
/// `type` member naming `TypeExpr` keys its own `impl <Name>::<kind> <name>`
/// row (so a NEW `TypeExpr`-naming method inside an already-classified impl
/// mints a NEW unclassified key). The impl header (generics / self type /
/// trait path) plus any rare non-fn/const/type members key the bare
/// `impl <Name>` row only when they themselves name `TypeExpr`.
fn collect_impl_members(
    krate: &str,
    file: &str,
    mod_prefix: &str,
    i: &syn::ItemImpl,
    alias_needles: &BTreeSet<String>,
    out: &mut BTreeSet<SiteKey>,
) {
    use quote::ToTokens;
    let name = impl_name(i);
    // Header tokens: generics (incl. where clause), self type, trait path.
    let mut header_tokens = proc_macro2::TokenStream::new();
    i.generics.to_tokens(&mut header_tokens);
    i.generics.where_clause.to_tokens(&mut header_tokens);
    i.self_ty.to_tokens(&mut header_tokens);
    if let Some((_, trait_path, _)) = &i.trait_ {
        trait_path.to_tokens(&mut header_tokens);
    }
    for member in &i.items {
        let (member_key, attrs, tokens) = match member {
            syn::ImplItem::Fn(f) => (format!("fn {}", f.sig.ident), &f.attrs, f.to_token_stream()),
            syn::ImplItem::Const(c) => {
                (format!("const {}", c.ident), &c.attrs, c.to_token_stream())
            }
            syn::ImplItem::Type(t) => (format!("type {}", t.ident), &t.attrs, t.to_token_stream()),
            other => {
                // Rare member kinds (macro invocations, verbatim) fold into
                // the header row so nothing escapes the enumeration.
                other.to_tokens(&mut header_tokens);
                continue;
            }
        };
        if attrs_are_test_gated(attrs) {
            continue;
        }
        if tokens_contain_type_expr(tokens, alias_needles) {
            out.insert(SiteKey {
                krate: krate.to_string(),
                file: file.to_string(),
                item: format!("{mod_prefix}{name}::{member_key}"),
            });
        }
    }
    if tokens_contain_type_expr(header_tokens, alias_needles) {
        out.insert(SiteKey {
            krate: krate.to_string(),
            file: file.to_string(),
            item: format!("{mod_prefix}{name}"),
        });
    }
}

/// One enumerated authority-surface row.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SiteKey {
    krate: String,
    file: String,
    item: String,
}

fn collect_items(
    krate: &str,
    file: &str,
    mod_prefix: &str,
    items: &[Item],
    alias_needles: &BTreeSet<String>,
    out: &mut BTreeSet<SiteKey>,
) {
    for item in items {
        if let Item::Mod(m) = item {
            if attrs_are_test_gated(&m.attrs) {
                continue;
            }
            if let Some((_, inner)) = &m.content {
                let prefix = format!("{mod_prefix}{}::", m.ident);
                collect_items(krate, file, &prefix, inner, alias_needles, out);
            }
            continue;
        }
        if let Item::Impl(i) = item {
            if attrs_are_test_gated(&i.attrs) {
                continue;
            }
            collect_impl_members(krate, file, mod_prefix, i, alias_needles, out);
            continue;
        }
        let Some((key, attrs)) = item_key(item) else {
            continue;
        };
        if attrs_are_test_gated(&attrs) {
            continue;
        }
        if !tokens_contain_type_expr(quote::ToTokens::to_token_stream(item), alias_needles) {
            continue;
        }
        out.insert(SiteKey {
            krate: krate.to_string(),
            file: file.to_string(),
            item: format!("{mod_prefix}{key}"),
        });
    }
}

/// Collect a parsed file's authority-surface rows: gather the file-local
/// `TypeExpr` rename needles first, then enumerate the items.
fn collect_file(krate: &str, file: &str, parsed: &syn::File, out: &mut BTreeSet<SiteKey>) {
    let mut alias_needles = BTreeSet::new();
    collect_alias_needles(&parsed.items, &mut alias_needles);
    collect_items(krate, file, "", &parsed.items, &alias_needles, out);
}

/// Enumerate the production `TypeExpr` authority surface of the scanned
/// crates.
fn enumerate_authority_surface() -> BTreeSet<SiteKey> {
    let root = workspace_root();
    let mut out = BTreeSet::new();
    for krate in ENUMERATED_CRATES {
        let src_root = root.join("crates").join(krate).join("src");
        if !src_root.exists() {
            continue;
        }
        for entry in WalkDir::new(&src_root) {
            let entry = entry.expect("walkdir entry");
            if !entry.file_type().is_file()
                || entry.path().extension().and_then(|e| e.to_str()) != Some("rs")
            {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(root.join("crates").join(krate))
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_file(&rel) || is_gated_file(krate, &rel) {
                continue;
            }
            let src = std::fs::read_to_string(entry.path()).expect("read source file");
            // Fast reject before the syn parse.
            if !src.contains("TypeExpr") {
                continue;
            }
            let parsed = syn::parse_file(&src)
                .unwrap_or_else(|e| panic!("syn::parse_file({rel}) failed: {e}"));
            collect_file(krate, &rel, &parsed, &mut out);
        }
    }
    out
}

/// One parsed manifest row.
#[derive(Debug, Clone)]
struct ManifestRow {
    class: String,
    key: SiteKey,
    justification: String,
}

/// Parse the classification rows out of the manifest doc. Rows are markdown
/// table lines of the shape:
/// `| C1 | verter_session | src/... | fn foo | justification |`
/// Blanket rows use `(crate)` in the file column.
fn parse_manifest(doc: &str) -> Vec<ManifestRow> {
    let mut rows = Vec::new();
    for line in doc.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("| C") {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        // Prose tables (class definitions etc.) also start with `| C...`;
        // only a first cell of the `C<digit>` shape is a classification row.
        let class = cells[0].to_string();
        if !(class.starts_with('C') && class.chars().nth(1).is_some_and(|c| c.is_ascii_digit())) {
            continue;
        }
        assert!(
            matches!(class.as_str(), "C1" | "C2" | "C3" | "C3-pending"),
            "manifest row class must be C1 / C2 / C3 / C3-pending, got `{class}`: {trimmed}"
        );
        if cells.len() != 5 {
            panic!(
                "manifest row must have exactly 5 cells \
                 (| class | crate | file | item | justification |): {trimmed}"
            );
        }
        assert!(
            !cells[4].is_empty() && cells[4] != "TODO",
            "manifest row must carry a real justification: {trimmed}"
        );
        rows.push(ManifestRow {
            class,
            key: SiteKey {
                krate: cells[1].to_string(),
                file: cells[2].to_string(),
                item: cells[3].to_string(),
            },
            justification: cells[4].to_string(),
        });
    }
    rows
}

fn load_manifest() -> Vec<ManifestRow> {
    let path = workspace_root().join(MANIFEST_DOC);
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {MANIFEST_DOC}: {e}"));
    parse_manifest(&doc)
}

/// The join core, factored so the discrimination self-tests can drive it with
/// synthetic inputs. Returns human-readable violations (empty = green).
fn join_violations(surface: &BTreeSet<SiteKey>, rows: &[ManifestRow]) -> Vec<String> {
    let mut violations = Vec::new();

    // Manifest rows keyed for the join; duplicate keys are an error.
    let mut by_key: BTreeMap<&SiteKey, &ManifestRow> = BTreeMap::new();
    for row in rows {
        if by_key.insert(&row.key, row).is_some() {
            violations.push(format!(
                "DUPLICATE manifest row for {}/{} `{}`",
                row.key.krate, row.key.file, row.key.item
            ));
        }
    }

    // (a) Every enumerated site carries a live classification (C1/C2 or a
    // C3-pending dead pin). A missing row is an UNCLASSIFIED authority site.
    for site in surface {
        match by_key.get(site) {
            Some(row) if matches!(row.class.as_str(), "C1" | "C2" | "C3-pending") => {}
            Some(row) => violations.push(format!(
                "RESURRECTED tombstone: {}/{} `{}` exists in the tree but the manifest \
                 classifies it `{}` (deleted). Delete the item again or reclassify.",
                site.krate, site.file, site.item, row.class
            )),
            None => violations.push(format!(
                "UNCLASSIFIED production TypeExpr authority site — add a manifest row:\n\
                 | C? | {} | {} | {} | TODO |",
                site.krate, site.file, site.item
            )),
        }
    }

    // (b) Every live-classified manifest row resolves to an enumerated site
    // (no stale rows silently rotting in the doc).
    for row in rows {
        let live = surface.contains(&row.key);
        match row.class.as_str() {
            "C1" | "C2" | "C3-pending" => {
                if row.key.file == "(crate)" {
                    // Blanket rows are checked in the blanket test, not here.
                    continue;
                }
                if !live {
                    violations.push(format!(
                        "STALE manifest row: {}/{} `{}` ({}) no longer resolves to a \
                         production TypeExpr site. Remove the row or flip it to C3.",
                        row.key.krate, row.key.file, row.key.item, row.class
                    ));
                }
            }
            "C3" => {
                // Tombstone: violation is emitted from the surface side above
                // when the item resurrects; nothing to check when absent.
            }
            _ => unreachable!("class validated at parse time"),
        }
    }

    violations
}

// ════════════════════════════════════════════════════════════════════
// The manifest join — the load-bearing certification test.
// ════════════════════════════════════════════════════════════════════

#[test]
fn every_production_type_expr_authority_site_is_classified() {
    let surface = enumerate_authority_surface();
    assert!(
        !surface.is_empty(),
        "self-check: the enumeration must be non-vacuous (the sanctioned \
         C1/C2 surface is not empty on this tree)"
    );
    let rows = load_manifest();
    let violations = join_violations(&surface, &rows);
    assert!(
        violations.is_empty(),
        "terminal TypeExpr authority manifest violations ({}):\n\n{}\n\n\
         Every production TypeExpr authority site must carry exactly one \
         C1/C2/C3 disposition in {MANIFEST_DOC}. A site that is a live \
         query-time semantic decision fits NO class — do not paper it over: \
         it is a residual migration hole to escalate.",
        violations.len(),
        violations.join("\n\n")
    );
}

/// The blanket vocabulary crates stay leaves: neither may grow a dependency
/// on `verter_semantic` / `verter_session` (which would give the vocabulary /
/// lowering producer access to the store or dispatch), and each carries its
/// crate-level blanket row in the manifest.
#[test]
fn blanket_vocabulary_crates_stay_leaves_and_carry_blanket_rows() {
    let root = workspace_root();
    let rows = load_manifest();
    for (krate, expected_class) in BLANKET_CRATES {
        let manifest_path = root.join("crates").join(krate).join("Cargo.toml");
        let cargo = std::fs::read_to_string(&manifest_path)
            .unwrap_or_else(|e| panic!("read {krate}/Cargo.toml: {e}"));
        for forbidden in ["verter_semantic", "verter_session"] {
            assert!(
                !cargo.contains(forbidden),
                "{krate}/Cargo.toml must not depend on {forbidden} — the \
                 blanket manifest row rests on the crate being unable to \
                 reach the store/dispatch"
            );
        }
        let row = rows
            .iter()
            .find(|r| r.key.krate == *krate && r.key.file == "(crate)")
            .unwrap_or_else(|| {
                panic!("manifest must carry the crate-level blanket row for {krate}")
            });
        assert_eq!(
            row.class, *expected_class,
            "{krate} blanket row must be {expected_class}"
        );
        assert!(
            !row.justification.is_empty(),
            "{krate} blanket row needs a justification"
        );
    }
}

/// The exclusions the enumeration takes are real gates, re-checked
/// structurally so a gate removal reddens this guard rather than silently
/// widening the unscanned surface.
#[test]
fn excluded_subtrees_are_really_gated() {
    let root = workspace_root();

    // oracle_core and typeinfo_tests are out of the default production build.
    let typeinfo_mod =
        std::fs::read_to_string(root.join("crates/verter_session/src/typeinfo/mod.rs"))
            .expect("read typeinfo/mod.rs");
    for (mod_decl, gate) in [
        (
            "pub(crate) mod oracle_core;",
            "#[cfg(any(test, feature = \"oracle-gen\"))]",
        ),
        ("mod typeinfo_tests;", "#[cfg(test)]"),
    ] {
        let mod_decl_pos = typeinfo_mod
            .find(mod_decl)
            .unwrap_or_else(|| panic!("typeinfo/mod.rs must still declare `{mod_decl}`"));
        let before = &typeinfo_mod[..mod_decl_pos];
        let gate_pos = before
            .rfind(gate)
            .unwrap_or_else(|| panic!("`{mod_decl}` must be {gate}-gated"));
        assert!(
            before[gate_pos..].lines().count() <= 2,
            "the {gate} gate must immediately precede `{mod_decl}`"
        );
    }

    // The oracle bins stay behind required-features.
    let cargo = std::fs::read_to_string(root.join("crates/verter_session/Cargo.toml"))
        .expect("read verter_session/Cargo.toml");
    for bin in ["oracle_gen", "oracle_upgrade"] {
        let pos = cargo
            .find(&format!("name = \"{bin}\""))
            .unwrap_or_else(|| panic!("[[bin]] {bin} must exist"));
        let tail = &cargo[pos..cargo.len().min(pos + 300)];
        assert!(
            tail.contains("required-features = [\"oracle-gen\"]"),
            "[[bin]] {bin} must stay behind required-features = [\"oracle-gen\"]"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// Discrimination self-tests — the guard is not a rubber stamp.
// ════════════════════════════════════════════════════════════════════

/// A synthetic NEW unclassified authority site makes the join RED.
#[test]
fn join_reds_on_a_new_unclassified_site() {
    let mut surface = enumerate_authority_surface();
    surface.insert(SiteKey {
        krate: "verter_session".to_string(),
        file: "src/synthetic_probe.rs".to_string(),
        item: "fn synthetic_unclassified_type_expr_walker".to_string(),
    });
    let rows = load_manifest();
    let violations = join_violations(&surface, &rows);
    assert!(
        violations
            .iter()
            .any(|v| v.contains("UNCLASSIFIED") && v.contains("synthetic_unclassified")),
        "a new unclassified site must produce an UNCLASSIFIED violation; got: {violations:?}"
    );
}

/// A resurrected C3 tombstone makes the join RED.
#[test]
fn join_reds_on_a_resurrected_tombstone() {
    let rows = load_manifest();
    let tombstone = rows
        .iter()
        .find(|r| r.class == "C3")
        .expect("the manifest carries at least one C3 tombstone");
    let mut surface = enumerate_authority_surface();
    surface.insert(tombstone.key.clone());
    let violations = join_violations(&surface, &rows);
    assert!(
        violations.iter().any(|v| v.contains("RESURRECTED")),
        "a resurrected tombstone must produce a RESURRECTED violation; got: {violations:?}"
    );
}

/// A stale live-classified row (site vanished without a manifest update)
/// makes the join RED.
#[test]
fn join_reds_on_a_stale_live_row() {
    let surface = enumerate_authority_surface();
    let mut rows = load_manifest();
    rows.push(ManifestRow {
        class: "C2".to_string(),
        key: SiteKey {
            krate: "verter_session".to_string(),
            file: "src/synthetic_probe.rs".to_string(),
            item: "fn synthetic_vanished_site".to_string(),
        },
        justification: "synthetic".to_string(),
    });
    let violations = join_violations(&surface, &rows);
    assert!(
        violations.iter().any(|v| v.contains("STALE")),
        "a live-classified row without a live site must produce a STALE violation"
    );
}

/// The token scan is word-exact: `NoTypeExpr` / `TypeExprId` idents and
/// doc-comment prose do not enumerate; a bare `TypeExpr` ident does —
/// including inside macro invocation token streams.
#[test]
fn token_scan_is_word_exact() {
    let no_match: syn::File = syn::parse_str(
        r#"
        /// Mentions TypeExpr in prose only.
        #[derive(NoTypeExpr)]
        struct Fact { id: TypeExprId }
        fn f(x: &str) -> String { x.to_string() }
        "#,
    )
    .unwrap();
    let mut out = BTreeSet::new();
    collect_file("k", "f.rs", &no_match, &mut out);
    assert!(
        out.is_empty(),
        "NoTypeExpr / TypeExprId / doc prose must not enumerate: {out:?}"
    );

    let matches: syn::File = syn::parse_str(
        r#"
        struct Carrier { body: TypeExpr }
        fn walker(x: &verter_type_expr::TypeExpr) {}
        fn in_macro() { assert!(matches!(y, TypeExpr::Object(_))); }
        #[cfg(test)]
        fn gated(x: &TypeExpr) {}
        "#,
    )
    .unwrap();
    let mut out = BTreeSet::new();
    collect_file("k", "f.rs", &matches, &mut out);
    let items: Vec<&str> = out.iter().map(|s| s.item.as_str()).collect();
    assert_eq!(
        items,
        vec!["fn in_macro", "fn walker", "struct Carrier"],
        "bare TypeExpr idents (field / signature / macro body) must \
         enumerate and cfg(test) items must not"
    );
}

/// Vector A: impl members enumerate PER METHOD — a `TypeExpr`-naming method
/// inside an impl mints its own `impl <Name>::fn <method>` key (so a NEW
/// method inside an already-classified impl cannot ride the impl's row),
/// `TypeExpr`-free siblings and `cfg(test)`-gated members do not enumerate,
/// and the bare `impl <Name>` header row appears only when the header itself
/// names `TypeExpr`.
#[test]
fn impl_members_enumerate_per_method() {
    let file: syn::File = syn::parse_str(
        r#"
        impl Engine {
            fn classified(&self, x: &TypeExpr) -> bool { matches!(x, TypeExpr::Object(_)) }
            fn red_probe(&self, x: &TypeExpr) -> bool { matches!(x, TypeExpr::Object(_)) }
            fn unrelated(&self) -> u32 { 0 }
            #[cfg(test)]
            fn gated(&self, x: &TypeExpr) {}
            const LIMIT: usize = 4;
            type Out = TypeExpr;
        }
        impl From<TypeExpr> for Wire {
            fn from(_: TypeExpr) -> Self { Wire }
        }
        "#,
    )
    .unwrap();
    let mut out = BTreeSet::new();
    collect_file("k", "f.rs", &file, &mut out);
    let items: Vec<&str> = out.iter().map(|s| s.item.as_str()).collect();
    assert_eq!(
        items,
        vec![
            "impl Engine::fn classified",
            "impl Engine::fn red_probe",
            "impl Engine::type Out",
            "impl From for Wire",
            "impl From for Wire::fn from",
        ],
        "impl members must key per-method (distinct rows per member), \
         TypeExpr-free / cfg(test) members must not enumerate, and the \
         header row must appear only for a TypeExpr-naming header"
    );
}

/// Vector B: cfg gating is negation-aware — `#[cfg(not(test))]` is
/// PRODUCTION and must enumerate; `cfg(test)` / `cfg(any(test, feature =
/// "oracle-gen"))` / `cfg(all(test, unix))` stay excluded.
#[test]
fn negated_cfg_items_are_production() {
    let file: syn::File = syn::parse_str(
        r#"
        #[cfg(not(test))]
        fn prod_probe(x: &TypeExpr) {}
        #[cfg(test)]
        fn test_gated(x: &TypeExpr) {}
        #[cfg(any(test, feature = "oracle-gen"))]
        fn oracle_gated(x: &TypeExpr) {}
        #[cfg(any(test, feature = "test-support"))]
        fn test_support_gated(x: &TypeExpr) {}
        #[cfg(all(test, unix))]
        fn all_gated(x: &TypeExpr) {}
        #[cfg(unix)]
        fn platform_gated(x: &TypeExpr) {}
        "#,
    )
    .unwrap();
    let mut out = BTreeSet::new();
    collect_file("k", "f.rs", &file, &mut out);
    let items: Vec<&str> = out.iter().map(|s| s.item.as_str()).collect();
    assert_eq!(
        items,
        vec!["fn platform_gated", "fn prod_probe"],
        "cfg(not(test)) and platform cfgs are production and must \
         enumerate; test / oracle-gen gates (incl. any/all forms) must not"
    );
}

/// Vector C: a file-local `use ... TypeExpr as X;` rename does not slip the
/// scan — items naming only the alias enumerate too; unrelated idents that
/// merely collide with nothing stay out.
#[test]
fn local_type_expr_alias_is_matched() {
    let file: syn::File = syn::parse_str(
        r#"
        use verter_type_expr::TypeExpr as Expr;
        use other::{Thing, TypeExpr as Aliased};
        fn renamed_probe(x: &Expr) -> bool { matches!(x, Expr::Object(_)) }
        fn grouped_probe(x: &Aliased) {}
        fn clean(x: &Thing) {}
        "#,
    )
    .unwrap();
    let mut out = BTreeSet::new();
    collect_file("k", "f.rs", &file, &mut out);
    let items: Vec<&str> = out.iter().map(|s| s.item.as_str()).collect();
    assert_eq!(
        items,
        vec!["fn grouped_probe", "fn renamed_probe"],
        "items naming a local TypeExpr rename must enumerate; \
         alias-free items must not"
    );
}
