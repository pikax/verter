//! Graph-native whole_env consumer-reader inventory guard — every `whole_env()` consumer has a
//! bounded graph-native per-symbol reader present in the production tree
//! BEFORE the eventual producer flip, and NO graph-native reader body
//! routes through the whole-env path.
//!
//! Four `whole_env()` consumers (all reaching `DeclBodyMemo::whole_env()`
//! through `VerterHost::base_eval_env_arc`) each gain a NON-BREAKING,
//! bounded, graph-native per-symbol reader that routes through
//! `ShallowFileState::{type_decl,value_decl,header_index}` instead of
//! materialising the whole-file env. The legacy `whole_env()` path stays
//! in production as the equivalence ORACLE.
//!
//! ## Identity is structural (`syn`), not text
//!
//! Every identity-sensitive question this guard answers — which `fn` a token
//! sits in, the `impl`/`trait`/`mod` path a `fn` belongs to, whether a `fn`
//! is `#[cfg(test)]`-gated, and whether the EXPECTED defining body exists at
//! its anchored file+impl — is answered by parsing each production `src/**`
//! file with [`syn::parse_file`] and walking its item tree (a
//! [`syn::visit::Visit`] impl). Hand-rolled brace counting / line-oriented
//! attribute scanning cannot be made fail-closed for these questions; `syn`
//! is the standard in-house tool for this class of guard (see
//! `fact_tracer_callsite_inventory.rs`, `carrier_encapsulation_guards.rs`).
//!
//! This is NOT a type resolver and does NOT involve OXC: it is a static,
//! test-time scan of the guard's own crate's Rust source. The "OXC is the
//! only front-end" / "no second resolver" rules govern TYPE resolution at
//! query time, not a `syn`-based source-structure guard.
//!
//! ## Invariants enforced (each with paired discriminating self-tests)
//!
//! 1. **Presence (anchored)**: each consumer's graph-native reader is DEFINED
//!    at its EXPECTED `(file, impl/type, fn)` anchor in the production tree
//!    (the ordering gate the eventual producer flip depends on), and the
//!    legacy oracle is retained at ITS anchor (non-breaking). A same-named
//!    body surviving in a NON-anchored file does NOT satisfy presence
//!    (Finding #1).
//! 2. **Uniqueness (fail-closed)**: each anchored `(file, impl/type, fn)`
//!    resolves to EXACTLY ONE non-test definition. A second definition at the
//!    same anchor reddens the guard (Finding #2).
//! 3. **Bounded body**: each graph-native reader's function BODY contains NO
//!    call to the whole-env path (`base_eval_env_arc`, `base_eval_env`,
//!    `whole_env`, or the legacy alias-peel `resolve_value_export_target`
//!    which transitively materialises the whole env). EVERY same-named
//!    definition body is scanned. Tokens inside comments / string literals
//!    are invisible to the AST walk and cannot trip (or satisfy) the guard.
//! 4. **Direct-reach tripwire (NOT an exhaustive-consumer proof)**: no
//!    production fn body DIRECTLY names a whole-env reach token
//!    (`base_eval_env_arc`, `base_eval_env`, `whole_env` — the
//!    materialization roots) unless its EXACT `(file, impl/type, fn)` anchor
//!    is allowlisted — the accessor/forwarder/producer roots plus the four
//!    oracle consumers. A `#[cfg(test)]`-gated fn is test code and is excluded
//!    (Finding #3). The cfg-test predicate matches the `test` IDENT inside a
//!    `cfg(...)` meta tree (recursing `all`/`any`/`not`), so
//!    `feature = "test_util"` is safe; same-line and multiline attribute
//!    shapes are handled by the parser for free.
//!
//!    SCOPE (honest claim): this scan is a DIRECT-reach tripwire only. It
//!    keys on the literal materialization-root tokens, so it does NOT — and
//!    cannot soundly — catch a fn that reaches the whole-file env
//!    TRANSITIVELY by calling the retained oracle (e.g. a normal production
//!    caller of `resolve_value_export_target`, whose alias-peel reaches
//!    `base_eval_env_arc` one hop down). Several legitimate production callers
//!    already do exactly that and correctly PASS this scan (they call the
//!    enumerated oracle, not the root directly). Treating the oracle fns as
//!    transitive reach tokens here is NOT done: it would force a syn scanner
//!    to emulate transitive call-graph resolution, which cannot soundly
//!    converge over a real codebase. Consumer-SET COMPLETENESS — that exactly
//!    the four enumerated consumers (C1–C4) read a whole-file env and there is
//!    no fifth — is established by the codex-confirmed EXHAUSTIVE ENUMERATION
//!    of the `whole_env()` consumers, the per-consumer oracle-EQUIVALENCE
//!    tests, and review; NOT by this token-scan. What this scan adds on top of
//!    those is a cheap, sound tripwire: a NEW production fn that introduces a
//!    DIRECT root reach outside the allowlist reddens immediately (a new
//!    materialization site can't appear unnoticed), and every existing direct
//!    root reach is anchored to a reviewed `(file, impl, fn)`.
//! 5. **No reader→oracle call-through (GOV5)**: a graph-native reader BODY must
//!    reach its result through the per-symbol shallow primitives, NEVER by
//!    calling its retained whole-env ORACLE. The reader-body scan
//!    (`graph_native_reader_bodies_do_not_route_through_whole_env`) therefore
//!    bans, on top of the transitive legacy whole-env tokens
//!    ([`BANNED_WHOLE_ENV_TOKENS`]), a call to ANY oracle fn — the four
//!    `oracle_fn` names DERIVED from [`WHOLE_ENV_CONSUMER_INVENTORY`] via
//!    [`oracle_bare`] (no hardcoded parallel list). The whole-identifier
//!    discipline of the AST collector keeps the ban precise: a reader calling a
//!    `_graph_native`-suffixed sibling surfaces ONLY the full suffixed ident, so
//!    `…_graph_native` is NEVER mistaken for the bare oracle. This ban is
//!    reader-body-scoped: the oracle fns legitimately call each other / the
//!    roots, so they are NOT added to the GLOBAL reach scan's banned set (the
//!    reach scan correctly anchors the oracle bodies for the ROOT tokens).
//!
//! ## Accepted residual limits of this source-token guard (GOV5)
//!
//! This is a NON-BREAKING readiness guard over the guard crate's own Rust
//! source; the real producer flip is a future stage with its own gate. Two
//! laundering shapes are KNOWN, ACCEPTED residuals of any source-token guard
//! and are intentionally NOT defended here:
//!
//! - **Macro expansion without a banned source token**: a `macro_rules!` /
//!   proc-macro invocation that EXPANDS to a whole-env reach (or an oracle call)
//!   without the banned identifier appearing as a literal source token in the
//!   reader body. The AST collector sees the pre-expansion token stream, so an
//!   expansion-only reach is invisible. (Banned idents that DO appear as literal
//!   tokens inside a macro invocation ARE caught — see
//!   `banned_ident_inside_macro_is_detected`.)
//! - **Arbitrary alias laundering**: a banned call reached through an
//!   aliased / re-exported name (`use base_eval_env_arc as ble;` then `ble(…)`,
//!   or an indirection through a differently-named wrapper). The guard matches
//!   the WHOLE identifier as written; it does not resolve aliases or follow
//!   re-export chains.
//!
//! These residuals are explicit, not silent: the readiness guard fails closed
//! on the direct source-token shapes (the load-bearing case for the ordering
//! gate), and the eventual producer flip carries its own behavioural gate.

use std::collections::{BTreeMap, HashMap, HashSet};
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

/// Production `.rs` files under `crates/verter_session/src`, relative to
/// the crate root, with their source — test fixtures excluded.
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
// `syn` STRUCTURAL INVENTORY
// ════════════════════════════════════════════════════════════════════

/// The structural path of the enclosing item a `fn` definition lives in. This
/// is the impl/type/mod context that, together with the file path and fn name,
/// forms the fail-closed anchor identity (Finding #2). Two `fn new`s in two
/// different `impl` blocks of the SAME file have DISTINCT `impl_path`s, so the
/// accessor/producer anchors are e.g. `impl DeclBodyMemo` `new`, never the
/// bare `(file, new)`.
///
/// The string is a normalized rendering:
/// - inside `impl Type { … }` → `"impl Type"`
/// - inside `impl Trait for Type { … }` → `"impl Trait for Type"`
/// - inside `mod m { … }` (free fn) → `"mod m"` (chain joined by `::`)
/// - a free fn at file scope → `""`
///
/// Lifetimes/elision are normalized away (`<'_>`/`<'a>` etc.) so the anchor is
/// stable against lifetime churn.
type ImplPath = String;

/// One structural fn DEFINITION discovered by the `syn` walk: the file
/// (crate-relative, forward-slashed), the fn name, its enclosing impl/type/mod
/// path, whether it (or any enclosing item) is `#[cfg(test)]`-gated, and
/// whether the fn carries a real body (`{ … }`, not a bodyless trait
/// declaration). The body's whole-env reach tokens are pre-extracted as the
/// set of WHOLE-identifier callees referenced anywhere in the body subtree.
#[derive(Debug, Clone)]
struct FnDef {
    file: String,
    name: String,
    impl_path: ImplPath,
    cfg_test: bool,
    has_body: bool,
    /// WHOLE-identifier path segments / method names referenced in the body
    /// subtree (deduplicated). Used by the bounded-body and reach scans.
    referenced_idents: HashSet<String>,
}

impl FnDef {
    /// The fail-closed anchor identity: `(file, impl_path, name)`.
    fn anchor(&self) -> (String, String, String) {
        (self.file.clone(), self.impl_path.clone(), self.name.clone())
    }
}

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

/// Collect every WHOLE-identifier referenced in a fn body subtree: the last
/// segment of every path expression, every method-call name, every NAMED field
/// access (`self.whole_env` surfaces `whole_env`), every struct field-init
/// member name (`Self { whole_env: … }` surfaces `whole_env`), and every WHOLE
/// IDENT inside a macro invocation's token stream (`m!(self.base_eval_env_arc(…))`
/// surfaces `base_eval_env_arc`). This is the AST analogue of the
/// whole-identifier token scan, and it preserves the whole-identifier
/// discipline: `self.resolve_value_export_target_graph_native(…)` surfaces ONLY
/// the full `…_graph_native` ident, never the banned bare
/// `resolve_value_export_target` (a path segment / field / macro ident is one
/// whole token, never a substring). Tokens in comments / string literals do not
/// exist in the AST and are invisible here (Finding #2).
struct IdentCollector {
    idents: HashSet<String>,
}

impl IdentCollector {
    /// Surface every WHOLE IDENT inside a macro token stream, recursing into
    /// `(…)` / `[…]` / `{…}` groups. A banned ident inside a macro body counts
    /// as a reach. Whole-ident discipline holds: `proc_macro2` keeps
    /// `base_eval_env_arc` and `resolve_value_export_target_graph_native` each
    /// as a SINGLE `Ident` token (never split into substrings), and string /
    /// numeric literals never surface a bare ident.
    fn collect_macro_tokens(&mut self, ts: &proc_macro2::TokenStream) {
        use proc_macro2::TokenTree;
        for tt in ts.clone() {
            match tt {
                TokenTree::Ident(id) => {
                    self.idents.insert(id.to_string());
                }
                TokenTree::Group(g) => self.collect_macro_tokens(&g.stream()),
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for IdentCollector {
    fn visit_path(&mut self, p: &'ast syn::Path) {
        for seg in &p.segments {
            self.idents.insert(seg.ident.to_string());
        }
        syn::visit::visit_path(self, p);
    }

    fn visit_expr_method_call(&mut self, c: &'ast syn::ExprMethodCall) {
        self.idents.insert(c.method.to_string());
        syn::visit::visit_expr_method_call(self, c);
    }

    /// `expr.field` — a NAMED member access surfaces the field ident
    /// (`self.whole_env` → `whole_env`). Tuple-index members (`.0`) carry no
    /// ident and are ignored.
    fn visit_expr_field(&mut self, f: &'ast syn::ExprField) {
        if let syn::Member::Named(name) = &f.member {
            self.idents.insert(name.to_string());
        }
        syn::visit::visit_expr_field(self, f);
    }

    /// `Self { whole_env: … }` — a struct field-init NAMED member surfaces the
    /// member ident. Tuple-struct positional members carry no ident.
    fn visit_field_value(&mut self, fv: &'ast syn::FieldValue) {
        if let syn::Member::Named(name) = &fv.member {
            self.idents.insert(name.to_string());
        }
        syn::visit::visit_field_value(self, fv);
    }

    /// A macro invocation (`some_macro!(self.base_eval_env_arc(id))`) — scan the
    /// macro's raw token stream for banned WHOLE idents. The default `syn` walk
    /// does NOT descend macro tokens, so without this a reach hidden in a macro
    /// body would be invisible (Finding #2).
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        // The macro path itself (`some_macro`) is a normal path; record it via
        // the path visitor for consistency, then scan the argument tokens.
        for seg in &m.path.segments {
            self.idents.insert(seg.ident.to_string());
        }
        self.collect_macro_tokens(&m.tokens);
        syn::visit::visit_macro(self, m);
    }
}

fn collect_body_idents(block: &syn::Block) -> HashSet<String> {
    let mut c = IdentCollector {
        idents: HashSet::new(),
    };
    c.visit_block(block);
    c.idents
}

/// The `syn::Visit` scanner that builds the structural fn-definition inventory
/// for one file. It tracks the enclosing impl/mod path stack and a cfg-test
/// nesting depth (an item nested anywhere inside a `#[cfg(test)]` impl/mod, or
/// directly carrying `#[cfg(test)]`, is test code). Both free `fn`s
/// (`ItemFn`) and `impl`-method `fn`s (`ImplItemFn`) are recorded.
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

    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        let cfg_test = self.cfg_test_depth > 0 || attrs_are_cfg_test(&f.attrs);
        self.defs.push(FnDef {
            file: self.file.to_string(),
            name: f.sig.ident.to_string(),
            impl_path: self.current_path(),
            cfg_test,
            has_body: true,
            referenced_idents: collect_body_idents(&f.block),
        });
        // Descend (nested fns / closures inside the body are recorded too).
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
            referenced_idents: collect_body_idents(&f.block),
        });
        syn::visit::visit_impl_item_fn(self, f);
    }

    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        // A trait method may carry a DEFAULT body or be a bare declaration
        // (`fn foo(&self);`). A bodiless declaration is NOT a definition for
        // presence/bounded-body, but a default body IS scanned.
        let cfg_test = self.cfg_test_depth > 0 || attrs_are_cfg_test(&f.attrs);
        match &f.default {
            Some(block) => self.defs.push(FnDef {
                file: self.file.to_string(),
                name: f.sig.ident.to_string(),
                impl_path: self.current_path(),
                cfg_test,
                has_body: true,
                referenced_idents: collect_body_idents(block),
            }),
            None => self.defs.push(FnDef {
                file: self.file.to_string(),
                name: f.sig.ident.to_string(),
                impl_path: self.current_path(),
                cfg_test,
                has_body: false,
                referenced_idents: HashSet::new(),
            }),
        }
        syn::visit::visit_trait_item_fn(self, f);
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
}

/// Parse every production `src/**` file with `syn` and return the flat
/// structural inventory of every fn definition (free, impl-method, and trait
/// default/declaration). A parse error is a hard panic (corruption signal),
/// matching the in-house pattern.
fn build_fn_inventory(files: &[(String, String)]) -> Vec<FnDef> {
    let mut defs = Vec::new();
    for (rel, src) in files {
        let parsed = syn::parse_file(src).unwrap_or_else(|e| panic!("parse {rel}: {e}"));
        let mut scanner = InventoryScanner::new(rel, &mut defs);
        syn_visit_file(&mut scanner, &parsed);
    }
    defs
}

/// Parse one synthetic `(file, src)` slice into the structural inventory —
/// the self-test entry point (mirrors `build_fn_inventory` but does not panic
/// the whole run on a fixture parse error).
fn inventory_for(files: &[(String, String)]) -> Vec<FnDef> {
    build_fn_inventory(files)
}

// ════════════════════════════════════════════════════════════════════
// ANCHOR TABLES
// ════════════════════════════════════════════════════════════════════

/// One whole_env() consumer inventory row: the legacy `whole_env()` consumer
/// fn (the oracle) and the graph-native reader fn that must accompany it. Each
/// is anchored to its EXPECTED `(file, impl/type, fn)` — verified against the
/// real tree via `git grep`/reading (see the module doc + the resolved-anchors
/// self-test). Anchoring presence to the EXPECTED defining file+impl (not "any
/// same-named body anywhere") is Finding #1: a same-named body surviving in a
/// non-anchored file (the C4 trait default in `runtime_values.rs`, the C4
/// delegate in `host_manage.rs`) does NOT satisfy presence — only the REAL
/// bounded reader at `eval_env.rs` `impl VerterHost` does.
struct ConsumerRow {
    /// Human label.
    consumer: &'static str,
    /// The legacy oracle consumer fn (must STILL exist — non-breaking), bare
    /// name (no `fn ` prefix).
    oracle_fn: &'static str,
    /// The production file (crate-relative, forward-slashed) DEFINING the
    /// oracle.
    oracle_file: &'static str,
    /// The oracle's enclosing impl/type path (`render_impl_path` form).
    oracle_impl: &'static str,
    /// The bounded graph-native per-symbol reader fn, bare name.
    graph_native_fn: &'static str,
    /// The production file DEFINING the REAL bounded graph-native reader
    /// (Finding #1 anchor — verified the bounded body lives HERE, not in a
    /// trait default / delegate).
    graph_native_file: &'static str,
    /// The graph-native reader's enclosing impl/type path.
    graph_native_impl: &'static str,
    /// Rationale (required for every row).
    reason: &'static str,
}

/// The four enumerated `whole_env()` consumers and their graph-native readers.
const WHOLE_ENV_CONSUMER_INVENTORY: &[ConsumerRow] = &[
    ConsumerRow {
        consumer: "C1 local_type_declaration_id",
        oracle_fn: "local_type_declaration_id",
        oracle_file: "src/host_manage/eval_env.rs",
        oracle_impl: "impl VerterHost",
        graph_native_fn: "local_type_declaration_id_graph_native",
        graph_native_file: "src/host_manage/eval_env.rs",
        graph_native_impl: "impl VerterHost",
        reason: "C1 routes the import guard + local-type presence through routed_shallow_state + \
                 the per-symbol declaration-header index; the oracle stays authoritative for the \
                 opaque declaration_id value (stable-unique, not equal-to-oracle)",
    },
    ConsumerRow {
        consumer: "C2 peel_value_decl_alias",
        oracle_fn: "peel_value_decl_alias",
        oracle_file: "src/host_manage/eval_env.rs",
        oracle_impl: "impl VerterHost",
        graph_native_fn: "peel_value_decl_alias_graph_native",
        graph_native_file: "src/host_manage/eval_env.rs",
        graph_native_impl: "impl VerterHost",
        reason: "C2 walks the same single-segment typeof alias chain via per-symbol value_decl + \
                 value-header presence, never the whole env",
    },
    ConsumerRow {
        consumer: "C3 build_fallthrough_eval_env_lightweight (dep extraction)",
        oracle_fn: "build_fallthrough_eval_env_lightweight",
        oracle_file: "src/host_manage/fallthrough.rs",
        oracle_impl: "impl VerterHost",
        graph_native_fn: "fallthrough_runtime_value_deps_graph_native",
        graph_native_file: "src/host_manage/fallthrough.rs",
        graph_native_impl: "impl VerterHost",
        reason: "the consumer's whole-env clone is the eventual storage flip (out of scope); the \
                 readiness deliverable is the graph-native runtime-value dep SET, equal to the \
                 materializer's touched (canonical, name) pairs",
    },
    ConsumerRow {
        consumer: "C4 dependency_eval_env (per-name value read)",
        oracle_fn: "dependency_eval_env",
        oracle_file: "src/host_manage.rs",
        oracle_impl: "impl ImportedRuntimeValueResolver for HostRuntimeValueResolver",
        // Finding #1: the REAL bounded C4 reader is the host method in
        // eval_env.rs (`impl VerterHost`), NOT the `runtime_values.rs` trait
        // default (an unconditional `None` stub) nor the `host_manage.rs`
        // delegate (which forwards to this one). Presence anchors to THIS file
        // + impl; deleting it would redden presence even though same-named
        // bodies survive elsewhere.
        graph_native_fn: "dependency_value_symbol_graph_native",
        graph_native_file: "src/host_manage/eval_env.rs",
        graph_native_impl: "impl VerterHost",
        reason: "the consumer's sole whole-env use is source_env.value_symbols.get(name) after a \
                 prepared_value_decl miss; the per-name reader reproduces that read via value_decl \
                 without the whole env (the runtime_values.rs trait default + host_manage.rs \
                 delegate are NOT the anchored reader)",
    },
];

/// The bare (whole-identifier) oracle fn name for an inventory row's
/// `oracle_fn`. The `oracle_fn` field is already stored bare (no `fn ` prefix),
/// so this is the identity projection — its purpose is to centralise the
/// "compare against the bare oracle whole identifier" intent at the GOV5
/// reader→oracle call-through ban, so the ban derives the four oracle names from
/// [`WHOLE_ENV_CONSUMER_INVENTORY`] instead of hardcoding a parallel list. The
/// AST ident collector stores WHOLE identifiers, so testing
/// `referenced_idents.contains(oracle_bare(row.oracle_fn))` matches ONLY a call
/// to the bare oracle, never its `_graph_native`-suffixed sibling.
fn oracle_bare(oracle_fn: &str) -> &str {
    oracle_fn
}

/// The whole-env-path tokens banned inside a graph-native reader body. A
/// reader that references any of these (as a whole identifier — surfaced by
/// the AST ident collector) is NOT graph-native (it reaches
/// `DeclBodyMemo::whole_env()`). `base_eval_env_arc` does not satisfy the
/// `base_eval_env` ban (distinct idents), and
/// `resolve_value_export_target_graph_native` does NOT match
/// `resolve_value_export_target` (the collector keeps only the full segment).
const BANNED_WHOLE_ENV_TOKENS: &[&str] = &[
    "base_eval_env_arc",
    "base_eval_env",
    "whole_env",
    "resolve_value_export_target",
];

/// The whole-env MATERIALIZATION-ROOT tokens. A production fn body referencing
/// any of these reaches the whole-file env. Excludes
/// `resolve_value_export_target` (a reader-body ban, not a materialization
/// root).
const WHOLE_ENV_REACH_TOKENS: &[&str] = &["base_eval_env_arc", "base_eval_env", "whole_env"];

/// An EXACT whole-env reach anchor: `(file, impl/type, fn)` — the fail-closed
/// identity (Finding #2). A same-named fn in a DIFFERENT file, or in a
/// DIFFERENT impl of the same file, is NOT exempt.
struct WholeEnvAnchor {
    rel: &'static str,
    impl_path: &'static str,
    fn_name: &'static str,
}

/// The accessor/forwarder/producer definitions that ARE the whole-env
/// materialization roots (the accessor IS the root) — the ONLY whole-env
/// reachers that are NOT enumerated oracle consumers. Each is anchored to its
/// EXACT `(file, impl, fn)` so a same-named fn elsewhere — or a second `fn new`
/// in another impl of the same file — cannot inherit the exemption. Adding a
/// NEW anchor here is a deliberate, reviewed widening. The oracle CONSUMER
/// anchors are NOT listed here; they are derived from
/// `WHOLE_ENV_CONSUMER_INVENTORY` so adding/retargeting a row keeps the oracle
/// anchors in sync automatically.
const ACCESSOR_WHOLE_ENV_ANCHORS: &[WholeEnvAnchor] = &[
    // `VerterHost::base_eval_env_arc` — the host-side whole-env accessor root.
    WholeEnvAnchor {
        rel: "src/host_manage/eval_env.rs",
        impl_path: "impl VerterHost",
        fn_name: "base_eval_env_arc",
    },
    // `VerterHost::base_eval_env` — the source-keyed forwarder.
    WholeEnvAnchor {
        rel: "src/host_manage/eval_env.rs",
        impl_path: "impl VerterHost",
        fn_name: "base_eval_env",
    },
    // `DeclBodyMemo::whole_env` — the lazy whole-env accessor.
    WholeEnvAnchor {
        rel: "src/decl_body_memo.rs",
        impl_path: "impl DeclBodyMemo",
        fn_name: "whole_env",
    },
    // NOTE: `DeclBodyMemo::whole_env_materialized` is `#[cfg(test)]`-gated
    // (test-only observability that reads the `whole_env` OnceLock field
    // without forcing it). cfg-test fns are EXCLUDED from the reach scan
    // (Finding #3), so it never registers as a production reacher and needs NO
    // anchor — listing it here would make the uniqueness guard demand a
    // non-test definition that does not exist. It is intentionally absent.
    // `DeclBodyMemo::seeded_from_env` — the seeded constructor that pre-sets
    // the `whole_env` OnceLock field.
    WholeEnvAnchor {
        rel: "src/decl_body_memo.rs",
        impl_path: "impl DeclBodyMemo",
        fn_name: "seeded_from_env",
    },
    // `DeclBodyMemo::new` — the lazy constructor that DEFINES the
    // `whole_env: OnceLock::new()` field.
    WholeEnvAnchor {
        rel: "src/decl_body_memo.rs",
        impl_path: "impl DeclBodyMemo",
        fn_name: "new",
    },
];

/// The full whole-env reach allowlist as EXACT `(rel, impl_path, fn)` anchors:
/// the accessor/producer anchors PLUS the four oracle CONSUMER anchors
/// (derived from the inventory rows). A reach hit is permitted ONLY if its
/// `(file, impl/type, enclosing_fn)` is in this set.
fn whole_env_anchors() -> HashSet<(String, String, String)> {
    let mut set: HashSet<(String, String, String)> = ACCESSOR_WHOLE_ENV_ANCHORS
        .iter()
        .map(|a| {
            (
                a.rel.to_string(),
                a.impl_path.to_string(),
                a.fn_name.to_string(),
            )
        })
        .collect();
    for row in WHOLE_ENV_CONSUMER_INVENTORY {
        set.insert((
            row.oracle_file.to_string(),
            row.oracle_impl.to_string(),
            row.oracle_fn.to_string(),
        ));
    }
    set
}

/// All NON-test definitions of `(file, impl_path, name)` in the inventory.
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

/// Whether a NON-test, real-body definition exists at the EXACT anchor.
fn anchored_definition_present(inv: &[FnDef], file: &str, impl_path: &str, name: &str) -> bool {
    inv.iter().any(|d| {
        !d.cfg_test && d.has_body && d.file == file && d.impl_path == impl_path && d.name == name
    })
}

/// Every NON-test, real-body definition body (across ALL files/impls) of a
/// fn named `name` — used by the bounded-body scan (a trait default + a
/// delegate + the real reader must ALL be clean). Returns `(file, impl_path,
/// referenced_idents)` per definition.
fn all_named_bodies<'a>(inv: &'a [FnDef], name: &str) -> Vec<&'a FnDef> {
    inv.iter()
        .filter(|d| !d.cfg_test && d.has_body && d.name == name)
        .collect()
}

// ════════════════════════════════════════════════════════════════════
// PRESENCE + BOUNDED-BODY (Invariants 1, 2, 3)
// ════════════════════════════════════════════════════════════════════

#[test]
fn every_whole_env_consumer_has_a_graph_native_reader_in_production() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    let mut missing = Vec::new();
    for row in WHOLE_ENV_CONSUMER_INVENTORY {
        assert!(
            !row.reason.trim().is_empty(),
            "inventory row `{}` must carry a non-empty reason",
            row.consumer
        );
        // The legacy oracle must STILL exist at its anchor (non-breaking).
        if !anchored_definition_present(&inv, row.oracle_file, row.oracle_impl, row.oracle_fn) {
            missing.push(format!(
                "consumer `{}`: legacy oracle `{}` is MISSING at its anchor ({} :: {}) — the \
                 oracle must be retained (non-breaking)",
                row.consumer, row.oracle_fn, row.oracle_file, row.oracle_impl
            ));
        }
        // The graph-native reader must be PRESENT at its EXPECTED defining
        // file+impl before the eventual producer flip (Finding #1: a
        // same-named body in a non-anchored file does NOT satisfy presence).
        if !anchored_definition_present(
            &inv,
            row.graph_native_file,
            row.graph_native_impl,
            row.graph_native_fn,
        ) {
            missing.push(format!(
                "consumer `{}`: graph-native reader `{}` is MISSING at its anchor ({} :: {}) — it \
                 must be present BEFORE the eventual producer flip (a same-named body in some \
                 OTHER file does NOT satisfy presence)",
                row.consumer, row.graph_native_fn, row.graph_native_file, row.graph_native_impl
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "graph-native consumer-reader readiness inventory: a whole_env() consumer lacks its \
         graph-native reader at its anchor (or the oracle was removed).\n{}",
        missing.join("\n")
    );
}

/// Invariant 2 (Finding #2): each anchored `(file, impl/type, fn)` resolves to
/// EXACTLY ONE non-test definition. A second definition at the same anchor —
/// or an anchor that becomes ambiguous after an edit — reddens the guard. The
/// anchor set is every oracle anchor + every graph-native anchor + every
/// accessor/producer anchor.
#[test]
fn anchored_definitions_are_unique() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);

    // Build the union of every anchor this guard relies on.
    let mut anchors: Vec<(String, String, String)> = Vec::new();
    for row in WHOLE_ENV_CONSUMER_INVENTORY {
        anchors.push((
            row.oracle_file.to_string(),
            row.oracle_impl.to_string(),
            row.oracle_fn.to_string(),
        ));
        anchors.push((
            row.graph_native_file.to_string(),
            row.graph_native_impl.to_string(),
            row.graph_native_fn.to_string(),
        ));
    }
    for a in ACCESSOR_WHOLE_ENV_ANCHORS {
        anchors.push((
            a.rel.to_string(),
            a.impl_path.to_string(),
            a.fn_name.to_string(),
        ));
    }

    let mut violations = Vec::new();
    for (file, impl_path, name) in &anchors {
        let count = anchored_defs(&inv, file, impl_path, name).len();
        if count != 1 {
            violations.push(format!(
                "anchor `{file} :: {impl_path} :: fn {name}` resolves to {count} non-test \
                 definitions — every anchor must resolve to EXACTLY ONE (a second definition at \
                 the same (file, impl, fn) makes the exemption ambiguous; qualify the anchor by \
                 impl/type or remove the duplicate — do NOT drop the check)"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "graph-native consumer-reader readiness: an anchored definition is not unique — the \
         exact-anchor exemption is ambiguous.\n{}",
        violations.join("\n")
    );
}

/// The four oracle whole identifiers (`oracle_fn` of every inventory row),
/// derived from [`WHOLE_ENV_CONSUMER_INVENTORY`] via [`oracle_bare`] — no
/// hardcoded parallel list. A graph-native reader body calling ANY of these
/// launders through the retained whole-env oracle (GOV5) and is a violation.
fn oracle_call_ban_idents() -> HashSet<String> {
    WHOLE_ENV_CONSUMER_INVENTORY
        .iter()
        .map(|row| oracle_bare(row.oracle_fn).to_string())
        .collect()
}

/// The load-bearing invariant: NO graph-native reader body routes through the
/// whole-env path. EVERY same-named definition (real reader + any trait
/// default / delegate) must be bounded — referencing none of the banned
/// whole-env tokens AND calling NONE of the retained oracle fns (GOV5). The AST
/// ident collector keeps only WHOLE identifiers, so
/// `resolve_value_export_target_graph_native` does not match the banned bare
/// `resolve_value_export_target`, a `_graph_native`-suffixed sibling reader call
/// does NOT match the bare oracle name, and tokens inside comments/strings are
/// invisible.
#[test]
fn graph_native_reader_bodies_do_not_route_through_whole_env() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    let oracle_ban = oracle_call_ban_idents();
    let mut violations = Vec::new();
    for row in WHOLE_ENV_CONSUMER_INVENTORY {
        let bodies = all_named_bodies(&inv, row.graph_native_fn);
        if bodies.is_empty() {
            violations.push(format!(
                "consumer `{}`: graph-native reader `{}` BODY not found — cannot prove it is \
                 bounded graph-native",
                row.consumer, row.graph_native_fn
            ));
            continue;
        }
        for def in &bodies {
            for token in BANNED_WHOLE_ENV_TOKENS {
                if def.referenced_idents.contains(*token) {
                    violations.push(format!(
                        "consumer `{}`: graph-native reader `{}` ({} :: {}) BODY references the \
                         whole-env path token `{token}` — a graph-native reader must NEVER reach \
                         the whole env (the oracle is the only permitted whole-env caller)",
                        row.consumer, row.graph_native_fn, def.file, def.impl_path
                    ));
                }
            }
            // GOV5: a reader body must NOT call ANY oracle fn — that launders
            // through the retained whole-env path by name. The whole-identifier
            // collector exempts `_graph_native`-suffixed siblings (a distinct
            // whole ident is never the bare oracle).
            for oracle in &oracle_ban {
                if def.referenced_idents.contains(oracle) {
                    violations.push(format!(
                        "consumer `{}`: graph-native reader `{}` ({} :: {}) BODY calls the retained \
                         oracle `{oracle}` — a graph-native reader must reach its result through \
                         the per-symbol shallow primitives, NEVER by calling the whole-env oracle",
                        row.consumer, row.graph_native_fn, def.file, def.impl_path
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "graph-native consumer-reader readiness: a graph-native reader body routes through the \
         whole-env path — it is not bounded graph-native.\n{}",
        violations.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// DIRECT WHOLE-ENV REACH TRIPWIRE (Invariant 4, Findings #1 + #3)
// ════════════════════════════════════════════════════════════════════

/// A single un-allowlisted DIRECT whole-env reach: file, enclosing impl/type,
/// fn, token. "Direct" = the materialization-root token appears as a literal
/// whole identifier in the fn body; this is NOT a transitive-reach detector
/// (see [`no_unanchored_direct_whole_env_reach_in_production`]).
#[derive(Debug, Clone)]
struct WholeEnvReachHit {
    rel: String,
    impl_path: String,
    enclosing_fn: String,
    token: String,
}

/// Scan the structural inventory for DIRECT whole-env reach-token references
/// (the materialization roots `base_eval_env_arc` / `base_eval_env` /
/// `whole_env`) that live inside a NON-test fn whose EXACT
/// `(file, impl/type, fn)` anchor is NOT permitted (Finding #1: keying on
/// `(file, impl, fn)` flags a same-named fn in a different module OR a
/// different impl). A `#[cfg(test)]`-gated fn is test code and excluded
/// (Finding #3 — it never enters the inventory as a production reacher). The
/// accessor/producer/oracle definitions are anchored at their own
/// `(file, impl, fn)`, so their own bodies do not register.
///
/// This is DIRECT reach only: a fn that reaches the whole env TRANSITIVELY
/// (by calling the retained oracle, whose body reaches a root one hop down)
/// has no root token in its OWN body and is intentionally not flagged — the
/// scan does not emulate transitive call-graph resolution.
fn whole_env_reach_hits(
    inv: &[FnDef],
    anchors: &HashSet<(String, String, String)>,
) -> Vec<WholeEnvReachHit> {
    let mut hits = Vec::new();
    for def in inv {
        if def.cfg_test || !def.has_body {
            continue;
        }
        for token in WHOLE_ENV_REACH_TOKENS {
            if def.referenced_idents.contains(*token) && !anchors.contains(&def.anchor()) {
                hits.push(WholeEnvReachHit {
                    rel: def.file.clone(),
                    impl_path: def.impl_path.clone(),
                    enclosing_fn: def.name.clone(),
                    token: (*token).to_string(),
                });
            }
        }
    }
    hits
}

/// DIRECT-reach tripwire (Findings #1 + #3). Walk the structural inventory and
/// assert every fn body that DIRECTLY names a whole-env materialization root
/// (`base_eval_env_arc` / `base_eval_env` / `whole_env`) sits at an ALLOWLISTED
/// `(file, impl, fn)` anchor — the accessor/forwarder/producer roots plus the
/// four enumerated oracle consumers. A NEW direct root reach outside the
/// allowlist reddens immediately.
///
/// HONEST SCOPE — this is NOT an exhaustive-consumer proof. It cannot catch a
/// fn that reaches the whole-file env TRANSITIVELY by calling the retained
/// oracle (the scan keys on the literal root tokens, and emulating transitive
/// call-graph resolution in a syn scanner cannot soundly converge). Existing
/// legitimate production callers already reach a whole env transitively through
/// the enumerated oracle and correctly PASS this scan. That exactly the four
/// enumerated consumers (C1–C4) read a whole-file env — with no fifth — is
/// established by the codex-confirmed EXHAUSTIVE ENUMERATION of the
/// `whole_env()` consumers + the per-consumer oracle-EQUIVALENCE tests +
/// review, NOT by this token-scan. What this scan adds is a cheap, SOUND
/// tripwire on the load-bearing direct shape: a new materialization site cannot
/// appear unnoticed, and every existing direct root reach is reviewed.
#[test]
fn no_unanchored_direct_whole_env_reach_in_production() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    let anchors = whole_env_anchors();
    let mut violations = Vec::new();
    for hit in whole_env_reach_hits(&inv, &anchors) {
        violations.push(format!(
            "{} :: {} :: fn `{}` DIRECTLY names whole-env materialization-root token `{}` but \
             `({}, {}, {})` is NOT an allowlisted whole-env anchor — a NEW production fn introduces \
             a direct whole-env materialization reach. Either it is a genuine 5th oracle consumer \
             (add a WHOLE_ENV_CONSUMER_INVENTORY row WITH its anchors AND graph-native reader, AND \
             re-confirm the four-consumer enumeration — this scan does not establish that \
             completeness) or it is an accessor/forwarder (add a WholeEnvAnchor to \
             ACCESSOR_WHOLE_ENV_ANCHORS).",
            hit.rel,
            hit.impl_path,
            hit.enclosing_fn,
            hit.token,
            hit.rel,
            hit.impl_path,
            hit.enclosing_fn
        ));
    }
    assert!(
        violations.is_empty(),
        "graph-native consumer-reader readiness: a NEW production fn DIRECTLY names a whole-env \
         materialization root outside the allowlisted anchors — the direct-reach tripwire fired \
         (this is a direct-reach tripwire, NOT an exhaustive-consumer proof; consumer-set \
         completeness is established by the enumeration + oracle-equivalence tests + review).\n{}",
        violations.join("\n")
    );
}

/// The DISCRIMINATING contract proof for
/// [`no_unanchored_direct_whole_env_reach_in_production`] — pins exactly what
/// the renamed direct-reach tripwire DOES and does NOT enforce, so the honest
/// scope is regression-locked:
///
/// 1. RED — a NEW non-anchor fn that DIRECTLY names a root (`base_eval_env_arc`
///    or `whole_env`) IS flagged (the tripwire fires).
/// 2. PASS (honest scope, NOT a regression) — a NEW non-anchor fn that reaches
///    the whole env only TRANSITIVELY, by calling the retained oracle
///    `resolve_value_export_target` with NO direct root token in its own body,
///    is NOT flagged. This is the documented limit: the tripwire keys on direct
///    root tokens and does not emulate transitive call-graph resolution.
/// 3. PASS — the same direct root reach AT an allowlisted anchor is accepted.
#[test]
fn direct_whole_env_reach_tripwire_fires_on_direct_root_not_on_transitive_oracle_call() {
    let anchors = whole_env_anchors();

    // (1) RED — direct root reaches at a NON-anchor location are flagged.
    let direct = "impl Host {\n    \
        fn arc_reach(&self, id: &str) -> Option<i32> { let _ = self.base_eval_env_arc(id); Some(0) }\n    \
        fn field_reach(&self) -> Option<i32> { let _ = self.whole_env.get(); Some(0) }\n}\n";
    let direct_inv = inventory_for(&[(
        "src/host_manage/some_other_module.rs".to_string(),
        direct.to_string(),
    )]);
    let direct_hits = whole_env_reach_hits(&direct_inv, &anchors);
    assert!(
        direct_hits
            .iter()
            .any(|h| h.enclosing_fn == "arc_reach" && h.token == "base_eval_env_arc"),
        "self-test (direct-reach tripwire RED): a NEW non-anchor fn directly naming \
         `base_eval_env_arc` MUST be flagged — got {:?}",
        direct_hits
            .iter()
            .map(|h| (h.enclosing_fn.as_str(), h.token.as_str()))
            .collect::<Vec<_>>()
    );
    assert!(
        direct_hits
            .iter()
            .any(|h| h.enclosing_fn == "field_reach" && h.token == "whole_env"),
        "self-test (direct-reach tripwire RED): a NEW non-anchor fn directly naming `whole_env` \
         MUST be flagged — got {:?}",
        direct_hits
            .iter()
            .map(|h| (h.enclosing_fn.as_str(), h.token.as_str()))
            .collect::<Vec<_>>()
    );

    // (2) PASS (honest scope) — a TRANSITIVE-only reacher (calls the oracle
    // `resolve_value_export_target`, no direct root token) is NOT flagged. This
    // is exactly the documented limit, not an enforcement gap the tripwire
    // claims to close.
    let transitive = "impl Host {\n    \
        fn transitive_reacher(&self, dep: &str, name: &str) -> Option<(String, String)> {\n        \
            self.resolve_value_export_target(dep, name)\n    \
        }\n}\n";
    let transitive_inv = inventory_for(&[(
        "src/host_manage/some_other_module.rs".to_string(),
        transitive.to_string(),
    )]);
    let tr = transitive_inv
        .iter()
        .find(|d| d.name == "transitive_reacher")
        .expect("fn transitive_reacher");
    // The oracle call IS surfaced (the negative result below is not vacuous)…
    assert!(
        tr.referenced_idents.contains("resolve_value_export_target"),
        "self-test: the transitive reacher's oracle call must be surfaced by the collector"
    );
    // …yet NO direct root token is present, so the direct-reach scan does not
    // fire on it (the honest scope).
    for token in WHOLE_ENV_REACH_TOKENS {
        assert!(
            !tr.referenced_idents.contains(*token),
            "self-test: the transitive reacher must contain NO direct root token `{token}` — its \
             only whole-env reach is through the oracle call"
        );
    }
    assert!(
        whole_env_reach_hits(&transitive_inv, &anchors)
            .iter()
            .all(|h| h.enclosing_fn != "transitive_reacher"),
        "self-test (direct-reach tripwire honest scope): a TRANSITIVE-only reacher (calls the \
         oracle, no direct root token) is NOT flagged — the tripwire keys on direct roots and does \
         not emulate transitive call-graph resolution"
    );

    // (3) PASS — the same direct root reach AT an allowlisted anchor is accepted.
    let at_anchor = "impl VerterHost {\n    \
        fn local_type_declaration_id(&self, id: &str) -> Option<i32> { let _ = self.base_eval_env_arc(id); Some(0) }\n}\n";
    let at_anchor_inv = inventory_for(&[(
        "src/host_manage/eval_env.rs".to_string(),
        at_anchor.to_string(),
    )]);
    assert!(
        whole_env_reach_hits(&at_anchor_inv, &anchors).is_empty(),
        "self-test (direct-reach tripwire PASS): a direct `base_eval_env_arc` reach inside the \
         allowlisted oracle anchor (eval_env.rs :: impl VerterHost :: local_type_declaration_id) \
         must NOT be flagged"
    );
}

// ════════════════════════════════════════════════════════════════════
// no-new-materialize-bridge (unchanged invariant, syn body scan)
// ════════════════════════════════════════════════════════════════════

/// No new hot `materialize_type_expr` bridge — the graph-native readers read
/// already-lowered typed IR directly; they must never raise → re-lower via
/// `materialize_type_expr`. This row-scoped guard asserts none of the
/// graph-native reader FILES introduces a `materialize_type_expr` reference in
/// a NON-test fn body.
#[test]
fn graph_native_reader_files_introduce_no_materialize_type_expr_bridge() {
    let reader_files = [
        "src/host_manage/eval_env.rs",
        "src/host_manage/fallthrough.rs",
        "src/resolver_core/runtime_values.rs",
    ];
    let files: Vec<(String, String)> = reader_files
        .iter()
        .map(|rel| {
            let src = std::fs::read_to_string(crate_root().join(rel))
                .unwrap_or_else(|e| panic!("read {rel}: {e}"));
            (rel.to_string(), src)
        })
        .collect();
    let inv = build_fn_inventory(&files);
    let mut hits = Vec::new();
    for def in &inv {
        if def.cfg_test || !def.has_body {
            continue;
        }
        if def.referenced_idents.contains("materialize_type_expr") {
            hits.push(format!(
                "{} :: {} :: fn {}",
                def.file, def.impl_path, def.name
            ));
        }
    }
    assert!(
        hits.is_empty(),
        "graph-native consumer-reader readiness: a graph-native reader file references \
         `materialize_type_expr` — the readers read already-lowered typed IR directly and must \
         never raise→re-lower. Offending:\n{}",
        hits.join("\n")
    );
}

// ════════════════════════════════════════════════════════════════════
// DISCRIMINATING SELF-TESTS
// ════════════════════════════════════════════════════════════════════

/// Resolved-anchor report + non-vacuity: the real tree has each anchored
/// definition present and unique, and the inventory enumerates four consumers.
#[test]
fn resolved_anchors_are_present_unique_and_enumerated() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);

    assert_eq!(
        WHOLE_ENV_CONSUMER_INVENTORY.len(),
        4,
        "the inventory must enumerate all four whole_env() consumers (C1–C4)"
    );

    for row in WHOLE_ENV_CONSUMER_INVENTORY {
        assert_eq!(
            anchored_defs(
                &inv,
                row.graph_native_file,
                row.graph_native_impl,
                row.graph_native_fn
            )
            .len(),
            1,
            "self-test: the graph-native reader `{}` must be present AND unique at its anchor ({} \
             :: {})",
            row.graph_native_fn,
            row.graph_native_file,
            row.graph_native_impl
        );
        assert_eq!(
            anchored_defs(&inv, row.oracle_file, row.oracle_impl, row.oracle_fn).len(),
            1,
            "self-test: the oracle `{}` must be present AND unique at its anchor ({} :: {})",
            row.oracle_fn,
            row.oracle_file,
            row.oracle_impl
        );
    }

    // A deliberately-absent reader name reports NOT present (non-vacuity).
    assert!(
        !anchored_definition_present(
            &inv,
            "src/host_manage/eval_env.rs",
            "impl VerterHost",
            "this_graph_native_reader_does_not_exist_xyzzy"
        ),
        "self-test: a deliberately-absent reader name must NOT be present at an anchor"
    );
}

/// Finding #1: the C4 reader defined ONLY in a non-anchored file (a stray
/// `runtime_values.rs`-like body) with the eval_env.rs anchor absent →
/// presence FAILS for the C4 anchor.
#[test]
fn anchored_reader_in_wrong_file_is_not_counted_present() {
    // The C4 reader body lives ONLY in a non-anchored file/impl.
    let wrong_file = vec![(
        "src/resolver_core/runtime_values.rs".to_string(),
        "trait ImportedRuntimeValueResolver {\n    \
            fn dependency_value_symbol_graph_native(&self) -> Option<i32> { None }\n}\n"
            .to_string(),
    )];
    let inv = inventory_for(&wrong_file);
    // The C4 anchor (eval_env.rs :: impl VerterHost) is ABSENT.
    assert!(
        !anchored_definition_present(
            &inv,
            "src/host_manage/eval_env.rs",
            "impl VerterHost",
            "dependency_value_symbol_graph_native"
        ),
        "self-test (Finding #1): the C4 reader defined only in a non-anchored file must NOT \
         satisfy presence at its eval_env.rs anchor"
    );
    // …but it IS detected at the file/impl it actually lives in (proves the
    // detector is not always-false).
    assert!(
        anchored_definition_present(
            &inv,
            "src/resolver_core/runtime_values.rs",
            "trait ImportedRuntimeValueResolver",
            "dependency_value_symbol_graph_native"
        ),
        "self-test discrimination: the body IS present at the trait-default file/impl it lives in"
    );
}

/// Finding #2: a SECOND `fn new` in the anchored accessor FILE — in a
/// DIFFERENT impl — reaching the whole env is FLAGGED. The impl-qualified
/// anchor (`impl DeclBodyMemo :: new`) does NOT cover the second impl's `new`.
#[test]
fn second_same_named_fn_in_anchored_file_reaching_whole_env_is_rejected() {
    let anchors = whole_env_anchors();
    // The anchored accessor file decl_body_memo.rs with TWO impls each with
    // `fn new`; only `impl DeclBodyMemo :: new` is anchored. The OTHER impl's
    // `new` reaches `base_eval_env_arc`.
    let src = "impl DeclBodyMemo {\n    \
            fn new() -> Self { Self { whole_env: OnceLock::new() } }\n}\n\
        impl SomethingElse {\n    \
            fn new(&self) -> Option<i32> { let _ = self.base_eval_env_arc(\"x\"); Some(0) }\n}\n";
    let inv = inventory_for(&[("src/decl_body_memo.rs".to_string(), src.to_string())]);
    let hits = whole_env_reach_hits(&inv, &anchors);
    assert!(
        hits.iter().any(|h| h.enclosing_fn == "new"
            && h.impl_path == "impl SomethingElse"
            && h.token == "base_eval_env_arc"),
        "self-test (Finding #2): a SECOND `fn new` in the anchored file but a DIFFERENT impl \
         (`impl SomethingElse`) reaching `base_eval_env_arc` MUST be flagged — the impl-qualified \
         anchor only covers `impl DeclBodyMemo :: new`. Got {:?}",
        hits.iter()
            .map(|h| (
                h.impl_path.as_str(),
                h.enclosing_fn.as_str(),
                h.token.as_str()
            ))
            .collect::<Vec<_>>()
    );
    // Discrimination: the anchored `impl DeclBodyMemo :: new` is NOT flagged.
    assert!(
        !hits
            .iter()
            .any(|h| h.impl_path == "impl DeclBodyMemo" && h.enclosing_fn == "new"),
        "self-test (Finding #2 discrimination): the anchored `impl DeclBodyMemo :: new` must NOT \
         be flagged"
    );
}

/// Finding #2: a synthetic TWO non-test definitions of an anchored
/// `(file, impl, fn)` makes `anchored_defs` return 2 — the uniqueness guard
/// reddens.
#[test]
fn anchor_uniqueness_guard_fires_on_duplicate_definition() {
    let dup = vec![(
        "src/decl_body_memo.rs".to_string(),
        "impl DeclBodyMemo {\n    \
            fn new() -> Self { Self::a() }\n    \
            fn new() -> Self { Self::b() }\n}\n"
            .to_string(),
    )];
    let inv = inventory_for(&dup);
    let count = anchored_defs(&inv, "src/decl_body_memo.rs", "impl DeclBodyMemo", "new").len();
    assert_eq!(
        count, 2,
        "self-test (Finding #2): two non-test definitions at the SAME (file, impl, fn) anchor must \
         be counted as 2 so the uniqueness guard reddens — got {count}"
    );
    // Discrimination: a single definition counts as exactly 1.
    let single = vec![(
        "src/decl_body_memo.rs".to_string(),
        "impl DeclBodyMemo {\n    fn new() -> Self { Self::a() }\n}\n".to_string(),
    )];
    let inv1 = inventory_for(&single);
    assert_eq!(
        anchored_defs(&inv1, "src/decl_body_memo.rs", "impl DeclBodyMemo", "new").len(),
        1,
        "self-test (Finding #2 discrimination): a single definition at the anchor counts as 1"
    );
}

/// Finding #1 load-bearing fail-closed proof: a NEW production fn calling
/// `base_eval_env_arc` whose `(file, impl, fn)` is NOT allowlisted MUST be
/// reported.
#[test]
fn fifth_consumer_calling_whole_env_is_rejected() {
    let anchors = whole_env_anchors();
    let src = "impl Host {\n    \
        fn some_new_consumer(&self, id: &str) -> Option<i32> {\n        \
            let e = self.base_eval_env_arc(id)?;\n        \
            e.value_symbols.len().try_into().ok()\n    \
        }\n}\n";
    let inv = inventory_for(&[("synthetic.rs".to_string(), src.to_string())]);
    let hits = whole_env_reach_hits(&inv, &anchors);
    assert!(
        hits.iter()
            .any(|h| h.enclosing_fn == "some_new_consumer" && h.token == "base_eval_env_arc"),
        "self-test: a NEW non-allowlisted fn `some_new_consumer` calling `base_eval_env_arc` MUST \
         be reported (the fail-closed proof) — got {:?}",
        hits.iter()
            .map(|h| (h.enclosing_fn.as_str(), h.token.as_str()))
            .collect::<Vec<_>>()
    );
}

/// Finding #1: the SAME call inside the oracle consumer
/// (`local_type_declaration_id`) AT ITS ANCHOR (file + impl) is accepted.
#[test]
fn allowlisted_consumer_calling_whole_env_is_accepted() {
    let anchors = whole_env_anchors();
    let src = "impl VerterHost {\n    \
        fn local_type_declaration_id(&self, id: &str) -> Option<i32> {\n        \
            let e = self.base_eval_env_arc(id)?;\n        \
            e.value_symbols.len().try_into().ok()\n    \
        }\n}\n";
    let inv = inventory_for(&[("src/host_manage/eval_env.rs".to_string(), src.to_string())]);
    let hits = whole_env_reach_hits(&inv, &anchors);
    assert!(
        hits.is_empty(),
        "self-test: `base_eval_env_arc` inside the oracle consumer `local_type_declaration_id` AT \
         its anchor (eval_env.rs :: impl VerterHost) must NOT be a violation — got {:?}",
        hits.iter()
            .map(|h| (h.enclosing_fn.as_str(), h.token.as_str()))
            .collect::<Vec<_>>()
    );
}

/// Finding #1 (the load-bearing exact-anchor proof): the allowlisted name
/// `new` reaching `base_eval_env_arc` in an UNRELATED file is FLAGGED — `new`
/// is anchored ONLY for `src/decl_body_memo.rs :: impl DeclBodyMemo`.
#[test]
fn reused_allowlisted_name_in_unrelated_file_is_rejected() {
    let anchors = whole_env_anchors();
    let src = "impl Foo {\n    \
        fn new(&self) -> Option<i32> {\n        \
            let _ = self.base_eval_env_arc(\"x\");\n        \
            Some(0)\n    \
        }\n}\n";
    let inv = inventory_for(&[(
        "src/host_manage/some_other_module.rs".to_string(),
        src.to_string(),
    )]);
    let hits = whole_env_reach_hits(&inv, &anchors);
    assert!(
        hits.iter()
            .any(|h| h.enclosing_fn == "new" && h.token == "base_eval_env_arc"),
        "self-test (Finding #1): a stray `fn new` reaching `base_eval_env_arc` in \
         `src/host_manage/some_other_module.rs` MUST be flagged — `new` is anchored ONLY for \
         `src/decl_body_memo.rs :: impl DeclBodyMemo`. Got {:?}",
        hits.iter()
            .map(|h| (h.enclosing_fn.as_str(), h.token.as_str()))
            .collect::<Vec<_>>()
    );
}

/// Finding #1 discrimination anchor: the SAME bare name `new` reaching the
/// whole env AT its real anchor (`src/decl_body_memo.rs :: impl DeclBodyMemo
/// :: new`) is ACCEPTED.
#[test]
fn exact_anchor_definition_is_accepted() {
    let anchors = whole_env_anchors();
    let src = "impl DeclBodyMemo {\n    \
        fn new() -> Self {\n        \
            Self { whole_env: OnceLock::new() }\n    \
        }\n}\n";
    let inv = inventory_for(&[("src/decl_body_memo.rs".to_string(), src.to_string())]);
    let hits = whole_env_reach_hits(&inv, &anchors);
    assert!(
        hits.is_empty(),
        "self-test (Finding #1): `fn new` touching `whole_env` AT its real anchor (decl_body_memo.rs \
         :: impl DeclBodyMemo) must NOT be a violation — got {:?}",
        hits.iter()
            .map(|h| (h.enclosing_fn.as_str(), h.token.as_str()))
            .collect::<Vec<_>>()
    );
}

/// Finding #3: a `#[cfg(test)]`-gated fn reaching `base_eval_env_arc` is TEST
/// code, not a production consumer, and must NOT be a reach violation — for
/// the same-line attribute shape.
#[test]
fn cfg_test_same_line_definition_rejected_in_reach_and_presence() {
    let anchors = whole_env_anchors();
    // Same-line `#[cfg(test)] fn …` — handled by the parser.
    let src = "impl Host {\n    \
        #[cfg(test)] fn t(&self, id: &str) -> Option<i32> { let _ = self.base_eval_env_arc(id); Some(0) }\n}\n";
    let inv = inventory_for(&[(
        "src/host_manage/some_other_module.rs".to_string(),
        src.to_string(),
    )]);
    let hits = whole_env_reach_hits(&inv, &anchors);
    assert!(
        hits.is_empty(),
        "self-test (Finding #3): a same-line `#[cfg(test)] fn t` reaching `base_eval_env_arc` must \
         NOT be a production reach violation (it is test code) — got {:?}",
        hits.iter()
            .map(|h| (h.enclosing_fn.as_str(), h.token.as_str()))
            .collect::<Vec<_>>()
    );
    // Presence: a same-line `#[cfg(test)] fn reader` does NOT satisfy presence.
    let reader_src = "impl VerterHost {\n    #[cfg(test)] fn reader() -> Option<i32> { None }\n}\n";
    let reader_inv = inventory_for(&[(
        "src/host_manage/eval_env.rs".to_string(),
        reader_src.to_string(),
    )]);
    assert!(
        !anchored_definition_present(
            &reader_inv,
            "src/host_manage/eval_env.rs",
            "impl VerterHost",
            "reader"
        ),
        "self-test (Finding #3): a same-line `#[cfg(test)] fn reader` must NOT satisfy presence"
    );
}

/// Finding #1 + #3: a MULTILINE `#[cfg(all(\n unix, \n test \n))]` attr — which
/// IS test-only (`all` with a `test` conjunct is compiled-out without `test`) —
/// is detected (the gated fn is rejected from presence), proving the multiline
/// attribute shape parses. Discrimination: WITHOUT the attr the same fn IS
/// present. (The old form of this test used `any(unix, test)` and WRONGLY
/// asserted it was test-only — `any(unix, test)` COMPILES in a normal build, so
/// it is production code; see `cfg_any_unix_test_is_production`.)
#[test]
fn cfg_test_multiline_all_attr_rejected() {
    let multiline = "impl VerterHost {\n    \
        #[cfg(all(\n        unix,\n        test\n    ))]\n    \
        fn reader() -> Option<i32> { None }\n}\n";
    let inv = inventory_for(&[(
        "src/host_manage/eval_env.rs".to_string(),
        multiline.to_string(),
    )]);
    assert!(
        !anchored_definition_present(
            &inv,
            "src/host_manage/eval_env.rs",
            "impl VerterHost",
            "reader"
        ),
        "self-test (Finding #1/#3): a multiline `#[cfg(all(\\n unix, \\n test \\n))]` gate is \
         test-only (an `all` with a `test` conjunct) — it must be detected and the fn rejected \
         from presence"
    );
    // Discrimination: WITHOUT the cfg-test attr, the fn IS present.
    let plain = "impl VerterHost {\n    fn reader() -> Option<i32> { None }\n}\n";
    let plain_inv =
        inventory_for(&[("src/host_manage/eval_env.rs".to_string(), plain.to_string())]);
    assert!(
        anchored_definition_present(
            &plain_inv,
            "src/host_manage/eval_env.rs",
            "impl VerterHost",
            "reader"
        ),
        "self-test (Finding #3 discrimination): the same fn WITHOUT a cfg-test attr IS present"
    );
}

/// Finding #3: a fn nested in a `#[cfg(test)] mod tests { … }` is test code —
/// not present.
#[test]
fn cfg_test_nested_mod_definition_rejected() {
    let nested = "#[cfg(test)]\nmod tests {\n    \
        fn reader() -> Option<i32> { None }\n}\n";
    let inv = inventory_for(&[(
        "src/host_manage/eval_env.rs".to_string(),
        nested.to_string(),
    )]);
    // The fn lives under `mod tests`, so its impl_path is "mod tests" — and it
    // is cfg-test, so it is never counted at any anchor.
    assert!(
        !inv.iter().any(|d| d.name == "reader" && !d.cfg_test),
        "self-test (Finding #3): a fn nested in a `#[cfg(test)] mod tests` must be cfg-test-marked \
         (never a production definition)"
    );
}

/// Finding #3: `feature = "test_util"` is a PRODUCTION cfg gate and must NOT
/// be mistaken for the `test` cfg — the gated fn stays present.
#[test]
fn feature_test_util_cfg_is_not_cfg_test() {
    let prod_cfg = "impl VerterHost {\n    \
        #[cfg(feature = \"test_util\")]\n    \
        fn reader() -> Option<i32> { None }\n}\n";
    let inv = inventory_for(&[(
        "src/host_manage/eval_env.rs".to_string(),
        prod_cfg.to_string(),
    )]);
    assert!(
        anchored_definition_present(
            &inv,
            "src/host_manage/eval_env.rs",
            "impl VerterHost",
            "reader"
        ),
        "self-test (Finding #3): a `#[cfg(feature = \"test_util\")]` gate is a PRODUCTION cfg — \
         `test_util` must not be mistaken for the `test` cfg, so the fn stays present"
    );
    // Direct proof of the predicate primitive: a literal `"test_util"` does
    // NOT contain a bare `test` IDENT, but `cfg(test)` does.
    let test_util_attr: Attribute = syn::parse_quote!(#[cfg(feature = "test_util")]);
    let plain_test_attr: Attribute = syn::parse_quote!(#[cfg(test)]);
    let compound: Attribute = syn::parse_quote!(#[cfg(all(unix, test))]);
    assert!(
        !attrs_are_cfg_test(std::slice::from_ref(&test_util_attr)),
        "self-test (Finding #3): `cfg(feature = \"test_util\")` must NOT match the `test` cfg ident"
    );
    assert!(
        attrs_are_cfg_test(std::slice::from_ref(&plain_test_attr)),
        "self-test (Finding #3): `cfg(test)` must match"
    );
    assert!(
        attrs_are_cfg_test(std::slice::from_ref(&compound)),
        "self-test (Finding #3): `cfg(all(unix, test))` must match (recurses meta lists)"
    );
}

// ────────────────────────────────────────────────────────────────────
// Finding #1 — cfg-test evaluator truth table (conservative "compiled-out
// unless test" boolean semantics; the old fail-open scan is fixed)
// ────────────────────────────────────────────────────────────────────

/// Finding #1: `cfg(any(unix, test))` COMPILES in a normal (non-test) build —
/// it is PRODUCTION code, NOT test-only. (`any` is compiled-out without `test`
/// only if EVERY disjunct needs `test`; `unix` does not.) The old fail-open
/// scanner WRONGLY marked it test-only.
#[test]
fn cfg_any_unix_test_is_production() {
    let attr: Attribute = syn::parse_quote!(#[cfg(any(unix, test))]);
    assert!(
        !attrs_are_cfg_test(std::slice::from_ref(&attr)),
        "self-test (Finding #1): `cfg(any(unix, test))` is PRODUCTION (it compiles without `test`) \
         — it must NOT be treated as test-only"
    );
}

/// Finding #1: `cfg(any(test, debug_assertions))` is PRODUCTION — it compiles
/// in a non-test debug build (the `debug_assertions` disjunct holds).
#[test]
fn cfg_any_test_debug_assertions_is_production() {
    let attr: Attribute = syn::parse_quote!(#[cfg(any(test, debug_assertions))]);
    assert!(
        !attrs_are_cfg_test(std::slice::from_ref(&attr)),
        "self-test (Finding #1): `cfg(any(test, debug_assertions))` is PRODUCTION — not test-only"
    );
}

/// Finding #1: `cfg(all(unix, test))` is TEST-ONLY — `all` requires every
/// conjunct, and the `test` conjunct makes it compiled-out without `test`.
#[test]
fn cfg_all_unix_test_is_test_only() {
    let attr: Attribute = syn::parse_quote!(#[cfg(all(unix, test))]);
    assert!(
        attrs_are_cfg_test(std::slice::from_ref(&attr)),
        "self-test (Finding #1): `cfg(all(unix, test))` is TEST-ONLY (an `all` with a `test` \
         conjunct is compiled-out without `test`)"
    );
}

/// Finding #1: `cfg(any(test))` (single disjunct) is TEST-ONLY — EVERY disjunct
/// needs `test`, so it is compiled-out without `test`.
#[test]
fn cfg_any_test_alone_is_test_only() {
    let attr: Attribute = syn::parse_quote!(#[cfg(any(test))]);
    assert!(
        attrs_are_cfg_test(std::slice::from_ref(&attr)),
        "self-test (Finding #1): `cfg(any(test))` is TEST-ONLY (its only disjunct needs `test`)"
    );
}

/// Finding #1: `cfg(not(test))` is PRODUCTION code that runs ONLY in a non-test
/// build — it must NOT be dropped as test-only.
#[test]
fn cfg_not_test_is_production() {
    let attr: Attribute = syn::parse_quote!(#[cfg(not(test))]);
    assert!(
        !attrs_are_cfg_test(std::slice::from_ref(&attr)),
        "self-test (Finding #1): `cfg(not(test))` is PRODUCTION (runs in a non-test build) — it \
         must NOT be treated as test-only"
    );
}

/// Finding #1: nested combinators — `cfg(all(unix, any(test)))` is TEST-ONLY
/// (the `any(test)` conjunct demands `test`), while
/// `cfg(any(unix, all(test, feature = "x")))` is PRODUCTION (the `unix`
/// disjunct holds without `test`).
#[test]
fn cfg_nested_combinator_semantics() {
    let test_only: Attribute = syn::parse_quote!(#[cfg(all(unix, any(test)))]);
    assert!(
        attrs_are_cfg_test(std::slice::from_ref(&test_only)),
        "self-test (Finding #1): `cfg(all(unix, any(test)))` is TEST-ONLY (a conjunct demands test)"
    );
    let production: Attribute = syn::parse_quote!(#[cfg(any(unix, all(test, feature = "x")))]);
    assert!(
        !attrs_are_cfg_test(std::slice::from_ref(&production)),
        "self-test (Finding #1): `cfg(any(unix, all(test, feature = \"x\")))` is PRODUCTION (the \
         `unix` disjunct holds without test)"
    );
}

/// Finding #1 (load-bearing fail-closed proof): a NON-anchor production fn under
/// `#[cfg(any(unix, test))]` that reaches `base_eval_env_arc` MUST be FLAGGED by
/// the reach scan — the old fail-open evaluator skipped it (treating
/// `any(unix, test)` as test-only), letting a production whole-env caller pass
/// undetected. Discrimination: the SAME fn under `#[cfg(test)]` is NOT flagged.
#[test]
fn reach_scan_flags_any_unix_test_gated_production_consumer() {
    let anchors = whole_env_anchors();
    // Production-compiled fn (`any(unix, test)`) at a NON-anchor location.
    let prod = "impl Host {\n    \
        #[cfg(any(unix, test))]\n    \
        fn sneaky(&self, id: &str) -> Option<i32> { let _ = self.base_eval_env_arc(id); Some(0) }\n}\n";
    let inv = inventory_for(&[(
        "src/host_manage/some_other_module.rs".to_string(),
        prod.to_string(),
    )]);
    let hits = whole_env_reach_hits(&inv, &anchors);
    assert!(
        hits.iter()
            .any(|h| h.enclosing_fn == "sneaky" && h.token == "base_eval_env_arc"),
        "self-test (Finding #1): a NON-anchor fn under `#[cfg(any(unix, test))]` reaching \
         `base_eval_env_arc` is PRODUCTION code and MUST be flagged (closes the fail-open hole) — \
         got {:?}",
        hits.iter()
            .map(|h| (h.enclosing_fn.as_str(), h.token.as_str()))
            .collect::<Vec<_>>()
    );
    // Discrimination: under `#[cfg(test)]` the same fn is test code → NOT flagged.
    let test_gated = "impl Host {\n    \
        #[cfg(test)]\n    \
        fn sneaky(&self, id: &str) -> Option<i32> { let _ = self.base_eval_env_arc(id); Some(0) }\n}\n";
    let test_inv = inventory_for(&[(
        "src/host_manage/some_other_module.rs".to_string(),
        test_gated.to_string(),
    )]);
    assert!(
        whole_env_reach_hits(&test_inv, &anchors).is_empty(),
        "self-test (Finding #1 discrimination): the same fn under `#[cfg(test)]` is test code and \
         must NOT be flagged"
    );
}

// ────────────────────────────────────────────────────────────────────
// Finding #2 — IdentCollector covers field access + macro token streams
// ────────────────────────────────────────────────────────────────────

/// Finding #2: a NAMED field access to `whole_env` (`self.whole_env.get()`) is
/// surfaced by the collector and FLAGGED by the reach scan at a non-anchor
/// location. The old collector recorded only path segments + method names, so a
/// whole-env reach through a FIELD was invisible.
#[test]
fn field_access_to_whole_env_is_detected() {
    let anchors = whole_env_anchors();
    let src = "impl Host {\n    \
        fn reader(&self) -> Option<i32> { let _ = self.whole_env.get(); Some(0) }\n}\n";
    let inv = inventory_for(&[(
        "src/host_manage/some_other_module.rs".to_string(),
        src.to_string(),
    )]);
    let r = inv.iter().find(|d| d.name == "reader").expect("fn reader");
    assert!(
        r.referenced_idents.contains("whole_env"),
        "self-test (Finding #2): a `self.whole_env` field access must surface `whole_env`"
    );
    let hits = whole_env_reach_hits(&inv, &anchors);
    assert!(
        hits.iter()
            .any(|h| h.enclosing_fn == "reader" && h.token == "whole_env"),
        "self-test (Finding #2): `self.whole_env.get()` at a non-anchor location MUST be flagged \
         (whole-env reach via field access) — got {:?}",
        hits.iter()
            .map(|h| (h.enclosing_fn.as_str(), h.token.as_str()))
            .collect::<Vec<_>>()
    );
}

/// Finding #2: a struct field-init member named `whole_env`
/// (`Self { whole_env: x }`) surfaces the member ident.
#[test]
fn struct_field_init_member_is_detected() {
    let src = "impl Host {\n    \
        fn reader(&self) -> Self { Self { whole_env: 1 } }\n}\n";
    let inv = inventory_for(&[("synthetic.rs".to_string(), src.to_string())]);
    let r = inv.iter().find(|d| d.name == "reader").expect("fn reader");
    assert!(
        r.referenced_idents.contains("whole_env"),
        "self-test (Finding #2): a `Self {{ whole_env: … }}` field-init member must surface \
         `whole_env`"
    );
}

/// Finding #2: a banned WHOLE ident inside a MACRO invocation's token stream
/// (`my_macro!(self.base_eval_env_arc(id))`) is surfaced and FLAGGED. The
/// default `syn` walk does not descend macro tokens, so the old collector
/// missed it.
#[test]
fn banned_ident_inside_macro_is_detected() {
    let anchors = whole_env_anchors();
    let src = "impl Host {\n    \
        fn reader(&self, id: &str) { my_macro!(self.base_eval_env_arc(id)); }\n}\n";
    let inv = inventory_for(&[(
        "src/host_manage/some_other_module.rs".to_string(),
        src.to_string(),
    )]);
    let r = inv.iter().find(|d| d.name == "reader").expect("fn reader");
    assert!(
        r.referenced_idents.contains("base_eval_env_arc"),
        "self-test (Finding #2): a banned ident inside a macro token stream must surface"
    );
    let hits = whole_env_reach_hits(&inv, &anchors);
    assert!(
        hits.iter()
            .any(|h| h.enclosing_fn == "reader" && h.token == "base_eval_env_arc"),
        "self-test (Finding #2): `my_macro!(self.base_eval_env_arc(id))` at a non-anchor location \
         MUST be flagged — got {:?}",
        hits.iter()
            .map(|h| (h.enclosing_fn.as_str(), h.token.as_str()))
            .collect::<Vec<_>>()
    );
}

/// Finding #2 (whole-ident discipline): the `_graph_native`-suffixed method
/// call must NOT match the banned bare `resolve_value_export_target`, and a
/// field/macro reach to a `_graph_native`-suffixed ident is likewise not a bare
/// match. The full suffixed ident IS surfaced; the bare ban is NOT triggered.
#[test]
fn graph_native_suffixed_call_is_not_a_banned_reach() {
    let src = "impl Host {\n    \
        fn reader(&self) { let _ = self.resolve_value_export_target_graph_native(1, 2); }\n}\n";
    let inv = inventory_for(&[("synthetic.rs".to_string(), src.to_string())]);
    let r = inv.iter().find(|d| d.name == "reader").expect("fn reader");
    assert!(
        !r.referenced_idents.contains("resolve_value_export_target"),
        "self-test (Finding #2 discipline): `resolve_value_export_target_graph_native` must NOT \
         match the banned bare `resolve_value_export_target`"
    );
    assert!(
        r.referenced_idents
            .contains("resolve_value_export_target_graph_native"),
        "self-test (Finding #2): the full `_graph_native` method ident IS surfaced"
    );
    // The bounded-body banned-token scan must NOT trip on the suffixed call.
    for token in BANNED_WHOLE_ENV_TOKENS {
        if *token == "resolve_value_export_target" {
            assert!(
                !r.referenced_idents.contains(*token),
                "self-test: the suffixed graph-native call must not register the bare banned token"
            );
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// GOV5 — a graph-native reader body must NOT call its retained oracle
// ────────────────────────────────────────────────────────────────────

/// The reader-body GOV5 ban predicate over a single fn def: does the body call
/// ANY oracle whole identifier? Mirrors the production scan
/// (`graph_native_reader_bodies_do_not_route_through_whole_env`) so the
/// self-tests discriminate the exact condition the guard enforces.
fn reader_body_calls_an_oracle(def: &FnDef) -> bool {
    oracle_call_ban_idents()
        .iter()
        .any(|oracle| def.referenced_idents.contains(oracle))
}

/// GOV5: a synthetic graph-native reader whose body calls its bare ORACLE
/// (`self.local_type_declaration_id(...)`) — with NO directly-banned root token —
/// is FLAGGED by the reader-body oracle-call ban. (Pre-fix this passed: the
/// oracle name was not in `BANNED_WHOLE_ENV_TOKENS`.)
#[test]
fn graph_native_reader_calling_its_oracle_is_rejected() {
    let src = "impl VerterHost {\n    \
        fn local_type_declaration_id_graph_native(&self) -> Option<i32> {\n        \
            let _ = self.local_type_declaration_id(\"src\", \"name\");\n        \
            Some(0)\n    \
        }\n}\n";
    let inv = inventory_for(&[("src/host_manage/eval_env.rs".to_string(), src.to_string())]);
    let def = inv
        .iter()
        .find(|d| d.name == "local_type_declaration_id_graph_native")
        .expect("graph-native reader def");
    // No directly-banned root token is present — proving the ban fires on the
    // ORACLE name, not on a whole-env root.
    for token in BANNED_WHOLE_ENV_TOKENS {
        assert!(
            !def.referenced_idents.contains(*token),
            "self-test (GOV5): the synthetic reader must contain NO directly-banned root token \
             `{token}` — the violation must come from the oracle call alone"
        );
    }
    assert!(
        reader_body_calls_an_oracle(def),
        "self-test (GOV5): a graph-native reader body calling its bare oracle \
         `local_type_declaration_id` MUST be flagged by the reader-body oracle-call ban — got \
         referenced idents {:?}",
        def.referenced_idents
    );
}

/// GOV5 whole-ident discipline: a graph-native reader body calling a
/// `_graph_native`-suffixed SIBLING (`self.peel_value_decl_alias_graph_native(...)`
/// or `self.local_type_declaration_id_graph_native(...)`) is NOT flagged — the
/// `_graph_native` whole identifier is a distinct token, never the bare oracle.
#[test]
fn graph_native_reader_calling_sibling_graph_native_is_allowed() {
    let src = "impl VerterHost {\n    \
        fn dependency_value_symbol_graph_native(&self) -> Option<i32> {\n        \
            let _ = self.peel_value_decl_alias_graph_native(\"src\", \"name\");\n        \
            let _ = self.local_type_declaration_id_graph_native(\"src\", \"name\");\n        \
            Some(0)\n    \
        }\n}\n";
    let inv = inventory_for(&[("src/host_manage/eval_env.rs".to_string(), src.to_string())]);
    let def = inv
        .iter()
        .find(|d| d.name == "dependency_value_symbol_graph_native")
        .expect("graph-native reader def");
    // The suffixed sibling idents ARE surfaced (proves the collector saw the
    // calls and the negative result is not vacuous).
    assert!(
        def.referenced_idents
            .contains("peel_value_decl_alias_graph_native")
            && def
                .referenced_idents
                .contains("local_type_declaration_id_graph_native"),
        "self-test (GOV5 discrimination): the full `_graph_native` sibling idents must be surfaced"
    );
    // …yet the bare oracle names are NOT present, so the ban does NOT fire.
    assert!(
        !reader_body_calls_an_oracle(def),
        "self-test (GOV5 discrimination): calling a `_graph_native`-suffixed sibling must NOT be \
         flagged — the suffixed whole identifier is not the bare oracle. Got referenced idents {:?}",
        def.referenced_idents
    );
}

// ────────────────────────────────────────────────────────────────────
// Finding #3 — impl_path renders full module-qualified + generic paths,
// normalizing ONLY lifetimes
// ────────────────────────────────────────────────────────────────────

/// Finding #3: two impls of the SAME bare-named self-type but DIFFERENT
/// module-qualified TRAIT paths render to DIFFERENT anchor identities — the old
/// renderer (last-segment only) collapsed them.
#[test]
fn impl_path_distinguishes_module_qualified_traits() {
    let a: ItemImpl = syn::parse_quote!(impl a::T for X {});
    let b: ItemImpl = syn::parse_quote!(impl b::T for X {});
    assert_ne!(
        render_impl_path(&a),
        render_impl_path(&b),
        "self-test (Finding #3): `impl a::T for X` and `impl b::T for X` must render to DIFFERENT \
         anchor identities (module quals kept) — got {} vs {}",
        render_impl_path(&a),
        render_impl_path(&b)
    );
    assert_eq!(render_impl_path(&a), "impl a::T for X");
    assert_eq!(render_impl_path(&b), "impl b::T for X");
}

/// Finding #3: two impls of the SAME trait for self-types differing ONLY in a
/// TYPE generic arg render to DIFFERENT identities — the old renderer dropped
/// all generics and collapsed `Foo<u8>` with `Foo<u16>`.
#[test]
fn impl_path_distinguishes_generic_args() {
    let u8_impl: ItemImpl = syn::parse_quote!(impl T for Foo<u8> {});
    let u16_impl: ItemImpl = syn::parse_quote!(impl T for Foo<u16> {});
    assert_ne!(
        render_impl_path(&u8_impl),
        render_impl_path(&u16_impl),
        "self-test (Finding #3): `impl T for Foo<u8>` and `impl T for Foo<u16>` must render \
         DIFFERENT (type generics kept) — got {} vs {}",
        render_impl_path(&u8_impl),
        render_impl_path(&u16_impl)
    );
    assert_eq!(render_impl_path(&u8_impl), "impl T for Foo<u8>");
    assert_eq!(render_impl_path(&u16_impl), "impl T for Foo<u16>");
}

/// Finding #3: lifetimes — and ONLY lifetimes — are normalized away. Impls
/// differing only in a lifetime arg (`T<'_>` vs `T<'a>`) render the SAME, and a
/// self-type generic that mixes a lifetime with a kept type arg drops only the
/// lifetime (`Foo<'a, u8>` → `Foo<u8>`).
#[test]
fn impl_path_normalizes_only_lifetimes() {
    let elided: ItemImpl = syn::parse_quote!(impl T<'_> for X {});
    let named: ItemImpl = syn::parse_quote!(impl T<'a> for X {});
    assert_eq!(
        render_impl_path(&elided),
        render_impl_path(&named),
        "self-test (Finding #3): `impl T<'_> for X` and `impl T<'a> for X` must render the SAME \
         (lifetimes normalized) — got {} vs {}",
        render_impl_path(&elided),
        render_impl_path(&named)
    );
    assert_eq!(render_impl_path(&elided), "impl T for X");
    // A mixed lifetime + type arg keeps only the type arg.
    let mixed: ItemImpl = syn::parse_quote!(impl T for Foo<'a, u8> {});
    assert_eq!(
        render_impl_path(&mixed),
        "impl T for Foo<u8>",
        "self-test (Finding #3): a mixed `Foo<'a, u8>` self-type drops the lifetime and keeps the \
         type arg"
    );
}

/// Bounded-body discrimination: the AST ident collector (i) surfaces a real
/// `base_eval_env_arc` call, (ii) does NOT surface a banned token that only
/// appears in a comment/string (invisible to the AST), (iii) does NOT confuse
/// `resolve_value_export_target_graph_native` for the banned bare
/// `resolve_value_export_target`, and (iv) the real tree passes.
#[test]
fn bounded_body_guard_discriminates_violation_from_clean() {
    // (i) A real call is surfaced.
    let violating = vec![(
        "synthetic.rs".to_string(),
        "impl H {\n    fn r(&self) -> Option<i32> { let e = self.base_eval_env_arc(\"x\")?; Some(0) }\n}\n".to_string(),
    )];
    let inv = inventory_for(&violating);
    let r = inv.iter().find(|d| d.name == "r").expect("fn r");
    assert!(
        r.referenced_idents.contains("base_eval_env_arc"),
        "self-test: a real `base_eval_env_arc(...)` call must be surfaced by the AST collector"
    );

    // (ii) A banned token only inside a comment + a string literal is NOT
    // surfaced (the AST never sees comment/string contents).
    let clean = vec![(
        "synthetic.rs".to_string(),
        "impl H {\n    fn r(&self) -> Option<i32> {\n        // base_eval_env_arc must never be called here\n        let _ = \"base_eval_env_arc\";\n        self.routed_shallow_state(\"c\")\n    }\n}\n".to_string(),
    )];
    let clean_inv = inventory_for(&clean);
    let cr = clean_inv.iter().find(|d| d.name == "r").expect("fn r");
    assert!(
        !cr.referenced_idents.contains("base_eval_env_arc"),
        "self-test: a `base_eval_env_arc` mention only inside a comment/string must NOT be \
         surfaced by the AST collector"
    );

    // (iii) Whole-identifier boundary: the graph-native variant must NOT match
    // the banned bare token, but the bare legacy peel IS caught.
    let gn = vec![(
        "synthetic.rs".to_string(),
        "impl H {\n    fn r(&self) { let _ = self.resolve_value_export_target_graph_native(1, 2); }\n}\n".to_string(),
    )];
    let gn_inv = inventory_for(&gn);
    let gnr = gn_inv.iter().find(|d| d.name == "r").expect("fn r");
    assert!(
        !gnr.referenced_idents
            .contains("resolve_value_export_target"),
        "self-test: `resolve_value_export_target_graph_native` must NOT match the banned bare \
         `resolve_value_export_target`"
    );
    assert!(
        gnr.referenced_idents
            .contains("resolve_value_export_target_graph_native"),
        "self-test: the full `_graph_native` method ident IS surfaced"
    );
    let legacy = vec![(
        "synthetic.rs".to_string(),
        "impl H {\n    fn r(&self) { let _ = self.resolve_value_export_target(1, 2); }\n}\n"
            .to_string(),
    )];
    let legacy_inv = inventory_for(&legacy);
    let lr = legacy_inv.iter().find(|d| d.name == "r").expect("fn r");
    assert!(
        lr.referenced_idents.contains("resolve_value_export_target"),
        "self-test: the bare legacy `resolve_value_export_target(...)` call IS a banned reference"
    );

    // (iv) The real tree passes — every real reader body (and any trait
    // default / delegate of the same name) is clean.
    let files = production_src_files();
    let real_inv = build_fn_inventory(&files);
    for row in WHOLE_ENV_CONSUMER_INVENTORY {
        let bodies = all_named_bodies(&real_inv, row.graph_native_fn);
        assert!(
            !bodies.is_empty(),
            "real reader body for `{}` must exist",
            row.graph_native_fn
        );
        for def in &bodies {
            for token in BANNED_WHOLE_ENV_TOKENS {
                assert!(
                    !def.referenced_idents.contains(*token),
                    "self-test: real reader `{}` ({} :: {}) must not reference `{token}`",
                    row.graph_native_fn,
                    def.file,
                    def.impl_path
                );
            }
        }
    }
}

/// The `impl`-path renderer normalizes lifetimes away so trait impls anchor
/// stably: `impl Trait for Type<'_>` and `impl Trait for Type<'a>` render
/// identically, and `impl Type` (inherent) renders without a trait prefix.
#[test]
fn impl_path_renderer_normalizes_lifetimes() {
    let elided: ItemImpl =
        syn::parse_quote!(impl ImportedRuntimeValueResolver for HostRuntimeValueResolver<'_> {});
    let named: ItemImpl =
        syn::parse_quote!(impl ImportedRuntimeValueResolver for HostRuntimeValueResolver<'a> {});
    assert_eq!(
        render_impl_path(&elided),
        "impl ImportedRuntimeValueResolver for HostRuntimeValueResolver",
        "self-test: a trait impl renders `impl Trait for Type` with lifetimes stripped"
    );
    assert_eq!(
        render_impl_path(&elided),
        render_impl_path(&named),
        "self-test: `<'_>` and `<'a>` self-types render identically"
    );
    let inherent: ItemImpl = syn::parse_quote!(impl VerterHost {});
    assert_eq!(
        render_impl_path(&inherent),
        "impl VerterHost",
        "self-test: an inherent impl renders `impl Type` with no trait prefix"
    );
}

/// Non-vacuity of the structural inventory itself: parsing the real tree
/// yields a large set of fn definitions including the four known readers, and
/// the cfg-test flag is set for at least one real `#[cfg(test)]` item (proving
/// the gate is not always-false on the real tree). Also asserts each row's
/// graph-native + oracle anchors are unambiguous, building the resolved-anchor
/// report.
#[test]
fn structural_inventory_is_non_vacuous_and_anchors_resolve() {
    let files = production_src_files();
    let inv = build_fn_inventory(&files);
    assert!(
        inv.len() > 100,
        "self-test: the structural inventory must contain many fn definitions — got {}",
        inv.len()
    );
    assert!(
        inv.iter().any(|d| d.cfg_test),
        "self-test: at least one real `#[cfg(test)]` definition must be flagged cfg_test on the \
         real tree (proves the gate is not always-false)"
    );

    // Build + print the resolved anchor report (visible on failure / -- \
    // --nocapture).
    let mut report: BTreeMap<&str, String> = BTreeMap::new();
    for row in WHOLE_ENV_CONSUMER_INVENTORY {
        let gn = anchored_defs(
            &inv,
            row.graph_native_file,
            row.graph_native_impl,
            row.graph_native_fn,
        );
        let oracle = anchored_defs(&inv, row.oracle_file, row.oracle_impl, row.oracle_fn);
        report.insert(
            row.consumer,
            format!(
                "oracle=({} :: {} :: {}) x{}  reader=({} :: {} :: {}) x{}",
                row.oracle_file,
                row.oracle_impl,
                row.oracle_fn,
                oracle.len(),
                row.graph_native_file,
                row.graph_native_impl,
                row.graph_native_fn,
                gn.len()
            ),
        );
    }
    // Every resolved anchor is exactly one.
    let mut report_lines: Vec<String> = Vec::new();
    for (k, v) in &report {
        report_lines.push(format!("{k}: {v}"));
    }
    let dump = report_lines.join("\n");
    let mut by_anchor: HashMap<(String, String, String), usize> = HashMap::new();
    for row in WHOLE_ENV_CONSUMER_INVENTORY {
        *by_anchor
            .entry((
                row.graph_native_file.to_string(),
                row.graph_native_impl.to_string(),
                row.graph_native_fn.to_string(),
            ))
            .or_insert(0) += anchored_defs(
            &inv,
            row.graph_native_file,
            row.graph_native_impl,
            row.graph_native_fn,
        )
        .len();
    }
    for ((f, i, n), c) in &by_anchor {
        assert_eq!(
            *c, 1,
            "self-test: graph-native anchor ({f} :: {i} :: {n}) must resolve to exactly one \
             definition — got {c}.\nResolved anchors:\n{dump}"
        );
    }
}
