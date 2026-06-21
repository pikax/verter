//! Frozen declaration-BODY-reader inventory guard — a temporary
//! `syn`-structural inventory that anchors every SEMANTIC declaration-body
//! reader (the frozen migration surface for the upcoming body-storage flip)
//! and the small COMPAT/output body-read set, so the two stay cleanly
//! separated and a NEW lowered-body reader cannot silently appear.
//!
//! ## Why this guard exists (and that it is TEMPORARY)
//!
//! Type-declaration BODY storage is migrating from the lower-crate typed IR
//! (`TypeExpr`) to a `verter_session` arena handle. The migration flips the
//! body PRODUCER and converts every SEMANTIC body reader in ONE pass against a
//! FROZEN, closed list of reader sites; the small set of OUTPUT/COMPAT body
//! reads (a fingerprint hash input; a typeinfo/hover oracle contributor read)
//! are routed through narrow, purpose-named compat helpers BEFORE the flip and
//! are deliberately NOT in the frozen semantic set.
//!
//! This guard pins, structurally:
//!   1. PRESENCE — every enumerated SEMANTIC body reader is defined at its
//!      `(file, impl/mod, fn)` anchor in the production tree (so the migration
//!      pass operates on a stable, enumerated surface).
//!   2. UNIQUENESS — each anchor resolves to exactly one non-test definition
//!      (a moved / duplicated / disappeared anchor reddens).
//!   3. BOUNDED CLASSIFICATION (the load-bearing tripwire) — every
//!      `<recv>.body.<TypeDeclBody-method>` read in the production tree sits at
//!      a `(file, impl/mod, fn)` anchor that is in EITHER the SEMANTIC inventory
//!      OR the COMPAT inventory. A NEW or MOVED such read outside both reddens
//!      immediately, so a new lowered-body reader cannot widen the migration
//!      surface unnoticed.
//!   4. COMPAT PURITY — the two body-fact / oracle CONSUMER files
//!      (`fact_emission.rs`, the oracle `source_walk.rs`) contain NO direct
//!      lowered-body method-chain read after the compat routing: their only
//!      body reads go through the named compat helpers.
//!
//! When the migration pass lands, the semantic readers it touches are migrated
//! and this guard's enumeration no longer describes the tree — at that point the
//! guard is absorbed/deleted with the readers. It is intentionally TEMPORARY.
//!
//! ## Identity is structural (`syn`), not text — and this is an inventory
//! guard, NOT a spelling scanner
//!
//! Every identity-sensitive question — which `fn` a token sits in (including a
//! token in a NESTED fn / closure), the `impl`/`trait`/`mod` path a `fn` belongs
//! to, whether a `fn` is `#[cfg(test)]`-gated — is answered by parsing each
//! production `src/**` file with [`syn::parse_file`] and walking its item tree
//! (a [`syn::visit::Visit`] impl), exactly as the sibling
//! `whole_env_consumer_graph_native_inventory.rs` inventory guard does. Tokens
//! in comments / string literals are invisible to the AST walk and cannot trip
//! (or satisfy) the guard.
//!
//! The load-bearing classification net (invariant 3) keys on an UNAMBIGUOUS
//! STRUCTURAL SHAPE, not on a denylist of identifier spellings and not on a
//! receiver BINDING NAME (which would fail open on rename): a
//! `<receiver>.body.<method>` chain whose method is one of `lookup_object` /
//! `contributors` / `is_merged`. Those three methods are defined ONLY on
//! `TypeDeclBody` (the type carried by `LoweredTypeDecl.body` /
//! `PreparedTypeDecl.body`), and the chain requires the read to go through a
//! `.body` field — so the shape is a precise, type-faithful proxy for "a lowered
//! type-declaration body read" that needs no type resolution. A bare `group
//! .contributors()` (on a decl GROUP, not a `.body`) or a `program.body` (an
//! unrelated `.body` field with no such method) does NOT match the shape.
//!
//! ## Honest scope (what this guard does and does NOT prove)
//!
//! Like the sibling inventory guard, this is a presence/uniqueness inventory of
//! an ENUMERATED set PLUS a SOUND tripwire on a precise structural shape — it is
//! NOT an exhaustive proof that the enumerated semantic set is complete. The
//! tripwire keys on the `<recv>.body.<method>` chain; it does NOT (and cannot
//! soundly, without type resolution) classify a bare `<recv>.body` FIELD read
//! whose receiver happens to be a lowered/prepared carrier — such reads share a
//! shape with unrelated `.body` fields (`program.body`, a request body, …). The
//! ENUMERATION (the SEMANTIC inventory below) is the completeness statement for
//! those bare-field readers; the migration pass carries its own frozen-file gate
//! and behavioural parity rail on top. What this guard adds is: every enumerated
//! reader is anchored and unique, every `<recv>.body.<method>` read is pinned to
//! a reviewed anchor, and a new such read outside the inventory reddens at once.

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

/// The `TypeDeclBody` method names whose invocation ON A `.body` field-access
/// receiver constitutes a lowered/prepared type-declaration BODY read. These
/// three methods are defined ONLY on `TypeDeclBody` (the type carried by
/// `LoweredTypeDecl.body` / `PreparedTypeDecl.body`); requiring the receiver to
/// be a `.body` field makes the shape a precise, type-faithful proxy with no
/// type resolution and no binding-name heuristic. A `group.contributors()` (on a
/// decl GROUP, receiver is not `.body`) does NOT match.
const BODY_READ_METHODS: &[&str] = &["lookup_object", "contributors", "is_merged"];

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
/// Detection (pure AST shape, no text, no binding name): a method call whose
/// method ident is in [`BODY_READ_METHODS`] AND whose receiver expression is a
/// NAMED field access `<expr>.body`. Tokens in comments / string literals are
/// invisible to the AST and cannot trip this.
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

/// Whether `expr` is a NAMED field access `<something>.body` (the receiver shape
/// that makes a `TypeDeclBody` method call a lowered-body read).
fn is_dot_body_field(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::Field(syn::ExprField {
            member: syn::Member::Named(name),
            ..
        }) if *name == "body"
    )
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
// ANCHOR TABLES — the frozen SEMANTIC surface + the COMPAT surface
// ════════════════════════════════════════════════════════════════════

/// One anchored declaration-BODY reader: the production file (crate-relative,
/// forward-slashed), the enclosing impl/trait/mod path (`render_impl_path`
/// form; `""` for a free file-scope fn), the fn name, whether the fn performs a
/// `<recv>.body.<method>` method-chain read (so the tripwire's per-anchor view
/// can be cross-checked against PRESENCE), and a required rationale.
struct ReaderRow {
    file: &'static str,
    impl_path: &'static str,
    fn_name: &'static str,
    /// `true` iff this reader contains at least one `<recv>.body.<method>`
    /// (`lookup_object` / `contributors` / `is_merged`) read — the load-bearing
    /// tripwire shape. `false` for a reader that only reads `<recv>.body` as a
    /// bare FIELD (or `type_annotation` / `merged_contributors`), which the
    /// tripwire does not classify (honest scope) but PRESENCE still anchors.
    method_chain: bool,
    reason: &'static str,
}

/// The frozen SEMANTIC body-reader surface — every site the body-storage
/// migration will touch / migrate / delete. These read a stored
/// `PreparedTypeDecl.body` / `LoweredTypeDecl.body` / `LoweredValueDecl
/// .type_annotation` (or its transitive `named_decl_body` result) for a
/// SEMANTIC decision (resolution, routing, closure, heritage, projection,
/// member lookup, cache identity, cross-file facts) — NOT for an output
/// fingerprint or the typeinfo oracle. `fact_emission.rs` and the oracle
/// `source_walk.rs` are EXCLUDED (they are COMPAT after the compat routing).
const SEMANTIC_BODY_READERS: &[ReaderRow] = &[
    // ── PreparedTypeDecl.body readers ──────────────────────────────────
    ReaderRow {
        file: "src/project_semantic_dispatch/build.rs",
        impl_path: "impl ProjectSemanticDispatch",
        fn_name: "class_heritage_bases",
        method_chain: false,
        reason: "reads prepared.body to derive a class declaration's heritage base surface",
    },
    ReaderRow {
        file: "src/project_semantic_dispatch/build.rs",
        impl_path: "impl ProjectSemanticDispatch",
        fn_name: "lower_decl_body_with_provenance",
        method_chain: false,
        reason: "the body lowering site (reads prepared.body + the merged_contributors gate) — \
                 DO-NOT-TOUCH; the migration owns its flip",
    },
    ReaderRow {
        file: "src/project_semantic_dispatch/raise.rs",
        impl_path: "",
        fn_name: "userland_instantiation_body_is_closed_object",
        method_chain: false,
        reason: "reads prepared.body to decide whether a userland instantiation body is a closed \
                 object",
    },
    ReaderRow {
        // Free file-scope fn — the `impl KeyDomainBinding<'_>` block at raise.rs
        // closes before this definition (verified via `syn`, not line proximity).
        file: "src/project_semantic_dispatch/raise.rs",
        impl_path: "",
        fn_name: "prepared_decl_body_is_closed_unguarded",
        method_chain: false,
        reason: "reads prepared.body to decide closedness of a prepared decl body",
    },
    ReaderRow {
        // Free file-scope fn (same as above — outside the KeyDomainBinding impl).
        file: "src/project_semantic_dispatch/raise.rs",
        impl_path: "",
        fn_name: "prepared_instantiation_key_domain_is_closed",
        method_chain: false,
        reason: "reads prepared.body to decide whether an instantiation's key domain is closed",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_registry.rs",
        impl_path: "",
        fn_name: "component_meta_registry_owner_local_component_config_alias_name",
        method_chain: false,
        reason:
            "reads prepared.body (and a one-hop alias next.body) to classify a ComponentConfig \
                 alias",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_registry.rs",
        impl_path: "",
        fn_name: "collect_component_meta_registry_public_field_refs",
        method_chain: false,
        reason: "reads prepared.body across the registry public-field surface",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_registry.rs",
        impl_path: "",
        fn_name: "collect_component_meta_registry_public_indexed_access_roots",
        method_chain: false,
        reason: "reads prepared.body to collect indexed-access roots",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/shallow_preserve.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "should_preserve_transitive_ref",
        method_chain: false,
        reason: "reads prepared.body to decide transitive-ref preservation",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/shallow_preserve.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "fast_symbolic_imported_generic_route",
        method_chain: false,
        reason: "reads prepared.body on the fast symbolic imported-generic route",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/shallow_preserve.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "collapse_same_file_imported_alias_chain",
        method_chain: false,
        reason: "reads prepared.body when collapsing a same-file imported alias chain",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/shallow_preserve.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "try_fast_expand_shallow_alias_body",
        method_chain: false,
        reason: "reads prepared.body on the fast shallow-alias expand path",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/shallow_preserve.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "rewrite_fast_shallow_alias_body",
        method_chain: false,
        reason: "reads prepared.body on the fast shallow-alias rewrite path",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/registry_decl.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "owner_collection_expr",
        method_chain: false,
        reason: "reads prepared.body to build the owner collection expression",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/registry_decl.rs",
        impl_path: "impl ComponentMetaQueryEngine",
        fn_name: "named_decl_body",
        method_chain: false,
        reason: "the named_decl_body DEFINITION — reads prepared.body and returns the cloned body \
                 TypeExpr to its callers",
    },
    ReaderRow {
        file: "src/resolver_core/component_meta_query_engine/helpers.rs",
        impl_path: "",
        fn_name: "resolve_imported_registry_symbol_with_budget",
        method_chain: false,
        reason: "builds ResolvedImportedRegistrySymbol.body from prepared.body",
    },
    ReaderRow {
        file: "src/meta_resolve/projectors/macro_payload_substrate.rs",
        impl_path: "",
        fn_name: "lower_decl_body_to_node",
        method_chain: false,
        reason: "reads prepared.body to lower a decl body into a semantic node",
    },
    ReaderRow {
        file: "src/host_manage/component_meta_methods.rs",
        impl_path: "impl VerterHost",
        fn_name: "owner_local_generic_alias_substituted_body_via_dispatch",
        method_chain: false,
        reason: "the instantiation fast-lane gate reads prepared.body",
    },
    // ── named_decl_body callers (transitive .body via the returned body) ──
    ReaderRow {
        file: "src/host_manage/component_meta_methods.rs",
        impl_path: "impl VerterHost",
        fn_name: "collect_one_filtered_expr",
        method_chain: false,
        reason: "calls named_decl_body (×3) and consumes ResolvedImportedRegistrySymbol.body",
    },
    ReaderRow {
        file: "src/meta_resolve/registry_materialize.rs",
        impl_path: "",
        fn_name: "nested_symbolic_member_route_should_stay_symbolic",
        method_chain: false,
        reason: "calls named_decl_body and inspects the returned body",
    },
    ReaderRow {
        file: "src/meta_resolve/materialize/field_types.rs",
        impl_path: "",
        fn_name: "type_expr_has_package_backed_object_like_root_with_fence",
        method_chain: false,
        reason: "calls named_decl_body and inspects the returned body",
    },
    ReaderRow {
        file: "src/component_meta_resolution_policy/core.rs",
        impl_path: "impl PolicyCtx",
        fn_name: "locate_declaration",
        method_chain: false,
        reason: "calls named_decl_body and returns the located declaration body",
    },
    // ── Lowered* SEMANTIC readers (the method-chain + bare-field set) ────
    ReaderRow {
        file: "src/resolver_core/prepared_decl.rs",
        impl_path: "",
        fn_name: "prepare_type_decl_from_lowered",
        method_chain: true,
        reason: "the CLONE PATH the migration DELETES — reads lowered.body via lookup_object() / \
                 is_merged() / contributors(); DO-NOT-TOUCH/DELETE, anchor only",
    },
    ReaderRow {
        file: "src/resolver_core/prepared_decl.rs",
        impl_path: "",
        fn_name: "prepare_local_value_decl",
        method_chain: false,
        reason:
            "reads lowered.type_annotation (LoweredValueDecl) when preparing a local value decl",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "route_closure",
        method_chain: true,
        reason: "reads lowered.body.lookup_object() on the route-closure path",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "member_path_route_closure",
        method_chain: true,
        reason: "reads lowered.body.lookup_object() on the member-path route closure",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "member_route_closure",
        method_chain: true,
        reason: "reads lowered.body.lookup_object() on the member route closure",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "whole_route_closure",
        method_chain: true,
        reason: "reads lowered.body.lookup_object() on the whole route closure",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "follow_local_symbol_precise",
        method_chain: true,
        reason: "reads lowered.body.lookup_object() when following a local symbol precisely",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "follow_routed_expr",
        method_chain: true,
        reason: "reads lowered.body.lookup_object() when following a routed expression",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "impl ShallowFileState",
        fn_name: "extract_string_literal_keys_from_type_expr",
        method_chain: true,
        reason: "reads lowered.body.lookup_object() to extract string-literal keys",
    },
    ReaderRow {
        file: "src/resolver_core/shallow_file_state.rs",
        impl_path: "",
        fn_name: "collect_member_path_seed_names",
        method_chain: true,
        reason: "reads lowered.body.lookup_object() to collect member-path seed names (free fn)",
    },
    ReaderRow {
        file: "src/resolver_core/external_type_frontier.rs",
        impl_path: "impl ExternalTypeFrontier",
        fn_name: "resolve_through_export",
        method_chain: true,
        reason:
            "reads lowered.body.lookup_object() (two branches) when resolving through an export",
    },
    ReaderRow {
        file: "src/host_manage/eval_env.rs",
        impl_path: "impl VerterHost",
        fn_name: "peel_value_decl_alias_graph_native",
        method_chain: false,
        reason: "the typeof peel reads lowered.type_annotation (LoweredValueDecl)",
    },
];

/// One anchored COMPAT body reader: a purpose-named compat helper DEFINITION, or
/// the CALL SITE fn that routes its body read through such a helper.
struct CompatRow {
    file: &'static str,
    impl_path: &'static str,
    fn_name: &'static str,
    /// `true` iff this fn itself contains a `<recv>.body.<method>` read (the
    /// helper DEFS do; the consumer call-site fns do NOT — they call the helper).
    method_chain: bool,
    reason: &'static str,
}

/// The COMPAT body-read surface — the ONLY sanctioned output/compat body reads.
/// The three purpose-named compat helpers plus the consumer fns that route
/// through them. After the compat routing, the body-fact emitter and the
/// typeinfo oracle read a declaration body ONLY through these helpers.
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
/// `<recv>.body.<method>` read — the union of the SEMANTIC rows flagged
/// `method_chain` and the COMPAT rows flagged `method_chain`. The tripwire
/// reddens on any production `.body.<method>` read whose anchor is NOT in this
/// set.
fn method_chain_allowed_anchors() -> HashSet<(String, String, String)> {
    let mut set = HashSet::new();
    for r in SEMANTIC_BODY_READERS {
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
    for r in SEMANTIC_BODY_READERS {
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

// ════════════════════════════════════════════════════════════════════
// INVARIANT 1 — PRESENCE
// ════════════════════════════════════════════════════════════════════

/// Every enumerated SEMANTIC body reader AND every COMPAT row (helper def +
/// consumer call site) is defined at its `(file, impl/mod, fn)` anchor in the
/// production tree. A renamed / moved / deleted reader reddens — the migration
/// pass depends on this enumerated surface being stable.
#[test]
fn every_enumerated_body_reader_is_present_at_its_anchor() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    let mut missing = Vec::new();

    for r in SEMANTIC_BODY_READERS {
        assert!(
            !r.reason.trim().is_empty(),
            "semantic reader row `{} :: {} :: {}` must carry a non-empty reason",
            r.file,
            r.impl_path,
            r.fn_name
        );
        if !anchored_definition_present(&inv, r.file, r.impl_path, r.fn_name) {
            missing.push(format!(
                "SEMANTIC reader `{}` is MISSING at its anchor ({} :: {}) — the frozen migration \
                 surface enumeration is stale (a same-named body elsewhere does NOT satisfy the \
                 anchor)",
                r.fn_name, r.file, r.impl_path
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
        "frozen body-reader inventory: an enumerated reader is absent at its anchor.\n{}",
        missing.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// INVARIANT 2 — UNIQUENESS (fail-closed)
// ════════════════════════════════════════════════════════════════════

/// Each anchored `(file, impl/mod, fn)` resolves to EXACTLY ONE non-test
/// definition. A second definition at the same anchor makes the anchor
/// ambiguous and reddens — the migration pass must be able to address each
/// reader uniquely.
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
        "frozen body-reader inventory: an anchored reader is not unique.\n{}",
        violations.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// INVARIANT 3 — BOUNDED CLASSIFICATION (the load-bearing tripwire)
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
/// anchor is NOT in the method-chain allowlist (the SEMANTIC + COMPAT rows
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

/// The tripwire: every production `<recv>.body.<method>` lowered-body read sits
/// at an anchor in EITHER inventory. A NEW or MOVED such read outside both
/// reddens — a new lowered-body reader cannot silently widen the migration
/// surface.
#[test]
fn no_method_chain_body_read_outside_the_inventory() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    let allowed = method_chain_allowed_anchors();
    let mut violations = Vec::new();
    for hit in unclassified_method_chain_reads(&inv, &allowed) {
        violations.push(format!(
            "{} :: {} :: fn `{}` performs a `<recv>.body.{:?}` lowered-body read but its anchor is \
             NOT in the SEMANTIC or COMPAT inventory — a new/moved lowered-body reader appeared. \
             Add it to SEMANTIC_BODY_READERS (a real semantic reader the migration must migrate) \
             or COMPAT_BODY_READERS (a sanctioned output/compat read), with method_chain: true.",
            hit.file, hit.impl_path, hit.fn_name, hit.methods
        ));
    }
    assert!(
        violations.is_empty(),
        "frozen body-reader inventory: an un-inventoried `<recv>.body.<method>` read appeared — the \
         classification tripwire fired.\n{}",
        violations.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// INVARIANT 4 — COMPAT PURITY
// ════════════════════════════════════════════════════════════════════

/// The two body-fact / oracle CONSUMER files contain NO direct
/// `<recv>.body.<method>` lowered-body read in any non-test fn — their body
/// reads route through the named compat helpers (which live in other files /
/// read `type_annotation`). Any direct method-chain read in these files means
/// the compat routing regressed.
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
        "frozen body-reader inventory: a compat consumer file performs a direct lowered-body \
         method-chain read — the compat routing regressed.\n{}",
        violations.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// DISCRIMINATING SELF-TESTS
// ════════════════════════════════════════════════════════════════════

/// The body-read SHAPE detector discriminates exactly the `<recv>.body.<method>`
/// chain: it fires on `prepared.body.lookup_object()` / `lowered.body
/// .contributors()` / `x.body.is_merged()`, but NOT on a `group.contributors()`
/// (path receiver, not `.body`), NOT on a `body.lookup_object()` (a LOCAL var
/// named `body`, not a `.body` field), NOT on a bare `program.body` field (no
/// method), and NOT on a token only inside a comment / string literal.
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

    // Does NOT fire on a decl-GROUP `.contributors()` (receiver is `group`, not
    // `.body`), a LOCAL `body.lookup_object()` (path receiver named `body`), or
    // a bare `program.body` field (no method).
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
        "self-test: a `group.contributors()` (not `.body`), a LOCAL `body.lookup_object()` (path \
         receiver), and a bare `program.body` field must NOT be detected — got {:?}",
        nr.body_reads
    );

    // Does NOT fire on a token only inside a comment / string literal (invisible
    // to the AST).
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

/// Nested-fn attribution: a `<recv>.body.lookup_object()` read inside a NESTED
/// `fn` is attributed to the NESTED fn's anchor, NOT the enclosing fn — so the
/// per-anchor classification view is correct (this is exactly the case the
/// line-based grep mis-attributed during grounding).
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
/// inventoried SEMANTIC anchor (`shallow_file_state.rs :: impl ShallowFileState
/// :: route_closure`) is NOT.
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

/// Tripwire fail-closed on a MOVE: the SAME inventoried fn name moved to a
/// DIFFERENT impl (or file) is flagged — the anchor is `(file, impl, fn)`, so
/// `route_closure` in `impl SomethingElse` is NOT covered by the
/// `impl ShallowFileState :: route_closure` allowance.
#[test]
fn tripwire_fires_on_moved_inventoried_reader() {
    let allowed = method_chain_allowed_anchors();
    // `route_closure` (an inventoried name) but in a DIFFERENT impl of the same
    // file — performing the chain read.
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
         the chain read MUST be flagged — the allowance covers only `impl ShallowFileState :: \
         route_closure`. Got {:?}",
        hits.iter()
            .map(|h| (h.impl_path.as_str(), h.fn_name.as_str()))
            .collect::<Vec<_>>()
    );
    // Discrimination: the anchored `impl ShallowFileState :: route_closure`
    // (which here performs NO chain read) is not a false positive.
    assert!(
        !hits
            .iter()
            .any(|h| h.impl_path == "impl ShallowFileState" && h.fn_name == "route_closure"),
        "self-test (tripwire move discrimination): the anchored `impl ShallowFileState :: \
         route_closure` must NOT be flagged"
    );
}

/// Compat-purity RED→GREEN: a synthetic `fact_emission.rs` whose `compute` fn
/// performs a DIRECT `<recv>.body.lookup_object()` read IS flagged by the
/// purity scan; a clean version (routing through a helper call, no direct
/// chain) is NOT.
#[test]
fn compat_purity_detector_discriminates() {
    // RED — a direct chain read in a consumer file.
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

    // GREEN — the routed version calls the helper (no direct chain).
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
/// flagged by the tripwire (matching the real-tree `shallow_file_state.rs` test
/// `duplicate_interface_declarations_merge_members`, which reads
/// `body.body.is_merged()` inside `#[cfg(test)] mod tests`).
#[test]
fn cfg_test_chain_read_is_not_flagged() {
    let allowed = method_chain_allowed_anchors();
    let src = "#[cfg(test)]\nmod tests {\n    \
        fn t(lowered: &L) -> bool { lowered.body.is_merged() }\n}\n";
    let inv = inventory_for(&[(
        "src/resolver_core/some_new_module.rs".to_string(),
        src.to_string(),
    )]);
    // The fn IS in the inventory but flagged cfg_test…
    let t = inv.iter().find(|d| d.name == "t").expect("fn t");
    assert!(
        t.cfg_test && t.body_reads.contains("is_merged"),
        "self-test: the cfg-test fn must be marked cfg_test AND have its chain read recorded \
         (proves the negative below is not vacuous) — got cfg_test={}, reads={:?}",
        t.cfg_test,
        t.body_reads
    );
    // …so the tripwire excludes it.
    assert!(
        unclassified_method_chain_reads(&inv, &allowed)
            .iter()
            .all(|h| h.fn_name != "t"),
        "self-test: a `#[cfg(test)]`-gated chain read must NOT be flagged by the tripwire"
    );
}

/// One UNPLANTED CONTROL that stays GREEN: the REAL production tree passes ALL
/// FOUR invariants as-is (presence, uniqueness, the tripwire, and compat
/// purity) — proving the guard is green on the tree it ships against and the
/// self-tests above are not the only thing exercised.
#[test]
fn real_tree_satisfies_all_four_invariants() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);

    // (1) Presence — every enumerated reader is anchored.
    for r in SEMANTIC_BODY_READERS {
        assert!(
            anchored_definition_present(&inv, r.file, r.impl_path, r.fn_name),
            "control: SEMANTIC reader `{}` ({} :: {}) must be present on the real tree",
            r.fn_name,
            r.file,
            r.impl_path
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

    // (2) Uniqueness — every anchor resolves to exactly one def.
    for (file, impl_path, name) in all_inventory_anchors() {
        assert_eq!(
            anchored_defs(&inv, &file, &impl_path, &name).len(),
            1,
            "control: anchor `{file} :: {impl_path} :: {name}` must resolve to exactly one def"
        );
    }

    // (3) Tripwire — no un-inventoried method-chain read on the real tree.
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

    // (4) Compat purity — neither consumer file performs a direct chain read.
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
}

/// Non-vacuity: parsing the real tree yields a large fn inventory that records
/// at least one cfg-test fn AND at least one real `<recv>.body.<method>` read,
/// and the method-chain allowlist is exactly the inventoried `method_chain`
/// rows (2 COMPAT + 10 SEMANTIC = 12).
#[test]
fn real_tree_inventory_is_non_vacuous() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    assert!(
        inv.len() > 100,
        "self-test: the structural inventory must contain many fn definitions — got {}",
        inv.len()
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
    let semantic_chain = SEMANTIC_BODY_READERS
        .iter()
        .filter(|r| r.method_chain)
        .count();
    let compat_chain = COMPAT_BODY_READERS
        .iter()
        .filter(|r| r.method_chain)
        .count();
    assert_eq!(
        allowed.len(),
        semantic_chain + compat_chain,
        "self-test: the method-chain allowlist size must equal the inventoried method_chain rows"
    );
    assert_eq!(
        compat_chain, 2,
        "self-test: exactly two COMPAT helpers perform the `<recv>.body.<method>` chain read"
    );
    assert_eq!(
        semantic_chain, 10,
        "self-test: exactly ten SEMANTIC readers perform the `<recv>.body.<method>` chain read"
    );
}

// ────────────────────────────────────────────────────────────────────
// impl-path renderer self-tests (the proven machinery, re-pinned here)
// ────────────────────────────────────────────────────────────────────

/// The `impl`-path renderer keeps module quals + type generics, normalizes only
/// lifetimes: `impl<'a> ComponentMetaQueryEngine<'a>` → `impl
/// ComponentMetaQueryEngine`; `impl KeyDomainBinding<'_>` → `impl
/// KeyDomainBinding`; and two genuinely-distinct generic args stay distinct.
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
    // Type generics are kept (collision-safe).
    let u8_impl: ItemImpl = syn::parse_quote!(impl T for Foo<u8> {});
    let u16_impl: ItemImpl = syn::parse_quote!(impl T for Foo<u16> {});
    assert_ne!(
        render_impl_path(&u8_impl),
        render_impl_path(&u16_impl),
        "self-test: `Foo<u8>` and `Foo<u16>` must render differently (type generics kept)"
    );
}

/// The cfg-test evaluator: `cfg(test)` and `cfg(all(unix, test))` are test-only;
/// `cfg(any(unix, test))`, `cfg(not(test))`, and `cfg(feature = "test_util")`
/// are PRODUCTION (must be scanned).
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
