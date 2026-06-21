//! Residual `TypeExpr`-body-reader inventory guard — a `syn`-structural
//! inventory that partitions every declaration-BODY reader still carrying the
//! lower-crate typed IR (`TypeExpr`) into a `ReaderClass`, so the residual
//! `TypeExpr` reads are an enumerated, curated, justified set while the
//! genuinely-graph-backed readers route through the shared hot accessor.
//!
//! ## scanner records (durable guard-local record — Structural-Confinement-First)
//!
//! ```text
//! scanner_invariant: this guard is a scoped residual-reader inventory and ratchet.
//!   It enforces presence/uniqueness of curated residual and compat rows, rejects
//!   uninventoried `<recv>.body.<TypeDeclBody-method>` chains, audits each
//!   GraphBackedMigrated row for zero raw TypeExpr-body acquisition and required
//!   hot routing, enforces compat-consumer purity, and prevents GraphBackedPending
//!   growth. It does not structurally discover new bare `<expr>.body` or
//!   `<expr>.type_annotation` readers outside enumerated anchors.
//! scanner_justification: Rust privacy cannot expose lower-crate Prepared* TypeExpr
//!   fields only to selected downstream session owners, and untyped `syn` field
//!   reads cannot distinguish declaration-body fields from unrelated same-named
//!   fields.
//! mechanism_ruling: curated inventory plus bounded structural tripwires, not a
//!   global confinement proof. The structural replacement is private owner-layer
//!   body storage plus HotPrepared*/HotTypeRef semantic access and an explicit
//!   AuthoredDeclBody/authored-shape surface with no raw escape to graph-backed
//!   consumers.
//! hardening_rounds: 0
//! hardening_history: narrowed claim; no global field-name scanner round consumed.
//! ```
//!
//! The three per-class scanner records (authored-shape / graph-free / producer)
//! are reproduced on each class in [`ReaderClass`] below.
//!
//! ## What rail closes the surface (read this before trusting the tripwire)
//!
//! The ENUMERATION (`RESIDUAL_BODY_READERS` + `COMPAT_BODY_READERS`) is the
//! curated LEDGER of the `TypeExpr` body readers, partitioned by `ReaderClass`,
//! covering BOTH bare-field reads (`<recv>.body` used as a value, e.g.
//! `.body.clone()`) AND method-chain reads (`<recv>.body.<method>()`). It is NOT
//! an automatic completeness proof: it is a MANUALLY-CURATED list. The AUTOMATIC
//! rails are exactly these, and only these:
//!   (1) PRESENCE + UNIQUENESS — every row ALREADY on the list resolves to exactly
//!       one non-test definition at its anchor (a moved / duplicated / deleted
//!       enumerated reader reddens); this does NOT auto-discover a reader nobody
//!       added to the list.
//!   (2) the `<recv>.body.<TypeDeclBody-method>` METHOD-CHAIN TRIPWIRE — a bounded
//!       supplement that reddens a NEW or MOVED method-chain read whose anchor is
//!       not enumerated.
//!   (3) COMPAT PURITY — the two compat-consumer files perform no direct
//!       method-chain body read after the compat routing.
//!   (4) the GraphBackedMigrated MIGRATED-ANCHOR NO-READ AUDIT — each enumerated
//!       migrated anchor acquires no raw `TypeExpr` body and routes through its
//!       required hot accessor (scoped to the enumerated migrated anchors, NOT a
//!       global field scan).
//!   (5) the GraphBackedPending NON-GROWTH RATCHET — the pending count cannot
//!       grow.
//! A new reader that reads `.body` / `.type_annotation` as a BARE FIELD at a new
//! anchor, or that launders a method-chain read through a local binding
//! (`let b = &lowered.body; b.m()`), is NOT structurally caught by any of these
//! rails. Closing the surface against such a reader is carried by the author
//! keeping the curated enumeration complete and by the behavioural parity rail
//! (the retained oracle) — NOT by an automatic structural scan. None of the five
//! automatic rails, alone or together, proves "no new bare-field reader can
//! appear".
//!
//! ## What this guard confines
//!
//! Type-declaration BODY storage spans two carriers: the lower-crate typed IR
//! (`TypeExpr`) and a `verter_session` arena handle (`HotTypeRef`). The genuinely
//! graph-backed semantic consumers route through the thin `decl_body_hot_ref` hot
//! accessor (the `SemanticGraphStore` `Instantiate` memo) via graph-native
//! predicates/materializers; a second class of readers reads `TypeExpr` because
//! their decision is intrinsically about the authored syntactic form, because they
//! live below the session graph, because their semantic body data has no
//! graph-native arm for its shape, because they are the producer mint itself, or
//! because they are output/compat reads. This inventory enumerates and partitions
//! exactly that residual set. The classes shrink to empty when the structural arms
//! that retire each one exist (private owner-layer body storage plus
//! `HotPrepared*` / `HotTypeRef` semantic access, an explicit `AuthoredDeclBody` /
//! authored-shape surface, a graph-native closedness/key-domain classifier, a
//! below-graph layering seam, and the imported-registry-carrier / locator-split
//! refactors); at that point the `AuthoredShape` / `GraphFreeDto` /
//! `GraphBackedPending` classes are empty and this guard is deleted.
//!
//! This guard pins, structurally:
//!   1. PRESENCE — every enumerated reader is defined at its `(file, impl/mod,
//!      fn)` anchor in the production tree.
//!   2. UNIQUENESS — each anchor resolves to exactly one non-test definition
//!      (a moved / duplicated / disappeared anchor reddens).
//!   3. BOUNDED CLASSIFICATION (the tripwire SUPPLEMENT) — every
//!      `<recv>.body.<TypeDeclBody-method>` read in the production tree sits at
//!      a `(file, impl/mod, fn)` anchor that is in EITHER inventory. A NEW or
//!      MOVED such read outside both reddens immediately. The receiver match
//!      unwraps paren/reference wrappers and also recognizes the UFCS call form
//!      (`TypeDeclBody::m(&x.body)`).
//!   4. COMPAT PURITY — the two body-fact / oracle CONSUMER files
//!      (`fact_emission.rs`, the oracle `source_walk.rs`) contain NO direct
//!      lowered-body method-chain read after the compat routing.
//!   5. NO GraphBackedMigrated TypeExpr-body read — a `GraphBackedMigrated`
//!      anchor must NOT read `prepared.body` / a lowered `.body.<method>` /
//!      `named_decl_body` as a semantic input (it routes through its own required
//!      hot accessor — `decl_body_hot_ref` for the current row). If one does, the
//!      guard REDS.
//!
//! These five rails do NOT structurally discover a new bare `<expr>.body` /
//! `<expr>.type_annotation` reader at a new anchor (see "What rail closes the
//! surface" above) — that surface is closed by the curated enumeration and the
//! behavioural parity rail, not an automatic scan.
//!
//! ## Identity is structural (`syn`), not text
//!
//! Every identity-sensitive question — which `fn` a token sits in (including a
//! token in a NESTED fn / closure), the `impl`/`trait`/`mod` path a `fn` belongs
//! to, whether a `fn` is `#[cfg(test)]`-gated — is answered by parsing each
//! production `src/**` file with [`syn::parse_file`] and walking its item tree
//! (a [`syn::visit::Visit`] impl). Tokens in comments / string literals are
//! invisible to the AST walk and cannot trip (or satisfy) the guard.
//!
//! The tripwire (invariant 3) keys on an UNAMBIGUOUS STRUCTURAL SHAPE, not on a
//! denylist of identifier spellings and not on a receiver BINDING NAME: a
//! `<receiver>.body.<method>` chain (or its UFCS equivalent
//! `TypeDeclBody::<method>(&<receiver>.body)`) whose method is one of
//! `TypeDeclBody`'s reader methods (`lookup_object` / `contributors` /
//! `is_merged` / `primary` / `merged_member_names`). The receiver match unwraps
//! sound paren/reference wrappers, so `(lowered.body).m()` and `(&lowered.body)
//! .m()` are detected identically to `lowered.body.m()`.
//!
//! These method NAMES are NOT unique to `TypeDeclBody`: `contributors` /
//! `primary` also exist on the eval-env `TypeDeclGroup` / `ValueDeclGroup`, and
//! `is_merged` also exists on `HotPreparedTypeDecl`. The shape stays a SOUND
//! proxy not because the names are unique, but because the chain requires the
//! read to go through a `.body` FIELD — and NO `body:` field in the production
//! tree is typed as a colliding type (`TypeDeclGroup` / `ValueDeclGroup` /
//! `HotPreparedTypeDecl` / `TypeDeclInfo` / `ValueDeclInfo`). The only `.body`
//! field carrying these methods is `LoweredTypeDecl.body: TypeDeclBody`. Note
//! `PreparedTypeDecl.body` is a `TypeExpr`, NOT a `TypeDeclBody` (verter_semantic
//! `prepared.rs`), so a `PreparedTypeDecl.body` read CANNOT be a `.body.<method>`
//! chain — it is a bare-FIELD read, anchored by the ENUMERATION, not this
//! tripwire.
//!
//! ## Honest scope (what this guard does and does NOT prove)
//!
//! **This guard is NOT a structural completeness rail for NEW bare-field
//! declaration-body readers.** It is a CURATED ENUMERATION (the authoritative,
//! manually-maintained partition) + a BOUNDED method-chain TRIPWIRE (a supplement
//! that cheaply catches a new/moved `<recv>.body.<method>` read) + the
//! GraphBackedMigrated `syn` AST no-read rail (scoped to the EXACTLY enumerated
//! migrated anchors, NOT a global field scanner) — all backed by the behavioural
//! parity rail (the retained oracle). It is NOT an exhaustive proof that no new
//! body reader can appear, and NONE of its three structural rails alone closes
//! that surface.
//!
//! Concretely, the DISCLOSED LIMIT: a NEW non-inventoried bare `<recv>.body` /
//! `<recv>.type_annotation` FIELD read at a NEW anchor (used as a value, e.g.
//! `.body.clone()`), and a method-chain read laundered through a local binding
//! (`let b = &lowered.body; b.m()`), are OUT of the method-chain tripwire's sound
//! syntactic reach — they are caught ONLY if the author keeps the
//! MANUALLY-CURATED ENUMERATION complete (and by the behavioural parity rail), NOT
//! by an automatic structural scan. Closing this hole structurally requires private
//! owner-layer body storage plus a `HotPrepared*` / `HotTypeRef` semantic surface
//! and an explicit `AuthoredDeclBody` accessor (a global direct-field scanner is
//! rejected as a confinement proof — untyped `syn` field reads cannot attribute a
//! `.body` read to the declaration-body type); that structural arm is a separate
//! surface, not this guard. The
//! `enumeration_is_the_completeness_rail_for_bare_field_readers` self-test PROVES
//! this hole exists (a synthetic new bare-field reader passes the tripwire). The
//! GraphBackedMigrated AST no-read rail does NOT close it either: that rail is
//! scoped to the enumerated migrated anchors, so a brand-new bare-field reader at a
//! brand-new anchor is outside its scope by construction. The negative self-tests
//! that codify this non-detection are DISCLOSED-limit fixtures, not soundness
//! claims.

use std::collections::HashSet;
use std::path::PathBuf;

use syn::visit::Visit;
use syn::{
    visit::visit_file as syn_visit_file, Attribute, ImplItemFn, ItemFn, ItemImpl, ItemMod, Meta,
    Type,
};
use walkdir::WalkDir;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn is_test_file(rel: &str) -> bool {
    rel.ends_with("_tests.rs")
        || rel.ends_with("/tests.rs")
        || rel.contains("/tests/")
        || rel.contains("/tests_")
}

/// Production `.rs` files under `crates/verter_session/src`, relative to the
/// crate root, with their source — test fixtures excluded.
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
// ════════════════════════════════════════════════════════════════════
// `syn` STRUCTURAL INVENTORY — impl-path renderer + cfg-test evaluator
// (the proven machinery from the sibling inventory guard)
// ════════════════════════════════════════════════════════════════════

/// The structural path of the enclosing item a `fn` lives in (impl/trait/mod
/// context), forming the fail-closed anchor identity together with the file +
/// fn name. Two `fn foo`s in two different `impl` blocks of the same file have
/// DISTINCT `impl_path`s. Rendering: `impl Type` / `impl Trait for Type` /
/// `mod m` (chain joined by `::`) / `""` for a free file-scope fn. Lifetimes are
/// normalized away so anchors are stable against lifetime churn.
type ImplPath = String;
/// Render a `syn::Type` (an `impl`'s self-type) into a string suitable for an
/// anchor path, keeping the FULL module-qualified path AND type/const generic
/// args, normalizing ONLY lifetime args away (Finding #3). So
/// `a::HostRuntimeValueResolver` and `b::HostRuntimeValueResolver` render
/// DIFFERENTLY (module quals kept), `Foo<u8>` and `Foo<u16>` render
/// DIFFERENTLY (type generics kept), while `Foo<'_>` and `Foo<'a>` render
/// IDENTICALLY (lifetimes dropped). This keeps the lifetime-stable anchors
/// (`VerterHost`, `DeclBodyMemo`, `HostRuntimeValueResolver<'_>`) intact while
/// making two genuinely-distinct impls collision-safe.
fn render_self_ty(ty: &Type) -> String {
    match ty {
        Type::Path(tp) => {
            let prefix = if tp.qself.is_some() { "<qself>" } else { "" };
            format!("{prefix}{}", render_lifetime_normalized_path(&tp.path))
        }
        // Reference / other exotic self-types are rare for the anchored impls;
        // fall back to a whitespace-collapsed token rendering. (Lifetimes here
        // are not normalized, but no real anchor uses this branch.)
        other => {
            use quote::ToTokens;
            let mut s = other.to_token_stream().to_string();
            s.retain(|c| !c.is_whitespace());
            s
        }
    }
}

/// Render a `syn::Path` keeping every segment (module qualifiers) and every
/// segment's type/const generic args, but DROPPING lifetime generic args. The
/// segments join with `::`; angle-bracketed args render as `<A, B>` with
/// lifetimes removed (an all-lifetime arg list renders no `<>` at all, so
/// `Foo<'_>` → `Foo`).
fn render_lifetime_normalized_path(path: &syn::Path) -> String {
    let mut out = String::new();
    if path.leading_colon.is_some() {
        out.push_str("::");
    }
    for (idx, seg) in path.segments.iter().enumerate() {
        if idx > 0 {
            out.push_str("::");
        }
        out.push_str(&seg.ident.to_string());
        out.push_str(&render_segment_args(&seg.arguments));
    }
    out
}

/// Render a segment's path arguments, dropping lifetime args and keeping
/// type/const args (rendered as lifetime-normalized tokens). Parenthesized
/// (`Fn(…) -> …`) args are rendered verbatim (whitespace-collapsed) — no real
/// anchor uses them.
fn render_segment_args(args: &syn::PathArguments) -> String {
    use quote::ToTokens;
    match args {
        syn::PathArguments::None => String::new(),
        syn::PathArguments::AngleBracketed(ab) => {
            let kept: Vec<String> = ab
                .args
                .iter()
                .filter_map(|arg| match arg {
                    // Drop lifetimes entirely (`'_`, `'a`).
                    syn::GenericArgument::Lifetime(_) => None,
                    // Keep type / const / binding args; render their tokens with
                    // any nested lifetimes also stripped, whitespace collapsed.
                    other => {
                        let mut s = other.to_token_stream().to_string();
                        s = strip_lifetimes_from_token_string(&s);
                        s.retain(|c| !c.is_whitespace());
                        Some(s)
                    }
                })
                .collect();
            if kept.is_empty() {
                String::new()
            } else {
                format!("<{}>", kept.join(","))
            }
        }
        syn::PathArguments::Parenthesized(p) => {
            let mut s = p.to_token_stream().to_string();
            s.retain(|c| !c.is_whitespace());
            s
        }
    }
}

/// Remove lifetime tokens (`'ident` / `'_`) from a rendered token string. Used
/// to normalize lifetimes nested inside a kept type generic arg
/// (e.g. `Foo<Bar<'a>>` → `Foo<Bar>`). Operates on the token-rendered string
/// where a lifetime is an apostrophe followed by ident chars (or `_`).
fn strip_lifetimes_from_token_string(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            // Lifetime: consume the leading `'` and the following ident/`_`
            // chars. (A char literal like `'x'` would also start with `'`, but
            // type generic args never contain char literals.)
            while let Some(&n) = chars.peek() {
                if n.is_alphanumeric() || n == '_' {
                    chars.next();
                } else {
                    break;
                }
            }
            // Drop any now-dangling separator left from `<'a, T>` → `<, T>`.
            // Collapse a leading comma if present.
            while let Some(&n) = chars.peek() {
                if n == ',' || n.is_whitespace() {
                    chars.next();
                } else {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Render an `impl`'s path: `impl Trait for Type` or `impl Type`, with FULL
/// module-qualified trait + self paths, type/const generics kept, and ONLY
/// lifetimes normalized away (Finding #3).
fn render_impl_path(i: &ItemImpl) -> String {
    let self_ty = render_self_ty(&i.self_ty);
    match &i.trait_ {
        Some((_, trait_path, _)) => {
            let trait_rendered = render_lifetime_normalized_path(trait_path);
            format!("impl {trait_rendered} for {self_ty}")
        }
        None => format!("impl {self_ty}"),
    }
}

/// Whether any attribute in `attrs` is a `#[cfg(...)]` whose predicate makes
/// the item **compiled-out in a non-test build** — i.e. the predicate holds
/// ONLY when the `test` cfg is enabled (Finding #1). This is a CONSERVATIVE
/// boolean evaluation, NOT a "the `test` ident appears somewhere" scan: the
/// old fail-open scan wrongly marked `#[cfg(any(unix, test))]` (which COMPILES
/// in a normal production build) as test-only and the reach scan SKIPPED it —
/// so a production whole-env caller behind such a cfg passed undetected. The
/// semantics:
/// - `cfg(test)` → test-only (true).
/// - `cfg(all(A, B, …))` → test-only iff AT LEAST ONE conjunct is test-only
///   (`all` requires every conjunct; if one demands `test`, the whole `all`
///   demands `test`, so the item is compiled-out without `test`).
/// - `cfg(any(A, B, …))` → test-only iff EVERY disjunct is test-only (`any`
///   compiles if ANY branch holds; it is compiled-out in a non-test build only
///   if every branch needs `test`). An empty `any()` holds in no config →
///   vacuously test-only is unsound; treat empty `any()` as NOT test-only.
/// - `cfg(not(X))` → NOT test-only (conservatively false; `not(test)` is
///   production code that MUST be scanned).
/// - any non-`test` leaf (`unix`, `feature = "…"`, `debug_assertions`, …) →
///   not test-only. `feature = "test_util"` keeps `test_util` a distinct IDENT
///   from `test`, and string literals never surface a bare `test` IDENT.
///
/// `#[cfg_attr(...)]` is ignored (it is not a `cfg` gate on item presence).
///
/// A def is excluded from PRESENCE and from the REACH scan ONLY if it is
/// test-only by this evaluator. A def under `#[cfg(any(unix, test))]` is NOT
/// test-only → it MUST be scanned.
fn attrs_are_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        match &a.meta {
            // `#[cfg(<predicate>)]` — evaluate the single predicate inside the
            // outer `cfg(...)` list against the "compiled-out unless test"
            // semantics above.
            Meta::List(list) => cfg_predicate_is_test_only(&list.tokens),
            _ => false,
        }
    })
}

/// Evaluate a `cfg` predicate token stream (the tokens INSIDE the outer
/// `cfg(...)`, or inside a nested `all(...)` / `any(...)` / `not(...)` group)
/// for "is this predicate satisfiable ONLY when `test` is enabled". See
/// [`attrs_are_cfg_test`] for the boolean rules.
///
/// The stream is interpreted as a comma-separated list of predicate terms.
/// At the top level `cfg(...)` carries exactly one term, but a list with a
/// single term is the common shape; multiple top-level terms are treated as an
/// implicit conjunction (matching how `all`'s contents are evaluated).
fn cfg_predicate_is_test_only(ts: &proc_macro2::TokenStream) -> bool {
    let terms = split_cfg_terms(ts);
    if terms.is_empty() {
        return false;
    }
    // A bare `cfg(test)` (one term) or an implicit multi-term list behaves like
    // `all(...)`: test-only iff ANY term is test-only.
    terms.iter().any(|t| cfg_term_is_test_only(t))
}

/// A single predicate term: either a leaf ident (`test`, `unix`,
/// `debug_assertions`), a `name = "value"` option, or a combinator group
/// (`all(...)` / `any(...)` / `not(...)`).
fn cfg_term_is_test_only(term: &[proc_macro2::TokenTree]) -> bool {
    use proc_macro2::TokenTree;
    match term {
        // A lone `test` ident leaf → test-only. Any other lone ident (`unix`,
        // `debug_assertions`) → not test-only.
        [TokenTree::Ident(id)] => *id == "test",
        // `combinator(...)` → an ident followed by a parenthesized group.
        [TokenTree::Ident(id), TokenTree::Group(g)] => {
            let inner = g.stream();
            let parts = split_cfg_terms(&inner);
            match id.to_string().as_str() {
                // `all(A, B, …)` → test-only iff AT LEAST ONE conjunct is.
                "all" => parts.iter().any(|t| cfg_term_is_test_only(t)),
                // `any(A, B, …)` → test-only iff EVERY disjunct is (and the
                // list is non-empty; an empty `any()` holds nowhere and must
                // not be treated as test-only).
                "any" => !parts.is_empty() && parts.iter().all(|t| cfg_term_is_test_only(t)),
                // `not(X)` → conservatively NOT test-only (`not(test)` is
                // production code that must be scanned).
                "not" => false,
                // Unknown combinator-shaped term → conservatively not
                // test-only (must be scanned).
                _ => false,
            }
        }
        // `name = "value"` (e.g. `feature = "test_util"`) or any other shape →
        // not test-only. Distinct IDENTs / a string literal can never be the
        // bare `test` cfg.
        _ => false,
    }
}

/// Split a `cfg` predicate token stream into comma-separated terms, each a
/// `Vec<TokenTree>` (commas dropped). `all(unix, test)` inner stream splits
/// into `[[unix], [test]]`; `feature = "x"` stays one term
/// `[feature, =, "x"]`; `not(test)` stays one term `[not, (test)]`.
fn split_cfg_terms(ts: &proc_macro2::TokenStream) -> Vec<Vec<proc_macro2::TokenTree>> {
    use proc_macro2::TokenTree;
    let mut terms: Vec<Vec<TokenTree>> = Vec::new();
    let mut current: Vec<TokenTree> = Vec::new();
    for tt in ts.clone() {
        match &tt {
            TokenTree::Punct(p) if p.as_char() == ',' => {
                if !current.is_empty() {
                    terms.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(tt),
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

// ════════════════════════════════════════════════════════════════════
// BODY-READ DETECTION — the `<recv>.body.<method>` structural shape
// ════════════════════════════════════════════════════════════════════

/// The `TypeDeclBody` reader-method names whose invocation against a `.body`
/// field (the type carried by `LoweredTypeDecl.body`) constitutes a lowered
/// type-declaration BODY read. This is the FULL set of `TypeDeclBody`'s public
/// reader methods: `lookup_object`, `contributors`, `is_merged`, `primary`,
/// `merged_member_names` (verter_semantic `type_eval.rs`).
///
/// These are `TypeDeclBody` reader methods — but they are NOT defined ONLY on
/// `TypeDeclBody`: `contributors` / `primary` also exist on the eval-env
/// `TypeDeclGroup` / `ValueDeclGroup`, and `is_merged` also exists on
/// `HotPreparedTypeDecl`. The `<recv>.body.<method>` shape stays a SOUND proxy
/// not because the method names are unique, but because requiring the receiver
/// to be a `.body` FIELD access excludes every colliding owner: NO `body:` field
/// in the production tree is typed as a colliding type (`TypeDeclGroup` /
/// `ValueDeclGroup` / `HotPreparedTypeDecl` / `TypeDeclInfo` / `ValueDeclInfo`)
/// — the only `.body` field carrying these methods is `LoweredTypeDecl.body:
/// TypeDeclBody`. That no-colliding-`.body`-field property is a reviewed,
/// verified premise (a targeted `body:\s*<colliding-type>` field-type search
/// returns zero hits today); there is NO concrete false positive on the current
/// tree. A `group.contributors()` (receiver is a decl GROUP, not `.body`) does
/// NOT match.
const BODY_READ_METHODS: &[&str] = &[
    "lookup_object",
    "contributors",
    "is_merged",
    "primary",
    "merged_member_names",
];

/// One structural `fn` DEFINITION discovered by the `syn` walk: the file
/// (crate-relative, forward-slashed), the fn name, its enclosing impl/trait/mod
/// path, whether it (or any enclosing item) is `#[cfg(test)]`-gated, whether it
/// carries a real body, and the SET of `<recv>.body.<method>` body-read shapes
/// found ANYWHERE in its body subtree (including nested fns / closures — those
/// are recorded both here AND as their own `FnDef`, so an anchored fn is judged
/// only by its OWN direct reads in the tripwire's per-anchor view). The recorded
/// value is the method name (`lookup_object` / `contributors` / `is_merged`).
#[derive(Debug, Clone)]
struct FnDef {
    file: String,
    name: String,
    impl_path: ImplPath,
    cfg_test: bool,
    has_body: bool,
    /// The `<recv>.body.<method>` body-read method names referenced DIRECTLY in
    /// this fn's own statements (NOT descending nested item-fns — see
    /// [`BodyReadCollector`]). Empty for a fn that performs no such read.
    body_reads: HashSet<String>,
}

impl FnDef {
    /// The fail-closed anchor identity: `(file, impl_path, name)`.
    fn anchor(&self) -> (String, String, String) {
        (self.file.clone(), self.impl_path.clone(), self.name.clone())
    }

    /// Whether this fn directly performs at least one `<recv>.body.<method>`
    /// lowered-body read.
    fn reads_lowered_body(&self) -> bool {
        !self.body_reads.is_empty()
    }
}

/// Collect the `<recv>.body.<method>` body-read shapes in ONE fn's own body,
/// WITHOUT descending into nested item-`fn`s (a nested `fn`'s reads belong to
/// the nested fn's own `FnDef`, recorded separately by the scanner) — so the
/// per-anchor body-read set is exactly the reads written in that anchor's own
/// statement list. Closures ARE descended (a closure is part of the enclosing
/// fn's body, not its own anchored item).
///
/// Detection (pure AST shape, no text, no binding name): EITHER (a) a method
/// call whose method ident is in [`BODY_READ_METHODS`] AND whose receiver
/// expression, after paren/reference unwrap ([`unwrap_receiver`]), is a NAMED
/// field access `<expr>.body`; OR (b) the UFCS call form
/// `TypeDeclBody::<method>(&<expr>.body)` — a qualified call whose function-path
/// final segment is a body-read method and one of whose arguments is a `.body`
/// field. Tokens in comments / string literals are invisible to the AST and
/// cannot trip this. A method-chain read laundered through a local binding
/// (`let b = &lowered.body; b.m()`) is OUT of sound syntactic reach and
/// deliberately not detected here (covered by the enumeration — see the module
/// "Honest scope" note).
struct BodyReadCollector {
    reads: HashSet<String>,
}

impl BodyReadCollector {
    fn new() -> Self {
        Self {
            reads: HashSet::new(),
        }
    }
}

/// Peel sound, value-preserving receiver wrappers — parentheses (`(x)`) and a
/// reference/deref taken inline (`&x` / `&mut x` / `*x`) — that do NOT change
/// which field/value the chain ultimately reads. So `(lowered.body).m()`,
/// `(&lowered.body).m()`, and `(&mut lowered.body).m()` all unwrap to the
/// `lowered.body` field access. This is a BOUNDED, sound normalization (it only
/// strips wrappers that re-expose the SAME place expression), NOT a spelling
/// chase: it does not follow a local binding (`let b = &lowered.body; b.m()`),
/// which is an irreducible launder out of a syntactic guard's sound reach (see
/// the module "Honest scope" note).
fn unwrap_receiver(expr: &syn::Expr) -> &syn::Expr {
    match expr {
        syn::Expr::Paren(p) => unwrap_receiver(&p.expr),
        syn::Expr::Group(g) => unwrap_receiver(&g.expr),
        syn::Expr::Reference(r) => unwrap_receiver(&r.expr),
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Deref(_),
            expr,
            ..
        }) => unwrap_receiver(expr),
        other => other,
    }
}

/// Whether `expr`, AFTER paren/reference unwrap, is a NAMED field access
/// `<something>.body` (the receiver shape that makes a `TypeDeclBody` method call
/// a lowered-body read). Unwrapping makes `(lowered.body).m()` / `(&lowered
/// .body).m()` detected identically to `lowered.body.m()`.
fn is_dot_body_field(expr: &syn::Expr) -> bool {
    matches!(
        unwrap_receiver(expr),
        syn::Expr::Field(syn::ExprField {
            member: syn::Member::Named(name),
            ..
        }) if *name == "body"
    )
}

/// Whether `path`'s FINAL segment ident is one of the `TypeDeclBody` reader
/// methods — used to recognize a UFCS call `TypeDeclBody::lookup_object(&x.body)`
/// / `<TypeDeclBody>::contributors(...)` whose function path ends in a body-read
/// method. Returns the method name if so.
fn ufcs_body_read_method(func: &syn::Expr) -> Option<String> {
    let path = match func {
        syn::Expr::Path(p) => &p.path,
        _ => return None,
    };
    // A UFCS body read is a QUALIFIED call: `Type::method(..)` (≥2 segments) or
    // `<Type>::method(..)` (qself set). A bare `method(..)` free-fn call is NOT
    // a `.body` read shape and must not match.
    let qualified =
        path.segments.len() >= 2 || matches!(func, syn::Expr::Path(p) if p.qself.is_some());
    if !qualified {
        return None;
    }
    let last = path.segments.last()?;
    let name = last.ident.to_string();
    if BODY_READ_METHODS.contains(&name.as_str()) {
        Some(name)
    } else {
        None
    }
}

/// Whether any argument expression is (after paren/reference unwrap) a `.body`
/// field access — the receiver-as-argument shape of a UFCS body read.
fn any_arg_is_dot_body_field<'a>(args: impl Iterator<Item = &'a syn::Expr>) -> bool {
    args.into_iter().any(is_dot_body_field)
}

impl<'ast> Visit<'ast> for BodyReadCollector {
    fn visit_expr_method_call(&mut self, c: &'ast syn::ExprMethodCall) {
        let method = c.method.to_string();
        if BODY_READ_METHODS.contains(&method.as_str()) && is_dot_body_field(&c.receiver) {
            self.reads.insert(method);
        }
        // Descend operands / args / closures, but NOT nested item-fns (the
        // default `Visit` walk does not enter `ItemFn` from an expression
        // context anyway; closures are walked as part of this body).
        syn::visit::visit_expr_method_call(self, c);
    }

    /// Detect the UFCS / fully-qualified call form of a body read:
    /// `TypeDeclBody::lookup_object(&lowered.body)` /
    /// `<TypeDeclBody>::contributors(&lowered.body)` — a qualified call whose
    /// function-path final segment is a body-read method AND one of whose
    /// arguments is (after paren/reference unwrap) a `.body` field access.
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let Some(method) = ufcs_body_read_method(&c.func) {
            if any_arg_is_dot_body_field(c.args.iter()) {
                self.reads.insert(method);
            }
        }
        syn::visit::visit_expr_call(self, c);
    }

    /// Do NOT descend into a nested `fn` item declared inside this body — its
    /// reads are recorded as its own `FnDef`. (Overriding to a no-op keeps the
    /// enclosing fn's `body_reads` to its OWN statements.)
    fn visit_item_fn(&mut self, _f: &'ast ItemFn) {
        // intentionally empty: nested item-fns are recorded separately
    }
}

/// Collect the body-read method set for ONE fn block (its own statements +
/// closures, excluding nested item-fns).
fn collect_body_reads(block: &syn::Block) -> HashSet<String> {
    let mut c = BodyReadCollector::new();
    c.visit_block(block);
    c.reads
}

// ════════════════════════════════════════════════════════════════════
// GraphBackedMigrated no-read AUDIT — a `syn` AST visitor over ONE anchored
// fn body. A structural visitor that cannot see comments/strings, is
// alias/rename robust, and is scoped to the EXACT `(impl_path, fn)` anchor.
// ════════════════════════════════════════════════════════════════════

/// The forbidden CALL idents a `GraphBackedMigrated` anchor must NOT invoke in
/// its own body — the body-locator producers a migrated reader must route AROUND
/// (it reaches the body through `decl_body_hot_ref` instead). Matched as a
/// method-call ident OR a call-path FINAL segment (so `engine.named_decl_body(..)`
/// and `Self::named_decl_body(..)` / `prepared_type_decl(..)` all match), NOT as
/// a text needle.
const MIGRATED_FORBIDDEN_CALL_IDENTS: &[&str] = &["prepared_type_decl", "named_decl_body"];

/// The forbidden FIELD-access member names a `GraphBackedMigrated` anchor must NOT
/// read in its own body — a `<expr>.body` or `<expr>.type_annotation` declaration-
/// body field read (the `TypeExpr` carrier). Matched structurally as a
/// `syn::Member::Named`, so `prepared.body`, `(get_prepared()).body`, and an
/// aliased `p.body` (where `let p = get_prepared();`) are ALL caught — the launder
/// the text needle `"prepared.body"` missed.
const MIGRATED_FORBIDDEN_FIELD_MEMBERS: &[&str] = &["body", "type_annotation"];

/// The known hot-route call idents a `GraphBackedMigrated` row MAY declare as its
/// REQUIRED route — the shared hot accessor and the alternate graph-native
/// member-surface arm. This is the UNIVERSE of valid options, NOT the per-row
/// requirement: each row names the EXACT subset that satisfies it via
/// [`ReaderRow::required_hot_route`] (e.g. `lower_decl_body_to_node` requires
/// `decl_body_hot_ref` ONLY — `materialize_member_surface_node` does not satisfy
/// it). `materialize_member_surface_node` stays a valid option for a member-surface-
/// route row. The auditor consults the ROW's own set, never this universe.
const KNOWN_HOT_ROUTE_IDENTS: &[&str] = &["decl_body_hot_ref", "materialize_member_surface_node"];

/// A `syn::visit::Visit` AST auditor over ONE anchored fn's body: structurally
/// collects (a) every forbidden `TypeExpr`-body read — a `<expr>.body` /
/// `<expr>.type_annotation` field access OR a `prepared_type_decl` /
/// `named_decl_body` call — and (b) whether the body invokes the EXACT hot-route
/// ident(s) THIS row requires (`required_idents`, supplied per-row from
/// [`ReaderRow::required_hot_route`]). A call to a graph-native arm NOT in the
/// row's set does NOT mark the body routed. Descends closures and nested
/// expressions but NOT nested item-`fn`s (a nested fn is a separate anchor),
/// matching the per-anchor scope of [`BodyReadCollector`]. Tokens in comments /
/// string literals are invisible to the AST and cannot satisfy or trip it.
struct MigratedBodyAuditor<'a> {
    /// The EXACT hot-route call ident(s) that satisfy THIS row's requirement —
    /// the per-row [`ReaderRow::required_hot_route`] set, NOT a flat global list.
    required_idents: &'a [&'a str],
    /// The forbidden read shapes found, each a human label
    /// (`<recv>.body field read` / `named_decl_body(..) call` / …) for the
    /// diagnostic.
    forbidden_reads: Vec<String>,
    /// Whether one of THIS row's required hot-route calls was found.
    routes_through_hot_accessor: bool,
}

impl<'a> MigratedBodyAuditor<'a> {
    fn new(required_idents: &'a [&'a str]) -> Self {
        Self {
            required_idents,
            forbidden_reads: Vec::new(),
            routes_through_hot_accessor: false,
        }
    }
}

/// The FINAL segment ident of a call's function path (for a path call
/// `a::b::c(..)` → `c`; for `<T>::m(..)` → `m`). `None` if `func` is not a path.
fn call_path_final_ident(func: &syn::Expr) -> Option<String> {
    match func {
        syn::Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

impl<'ast> Visit<'ast> for MigratedBodyAuditor<'_> {
    fn visit_expr_field(&mut self, f: &'ast syn::ExprField) {
        if let syn::Member::Named(name) = &f.member {
            let n = name.to_string();
            if MIGRATED_FORBIDDEN_FIELD_MEMBERS.contains(&n.as_str()) {
                self.forbidden_reads
                    .push(format!("`<expr>.{n}` field read"));
            }
        }
        syn::visit::visit_expr_field(self, f);
    }

    fn visit_expr_method_call(&mut self, c: &'ast syn::ExprMethodCall) {
        let m = c.method.to_string();
        if MIGRATED_FORBIDDEN_CALL_IDENTS.contains(&m.as_str()) {
            self.forbidden_reads.push(format!("`.{m}(..)` method call"));
        }
        if self.required_idents.contains(&m.as_str()) {
            self.routes_through_hot_accessor = true;
        }
        syn::visit::visit_expr_method_call(self, c);
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let Some(ident) = call_path_final_ident(&c.func) {
            if MIGRATED_FORBIDDEN_CALL_IDENTS.contains(&ident.as_str()) {
                self.forbidden_reads.push(format!("`{ident}(..)` call"));
            }
            if self.required_idents.contains(&ident.as_str()) {
                self.routes_through_hot_accessor = true;
            }
        }
        syn::visit::visit_expr_call(self, c);
    }

    /// Do NOT descend into a nested item-`fn` — its reads belong to its own
    /// anchor (matching `BodyReadCollector`). Closures ARE descended (the default
    /// walk enters closure bodies as part of this fn).
    fn visit_item_fn(&mut self, _f: &'ast ItemFn) {
        // intentionally empty: nested item-fns are a separate anchor
    }
}

/// The structural verdict for ONE `GraphBackedMigrated` anchor body: the forbidden
/// `TypeExpr`-body reads it performs (empty = clean) and whether it routes through
/// the hot accessor. Produced by [`audit_migrated_anchor_block`].
#[derive(Debug)]
struct MigratedBodyVerdict {
    forbidden_reads: Vec<String>,
    routes_through_hot_accessor: bool,
}

/// Run the [`MigratedBodyAuditor`] `syn` visitor over one fn `Block`, judging
/// `routes_through_hot_accessor` against THIS row's `required_idents`.
fn audit_migrated_anchor_block(
    block: &syn::Block,
    required_idents: &[&str],
) -> MigratedBodyVerdict {
    let mut a = MigratedBodyAuditor::new(required_idents);
    a.visit_block(block);
    MigratedBodyVerdict {
        forbidden_reads: a.forbidden_reads,
        routes_through_hot_accessor: a.routes_through_hot_accessor,
    }
}

/// A `syn::visit::Visit` over ONE production file that, tracking the same
/// impl/trait/mod path stack + cfg-test depth as [`InventoryScanner`], finds the
/// EXACT `(impl_path, fn_name)` anchor and audits its body with
/// [`audit_migrated_anchor_block`]. Scoping by the rendered `impl_path` (not line
/// proximity) ensures the RIGHT fn in the RIGHT impl is checked. Records every
/// matching anchor's verdict (PRESENCE/UNIQUENESS guard the count separately).
struct MigratedAnchorAuditScanner<'a> {
    target_impl_path: &'a str,
    target_fn: &'a str,
    /// The EXACT hot-route ident(s) THIS target row requires (its
    /// [`ReaderRow::required_hot_route`]), threaded into the per-block auditor.
    required_idents: &'a [&'a str],
    path_stack: Vec<String>,
    cfg_test_depth: u32,
    /// Verdicts for every NON-test anchor matching `(target_impl_path,
    /// target_fn)` in this file.
    verdicts: Vec<MigratedBodyVerdict>,
}

impl<'a> MigratedAnchorAuditScanner<'a> {
    fn new(target_impl_path: &'a str, target_fn: &'a str, required_idents: &'a [&'a str]) -> Self {
        Self {
            target_impl_path,
            target_fn,
            required_idents,
            path_stack: Vec::new(),
            cfg_test_depth: 0,
            verdicts: Vec::new(),
        }
    }

    fn current_path(&self) -> String {
        self.path_stack.join("::")
    }

    /// Audit `block` IFF the current `(impl_path, fn)` and cfg-test status match
    /// the non-test target anchor.
    fn maybe_audit(&mut self, fn_ident: &syn::Ident, block: &syn::Block, fn_is_cfg_test: bool) {
        if self.cfg_test_depth > 0 || fn_is_cfg_test {
            return;
        }
        if self.current_path() == self.target_impl_path && *fn_ident == self.target_fn {
            self.verdicts
                .push(audit_migrated_anchor_block(block, self.required_idents));
        }
    }
}

impl<'ast> Visit<'ast> for MigratedAnchorAuditScanner<'_> {
    fn visit_item_mod(&mut self, m: &'ast ItemMod) {
        let entered_test = attrs_are_cfg_test(&m.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        self.path_stack.push(format!("mod {}", m.ident));
        syn::visit::visit_item_mod(self, m);
        self.path_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        let entered_test = attrs_are_cfg_test(&i.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        self.path_stack.push(render_impl_path(i));
        syn::visit::visit_item_impl(self, i);
        self.path_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_trait(&mut self, t: &'ast syn::ItemTrait) {
        let entered_test = attrs_are_cfg_test(&t.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        self.path_stack.push(format!("trait {}", t.ident));
        syn::visit::visit_item_trait(self, t);
        self.path_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        self.maybe_audit(&f.sig.ident, &f.block, attrs_are_cfg_test(&f.attrs));
        syn::visit::visit_item_fn(self, f);
    }

    fn visit_impl_item_fn(&mut self, f: &'ast ImplItemFn) {
        self.maybe_audit(&f.sig.ident, &f.block, attrs_are_cfg_test(&f.attrs));
        syn::visit::visit_impl_item_fn(self, f);
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        if let Some(block) = &f.default {
            self.maybe_audit(&f.sig.ident, block, attrs_are_cfg_test(&f.attrs));
        }
        syn::visit::visit_trait_item_fn(self, f);
    }
}

/// Parse `src` and audit the EXACT non-test `(impl_path, fn_name)` anchor's body
/// with the `syn` [`MigratedBodyAuditor`], judging routing against `required_idents`
/// — the per-row [`ReaderRow::required_hot_route`] of the audited anchor. Returns
/// each matching anchor's verdict (normally exactly one — UNIQUENESS pins the count).
fn audit_migrated_anchor(
    file: &str,
    src: &str,
    impl_path: &str,
    fn_name: &str,
    required_idents: &[&str],
) -> Vec<MigratedBodyVerdict> {
    let parsed = syn::parse_file(src).unwrap_or_else(|e| panic!("parse {file}: {e}"));
    let mut scanner = MigratedAnchorAuditScanner::new(impl_path, fn_name, required_idents);
    syn_visit_file(&mut scanner, &parsed);
    scanner.verdicts
}

/// The `syn::Visit` scanner that builds the structural fn-definition inventory
/// for one file, tracking the enclosing impl/trait/mod path stack and a cfg-test
/// nesting depth. Free `fn`s (`ItemFn`), `impl`-method `fn`s (`ImplItemFn`), and
/// trait default / declaration `fn`s are recorded; each carries the body-read
/// method set found in its OWN body.
struct InventoryScanner<'a> {
    file: &'a str,
    path_stack: Vec<String>,
    cfg_test_depth: u32,
    defs: &'a mut Vec<FnDef>,
}

impl<'a> InventoryScanner<'a> {
    fn new(file: &'a str, defs: &'a mut Vec<FnDef>) -> Self {
        Self {
            file,
            path_stack: Vec::new(),
            cfg_test_depth: 0,
            defs,
        }
    }

    fn current_path(&self) -> String {
        self.path_stack.join("::")
    }
}

impl<'ast> Visit<'ast> for InventoryScanner<'_> {
    fn visit_item_mod(&mut self, m: &'ast ItemMod) {
        let entered_test = attrs_are_cfg_test(&m.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        self.path_stack.push(format!("mod {}", m.ident));
        syn::visit::visit_item_mod(self, m);
        self.path_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        let entered_test = attrs_are_cfg_test(&i.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        self.path_stack.push(render_impl_path(i));
        syn::visit::visit_item_impl(self, i);
        self.path_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_trait(&mut self, t: &'ast syn::ItemTrait) {
        let entered_test = attrs_are_cfg_test(&t.attrs);
        if entered_test {
            self.cfg_test_depth += 1;
        }
        self.path_stack.push(format!("trait {}", t.ident));
        syn::visit::visit_item_trait(self, t);
        self.path_stack.pop();
        if entered_test {
            self.cfg_test_depth -= 1;
        }
    }

    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        let cfg_test = self.cfg_test_depth > 0 || attrs_are_cfg_test(&f.attrs);
        self.defs.push(FnDef {
            file: self.file.to_string(),
            name: f.sig.ident.to_string(),
            impl_path: self.current_path(),
            cfg_test,
            has_body: true,
            body_reads: collect_body_reads(&f.block),
        });
        // Descend so nested item-fns / impls inside this body are recorded too.
        syn::visit::visit_item_fn(self, f);
    }

    fn visit_impl_item_fn(&mut self, f: &'ast ImplItemFn) {
        let cfg_test = self.cfg_test_depth > 0 || attrs_are_cfg_test(&f.attrs);
        self.defs.push(FnDef {
            file: self.file.to_string(),
            name: f.sig.ident.to_string(),
            impl_path: self.current_path(),
            cfg_test,
            has_body: true,
            body_reads: collect_body_reads(&f.block),
        });
        syn::visit::visit_impl_item_fn(self, f);
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        let cfg_test = self.cfg_test_depth > 0 || attrs_are_cfg_test(&f.attrs);
        match &f.default {
            Some(block) => self.defs.push(FnDef {
                file: self.file.to_string(),
                name: f.sig.ident.to_string(),
                impl_path: self.current_path(),
                cfg_test,
                has_body: true,
                body_reads: collect_body_reads(block),
            }),
            None => self.defs.push(FnDef {
                file: self.file.to_string(),
                name: f.sig.ident.to_string(),
                impl_path: self.current_path(),
                cfg_test,
                has_body: false,
                body_reads: HashSet::new(),
            }),
        }
        syn::visit::visit_trait_item_fn(self, f);
    }
}

/// Parse every production `src/**` file with `syn` and return the flat
/// structural inventory of every fn definition. A parse error is a hard panic
/// (corruption signal), matching the in-house pattern.
fn build_fn_inventory(files: &[(String, String)]) -> Vec<FnDef> {
    let mut defs = Vec::new();
    for (rel, src) in files {
        let parsed = syn::parse_file(src).unwrap_or_else(|e| panic!("parse {rel}: {e}"));
        let mut scanner = InventoryScanner::new(rel, &mut defs);
        syn_visit_file(&mut scanner, &parsed);
    }
    defs
}

/// Parse one synthetic `(file, src)` slice into the structural inventory — the
/// self-test entry point.
fn inventory_for(files: &[(String, String)]) -> Vec<FnDef> {
    build_fn_inventory(files)
}

// ════════════════════════════════════════════════════════════════════
// READER CLASS — the partition the residual TypeExpr body readers carry
// ════════════════════════════════════════════════════════════════════

/// The class a residual `TypeExpr` declaration-body reader belongs to. The
/// partition is the architectural JUDGEMENT (no compiler fact expresses it while
/// raw `TypeExpr` bodies stay readable) the curated allowlist records — the
/// design ruling lives in
/// docs/arch/authored-shape-graph-native-migration-deferral.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReaderClass {
    /// MIGRATED onto the shared `decl_body_hot_ref` hot accessor / a graph-native
    /// arm — these anchors must NOT read a declaration body as `TypeExpr` for a
    /// semantic decision (enforced by
    /// [`graph_backed_migrated_anchors_perform_no_typeexpr_body_read`]).
    GraphBackedMigrated,

    /// Authored-shape reader — the decision is intrinsically about the AUTHORED
    /// syntactic form (literal `Pick` / `Omit` head, `IndexedAccess` object
    /// chain, `Ref { type_arguments }`, heritage `extends` / `implements` refs,
    /// closedness / key-domain over the authored `TypeExpr`). Stays `TypeExpr`.
    ///
    /// ```text
    /// scanner_invariant: reads of a declaration body as TypeExpr for authored-shape
    ///   decisions are CONFINED BY POLICY to the enumerated production anchors in this
    ///   class (the curated inventory row), NOT a structurally-enforced embargo. These
    ///   reads may inspect preserved syntax but must not feed graph-backed semantic
    ///   reduction. A new bare-field reader outside the enumeration is caught by the
    ///   curated enumeration + the behavioural parity rail, not by this guard.
    /// scanner_justification: Whether a reader requires authored syntax is an
    ///   architectural judgment not expressible by Rust while raw TypeExpr bodies
    ///   remain available.
    /// mechanism_ruling: curated inventory row, not a structural confinement proof.
    ///   The structural replacement is private body storage plus an AuthoredDeclBody
    ///   wrapper/tokened accessor.
    /// hardening_rounds: 0
    /// ```
    AuthoredShape,

    /// Graph-free DTO reader — lives BELOW the session `SemanticGraphStore`
    /// (shallow-file route closures, external-type-frontier resolve paths,
    /// eval-env value-decl peel) and cannot carry a `HotTypeRef` without making a
    /// below-graph DTO depend on the session graph. Stays `TypeExpr`.
    ///
    /// ```text
    /// scanner_invariant: carriage of a declaration body as TypeExpr on a below-graph
    ///   DTO/frontier/eval-env path is CONFINED BY POLICY to the enumerated paths in
    ///   this class (the curated inventory row), NOT a structurally-enforced embargo;
    ///   these paths must not depend on HotTypeRef, SemanticGraphStore, or
    ///   decl_body_hot_ref. A new bare-field reader outside the enumeration is caught
    ///   by the curated enumeration + the behavioural parity rail, not by this guard.
    /// scanner_justification: These paths intentionally live below the session
    ///   graph. Current layout may not encode every boundary as a crate
    ///   dependency.
    /// mechanism_ruling: curated inventory row, not a structural confinement proof.
    ///   Prefer Cargo/module dependency enforcement of the below-graph boundary; this
    ///   inventory only records the same-crate residual reads.
    /// hardening_rounds: 0
    /// ```
    GraphFreeDto,

    /// Graph-backed-PENDING reader — a genuinely graph-backed reader that reads
    /// the body as `TypeExpr` because its shape has no graph-native arm. Each row
    /// stays `TypeExpr` because the structural arm that would let it route through
    /// `decl_body_hot_ref` / graph-native dispatch is absent (the imported-registry
    /// body carrier carries `TypeExpr` rather than identity / `HotTypeRef`; the
    /// member-surface-route key enumerator has no graph-native `SemanticNodeData`
    /// key enumerator; the locator is a single `TypeExpr`-returning locator, not
    /// split into identity/hot vs authored-body locators; the
    /// `PreparedValueDecl.type_annotation` value-decl handle has no `HotPreparedValueDecl`
    /// annotation handle). Unlike the `AuthoredShape` / `GraphFreeDto` /
    /// `ProducerLowering` / `OutputCompat` classes (which record WHY a reader reads
    /// `TypeExpr` as a settled architectural fact), this class is a NON-GROWTH
    /// BOUNDED SET: it is bounded at the readers structurally requiring such an arm,
    /// may only shrink (a row leaves the moment its structural arm lands), never
    /// grow, and the cap guard [`graph_backed_pending_is_a_non_growth_bounded_class`]
    /// REDDENS on any growth. Each row's `reason` names the structural arm that
    /// would retire it.
    ///
    /// ```text
    /// scanner_invariant: a non-growth bounded set — every GraphBackedPending row is
    ///   a graph-backed reader whose shape has no graph-native arm; the set is
    ///   bounded at the readers structurally requiring such an arm, shrinks to empty
    ///   as those arms land, and never grows.
    /// scanner_justification: which graph-backed reader needs a larger structural arm
    ///   (a carrier identity flip, a graph-native key enumerator, a locator split, a
    ///   value-decl annotation handle) before it can route through decl_body_hot_ref
    ///   is an architectural judgement; the named arm in each row records the
    ///   boundary, a present architectural fact.
    /// mechanism_ruling: non-growth bounded set — the cap REDs on growth and is
    ///   LOWERED as each structural arm lands; the set empties as the arms land and
    ///   this inventory is deleted.
    /// hardening_rounds: 0
    /// ```
    GraphBackedPending,

    /// Producer lowering — the body mint itself (lowers the authored body into
    /// the semantic graph) and the eager clone path it backs. Exempt: it is the
    /// required bridge from authored IR into graph IR.
    ///
    /// ```text
    /// scanner_invariant: reads of the authored body to lower it into the semantic
    ///   graph and mint a HotTypeRef/SemanticNodeId are CONFINED BY POLICY to the
    ///   enumerated producer anchors in this class (the curated inventory row), NOT a
    ///   structurally-enforced embargo; these anchors must not return or cache TypeExpr
    ///   as a hot carrier. A new bare-field reader outside the enumeration is caught by
    ///   the curated enumeration + the behavioural parity rail, not by this guard.
    /// scanner_justification: The producer is the required bridge from authored IR
    ///   into graph IR; purpose is not distinguishable while the field is publicly
    ///   readable.
    /// mechanism_ruling: curated inventory row, not a structural confinement proof.
    ///   The structural replacement is private prepared-body access scoped to the
    ///   producer modules.
    /// hardening_rounds: 0
    /// ```
    ProducerLowering,

    /// Output / compat read — the sanctioned fingerprint-hash input and the
    /// typeinfo/hover oracle contributor read, routed through the named compat
    /// helpers. Output-only, not a semantic resolution decision.
    OutputCompat,
}

impl ReaderClass {
    /// The human label used in diagnostics and the per-class count pins.
    fn label(self) -> &'static str {
        match self {
            ReaderClass::GraphBackedMigrated => "GraphBackedMigrated",
            ReaderClass::AuthoredShape => "AuthoredShape",
            ReaderClass::GraphFreeDto => "GraphFreeDto",
            ReaderClass::GraphBackedPending => "GraphBackedPending",
            ReaderClass::ProducerLowering => "ProducerLowering",
            ReaderClass::OutputCompat => "OutputCompat",
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// ANCHOR TABLES — the partitioned residual TypeExpr surface + COMPAT
// ════════════════════════════════════════════════════════════════════

/// One anchored residual declaration-BODY reader: the production file
/// (crate-relative, forward-slashed), the enclosing impl/trait/mod path
/// (`render_impl_path` form; `""` for a free file-scope fn), the fn name, its
/// `ReaderClass`, whether the fn performs a `<recv>.body.<method>` method-chain
/// read (so the tripwire's per-anchor view can be cross-checked against
/// PRESENCE), and a required rationale naming WHY it sits in its class.
struct ReaderRow {
    file: &'static str,
    impl_path: &'static str,
    fn_name: &'static str,
    class: ReaderClass,
    /// `true` iff this reader contains at least one `<recv>.body.<method>`
    /// (`lookup_object` / `contributors` / `is_merged` / `primary` /
    /// `merged_member_names`) read — the load-bearing tripwire shape. `false` for
    /// a reader that only reads `<recv>.body` as a bare FIELD (or
    /// `type_annotation` / `merged_contributors`), which the tripwire does not
    /// classify (honest scope) but PRESENCE still anchors.
    method_chain: bool,
    /// For a [`ReaderClass::GraphBackedMigrated`] row: the EXACT hot-route call
    /// ident(s) that satisfy THIS anchor's required routing — the
    /// [`graph_backed_migrated_anchors_perform_no_typeexpr_body_read`] audit
    /// computes `routes_through_hot_accessor` against this per-row set, NOT a flat
    /// global list, so a row that must route through `decl_body_hot_ref` is NOT
    /// satisfied by an unrelated graph-native arm (`materialize_member_surface_node`).
    /// Empty `&[]` for every non-migrated row (the audit only consults it for
    /// `GraphBackedMigrated` anchors).
    required_hot_route: &'static [&'static str],
    reason: &'static str,
}

/// The residual `TypeExpr` body-reader surface, partitioned by [`ReaderClass`].
/// `GraphBackedMigrated` rows route through the shared `decl_body_hot_ref` hot
/// accessor / a graph-native arm and must NOT read a declaration body as
/// `TypeExpr`; the `AuthoredShape` / `GraphFreeDto` / `GraphBackedPending` /
/// `ProducerLowering` rows are the justified stay-allowlist. `fact_emission.rs`
/// and the oracle `source_walk.rs` are EXCLUDED (they are `OutputCompat` in
/// `COMPAT_BODY_READERS`).
const RESIDUAL_BODY_READERS: &[ReaderRow] = &[
    // ── GraphBackedMigrated — routed through decl_body_hot_ref ──────────
    ReaderRow {
        file: "src/meta_resolve/projectors/macro_payload_substrate.rs",
        impl_path: "",
        fn_name: "lower_decl_body_to_node",
        class: ReaderClass::GraphBackedMigrated,
        method_chain: false,
        // This anchor routes through `decl_body_hot_ref` SPECIFICALLY — the
        // unrelated `materialize_member_surface_node` graph-native arm does NOT
        // satisfy it (the per-row requirement is exact, not the union of every
        // graph-native arm).
        required_hot_route: &["decl_body_hot_ref"],
        reason: "resolves the named declaration body to a graph node through the shared \
                 decl_body_hot_ref hot accessor (the Instantiate memo, published Navigate) and \
                 returns its node; reads no prepared.body, no reverse bridge",
    },
    // ── ProducerLowering — the mint + the eager clone path it backs ─────
    ReaderRow {
        file: "src/project_semantic_dispatch/build.rs",
        impl_path: "impl ProjectSemanticDispatch",
        fn_name: "lower_decl_body_with_provenance",
        class: ReaderClass::ProducerLowering,
        method_chain: false,
        required_hot_route: &[],
        reason: "the body-lowering producer/mint — reads prepared.body + the merged_contributors \
                 gate to lower the authored body into the semantic graph; the required bridge from \
                 authored IR into graph IR (the hot accessor wraps THIS producer's Instantiate \
                 result)",
    },
    ReaderRow {
        file: "src/resolver_core/prepared_decl.rs",
        impl_path: "",
        fn_name: "prepare_type_decl_from_lowered",
        class: ReaderClass::ProducerLowering,
        method_chain: true,
        required_hot_route: &[],
        reason: "the eager clone path the producer backs — reads lowered.body via lookup_object() / \
                 is_merged() / contributors() when building the PreparedTypeDecl; producer-mint \
                 class, preserved",
    },
    ReaderRow {
        file: "src/resolver_core/prepared_decl.rs",
        impl_path: "",
        fn_name: "prepare_local_value_decl",
        class: ReaderClass::ProducerLowering,
        method_chain: false,
        required_hot_route: &[],
        reason: "the value eager clone path — reads lowered.type_annotation (LoweredValueDecl) when \
                 preparing a local value decl; producer-mint class, preserved",
    },
    // ── AuthoredShape — decision is intrinsically about authored syntax ──
    ReaderRow {
        file: "src/project_semantic_dispatch/build.rs",
        impl_path: "impl ProjectSemanticDispatch",
        fn_name: "class_heritage_bases",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body for the authored heritage extends/implements refs of a class \
                 declaration body — authored-syntax-intrinsic",
    },
    ReaderRow {
        file: "src/project_semantic_dispatch/raise.rs",
        impl_path: "",
        fn_name: "userland_instantiation_body_is_closed_object",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body to classify whether a userland instantiation body is a closed \
                 object — authored-shape closedness over the TypeExpr",
    },
    ReaderRow {
        // Free file-scope fn — the `impl KeyDomainBinding<'_>` block at raise.rs
        // closes before this definition (verified via `syn`, not line proximity).
        file: "src/project_semantic_dispatch/raise.rs",
        impl_path: "",
        fn_name: "prepared_decl_body_is_closed_unguarded",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body to classify closedness of a prepared decl body — authored-shape \
                 closedness over the TypeExpr",
    },
    ReaderRow {
        // Free file-scope fn (same as above — outside the KeyDomainBinding impl).
        file: "src/project_semantic_dispatch/raise.rs",
        impl_path: "",
        fn_name: "prepared_instantiation_key_domain_is_closed",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body to classify whether an instantiation's key domain is closed — \
                 authored-shape key-domain over the TypeExpr",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_registry.rs",
        impl_path: "",
        fn_name: "component_meta_registry_owner_local_component_config_alias_name",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body (and a one-hop alias next.body) to classify a ComponentConfig \
                 alias by authored shape",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_registry.rs",
        impl_path: "",
        fn_name: "collect_component_meta_registry_public_field_refs",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body across the registry public-field surface to collect authored \
                 field refs",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_registry.rs",
        impl_path: "",
        fn_name: "collect_component_meta_registry_public_indexed_access_roots",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body to collect authored indexed-access roots",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/shallow_preserve.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "should_preserve_transitive_ref",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body to decide transitive-ref preservation by authored shape",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/shallow_preserve.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "fast_symbolic_imported_generic_route",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body on the fast symbolic imported-generic route (authored ref \
                 shape)",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/shallow_preserve.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "collapse_same_file_imported_alias_chain",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body when collapsing a same-file imported alias chain (authored \
                 alias shape)",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/shallow_preserve.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "try_fast_expand_shallow_alias_body",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body on the fast shallow-alias expand path (authored alias shape)",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/shallow_preserve.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "rewrite_fast_shallow_alias_body",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body on the fast shallow-alias rewrite path (authored alias shape)",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/registry_decl.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "owner_collection_expr",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body to return the RAW alias body the registry walker classifies by \
                 authored shape (Ref{type_arguments}); the value is keyed into the TypeExpr-keyed \
                 OwnerCollectionDb a handle-derived value must never populate — authored-shape, \
                 stays TypeExpr",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/registry_decl.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "named_decl_body",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "the named_decl_body DEFINITION — reads prepared.body and returns the cloned body \
                 TypeExpr the authored-shape classifiers (C5/C6) and the C7 locator consume",
    },
    ReaderRow {
        file: "src/meta_resolve/registry_materialize.rs",
        impl_path: "",
        fn_name: "nested_symbolic_member_route_should_stay_symbolic",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "calls named_decl_body and classifies the returned body by authored shape \
                 (Ref{type_arguments} non-empty, utility route, indexed-access route) — \
                 authored-syntax-intrinsic",
    },
    ReaderRow {
        file: "src/meta_resolve/materialize/field_types.rs",
        impl_path: "",
        fn_name: "type_expr_has_package_backed_object_like_root_with_fence",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "calls named_decl_body and extracts the authored root (literal Pick/Omit, \
                 IndexedAccess.object, Ref head) — authored-syntax-intrinsic",
    },
    ReaderRow {
        file: "src/host_manage/component_meta_methods.rs",
        impl_path: "impl VerterHost",
        fn_name: "owner_local_generic_alias_substituted_body_via_dispatch",
        class: ReaderClass::AuthoredShape,
        method_chain: false,
        required_hot_route: &[],
        reason: "the instantiation fast-lane gate reads prepared.body for the authored generic-alias \
                 substitution shape",
    },
    // ── GraphFreeDto — below the session graph ──────────────────────────
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "route_closure",
        class: ReaderClass::GraphFreeDto,
        method_chain: true,
        required_hot_route: &[],
        reason: "reads lowered.body.lookup_object() on the route-closure path — below the session \
                 graph, cannot carry a HotTypeRef",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "member_path_route_closure",
        class: ReaderClass::GraphFreeDto,
        method_chain: true,
        required_hot_route: &[],
        reason: "reads lowered.body.lookup_object() on the member-path route closure — below the \
                 session graph",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "member_route_closure",
        class: ReaderClass::GraphFreeDto,
        method_chain: true,
        required_hot_route: &[],
        reason: "reads lowered.body.lookup_object() on the member route closure — below the session \
                 graph",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "whole_route_closure",
        class: ReaderClass::GraphFreeDto,
        method_chain: true,
        required_hot_route: &[],
        reason: "reads lowered.body.lookup_object() on the whole route closure — below the session \
                 graph",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "follow_local_symbol_precise",
        class: ReaderClass::GraphFreeDto,
        method_chain: true,
        required_hot_route: &[],
        reason: "reads lowered.body.lookup_object() when following a local symbol precisely — below \
                 the session graph",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "follow_routed_expr",
        class: ReaderClass::GraphFreeDto,
        method_chain: true,
        required_hot_route: &[],
        reason: "reads lowered.body.lookup_object() when following a routed expression — below the \
                 session graph",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "extract_string_literal_keys_from_type_expr",
        class: ReaderClass::GraphFreeDto,
        method_chain: true,
        required_hot_route: &[],
        reason: "reads lowered.body.lookup_object() to extract string-literal keys — below the \
                 session graph",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "",
        fn_name: "collect_member_path_seed_names",
        class: ReaderClass::GraphFreeDto,
        method_chain: true,
        required_hot_route: &[],
        reason: "reads lowered.body.lookup_object() to collect member-path seed names (free fn) — \
                 below the session graph",
    },
    ReaderRow {
        file: "src/resolver_core/external_type_frontier.rs",
        impl_path: "impl ExternalTypeFrontier",
        fn_name: "resolve_through_export",
        class: ReaderClass::GraphFreeDto,
        method_chain: true,
        required_hot_route: &[],
        reason: "reads lowered.body.lookup_object() (two branches) when resolving through an export \
                 — the external-type frontier lives below the session graph",
    },
    ReaderRow {
        file: "src/resolver_core/external_type_frontier.rs",
        impl_path: "impl ExternalTypeFrontier",
        fn_name: "resolve_one",
        class: ReaderClass::GraphFreeDto,
        method_chain: false,
        required_hot_route: &[],
        reason: "re-clones a frontier-produced ResolvedSymbol.body (existing.body.clone(), populated \
                 FROM the lowered body) when rebuilding from an already-resolved chain entry — the \
                 frontier lives below the session graph. A BARE .body field read, anchored by the \
                 enumeration, not the tripwire",
    },
    ReaderRow {
        file: "src/host_manage/eval_env.rs",
        impl_path: "impl VerterHost",
        fn_name: "peel_value_decl_alias_graph_native",
        class: ReaderClass::GraphFreeDto,
        method_chain: false,
        required_hot_route: &[],
        reason: "the typeof peel reads lowered.type_annotation (LoweredValueDecl) — the eval-env \
                 value-decl peel lives below the session graph",
    },
    ReaderRow {
        file: "src/host_manage/eval_env.rs",
        impl_path: "impl VerterHost",
        fn_name: "dependency_value_symbol_graph_native",
        class: ReaderClass::GraphFreeDto,
        method_chain: false,
        required_hot_route: &[],
        reason: "the graph-native value-symbol reader reads lowered.type_annotation \
                 (effective_value_decl → LoweredValueDecl) — same below-graph carrier class as \
                 peel_value_decl_alias_graph_native. A BARE .type_annotation field read, anchored by \
                 the enumeration",
    },
    // ── GraphBackedPending — graph-backed, no graph-native arm for its shape ──
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/route_keys.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "enumerate_member_surface_keys_via_route",
        class: ReaderClass::GraphBackedPending,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.body via prepared_type_decl(..).map(|p| p.body.clone()) inside the \
                 try_body route-key expansion closure (and the prepared-value .type_annotation route \
                 read) because no graph-native member-surface-ROUTE key enumerator exists. The \
                 structural arm that would retire this reader is a graph-native SemanticNodeData \
                 member-surface-route key enumerator (an IndexedAccess/Conditional-distributing \
                 variant over union/intersection/conditional/object surfaces). BARE \
                 .body/.type_annotation field reads, anchored by the enumeration",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/helpers.rs",
        impl_path: "",
        fn_name: "resolve_imported_registry_symbol_with_budget",
        class: ReaderClass::GraphBackedPending,
        method_chain: false,
        required_hot_route: &[],
        reason: "builds the ResolvedImportedRegistrySymbol.body CARRIER from prepared.body (NOT the \
                 prepared_type_decl(..).is_some() existence check, which stays a cheap shallow \
                 presence check) because the imported-registry body carrier holds a TypeExpr body, \
                 not identity. The structural arm that would retire this reader is an \
                 imported-registry body carrier that holds identity / HotTypeRef + graph-native \
                 materialization instead of a TypeExpr body",
    },
    ReaderRow {
        file: "src/host_manage/component_meta_methods.rs",
        impl_path: "impl VerterHost",
        fn_name: "append_component_meta_registry_entries",
        class: ReaderClass::GraphBackedPending,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads the imported-registry symbol body (resolved.body, the \
                 ResolvedImportedRegistrySymbol.body carrier populated from prepared.body.clone()) \
                 across registry-publication routing and calls named_decl_body (×3) because that \
                 carrier holds a TypeExpr body. The structural arm that would retire this reader is \
                 the same imported-registry body carrier holding identity / HotTypeRef + \
                 graph-native materialization, through which this consumer then routes. All BARE \
                 .body field reads, anchored by the enumeration",
    },
    ReaderRow {
        file: "src/component_meta_resolution_policy/core.rs",
        impl_path: "impl PolicyCtx",
        fn_name: "locate_declaration",
        class: ReaderClass::GraphBackedPending,
        method_chain: false,
        required_hot_route: &[],
        reason: "calls named_decl_body and returns the located declaration body TypeExpr because the \
                 locator is a single TypeExpr-returning LOCATOR, too broad to hand back a handle. The \
                 structural arm that would retire this reader splits the locator into an \
                 identity/hot-locator (for semantic consumers) vs an authored-body-locator (for \
                 authored-shape policy code), by downstream need",
    },
    ReaderRow {
        file: "src/project_semantic_dispatch/build.rs",
        impl_path: "impl ProjectSemanticDispatch",
        fn_name: "build_typeof",
        class: ReaderClass::GraphBackedPending,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.type_annotation (PreparedValueDecl, from \
                 effective_prepared_value_decl) and feeds it to shallow_lower_type_expr_with_context, \
                 a graph-feeding value-decl annotation reader, because the prepared value decl holds \
                 a TypeExpr annotation. The structural arm that would retire this reader is a \
                 HotPreparedValueDecl annotation handle. A BARE .type_annotation field read, anchored \
                 by the enumeration",
    },
    ReaderRow {
        file: "src/resolver_core/runtime_values.rs",
        impl_path: "",
        fn_name: "prepared_value_decl_to_value_decl_info",
        class: ReaderClass::GraphBackedPending,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads prepared.type_annotation.clone() (PreparedValueDecl) when building the \
                 ValueDeclInfo round-trip into the importing env, the value-resolution surface, \
                 because the prepared value decl holds a TypeExpr annotation. The structural arm that \
                 would retire this reader is a HotPreparedValueDecl annotation handle. A BARE \
                 .type_annotation field read (free fn), anchored by the enumeration",
    },
    ReaderRow {
        file: "src/host_manage/eval_env.rs",
        impl_path: "impl VerterHost",
        fn_name: "component_meta_binding_type_entries",
        class: ReaderClass::GraphBackedPending,
        method_chain: false,
        required_hot_route: &[],
        reason: "reads decl.type_annotation.clone() (decl = prepared_value_decl(..), a \
                 PreparedValueDecl) to build the component-meta binding type entries (publication) \
                 because the prepared value decl holds a TypeExpr annotation. The structural arm that \
                 would retire this reader is a HotPreparedValueDecl annotation handle. A BARE \
                 .type_annotation field read, anchored by the enumeration",
    },
];

/// One anchored COMPAT body reader: a purpose-named compat helper DEFINITION, or
/// the CALL SITE fn that routes its body read through such a helper. All
/// [`ReaderClass::OutputCompat`].
struct CompatRow {
    file: &'static str,
    impl_path: &'static str,
    fn_name: &'static str,
    /// `true` iff this fn itself contains a `<recv>.body.<method>` read (the
    /// helper DEFS do; the consumer call-site fns do NOT — they call the helper).
    method_chain: bool,
    reason: &'static str,
}

/// The COMPAT body-read surface — the ONLY sanctioned output/compat body reads
/// ([`ReaderClass::OutputCompat`]). The three purpose-named compat helpers plus
/// the consumer fns that route through them. After the compat routing, the
/// body-fact emitter and the typeinfo oracle read a declaration body ONLY through
/// these helpers.
const COMPAT_BODY_READERS: &[CompatRow] = &[
    // ── The three compat helper DEFINITIONS ────────────────────────────
    CompatRow {
        file: "src/decl_body_memo.rs",
        impl_path: "impl DeclBodyMemo",
        fn_name: "compat_type_body_hash_input",
        method_chain: true,
        reason: "TYPE-space fingerprint hash input — type_decl(name)?.body.lookup_object()",
    },
    CompatRow {
        file: "src/fact_emission.rs",
        impl_path: "",
        fn_name: "compat_value_body_hash_input",
        method_chain: false,
        reason: "VALUE-space fingerprint hash input — wraps value_body_for_hash over a resolved \
                 LoweredValueDecl (reads type_annotation/signatures, NOT a .body.<method> chain)",
    },
    CompatRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "compat_type_contributors_for_typeinfo",
        method_chain: true,
        reason: "typeinfo-oracle contributor read — type_decl(name)?.body.contributors()",
    },
    // ── The consumer call-site fns that route through the helpers ───────
    CompatRow {
        file: "src/fact_emission.rs",
        impl_path: "impl LazyBodyFactSource",
        fn_name: "compute",
        method_chain: false,
        reason: "the body-fact compute — routes TYPE/VALUE body fingerprint reads through the two \
                 compat hash-input helpers (no direct .body.<method> read remains)",
    },
    CompatRow {
        file: "src/typeinfo/oracle_core/source_walk.rs",
        impl_path: "",
        fn_name: "walk",
        method_chain: false,
        reason: "the typeinfo oracle source walk — routes the Type-space contributor read through \
                 compat_type_contributors_for_typeinfo (no direct .body.<method> read remains)",
    },
];

/// The two body-fact / oracle CONSUMER files that must contain NO direct
/// `<recv>.body.<method>` lowered-body read after the compat routing — their
/// body reads go through the named compat helpers (which live in OTHER files for
/// the type/contributor helpers, or read `type_annotation` not a `.body` chain
/// for the value helper).
const COMPAT_CONSUMER_FILES: &[&str] = &[
    "src/fact_emission.rs",
    "src/typeinfo/oracle_core/source_walk.rs",
];

/// Every `(file, impl_path, fn)` anchor that is ALLOWED to contain a
/// `<recv>.body.<method>` read — the union of the residual rows flagged
/// `method_chain` and the COMPAT rows flagged `method_chain`. The tripwire
/// reddens on any production `.body.<method>` read whose anchor is NOT in this
/// set.
fn method_chain_allowed_anchors() -> HashSet<(String, String, String)> {
    let mut set = HashSet::new();
    for r in RESIDUAL_BODY_READERS {
        if r.method_chain {
            set.insert((
                r.file.to_string(),
                r.impl_path.to_string(),
                r.fn_name.to_string(),
            ));
        }
    }
    for r in COMPAT_BODY_READERS {
        if r.method_chain {
            set.insert((
                r.file.to_string(),
                r.impl_path.to_string(),
                r.fn_name.to_string(),
            ));
        }
    }
    set
}

/// Whether a NON-test, real-body definition exists at the EXACT anchor.
fn anchored_definition_present(inv: &[FnDef], file: &str, impl_path: &str, name: &str) -> bool {
    inv.iter().any(|d| {
        !d.cfg_test && d.has_body && d.file == file && d.impl_path == impl_path && d.name == name
    })
}

/// All NON-test, real-body definitions at the EXACT anchor.
fn anchored_defs<'a>(inv: &'a [FnDef], file: &str, impl_path: &str, name: &str) -> Vec<&'a FnDef> {
    inv.iter()
        .filter(|d| {
            !d.cfg_test
                && d.has_body
                && d.file == file
                && d.impl_path == impl_path
                && d.name == name
        })
        .collect()
}

/// Every `(file, impl_path, fn)` anchor across BOTH inventories.
fn all_inventory_anchors() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for r in RESIDUAL_BODY_READERS {
        out.push((
            r.file.to_string(),
            r.impl_path.to_string(),
            r.fn_name.to_string(),
        ));
    }
    for r in COMPAT_BODY_READERS {
        out.push((
            r.file.to_string(),
            r.impl_path.to_string(),
            r.fn_name.to_string(),
        ));
    }
    out
}

/// One [`ReaderClass::GraphBackedMigrated`] anchor: its `(file, impl_path, fn)`
/// identity plus the EXACT hot-route ident(s) THIS anchor requires (its
/// [`ReaderRow::required_hot_route`]). The audit judges routing against
/// `required_hot_route`, NOT a flat global list — so an anchor that must route
/// through `decl_body_hot_ref` is NOT satisfied by an unrelated graph-native arm.
struct MigratedAnchor {
    file: String,
    impl_path: String,
    fn_name: String,
    required_hot_route: &'static [&'static str],
}

/// Every [`ReaderClass::GraphBackedMigrated`] row as a [`MigratedAnchor`] — the
/// migrated anchors that must NOT read a declaration body as a `TypeExpr` semantic
/// input, each carrying its own required hot-route set.
fn graph_backed_migrated_anchors() -> Vec<MigratedAnchor> {
    RESIDUAL_BODY_READERS
        .iter()
        .filter(|r| r.class == ReaderClass::GraphBackedMigrated)
        .map(|r| MigratedAnchor {
            file: r.file.to_string(),
            impl_path: r.impl_path.to_string(),
            fn_name: r.fn_name.to_string(),
            required_hot_route: r.required_hot_route,
        })
        .collect()
}

// ════════════════════════════════════════════════════════════════════
// INVARIANT 1 — PRESENCE
// ════════════════════════════════════════════════════════════════════

/// Every enumerated residual body reader AND every COMPAT row (helper def +
/// consumer call site) is defined at its `(file, impl/mod, fn)` anchor in the
/// production tree. A renamed / moved / deleted reader reddens — the partition
/// depends on this enumerated surface being stable.
#[test]
fn every_enumerated_body_reader_is_present_at_its_anchor() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    let mut missing = Vec::new();

    for r in RESIDUAL_BODY_READERS {
        assert!(
            !r.reason.trim().is_empty(),
            "residual reader row `{} :: {} :: {}` ({}) must carry a non-empty reason",
            r.file,
            r.impl_path,
            r.fn_name,
            r.class.label()
        );
        if !anchored_definition_present(&inv, r.file, r.impl_path, r.fn_name) {
            missing.push(format!(
                "{} reader `{}` is MISSING at its anchor ({} :: {}) — the residual partition \
                 enumeration is stale (a same-named body elsewhere does NOT satisfy the anchor)",
                r.class.label(),
                r.fn_name,
                r.file,
                r.impl_path
            ));
        }
    }
    for r in COMPAT_BODY_READERS {
        assert!(
            !r.reason.trim().is_empty(),
            "compat reader row `{} :: {} :: {}` must carry a non-empty reason",
            r.file,
            r.impl_path,
            r.fn_name
        );
        if !anchored_definition_present(&inv, r.file, r.impl_path, r.fn_name) {
            missing.push(format!(
                "COMPAT reader `{}` is MISSING at its anchor ({} :: {}) — the compat helper / its \
                 routed call site must exist",
                r.fn_name, r.file, r.impl_path
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "residual body-reader inventory: an enumerated reader is absent at its anchor.\n{}",
        missing.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// INVARIANT 2 — UNIQUENESS (fail-closed)
// ════════════════════════════════════════════════════════════════════

/// Each anchored `(file, impl/mod, fn)` resolves to EXACTLY ONE non-test
/// definition. A second definition at the same anchor makes the anchor
/// ambiguous and reddens — the partition must be able to address each reader
/// uniquely.
#[test]
fn every_anchored_body_reader_is_unique() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    let mut violations = Vec::new();
    for (file, impl_path, name) in all_inventory_anchors() {
        let count = anchored_defs(&inv, &file, &impl_path, &name).len();
        if count != 1 {
            violations.push(format!(
                "anchor `{file} :: {impl_path} :: fn {name}` resolves to {count} non-test \
                 definitions — every anchor must resolve to EXACTLY ONE (qualify by impl/type or \
                 remove the duplicate; do NOT drop the check)"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "residual body-reader inventory: an anchored reader is not unique.\n{}",
        violations.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// INVARIANT 3 — BOUNDED CLASSIFICATION (the tripwire SUPPLEMENT)
// ════════════════════════════════════════════════════════════════════

/// One un-inventoried `<recv>.body.<method>` read: the file, enclosing impl/mod,
/// fn, and the method names found.
#[derive(Debug, Clone)]
struct UnclassifiedMethodChainRead {
    file: String,
    impl_path: String,
    fn_name: String,
    methods: Vec<String>,
}

/// Find every production `<recv>.body.<method>` read whose `(file, impl, fn)`
/// anchor is NOT in the method-chain allowlist (the residual + COMPAT rows
/// flagged `method_chain`). A `#[cfg(test)]`-gated fn is test code and excluded.
fn unclassified_method_chain_reads(
    inv: &[FnDef],
    allowed: &HashSet<(String, String, String)>,
) -> Vec<UnclassifiedMethodChainRead> {
    let mut out = Vec::new();
    for d in inv {
        if d.cfg_test || !d.has_body || !d.reads_lowered_body() {
            continue;
        }
        if !allowed.contains(&d.anchor()) {
            let mut methods: Vec<String> = d.body_reads.iter().cloned().collect();
            methods.sort();
            out.push(UnclassifiedMethodChainRead {
                file: d.file.clone(),
                impl_path: d.impl_path.clone(),
                fn_name: d.name.clone(),
                methods,
            });
        }
    }
    out
}

/// The tripwire SUPPLEMENT: every production `<recv>.body.<method>` lowered-body
/// read (incl. paren/ref-unwrapped and UFCS spellings) sits at an anchor in
/// EITHER inventory. A NEW or MOVED such read outside both reddens at once. This
/// is a bounded supplement to the curated enumeration, NOT the completeness rail.
#[test]
fn no_method_chain_body_read_outside_the_inventory() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    let allowed = method_chain_allowed_anchors();
    let mut violations = Vec::new();
    for hit in unclassified_method_chain_reads(&inv, &allowed) {
        violations.push(format!(
            "{} :: {} :: fn `{}` performs a `<recv>.body.{:?}` lowered-body read but its anchor is \
             NOT in the residual or COMPAT inventory — a new/moved lowered-body reader appeared. \
             Add it to RESIDUAL_BODY_READERS (classified into a ReaderClass) or COMPAT_BODY_READERS \
             (a sanctioned output/compat read), with method_chain: true.",
            hit.file, hit.impl_path, hit.fn_name, hit.methods
        ));
    }
    assert!(
        violations.is_empty(),
        "residual body-reader inventory: an un-inventoried `<recv>.body.<method>` read appeared — \
         the classification tripwire fired.\n{}",
        violations.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// INVARIANT 4 — COMPAT PURITY
// ════════════════════════════════════════════════════════════════════

/// The two body-fact / oracle CONSUMER files contain NO direct
/// `<recv>.body.<method>` lowered-body read in any non-test fn — their body
/// reads route through the named compat helpers. Any direct method-chain read in
/// these files means the compat routing regressed.
#[test]
fn compat_consumer_files_contain_no_direct_method_chain_body_read() {
    let all = production_src_files();
    let consumer_files: Vec<(String, String)> = all
        .into_iter()
        .filter(|(rel, _)| COMPAT_CONSUMER_FILES.contains(&rel.as_str()))
        .collect();
    assert_eq!(
        consumer_files.len(),
        COMPAT_CONSUMER_FILES.len(),
        "every compat consumer file must be present in the production tree — got {:?}",
        consumer_files.iter().map(|(r, _)| r).collect::<Vec<_>>()
    );
    let inv = build_fn_inventory(&consumer_files);
    let mut violations = Vec::new();
    for d in &inv {
        if d.cfg_test || !d.has_body || !d.reads_lowered_body() {
            continue;
        }
        let mut methods: Vec<String> = d.body_reads.iter().cloned().collect();
        methods.sort();
        violations.push(format!(
            "{} :: {} :: fn `{}` performs a direct `<recv>.body.{:?}` read — the body-fact / oracle \
             consumer files must route ALL body reads through the named compat helpers",
            d.file, d.impl_path, d.name, methods
        ));
    }
    assert!(
        violations.is_empty(),
        "residual body-reader inventory: a compat consumer file performs a direct lowered-body \
         method-chain read — the compat routing regressed.\n{}",
        violations.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// INVARIANT 5 — NO GraphBackedMigrated TypeExpr-body read
// ════════════════════════════════════════════════════════════════════

/// Every `GraphBackedMigrated` anchor's OWN fn body (a) performs NO forbidden
/// `TypeExpr` declaration-body read — no `<expr>.body` / `<expr>.type_annotation`
/// field access, no `prepared_type_decl(..)` / `named_decl_body(..)` call — AND
/// (b) routes through the hot accessor(s) in its row's `required_hot_route` set
/// (the per-row required route, not a flat global union). This is a `syn`
/// AST audit ([`MigratedBodyAuditor`]) scoped to the EXACT `(file, impl_path, fn)`
/// anchor — NOT a text/needle scan: it is alias/rename robust, sees neither
/// comments nor string literals, and proves the precise anchor identity (the right
/// fn in the right impl). A migrated reader that regressed to reading a body
/// `TypeExpr` (even through an aliased local binding) REDS here.
#[test]
fn graph_backed_migrated_anchors_perform_no_typeexpr_body_read() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    let by_rel: std::collections::HashMap<&str, &str> = files
        .iter()
        .map(|(r, s)| (r.as_str(), s.as_str()))
        .collect();

    let migrated = graph_backed_migrated_anchors();
    assert!(
        !migrated.is_empty(),
        "non-vacuity: there must be at least one GraphBackedMigrated anchor (the migrated \
         non-vacuous consumer) — got none"
    );

    let mut violations = Vec::new();
    for anchor in &migrated {
        let MigratedAnchor {
            file,
            impl_path,
            fn_name: name,
            required_hot_route,
        } = anchor;
        // The anchor must be present + unique (PRESENCE/UNIQUENESS cover this too,
        // but assert here so a missing migrated anchor reds THIS invariant).
        let defs = anchored_defs(&inv, file, impl_path, name);
        if defs.len() != 1 {
            violations.push(format!(
                "GraphBackedMigrated anchor `{file} :: {impl_path} :: fn {name}` resolves to {} \
                 non-test defs — must be exactly one",
                defs.len()
            ));
            continue;
        }
        let Some(src) = by_rel.get(file.as_str()) else {
            violations.push(format!(
                "GraphBackedMigrated anchor file `{file}` not found in tree"
            ));
            continue;
        };
        // The `syn` AST audit, scoped to the exact `(impl_path, fn)` anchor, judging
        // routing against THIS anchor's OWN required hot-route set.
        let verdicts = audit_migrated_anchor(file, src, impl_path, name, required_hot_route);
        let verdict = match verdicts.as_slice() {
            [v] => v,
            other => {
                violations.push(format!(
                    "GraphBackedMigrated anchor `{file} :: {impl_path} :: fn {name}` resolved to {} \
                     audited (non-test) bodies — must be exactly one",
                    other.len()
                ));
                continue;
            }
        };
        // (a) No forbidden TypeExpr-body read (AST-structural — field access OR
        //     locator call, incl. aliased-launder).
        if !verdict.forbidden_reads.is_empty() {
            violations.push(format!(
                "GraphBackedMigrated anchor `{file} :: {impl_path} :: fn {name}` STILL performs \
                 forbidden TypeExpr-body read(s) {:?} — a migrated reader must route through the \
                 hot accessor(s) in its row's `required_hot_route` set, not read a declaration \
                 body as TypeExpr",
                verdict.forbidden_reads
            ));
        }
        // (b) Routes through THIS anchor's OWN required hot-route ident(s).
        if !verdict.routes_through_hot_accessor {
            violations.push(format!(
                "GraphBackedMigrated anchor `{file} :: {impl_path} :: fn {name}` does NOT call its \
                 required hot-route accessor (expected one of {required_hot_route:?}) — the \
                 migration is not in place"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "residual body-reader inventory: a GraphBackedMigrated anchor still reads a declaration \
         body as TypeExpr (or is not routed through the hot accessor).\n{}",
        violations.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// DISCRIMINATING SELF-TESTS
// ════════════════════════════════════════════════════════════════════

/// The body-read SHAPE detector discriminates exactly the `<recv>.body.<method>`
/// chain: it fires on `prepared.body.lookup_object()` / `lowered.body
/// .contributors()` / `x.body.is_merged()`, but NOT on a `group.contributors()`
/// (path receiver, not `.body`), NOT on a bare `program.body` field (no method),
/// and NOT on a token only inside a comment / string literal.
///
/// The `body.lookup_object()` non-detection below (a LOCAL var named `body`, not
/// a `.body` field) is a DISCLOSED-LIMIT fixture: a method-chain read laundered
/// through a local binding is OUT of the tripwire's sound syntactic reach. It is
/// covered by the MANUALLY-CURATED ENUMERATION + the behavioural parity gate, NOT
/// by this tripwire — deliberately not chased (anti-arms-race).
#[test]
fn body_read_shape_detector_discriminates() {
    // FIRES on the three chain shapes.
    let positive = "impl H {\n    \
        fn r(&self, prepared: &P, lowered: &L, x: &X) {\n        \
            let _ = prepared.body.lookup_object();\n        \
            let _ = lowered.body.contributors();\n        \
            let _ = x.body.is_merged();\n    \
        }\n}\n";
    let inv = inventory_for(&[("synthetic.rs".to_string(), positive.to_string())]);
    let r = inv.iter().find(|d| d.name == "r").expect("fn r");
    assert!(
        r.body_reads.contains("lookup_object")
            && r.body_reads.contains("contributors")
            && r.body_reads.contains("is_merged"),
        "self-test: all three `<recv>.body.<method>` chains must be detected — got {:?}",
        r.body_reads
    );

    // Does NOT fire on a decl-GROUP `.contributors()`, a bare `program.body`
    // field, or the DISCLOSED-LIMIT local-binding launder.
    let negative = "impl H {\n    \
        fn r(&self, group: &G, program: &Prog) {\n        \
            let _ = group.contributors();\n        \
            let body = group.merged_body();\n        \
            let _ = body.lookup_object();\n        \
            let _ = &program.body;\n    \
        }\n}\n";
    let neg_inv = inventory_for(&[("synthetic.rs".to_string(), negative.to_string())]);
    let nr = neg_inv.iter().find(|d| d.name == "r").expect("fn r");
    assert!(
        nr.body_reads.is_empty(),
        "self-test: a `group.contributors()` (not `.body`), the DISCLOSED-LIMIT local \
         `body.lookup_object()` (path receiver), and a bare `program.body` field must NOT be \
         detected — got {:?}",
        nr.body_reads
    );

    // Does NOT fire on a token only inside a comment / string literal.
    let comment = "impl H {\n    \
        fn r(&self) {\n        \
            // prepared.body.lookup_object() must never appear here\n        \
            let _ = \"lowered.body.contributors()\";\n    \
        }\n}\n";
    let com_inv = inventory_for(&[("synthetic.rs".to_string(), comment.to_string())]);
    let cr = com_inv.iter().find(|d| d.name == "r").expect("fn r");
    assert!(
        cr.body_reads.is_empty(),
        "self-test: a `<recv>.body.<method>` mention only inside a comment/string must NOT be \
         detected — got {:?}",
        cr.body_reads
    );
}

/// The BOUNDED hardening: the detector unwraps paren/reference receivers and
/// recognizes the UFCS call form, and matches the FULL `TypeDeclBody` reader
/// method set (incl. `primary` / `merged_member_names`). Each planted spelling
/// MUST be detected; the controls (a UFCS-shaped call whose argument is NOT a
/// `.body` field, and a bare unqualified `lookup_object(x)` free-fn call) MUST
/// NOT.
#[test]
fn body_read_shape_detector_handles_paren_ref_and_ufcs() {
    // FIRES: paren receiver, ref receiver, ref+paren receiver, deref receiver,
    // and the two methods `primary` / `merged_member_names` on a plain `.body`.
    let positive = "impl H {\n    \
        fn r(&self, lowered: &L) {\n        \
            let _ = (lowered.body).lookup_object();\n        \
            let _ = (&lowered.body).contributors();\n        \
            let _ = (&mut lowered.body).is_merged();\n        \
            let _ = (*lowered.body).primary();\n        \
            let _ = lowered.body.merged_member_names();\n    \
        }\n}\n";
    let inv = inventory_for(&[("synthetic.rs".to_string(), positive.to_string())]);
    let r = inv.iter().find(|d| d.name == "r").expect("fn r");
    for m in [
        "lookup_object",
        "contributors",
        "is_merged",
        "primary",
        "merged_member_names",
    ] {
        assert!(
            r.body_reads.contains(m),
            "self-test (paren/ref hardening): `{m}` via a paren/ref/deref `.body` receiver (or the \
             new method on a plain `.body`) MUST be detected — got {:?}",
            r.body_reads
        );
    }

    // FIRES: the UFCS / fully-qualified call forms with a `.body` argument.
    let ufcs = "impl H {\n    \
        fn r(&self, lowered: &L) {\n        \
            let _ = TypeDeclBody::contributors(&lowered.body);\n        \
            let _ = <TypeDeclBody>::lookup_object(lowered.body);\n        \
            let _ = TypeDeclBody::primary((&lowered.body));\n    \
        }\n}\n";
    let uinv = inventory_for(&[("synthetic.rs".to_string(), ufcs.to_string())]);
    let ur = uinv.iter().find(|d| d.name == "r").expect("fn r");
    for m in ["contributors", "lookup_object", "primary"] {
        assert!(
            ur.body_reads.contains(m),
            "self-test (UFCS hardening): the UFCS `TypeDeclBody::{m}(&<recv>.body)` form MUST be \
             detected — got {:?}",
            ur.body_reads
        );
    }

    // CONTROLS — MUST NOT fire.
    let controls = "impl H {\n    \
        fn r(&self, lowered: &L) {\n        \
            let _ = TypeDeclBody::contributors(&lowered.other);\n        \
            let _ = lookup_object(&lowered.body);\n    \
        }\n}\n";
    let cinv = inventory_for(&[("synthetic.rs".to_string(), controls.to_string())]);
    let cr = cinv.iter().find(|d| d.name == "r").expect("fn r");
    assert!(
        cr.body_reads.is_empty(),
        "self-test (hardening controls): a UFCS call with a non-`.body` argument and a bare \
         unqualified `lookup_object(..)` free-fn call must NOT be detected — got {:?}",
        cr.body_reads
    );
}

/// Nested-fn attribution: a `<recv>.body.lookup_object()` read inside a NESTED
/// `fn` is attributed to the NESTED fn's anchor, NOT the enclosing fn.
#[test]
fn nested_fn_body_read_is_attributed_to_the_nested_fn() {
    let src = "impl H {\n    \
        fn outer(&self, lowered: &L) {\n        \
            fn inner(lowered: &L) { let _ = lowered.body.lookup_object(); }\n        \
            inner(lowered);\n    \
        }\n}\n";
    let inv = inventory_for(&[("synthetic.rs".to_string(), src.to_string())]);
    let outer = inv
        .iter()
        .find(|d| d.name == "outer")
        .expect("fn outer present");
    let inner = inv
        .iter()
        .find(|d| d.name == "inner")
        .expect("fn inner present");
    assert!(
        outer.body_reads.is_empty(),
        "self-test: the ENCLOSING fn `outer` must NOT own the nested fn's body read — got {:?}",
        outer.body_reads
    );
    assert!(
        inner.body_reads.contains("lookup_object"),
        "self-test: the NESTED fn `inner` must own its own `<recv>.body.lookup_object()` read — got \
         {:?}",
        inner.body_reads
    );
}

/// Tripwire RED→GREEN: a NEW un-inventoried fn performing a
/// `<recv>.body.lookup_object()` read IS flagged; the SAME read at an
/// inventoried anchor (`shallow_file_state.rs :: impl ShallowFileState ::
/// route_closure`) is NOT.
#[test]
fn tripwire_fires_on_new_unlisted_reader_not_on_inventoried_anchor() {
    let allowed = method_chain_allowed_anchors();

    // RED — a brand-new reader at a non-inventoried anchor.
    let new_reader = "impl Sneaky {\n    \
        fn new_lowered_body_reader(&self, lowered: &L) -> bool {\n        \
            lowered.body.is_merged()\n    \
        }\n}\n";
    let new_inv = inventory_for(&[(
        "src/resolver_core/some_new_module.rs".to_string(),
        new_reader.to_string(),
    )]);
    let red = unclassified_method_chain_reads(&new_inv, &allowed);
    assert!(
        red.iter().any(|h| h.fn_name == "new_lowered_body_reader"
            && h.methods.contains(&"is_merged".to_string())),
        "self-test (tripwire RED): a NEW un-inventoried `<recv>.body.is_merged()` reader MUST be \
         flagged — got {:?}",
        red.iter()
            .map(|h| (h.fn_name.as_str(), h.methods.clone()))
            .collect::<Vec<_>>()
    );

    // GREEN — the same read shape AT an inventoried anchor is accepted.
    let at_anchor = "impl ShallowFileState {\n    \
        fn route_closure(&self, lowered: &L) -> bool {\n        \
            let _ = lowered.body.lookup_object();\n        \
            true\n    \
        }\n}\n";
    let anchor_inv = inventory_for(&[(
        "src/resolver_core/shallow_file_state.rs".to_string(),
        at_anchor.to_string(),
    )]);
    assert!(
        unclassified_method_chain_reads(&anchor_inv, &allowed).is_empty(),
        "self-test (tripwire GREEN): a `<recv>.body.lookup_object()` read at the inventoried anchor \
         (shallow_file_state.rs :: impl ShallowFileState :: route_closure) must NOT be flagged"
    );
}

/// Audit a SYNTHETIC migrated anchor body: wrap `body_src` in a one-fn
/// `impl Synthetic { fn migrated(&self) { … } }` shell, parse it with `syn`, and
/// audit the `migrated` anchor through [`audit_migrated_anchor`] against
/// `required_idents` — the per-row required hot-route set under test. Asserts the
/// shell resolves to exactly one audited body and returns its verdict. Shared by
/// the discrimination self-tests so each can feed a synthetic body and a chosen
/// required-route set.
fn audit_synthetic_migrated_body(body_src: &str, required_idents: &[&str]) -> MigratedBodyVerdict {
    let src = format!("impl Synthetic {{\n    fn migrated(&self) {{ {body_src} }}\n}}\n");
    let mut v = audit_migrated_anchor(
        "src/synthetic.rs",
        &src,
        "impl Synthetic",
        "migrated",
        required_idents,
    );
    assert_eq!(
        v.len(),
        1,
        "self-test: the synthetic anchor must resolve to exactly one audited body"
    );
    v.pop().unwrap()
}

/// The required hot-route is judged PER-ROW, not against a flat global union: a
/// body that routes ONLY through the unrelated graph-native arm
/// `materialize_member_surface_node` is judged NOT-routed for the
/// `lower_decl_body_to_node` row, whose `required_hot_route` is `decl_body_hot_ref`
/// ONLY. The SAME body IS judged routed for a row whose required set is
/// `materialize_member_surface_node` — so the ident stays a valid required route
/// for a member-surface-route row; it just does not satisfy THIS anchor.
///
/// This discriminates the per-row mechanism from a flat-union audit: under a flat
/// union `&["decl_body_hot_ref", "materialize_member_surface_node"]`, a
/// `materialize_member_surface_node`-only body would be judged ROUTED for
/// `lower_decl_body_to_node` (a too-broad accept). With the per-row set this case
/// is NOT-routed, so the first assertion below FAILS against a flat-union audit and
/// PASSES against the per-row audit.
#[test]
fn migrated_route_requirement_is_per_row_not_a_flat_union() {
    // The body routes ONLY through the unrelated graph-native arm — it does NOT
    // call `decl_body_hot_ref`.
    const SURFACE_ONLY_BODY: &str =
        "let n = engine.materialize_member_surface_node(cid, key)?; n.node()";

    // The `lower_decl_body_to_node` row requires `decl_body_hot_ref` ONLY.
    let lower_decl_route = RESIDUAL_BODY_READERS
        .iter()
        .find(|r| {
            r.class == ReaderClass::GraphBackedMigrated && r.fn_name == "lower_decl_body_to_node"
        })
        .expect("the lower_decl_body_to_node migrated row is present")
        .required_hot_route;
    assert_eq!(
        lower_decl_route,
        &["decl_body_hot_ref"],
        "the lower_decl_body_to_node row must require `decl_body_hot_ref` ONLY"
    );

    // RED-on-flat-union / GREEN-on-per-row: judged against THIS row's required set,
    // a `materialize_member_surface_node`-only body does NOT satisfy the route. A
    // flat-union audit would mark it ROUTED and this assertion would FAIL — that is
    // the discriminating property.
    let surface_only = audit_synthetic_migrated_body(SURFACE_ONLY_BODY, lower_decl_route);
    assert!(
        !surface_only.routes_through_hot_accessor,
        "per-row discrimination: a body that routes ONLY through \
         `materialize_member_surface_node` must NOT satisfy the `lower_decl_body_to_node` row \
         (whose required route is `decl_body_hot_ref` only). A flat-union audit would have \
         wrongly accepted it."
    );

    // Positive control: the SAME body IS judged routed for a row whose required
    // set is `materialize_member_surface_node` — proving the ident is still a
    // VALID required route, just not for this anchor.
    let surface_route_row =
        audit_synthetic_migrated_body(SURFACE_ONLY_BODY, &["materialize_member_surface_node"]);
    assert!(
        surface_route_row.routes_through_hot_accessor,
        "the `materialize_member_surface_node` arm IS a valid required route for a \
         member-surface-route row — the SAME body is judged routed when the row requires it"
    );

    // And the converse: a `decl_body_hot_ref`-only body satisfies the
    // `lower_decl_body_to_node` requirement but NOT a member-surface-route row's.
    const HOT_REF_ONLY_BODY: &str =
        "let h = dispatch.decl_body_hot_ref(cid, name, args, prc)?; h.node()";
    let hot_for_lower = audit_synthetic_migrated_body(HOT_REF_ONLY_BODY, lower_decl_route);
    assert!(
        hot_for_lower.routes_through_hot_accessor,
        "a `decl_body_hot_ref`-only body satisfies the `lower_decl_body_to_node` required route"
    );
    let hot_for_surface =
        audit_synthetic_migrated_body(HOT_REF_ONLY_BODY, &["materialize_member_surface_node"]);
    assert!(
        !hot_for_surface.routes_through_hot_accessor,
        "a `decl_body_hot_ref`-only body does NOT satisfy a row whose required route is \
         `materialize_member_surface_node` — the per-row set is exact, not a union"
    );

    // Structural sanity: every row's required route idents are drawn from the
    // KNOWN universe (no row can require an unknown ident), and the
    // `lower_decl_body_to_node` row deliberately requires a STRICT SUBSET of it
    // (it omits `materialize_member_surface_node`).
    for r in RESIDUAL_BODY_READERS {
        for ident in r.required_hot_route {
            assert!(
                KNOWN_HOT_ROUTE_IDENTS.contains(ident),
                "row `{} :: {}` requires unknown hot-route ident `{ident}` — every required \
                 route must be a known graph-native arm",
                r.file,
                r.fn_name
            );
        }
    }
    assert!(
        KNOWN_HOT_ROUTE_IDENTS.contains(&"materialize_member_surface_node")
            && !lower_decl_route.contains(&"materialize_member_surface_node"),
        "`materialize_member_surface_node` is a KNOWN route but is deliberately EXCLUDED from the \
         `lower_decl_body_to_node` row's required set — the per-row requirement is narrower than \
         the known universe"
    );
}

/// The GraphBackedMigrated-no-read AST audit discriminates: fed SYNTHETIC source
/// through the `syn` parse + [`audit_migrated_anchor`], a clean migrated fn that
/// routes through `decl_body_hot_ref` and reads no body passes; a regressed fn
/// that reads `prepared.body`, that calls `named_decl_body` / `prepared_type_decl`,
/// OR that LAUNDERS the body read through an aliased local binding
/// (`let p = get_prepared(); p.body.clone()`) is caught. The aliased case is the
/// launder a text needle `"prepared.body"` cannot see — the AST field-access match
/// catches it. This exercises the AST visitor, NOT a text path.
#[test]
fn graph_backed_migrated_no_read_check_discriminates() {
    // Synthetic anchors share one impl/fn anchor shape so the scanner finds them.
    // The migrated anchor under test (`lower_decl_body_to_node`) requires
    // `decl_body_hot_ref`, so the synthetic audits use that required-route set.
    let audit = |body_src: &str| -> MigratedBodyVerdict {
        audit_synthetic_migrated_body(body_src, &["decl_body_hot_ref"])
    };

    // CLEAN — routes through the hot accessor, reads no body field, no locator call.
    let clean =
        audit("let handle = dispatch.decl_body_hot_ref(cid, name, args, prc)?; handle.node()");
    assert!(
        clean.routes_through_hot_accessor,
        "self-test (clean): a migrated body that calls decl_body_hot_ref routes through the hot \
         accessor"
    );
    assert!(
        clean.forbidden_reads.is_empty(),
        "self-test (clean): a clean migrated body performs NO forbidden TypeExpr-body read — got {:?}",
        clean.forbidden_reads
    );

    // REGRESSED — direct `prepared.body` field read + `prepared_type_decl` call.
    let regressed_prepared =
        audit("let prepared = dispatch.ctx.prepared_type_decl(cid, name)?; lower(&prepared.body)");
    assert!(
        !regressed_prepared.routes_through_hot_accessor,
        "self-test (regressed prepared): does NOT route through the hot accessor (the not-routed arm \
         reds it)"
    );
    assert!(
        regressed_prepared
            .forbidden_reads
            .iter()
            .any(|r| r.contains("body"))
            && regressed_prepared
                .forbidden_reads
                .iter()
                .any(|r| r.contains("prepared_type_decl")),
        "self-test (regressed prepared): the AST audit catches BOTH the `prepared.body` field read \
         and the `prepared_type_decl(..)` call — got {:?}",
        regressed_prepared.forbidden_reads
    );

    // REGRESSED — `named_decl_body` call.
    let regressed_named = audit("let body = engine.named_decl_body(cid, name)?; lower(body)");
    assert!(
        !regressed_named.routes_through_hot_accessor,
        "self-test (regressed named): does NOT route through the hot accessor"
    );
    assert!(
        regressed_named
            .forbidden_reads
            .iter()
            .any(|r| r.contains("named_decl_body")),
        "self-test (regressed named): the AST audit catches the `named_decl_body(..)` call — got {:?}",
        regressed_named.forbidden_reads
    );

    // ALIASED LAUNDER — the read goes through a renamed local binding, so the
    // literal text needle `"prepared.body"` would NOT fire (the binding is named
    // `p`). The AST audit catches the `p.body` field access structurally. THIS is
    // the load-bearing discrimination of this AST audit over a literal-text scan.
    let aliased_launder = audit("let p = get_prepared(cid, name); let _ = p.body.clone(); None");
    assert!(
        aliased_launder
            .forbidden_reads
            .iter()
            .any(|r| r.contains("body")),
        "self-test (ALIASED LAUNDER): the AST audit MUST catch `p.body` where `let p = \
         get_prepared(..)` — the aliased body read the literal text needle `prepared.body` cannot \
         see. Got {:?}",
        aliased_launder.forbidden_reads
    );
    // Sanity that this case really is a launder past the literal-text needle: the
    // literal text `prepared.body` is absent from the laundered body source, yet the
    // AST audit still flags it.
    assert!(
        !"let p = get_prepared(cid, name); let _ = p.body.clone(); None".contains("prepared.body"),
        "self-test (launder premise): the laundered source must NOT contain the literal \
         `prepared.body` text — otherwise it is not a real launder past the literal-text needle"
    );

    // ALIASED LAUNDER via `type_annotation` too (the value-decl body carrier).
    let aliased_annotation =
        audit("let d = effective_value_decl(cid); let _ = d.type_annotation.clone(); None");
    assert!(
        aliased_annotation
            .forbidden_reads
            .iter()
            .any(|r| r.contains("type_annotation")),
        "self-test (ALIASED LAUNDER, annotation): the AST audit MUST catch `d.type_annotation` \
         through an aliased binding — got {:?}",
        aliased_annotation.forbidden_reads
    );

    // (c) the real migrated anchor's OWN body, audited via the AST path, passes
    // BOTH checks — so the invariant is GREEN on the tree it ships against and not
    // vacuous.
    let files = production_src_files();
    let by_rel: std::collections::HashMap<&str, &str> = files
        .iter()
        .map(|(r, s)| (r.as_str(), s.as_str()))
        .collect();
    let mut checked_any = false;
    for anchor in graph_backed_migrated_anchors() {
        let MigratedAnchor {
            file,
            impl_path,
            fn_name: name,
            required_hot_route,
        } = anchor;
        let src = by_rel
            .get(file.as_str())
            .unwrap_or_else(|| panic!("migrated anchor file `{file}` present"));
        let verdicts = audit_migrated_anchor(&file, src, &impl_path, &name, required_hot_route);
        assert_eq!(
            verdicts.len(),
            1,
            "self-test (real tree): migrated anchor `{file} :: {impl_path} :: fn {name}` must \
             resolve to exactly one audited body"
        );
        let verdict = &verdicts[0];
        assert!(
            verdict.routes_through_hot_accessor,
            "self-test (real tree): migrated anchor `{file} :: fn {name}` routes through its \
             required hot-route accessor {required_hot_route:?}"
        );
        assert!(
            verdict.forbidden_reads.is_empty(),
            "self-test (real tree): migrated anchor `{file} :: fn {name}` performs NO forbidden \
             TypeExpr-body read — got {:?}",
            verdict.forbidden_reads
        );
        checked_any = true;
    }
    assert!(
        checked_any,
        "self-test: at least one real GraphBackedMigrated anchor must be checked (non-vacuity)"
    );
}

/// The MANUALLY-CURATED enumeration — NOT any automatic structural scan — is the
/// only rail for BARE-FIELD body readers (which the method-chain tripwire
/// structurally cannot see, and which the GraphBackedMigrated AST no-read rail does
/// not cover at a NEW anchor). This test PROVES the DISCLOSED LIMIT: a
/// synthetic bare `<recv>.body.clone()` reader at a NON-inventoried anchor produces
/// ZERO method-chain hits, so a brand-new bare-field reader is NOT auto-caught — it
/// is closed ONLY by the author keeping the enumeration complete + the behavioural
/// parity rail. A representative bare-field reader of each non-migrated class is
/// present + unique on the real tree (the enumeration rows are load-bearing).
#[test]
fn enumeration_is_the_completeness_rail_for_bare_field_readers() {
    let allowed = method_chain_allowed_anchors();

    // (1) A bare `.body.clone()` read is INVISIBLE to the method-chain tripwire.
    let bare_field_reader = "impl Sneaky {\n    \
        fn new_bare_field_reader(&self, p: &P) -> Option<TypeExpr> {\n        \
            self.prepared_type_decl(\"x\").map(|p| p.body.clone())\n    \
        }\n}\n";
    let bf_inv = inventory_for(&[(
        "src/resolver_core/some_new_module.rs".to_string(),
        bare_field_reader.to_string(),
    )]);
    let bf = bf_inv
        .iter()
        .find(|d| d.name == "new_bare_field_reader")
        .expect("fn present");
    assert!(
        bf.body_reads.is_empty(),
        "self-test: a bare `<recv>.body.clone()` read must NOT register as a method-chain read — \
         the tripwire is blind to it, so the enumeration is its only rail. Got {:?}",
        bf.body_reads
    );
    assert!(
        unclassified_method_chain_reads(&bf_inv, &allowed).is_empty(),
        "self-test: a bare-field reader at a non-inventoried anchor produces NO tripwire hit — \
         proving the tripwire cannot be the completeness rail for bare-field readers"
    );

    // (2) Representative bare-field readers of each non-migrated class are
    // enumerated AND present + unique on the real tree (the enumeration row is
    // load-bearing). One per class so each class's enumeration is exercised.
    let files = production_src_files();
    let real_inv = build_fn_inventory(&files);
    let class_witness: [(&str, &str, &str, ReaderClass); 4] = [
        (
            "src/resolver_core/component_meta_query_engine/route_keys.rs",
            "impl ComponentMetaQueryEngine",
            "enumerate_member_surface_keys_via_route",
            ReaderClass::GraphBackedPending,
        ),
        (
            "src/resolver_core/external_type_frontier.rs",
            "impl ExternalTypeFrontier",
            "resolve_one",
            ReaderClass::GraphFreeDto,
        ),
        (
            "src/meta_resolve/materialize/field_types.rs",
            "",
            "type_expr_has_package_backed_object_like_root_with_fence",
            ReaderClass::AuthoredShape,
        ),
        (
            "src/resolver_core/prepared_decl.rs",
            "",
            "prepare_local_value_decl",
            ReaderClass::ProducerLowering,
        ),
    ];
    for (file, impl_path, fn_name, class) in class_witness {
        assert!(
            RESIDUAL_BODY_READERS.iter().any(|r| r.file == file
                && r.impl_path == impl_path
                && r.fn_name == fn_name
                && r.class == class),
            "self-test: the bare-field reader `{file} :: {impl_path} :: {fn_name}` must be a \
             `{}` row in RESIDUAL_BODY_READERS",
            class.label()
        );
        assert_eq!(
            anchored_defs(&real_inv, file, impl_path, fn_name).len(),
            1,
            "self-test: the reader `{file} :: {impl_path} :: {fn_name}` must resolve to exactly one \
             non-test def on the real tree (the enumeration row is load-bearing)"
        );
    }

    // (3) PRESENCE discriminates: the exact route_keys anchor IS present; a
    // mutated variant is NOT.
    assert!(
        anchored_definition_present(
            &real_inv,
            "src/resolver_core/component_meta_query_engine/route_keys.rs",
            "impl ComponentMetaQueryEngine",
            "enumerate_member_surface_keys_via_route"
        ),
        "self-test: the route_keys bare-field reader IS present at its anchor"
    );
    assert!(
        !anchored_definition_present(
            &real_inv,
            "src/resolver_core/component_meta_query_engine/route_keys.rs",
            "impl ComponentMetaQueryEngine",
            "enumerate_member_surface_keys_via_route_MUTATED_zzz"
        ),
        "self-test (presence discrimination): a mutated route_keys anchor must NOT be present"
    );
}

/// Tripwire fail-closed on a MOVE: the SAME inventoried fn name moved to a
/// DIFFERENT impl is flagged — the anchor is `(file, impl, fn)`.
#[test]
fn tripwire_fires_on_moved_inventoried_reader() {
    let allowed = method_chain_allowed_anchors();
    let moved = "impl ShallowFileState {\n    \
        fn route_closure(&self) {}\n}\n\
        impl SomethingElse {\n    \
        fn route_closure(&self, lowered: &L) -> bool { lowered.body.is_merged() }\n}\n";
    let inv = inventory_for(&[(
        "src/resolver_core/shallow_file_state.rs".to_string(),
        moved.to_string(),
    )]);
    let hits = unclassified_method_chain_reads(&inv, &allowed);
    assert!(
        hits.iter()
            .any(|h| h.fn_name == "route_closure" && h.impl_path == "impl SomethingElse"),
        "self-test (tripwire move RED): `route_closure` moved to `impl SomethingElse` performing \
         the chain read MUST be flagged. Got {:?}",
        hits.iter()
            .map(|h| (h.impl_path.as_str(), h.fn_name.as_str()))
            .collect::<Vec<_>>()
    );
    assert!(
        !hits
            .iter()
            .any(|h| h.impl_path == "impl ShallowFileState" && h.fn_name == "route_closure"),
        "self-test (tripwire move discrimination): the anchored `impl ShallowFileState :: \
         route_closure` must NOT be flagged"
    );
}

/// Compat-purity RED→GREEN: a synthetic `fact_emission.rs` whose `compute` fn
/// performs a DIRECT `<recv>.body.lookup_object()` read IS flagged; a clean
/// version (routing through a helper call) is NOT.
#[test]
fn compat_purity_detector_discriminates() {
    let dirty = "impl LazyBodyFactSource {\n    \
        fn compute(&self, lowered: &L) {\n        \
            let _ = lowered.body.lookup_object();\n    \
        }\n}\n";
    let dirty_inv = inventory_for(&[("src/fact_emission.rs".to_string(), dirty.to_string())]);
    let dirty_hits: Vec<&FnDef> = dirty_inv
        .iter()
        .filter(|d| !d.cfg_test && d.has_body && d.reads_lowered_body())
        .collect();
    assert!(
        dirty_hits.iter().any(|d| d.name == "compute"),
        "self-test (compat purity RED): a direct `<recv>.body.lookup_object()` read inside \
         fact_emission's `compute` MUST be detected — got {:?}",
        dirty_hits
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>()
    );

    let clean = "impl LazyBodyFactSource {\n    \
        fn compute(&self, name: &str) {\n        \
            let _ = self.memo.compat_type_body_hash_input(name);\n    \
        }\n}\n";
    let clean_inv = inventory_for(&[("src/fact_emission.rs".to_string(), clean.to_string())]);
    let clean_hits: Vec<&FnDef> = clean_inv
        .iter()
        .filter(|d| !d.cfg_test && d.has_body && d.reads_lowered_body())
        .collect();
    assert!(
        clean_hits.is_empty(),
        "self-test (compat purity GREEN): routing through `compat_type_body_hash_input` (no direct \
         `<recv>.body.<method>` read) must NOT be flagged — got {:?}",
        clean_hits.iter().map(|d| d.name.as_str()).collect::<Vec<_>>()
    );
}

/// Presence discriminates: a deliberately-absent reader name is NOT present at
/// an anchor (non-vacuity of the presence check).
#[test]
fn presence_check_discriminates_absent_reader() {
    let src = "impl ShallowFileState {\n    fn route_closure(&self) {}\n}\n";
    let inv = inventory_for(&[(
        "src/resolver_core/shallow_file_state.rs".to_string(),
        src.to_string(),
    )]);
    assert!(
        anchored_definition_present(
            &inv,
            "src/resolver_core/shallow_file_state.rs",
            "impl ShallowFileState",
            "route_closure"
        ),
        "self-test: the present `route_closure` IS detected at its anchor"
    );
    assert!(
        !anchored_definition_present(
            &inv,
            "src/resolver_core/shallow_file_state.rs",
            "impl ShallowFileState",
            "this_reader_does_not_exist_zzz"
        ),
        "self-test (presence discrimination): a deliberately-absent reader name must NOT be present"
    );
}

/// Uniqueness discriminates: two non-test definitions at the SAME anchor count
/// as 2 (so the uniqueness guard reddens); a single definition counts as 1.
#[test]
fn uniqueness_check_discriminates_duplicate() {
    let dup = "impl ShallowFileState {\n    \
        fn route_closure(&self) { let _ = 1; }\n    \
        fn route_closure(&self) { let _ = 2; }\n}\n";
    let inv = inventory_for(&[(
        "src/resolver_core/shallow_file_state.rs".to_string(),
        dup.to_string(),
    )]);
    assert_eq!(
        anchored_defs(
            &inv,
            "src/resolver_core/shallow_file_state.rs",
            "impl ShallowFileState",
            "route_closure"
        )
        .len(),
        2,
        "self-test: two definitions at the same anchor must count as 2 so uniqueness reddens"
    );
    let single = "impl ShallowFileState {\n    fn route_closure(&self) {}\n}\n";
    let inv1 = inventory_for(&[(
        "src/resolver_core/shallow_file_state.rs".to_string(),
        single.to_string(),
    )]);
    assert_eq!(
        anchored_defs(
            &inv1,
            "src/resolver_core/shallow_file_state.rs",
            "impl ShallowFileState",
            "route_closure"
        )
        .len(),
        1,
        "self-test (uniqueness discrimination): a single definition counts as 1"
    );
}

/// A `#[cfg(test)]`-gated fn performing the chain read is TEST code and is NOT
/// flagged by the tripwire.
#[test]
fn cfg_test_chain_read_is_not_flagged() {
    let allowed = method_chain_allowed_anchors();
    let src = "#[cfg(test)]\nmod tests {\n    \
        fn t(lowered: &L) -> bool { lowered.body.is_merged() }\n}\n";
    let inv = inventory_for(&[(
        "src/resolver_core/some_new_module.rs".to_string(),
        src.to_string(),
    )]);
    let t = inv.iter().find(|d| d.name == "t").expect("fn t");
    assert!(
        t.cfg_test && t.body_reads.contains("is_merged"),
        "self-test: the cfg-test fn must be marked cfg_test AND have its chain read recorded \
         (proves the negative below is not vacuous) — got cfg_test={}, reads={:?}",
        t.cfg_test,
        t.body_reads
    );
    assert!(
        unclassified_method_chain_reads(&inv, &allowed)
            .iter()
            .all(|h| h.fn_name != "t"),
        "self-test: a `#[cfg(test)]`-gated chain read must NOT be flagged by the tripwire"
    );
}

/// One UNPLANTED CONTROL that stays GREEN: the REAL production tree passes ALL
/// FIVE invariants as-is (presence, uniqueness, the tripwire, compat purity, and
/// the GraphBackedMigrated-no-read rail).
#[test]
fn real_tree_satisfies_all_invariants() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);

    // (1) Presence — every enumerated reader is anchored.
    for r in RESIDUAL_BODY_READERS {
        assert!(
            anchored_definition_present(&inv, r.file, r.impl_path, r.fn_name),
            "control: residual reader `{}` ({} :: {}) [{}] must be present on the real tree",
            r.fn_name,
            r.file,
            r.impl_path,
            r.class.label()
        );
    }
    for r in COMPAT_BODY_READERS {
        assert!(
            anchored_definition_present(&inv, r.file, r.impl_path, r.fn_name),
            "control: COMPAT reader `{}` ({} :: {}) must be present on the real tree",
            r.fn_name,
            r.file,
            r.impl_path
        );
    }

    // (2) Uniqueness.
    for (file, impl_path, name) in all_inventory_anchors() {
        assert_eq!(
            anchored_defs(&inv, &file, &impl_path, &name).len(),
            1,
            "control: anchor `{file} :: {impl_path} :: {name}` must resolve to exactly one def"
        );
    }

    // (3) Tripwire.
    let allowed = method_chain_allowed_anchors();
    let stray = unclassified_method_chain_reads(&inv, &allowed);
    assert!(
        stray.is_empty(),
        "control: the real tree must have NO un-inventoried `<recv>.body.<method>` read — got {:?}",
        stray
            .iter()
            .map(|h| (h.file.as_str(), h.impl_path.as_str(), h.fn_name.as_str()))
            .collect::<Vec<_>>()
    );

    // (4) Compat purity.
    let consumer_inv: Vec<&FnDef> = inv
        .iter()
        .filter(|d| {
            COMPAT_CONSUMER_FILES.contains(&d.file.as_str())
                && !d.cfg_test
                && d.has_body
                && d.reads_lowered_body()
        })
        .collect();
    assert!(
        consumer_inv.is_empty(),
        "control: the compat consumer files must perform NO direct `<recv>.body.<method>` read — \
         got {:?}",
        consumer_inv
            .iter()
            .map(|d| (d.file.as_str(), d.name.as_str()))
            .collect::<Vec<_>>()
    );

    // (5) GraphBackedMigrated-no-read — every migrated anchor reads no body
    // TypeExpr and routes through its OWN required hot-route accessor (the `syn`
    // AST audit).
    let by_rel: std::collections::HashMap<&str, &str> = files
        .iter()
        .map(|(r, s)| (r.as_str(), s.as_str()))
        .collect();
    for anchor in graph_backed_migrated_anchors() {
        let MigratedAnchor {
            file,
            impl_path,
            fn_name: name,
            required_hot_route,
        } = anchor;
        let src = by_rel
            .get(file.as_str())
            .expect("migrated anchor file present");
        let verdicts =
            audit_migrated_anchor(file.as_str(), src, &impl_path, &name, required_hot_route);
        assert_eq!(
            verdicts.len(),
            1,
            "control: migrated anchor `{file} :: {impl_path} :: fn {name}` must resolve to exactly \
             one audited body"
        );
        let verdict = &verdicts[0];
        assert!(
            verdict.forbidden_reads.is_empty(),
            "control: migrated anchor `{file} :: fn {name}` must perform NO forbidden TypeExpr-body \
             read — got {:?}",
            verdict.forbidden_reads
        );
        assert!(
            verdict.routes_through_hot_accessor,
            "control: migrated anchor `{file} :: fn {name}` routes through its required hot-route \
             accessor {required_hot_route:?}"
        );
    }
}

/// Non-vacuity + mechanically-pinned counts: parsing the real tree yields a
/// large fn inventory that records at least one cfg-test fn AND at least one real
/// `<recv>.body.<method>` read; the method-chain allowlist is exactly the
/// inventoried `method_chain` rows; the TOTAL inventory sizes are pinned; and the
/// PER-CLASS partition counts are pinned so a reader silently changing class (or
/// the partition drifting) reddens.
#[test]
fn real_tree_inventory_is_non_vacuous() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    assert!(
        inv.len() > 100,
        "self-test: the structural inventory must contain many fn definitions — got {}",
        inv.len()
    );

    // Total residual + compat surface sizes — pinned so the curated surface
    // cannot drift silently. The residual surface is 40 readers, partitioned by
    // ReaderClass; the migrated anchor `lower_decl_body_to_node` is the
    // GraphBackedMigrated row. The COMPAT surface is 5 rows.
    assert_eq!(
        RESIDUAL_BODY_READERS.len(),
        40,
        "self-test (count pin): RESIDUAL_BODY_READERS must have exactly 40 rows"
    );
    assert_eq!(
        COMPAT_BODY_READERS.len(),
        5,
        "self-test (count pin): COMPAT_BODY_READERS must have exactly 5 rows"
    );

    // Per-class partition pins. 1 GraphBackedMigrated + 3 ProducerLowering + 17
    // AuthoredShape + 12 GraphFreeDto + 7 GraphBackedPending = 40.
    let class_count = |c: ReaderClass| {
        RESIDUAL_BODY_READERS
            .iter()
            .filter(|r| r.class == c)
            .count()
    };
    assert_eq!(
        class_count(ReaderClass::GraphBackedMigrated),
        1,
        "self-test (partition pin): exactly one GraphBackedMigrated row (the migrated non-vacuous \
         anchor)"
    );
    assert_eq!(
        class_count(ReaderClass::ProducerLowering),
        3,
        "self-test (partition pin): exactly three ProducerLowering rows (the mint + the two clone \
         paths)"
    );
    assert_eq!(
        class_count(ReaderClass::AuthoredShape),
        17,
        "self-test (partition pin): exactly 17 AuthoredShape rows"
    );
    assert_eq!(
        class_count(ReaderClass::GraphFreeDto),
        12,
        "self-test (partition pin): exactly 12 GraphFreeDto rows"
    );
    assert_eq!(
        class_count(ReaderClass::GraphBackedPending),
        7,
        "self-test (partition pin): exactly seven GraphBackedPending rows. GraphBackedPending is a \
         non-growth bounded set (bound 0 is the empty set once every structural arm lands), NOT a \
         settled stay-class — the non-growth bound is \
         `graph_backed_pending_is_a_non_growth_bounded_class`; this exact pin coexists with the cap \
         and is LOWERED (toward 0) as each row's structural arm lands"
    );
    assert_eq!(
        class_count(ReaderClass::OutputCompat),
        0,
        "self-test (partition pin): OutputCompat rows live in COMPAT_BODY_READERS, not the residual \
         table"
    );
    assert_eq!(
        class_count(ReaderClass::GraphBackedMigrated)
            + class_count(ReaderClass::ProducerLowering)
            + class_count(ReaderClass::AuthoredShape)
            + class_count(ReaderClass::GraphFreeDto)
            + class_count(ReaderClass::GraphBackedPending),
        RESIDUAL_BODY_READERS.len(),
        "self-test: the per-class partition must cover every residual row exactly once"
    );

    assert!(
        inv.iter().any(|d| d.cfg_test),
        "self-test: at least one real `#[cfg(test)]` fn must be flagged (proves the gate is not \
         always-false)"
    );
    assert!(
        inv.iter()
            .filter(|d| !d.cfg_test && d.reads_lowered_body())
            .count()
            >= 10,
        "self-test: the real tree must contain many production `<recv>.body.<method>` reads"
    );

    // The method-chain allowlist is exactly the inventoried method_chain rows.
    let allowed = method_chain_allowed_anchors();
    let residual_chain = RESIDUAL_BODY_READERS
        .iter()
        .filter(|r| r.method_chain)
        .count();
    let compat_chain = COMPAT_BODY_READERS
        .iter()
        .filter(|r| r.method_chain)
        .count();
    assert_eq!(
        allowed.len(),
        residual_chain + compat_chain,
        "self-test: the method-chain allowlist size must equal the inventoried method_chain rows"
    );
    assert_eq!(
        compat_chain, 2,
        "self-test: exactly two COMPAT helpers perform the `<recv>.body.<method>` chain read"
    );
    assert_eq!(
        residual_chain, 10,
        "self-test: exactly ten residual readers perform the `<recv>.body.<method>` chain read"
    );
}

// ════════════════════════════════════════════════════════════════════
// GraphBackedPending — NON-GROWTH BOUNDED CLASS
// ════════════════════════════════════════════════════════════════════

/// The non-growth CAP on the `GraphBackedPending` class — its CURRENT count. The
/// class is a non-growth bounded set: it may only SHRINK as each row's named
/// structural arm lands. The cap REDDENS on growth (a new pending row), and is
/// LOWERED toward [`GRAPH_BACKED_PENDING_TARGET`] as a row leaves. It is NOT a
/// settled final allowlist size.
const GRAPH_BACKED_PENDING_CAP: usize = 7;

/// The bound of the empty `GraphBackedPending` set: ZERO. Each row leaves the
/// class the moment its structural arm lands; when the count reaches 0 the class
/// (and this guard) is deleted.
const GRAPH_BACKED_PENDING_TARGET: usize = 0;

/// Count the [`ReaderClass::GraphBackedPending`] rows in an arbitrary row slice —
/// the reusable cap predicate the production non-growth guard and its
/// discriminating self-test both call (so the self-test can feed a synthetic
/// over-cap slice and prove the cap REDDENS).
fn count_pending_in(rows: &[ReaderRow]) -> usize {
    rows.iter()
        .filter(|r| r.class == ReaderClass::GraphBackedPending)
        .count()
}

/// NON-GROWTH BOUNDED CLASS: the `GraphBackedPending` class is a non-growth
/// bounded set (its empty bound is 0), not a settled reader class — it must never
/// GROW past its current cap. A NEW pending row (a graph-backed reader added
/// instead of routed through a structural arm) pushes the count over
/// [`GRAPH_BACKED_PENDING_CAP`] and REDDENS this guard; when a pending row's named
/// structural arm lands and it leaves the class, the cap is LOWERED toward 0. This
/// is the non-growth rail; the exact `== 7` pin in
/// `real_tree_inventory_is_non_vacuous` coexists with it.
#[test]
fn graph_backed_pending_is_a_non_growth_bounded_class() {
    let pending = count_pending_in(RESIDUAL_BODY_READERS);
    assert!(
        pending <= GRAPH_BACKED_PENDING_CAP,
        "GraphBackedPending is a non-growth bounded set with an empty bound of \
         {GRAPH_BACKED_PENDING_TARGET} — it must never GROW. The class has {pending} rows but the \
         non-growth cap is {GRAPH_BACKED_PENDING_CAP}: a new graph-backed reader was added (to \
         GraphBackedPending) instead of routed through a structural arm to decl_body_hot_ref / \
         graph-native dispatch. Route it through its named structural arm, or — if an addition is \
         genuinely unavoidable — a MANUAL cap raise is a recorded architecture decision, not a \
         silent edit. The set only shrinks toward {GRAPH_BACKED_PENDING_TARGET}; it does not grow."
    );
    // Belt-and-braces: the cap itself must not have been quietly raised above the
    // landed count (the bound tracks the exact count, lowered as rows leave —
    // never padded with headroom that would silently absorb a new pending row).
    assert_eq!(
        GRAPH_BACKED_PENDING_CAP, pending,
        "the GraphBackedPending non-growth cap ({GRAPH_BACKED_PENDING_CAP}) must equal the landed \
         pending count ({pending}) — the bound carries NO growth headroom; lower it as rows leave, \
         never pad it"
    );
    assert_eq!(
        GRAPH_BACKED_PENDING_TARGET, 0,
        "the GraphBackedPending empty bound is 0 (the class empties as every structural arm lands)"
    );
}

/// DISCRIMINATING self-test for the non-growth cap: the cap predicate REDDENS on a
/// synthetic inventory carrying an 8th `GraphBackedPending` row (growth past the
/// cap of 7) and is GREEN at exactly 7. Proves the non-growth bound fires on a NEW
/// pending row rather than silently absorbing it.
#[test]
fn graph_backed_pending_cap_reddens_on_growth() {
    // A synthetic pending row template — `'static` literals so it fits `ReaderRow`.
    const fn synthetic_pending(fn_name: &'static str) -> ReaderRow {
        ReaderRow {
            file: "src/synthetic/over_cap_module.rs",
            impl_path: "impl Synthetic",
            fn_name,
            class: ReaderClass::GraphBackedPending,
            method_chain: false,
            required_hot_route: &[],
            reason: "synthetic GraphBackedPending row — a graph-backed reader whose shape has no \
                     graph-native arm; exists ONLY to exercise the non-growth cap",
        }
    }

    // GREEN at exactly the cap (7 synthetic pending rows).
    let at_cap: [ReaderRow; 7] = [
        synthetic_pending("p0"),
        synthetic_pending("p1"),
        synthetic_pending("p2"),
        synthetic_pending("p3"),
        synthetic_pending("p4"),
        synthetic_pending("p5"),
        synthetic_pending("p6"),
    ];
    assert_eq!(
        count_pending_in(&at_cap),
        GRAPH_BACKED_PENDING_CAP,
        "self-test: a synthetic 7-row pending inventory sits exactly AT the cap"
    );
    assert!(
        count_pending_in(&at_cap) <= GRAPH_BACKED_PENDING_CAP,
        "self-test (cap GREEN): the cap predicate PASSES at exactly {GRAPH_BACKED_PENDING_CAP} \
         pending rows"
    );

    // RED at cap + 1 (a synthetic 8th pending row — a NEW pending reader).
    let over_cap: [ReaderRow; 8] = [
        synthetic_pending("p0"),
        synthetic_pending("p1"),
        synthetic_pending("p2"),
        synthetic_pending("p3"),
        synthetic_pending("p4"),
        synthetic_pending("p5"),
        synthetic_pending("p6"),
        // The planted 8th row — growth the non-growth bound must reject.
        synthetic_pending("p7_new_pending_reader"),
    ];
    assert_eq!(
        count_pending_in(&over_cap),
        GRAPH_BACKED_PENDING_CAP + 1,
        "self-test: a synthetic 8-row pending inventory grows the class by one"
    );
    assert!(
        count_pending_in(&over_cap) > GRAPH_BACKED_PENDING_CAP,
        "self-test (cap RED): the cap predicate FAILS the moment an 8th pending row is added — the \
         non-growth bound reddens on a NEW graph-backed pending reader rather than absorbing it. \
         (Rows of OTHER classes do NOT count: only GraphBackedPending growth trips the bound.)"
    );

    // Discrimination: a synthetic row of a DIFFERENT class does NOT count toward
    // the pending cap (the bound is scoped to GraphBackedPending only).
    let mixed: [ReaderRow; 8] = [
        synthetic_pending("p0"),
        synthetic_pending("p1"),
        synthetic_pending("p2"),
        synthetic_pending("p3"),
        synthetic_pending("p4"),
        synthetic_pending("p5"),
        synthetic_pending("p6"),
        ReaderRow {
            file: "src/synthetic/over_cap_module.rs",
            impl_path: "impl Synthetic",
            fn_name: "authored_shape_not_pending",
            class: ReaderClass::AuthoredShape,
            method_chain: false,
            required_hot_route: &[],
            reason:
                "synthetic AuthoredShape row — must NOT count toward the GraphBackedPending cap",
        },
    ];
    assert_eq!(
        count_pending_in(&mixed),
        GRAPH_BACKED_PENDING_CAP,
        "self-test (cap scope): an 8th row of a NON-pending class does NOT grow the pending count — \
         the bound counts ONLY GraphBackedPending rows"
    );
    assert!(
        count_pending_in(&mixed) <= GRAPH_BACKED_PENDING_CAP,
        "self-test (cap scope GREEN): a non-pending 8th row keeps the cap predicate GREEN"
    );
}

// ────────────────────────────────────────────────────────────────────
// impl-path renderer self-tests (the shared `syn` anchor machinery)
// ────────────────────────────────────────────────────────────────────

/// The `impl`-path renderer keeps module quals + type generics, normalizes only
/// lifetimes.
#[test]
fn impl_path_renderer_normalizes_only_lifetimes() {
    let lifetimed: ItemImpl = syn::parse_quote!(
        impl<'a> ComponentMetaQueryEngine<'a> {}
    );
    assert_eq!(
        render_impl_path(&lifetimed),
        "impl ComponentMetaQueryEngine",
        "self-test: a lifetime-only generic impl renders without the lifetime"
    );
    let elided: ItemImpl = syn::parse_quote!(impl KeyDomainBinding<'_> {});
    assert_eq!(
        render_impl_path(&elided),
        "impl KeyDomainBinding",
        "self-test: `<'_>` is normalized away"
    );
    let inherent: ItemImpl = syn::parse_quote!(impl VerterHost {});
    assert_eq!(render_impl_path(&inherent), "impl VerterHost");
    let u8_impl: ItemImpl = syn::parse_quote!(impl T for Foo<u8> {});
    let u16_impl: ItemImpl = syn::parse_quote!(impl T for Foo<u16> {});
    assert_ne!(
        render_impl_path(&u8_impl),
        render_impl_path(&u16_impl),
        "self-test: `Foo<u8>` and `Foo<u16>` must render differently (type generics kept)"
    );
}

/// The cfg-test evaluator truth table.
#[test]
fn cfg_test_evaluator_truth_table() {
    let plain: Attribute = syn::parse_quote!(#[cfg(test)]);
    let all_unix_test: Attribute = syn::parse_quote!(#[cfg(all(unix, test))]);
    let any_unix_test: Attribute = syn::parse_quote!(#[cfg(any(unix, test))]);
    let not_test: Attribute = syn::parse_quote!(#[cfg(not(test))]);
    let feature_test_util: Attribute = syn::parse_quote!(#[cfg(feature = "test_util")]);
    assert!(attrs_are_cfg_test(std::slice::from_ref(&plain)));
    assert!(attrs_are_cfg_test(std::slice::from_ref(&all_unix_test)));
    assert!(
        !attrs_are_cfg_test(std::slice::from_ref(&any_unix_test)),
        "self-test: `cfg(any(unix, test))` is PRODUCTION (compiles without test)"
    );
    assert!(
        !attrs_are_cfg_test(std::slice::from_ref(&not_test)),
        "self-test: `cfg(not(test))` is PRODUCTION"
    );
    assert!(
        !attrs_are_cfg_test(std::slice::from_ref(&feature_test_util)),
        "self-test: `cfg(feature = \"test_util\")` is PRODUCTION (test_util != test)"
    );
}
